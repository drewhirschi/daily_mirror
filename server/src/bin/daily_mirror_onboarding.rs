//! Household onboarding from a server shell. The HTTP routes and this binary
//! call the same `server::onboarding` functions.
use std::io;

use server::auth::{AuthStore, User};
use server::catalog::PhotoCatalog;
use server::onboarding::{self, SignupRequest};
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
            let summary = onboarding::household_for_user(&queue, &user).await?;
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
        _ => return usage(),
    }
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
        "       daily-mirror-onboarding add-person <username> <display-name>"
    )))
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
