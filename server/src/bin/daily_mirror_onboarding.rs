//! Household onboarding from a server shell. The HTTP routes and this binary
//! call the same `server::onboarding` functions.
use std::io;

use server::auth::{AuthStore, User};
use server::catalog::PhotoCatalog;
use server::devices::DeviceRegistry;
use server::onboarding::{self, LinkRequest, PlannedPerson, SignupRequest};
use server::photos::PhotoStore;
use server::processing::ProcessingQueue;

#[tokio::main]
async fn main() -> io::Result<()> {
    // `vercel pull` writes the production Turso connection here. Existing
    // process variables still win, so local/test callers can override safely.
    dotenvy::from_filename(".env.local").ok();
    dotenvy::dotenv().ok();
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        return usage();
    };
    let auth = AuthStore::from_env()?;
    let queue = ProcessingQueue::new(PhotoCatalog::from_env()?);

    match command.as_str() {
        "signup" => {
            let username = args.next().ok_or_else(|| invalid("missing username"))?;
            let display_name = args.next().unwrap_or_else(|| username.clone());
            if args.next().is_some() {
                return usage();
            }
            let password = confirmed_password()?;
            let user = onboarding::signup(
                &auth,
                &queue,
                &SignupRequest {
                    username,
                    display_name,
                    password,
                },
            )
            .await?;
            println!(
                "Created {} with household {} and person {}",
                user.username,
                user.household_id.as_deref().unwrap_or("-"),
                user.person_id.as_deref().unwrap_or("-"),
            );
        }
        "household" => {
            let user = resolve(&auth, args.next()).await?;
            if args.next().is_some() {
                return usage();
            }
            let summary = onboarding::household_for_user(&queue, &auth, &user).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&summary).map_err(io::Error::other)?
            );
        }
        "add-person" => {
            let user = resolve(&auth, args.next()).await?;
            let display_name = args.next().ok_or_else(|| invalid("missing display name"))?;
            if args.next().is_some() {
                return usage();
            }
            let person = onboarding::add_household_person(&queue, &user, &display_name).await?;
            println!("Added {} ({})", person.display_name, person.id);
        }
        "link-household" => {
            let user = resolve(&auth, args.next()).await?;
            let (request, apply) = link_options(args)?;
            link_household(&auth, &queue, &user, &request, apply).await?;
        }
        "deletion-requests" => {
            if args.next().is_some() {
                return usage();
            }
            let requests = auth.pending_deletion_requests().await?;
            if requests.is_empty() {
                println!("No pending deletion requests");
            }
            for request in requests {
                println!("{}\t{}", request.requested_at, request.username);
            }
        }
        "delete-account" => {
            let user = resolve(&auth, args.next()).await?;
            let apply = match args.next().as_deref() {
                None => false,
                Some("--apply") => true,
                Some(_) => return usage(),
            };
            if args.next().is_some() {
                return usage();
            }
            // Deletion is only ever carried out for an account that asked.
            let Some(request) = auth.account_deletion_request(&user.id).await? else {
                return Err(invalid(format!(
                    "{} has not requested deletion; nothing done",
                    user.username
                )));
            };
            println!(
                "{} requested deletion at {}",
                request.username, request.requested_at
            );
            if !apply {
                println!("Dry run. Re-run with --apply to delete the account and its data.");
                return Ok(());
            }
            let store = PhotoStore::from_env()?;
            let deleted = onboarding::delete_account(&queue, &auth, &store, &user).await?;
            println!(
                "Deleted {}: {} photograph(s) removed, household {}",
                user.username,
                deleted.photos_deleted,
                if deleted.household_erased {
                    "erased"
                } else {
                    "kept for its other accounts"
                }
            );
        }
        _ => return usage(),
    }
    Ok(())
}

/// Parse the flags of `link-household`. Unknown flags are refused rather than
/// ignored, so a typo never silently turns a dry run into a write.
fn link_options(mut args: impl Iterator<Item = String>) -> io::Result<(LinkRequest, bool)> {
    let mut request = LinkRequest::default();
    let mut apply = false;
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| invalid(format!("{flag} needs a value")))
        };
        match flag.as_str() {
            "--household-id" => request.household_id = Some(value()?),
            "--person-id" => request.person_id = Some(value()?),
            "--role" => request.role = Some(value()?),
            "--name" => request.name = Some(value()?),
            "--new-person" => request.new_person = true,
            "--members" => {
                request.members = value()?
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            "--create-missing" => request.create_missing = true,
            "--apply" => apply = true,
            other => return Err(invalid(format!("unknown option {other}"))),
        }
    }
    Ok((request, apply))
}

/// Without `--apply` this prints the whole survey and the plan and writes
/// nothing, so an operator can read it before touching production data.
async fn link_household(
    auth: &AuthStore,
    queue: &ProcessingQueue,
    user: &User,
    request: &LinkRequest,
    apply: bool,
) -> io::Result<()> {
    let linkage = onboarding::account_linkage(queue, user).await?;
    println!("account {} ({})", linkage.username, linkage.user_id);
    println!("  display name:   {}", linkage.display_name);
    println!(
        "  household_id:   {}",
        linkage.household_id.as_deref().unwrap_or("(none)")
    );
    println!(
        "  person_id:      {}",
        linkage.person_id.as_deref().unwrap_or("(none)")
    );
    println!("  household_role: {}", linkage.role);
    println!(
        "  pairing table:  {}",
        linkage.legacy_household_id.as_deref().unwrap_or("(none)")
    );

    println!("\nhouseholds:");
    let households = onboarding::survey_households(queue, auth).await?;
    if households.is_empty() {
        println!("  (none)");
    }
    for listing in &households {
        println!("  {}  \"{}\"", listing.id, listing.display_name);
        println!(
            "    grid {}  devices {}  legacy pairing rows {}",
            listing.grid_size, listing.devices, listing.legacy_users
        );
        println!(
            "    members ({}): {}",
            listing.members.len(),
            if listing.members.is_empty() {
                "(none)".to_owned()
            } else {
                listing.members.join(", ")
            }
        );
        println!(
            "    accounts: {}",
            if listing.accounts.is_empty() {
                "(none)".to_owned()
            } else {
                listing.accounts.join(", ")
            }
        );
    }

    println!("\ncandidate people matching this account:");
    let candidates = onboarding::person_candidates(queue, user).await?;
    if candidates.is_empty() {
        println!("  (none)");
    }
    for candidate in &candidates {
        println!(
            "  {}  \"{}\"  household {}",
            candidate.id,
            candidate.display_name,
            candidate.household_id.as_deref().unwrap_or("(unseated)")
        );
    }

    if !apply {
        let plan = onboarding::plan_link_household(queue, auth, user, request).await?;
        println!("\nplan (dry run; nothing was written):");
        println!(
            "  household {} \"{}\"",
            plan.household_id, plan.household_name
        );
        match &plan.person {
            PlannedPerson::Existing { id, display_name } => {
                println!("  person {id} \"{display_name}\"");
            }
            PlannedPerson::Create { display_name } => {
                println!("  person: create \"{display_name}\"");
            }
        }
        println!("  role {}", plan.role);
        for member in &plan.members {
            match member {
                PlannedPerson::Existing { id, display_name } => {
                    println!("  member {id} \"{display_name}\"");
                }
                PlannedPerson::Create { display_name } => {
                    println!("  member: create \"{display_name}\"");
                }
            }
        }
        for step in &plan.steps {
            println!("  - {step}");
        }
        println!("\nre-run with --apply to execute this plan.");
        return Ok(());
    }

    let devices = DeviceRegistry::new(queue.clone());
    let outcome = onboarding::apply_link_household(queue, auth, &devices, user, request).await?;
    println!("\napplied:");
    if outcome.changes.is_empty() {
        println!("  (already linked; nothing changed)");
    }
    for change in &outcome.changes {
        println!("  - {change}");
    }
    println!(
        "\n{} is now {} of household {} as person {}",
        user.username, outcome.role, outcome.household_id, outcome.person_id
    );
    Ok(())
}

/// Signup is CLI-gated the same way the route is, so a server with signup off
/// still allows an operator to create the first account.
fn confirmed_password() -> io::Result<String> {
    let password = rpassword::prompt_password("Password (12+ characters): ")?;
    if password != rpassword::prompt_password("Confirm password: ")? {
        return Err(invalid("passwords did not match"));
    }
    Ok(password)
}

async fn resolve(auth: &AuthStore, username: Option<String>) -> io::Result<User> {
    let username = username.ok_or_else(|| invalid("missing username"))?;
    auth.user_by_username(&username)
        .await?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no user {username}")))
}

fn usage<T>() -> io::Result<T> {
    Err(invalid(concat!(
        "usage: daily-mirror-onboarding signup <username> [display-name]\n",
        "       daily-mirror-onboarding household <username>\n",
        "       daily-mirror-onboarding add-person <username> <display-name>\n",
        "       daily-mirror-onboarding link-household <username> [--household-id ID]\n",
        "           [--person-id ID] [--new-person] [--role admin|member] [--name NAME]\n",
        "           [--members \"A,B,C\"] [--create-missing] [--apply]\n",
        "       daily-mirror-onboarding deletion-requests\n",
        "       daily-mirror-onboarding delete-account <username> [--apply]"
    )))
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
