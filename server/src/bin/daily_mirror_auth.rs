use std::io;

use server::auth::AuthStore;

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
    if command != "create-user" {
        return usage();
    }
    let username = args.next().ok_or_else(|| invalid("missing username"))?;
    let display_name = args.next().unwrap_or_else(|| username.clone());
    if args.next().is_some() {
        return usage();
    }

    let password = rpassword::prompt_password("Password (12+ characters): ")?;
    let confirmation = rpassword::prompt_password("Confirm password: ")?;
    if password != confirmation {
        return Err(invalid("passwords did not match"));
    }

    let user = AuthStore::from_env()?
        .create_user(&username, &display_name, &password)
        .await?;
    println!("Created account {} ({})", user.username, user.display_name);
    Ok(())
}

fn usage<T>() -> io::Result<T> {
    Err(invalid(
        "usage: daily-mirror-auth create-user <username> [display-name]",
    ))
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
