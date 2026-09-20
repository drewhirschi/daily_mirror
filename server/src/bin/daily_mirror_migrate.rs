//! Apply and inspect the versioned schema migrations in `server/migrations/`.
//!
//!   daily-mirror-migrate status            # read-only: applied and pending
//!   daily-mirror-migrate up                # apply everything pending
//!   daily-mirror-migrate up --dry-run      # say what `up` would do
//!
//! The database is chosen exactly as every other binary chooses it:
//! `DAILY_MIRROR_DATABASE_URL`, plus `DAILY_MIRROR_DATABASE_AUTH_TOKEN` for a
//! Turso URL. `status` writes nothing at all, so it is safe against
//! production; `up` is what `server/scripts/deploy-prebuilt.sh` runs before a
//! production deploy.

use std::io;
use std::process::ExitCode;

use libsql::{Builder, Connection};
use server::migrations;

#[tokio::main]
async fn main() -> ExitCode {
    // Match the server and the other bins: an explicit process variable wins.
    dotenvy::from_filename(".env.local").ok();
    dotenvy::dotenv().ok();

    let mut arguments = std::env::args().skip(1);
    let command = arguments.next().unwrap_or_else(|| "status".to_owned());
    let mut dry_run = false;
    for argument in arguments {
        match argument.as_str() {
            "--dry-run" => dry_run = true,
            other => {
                eprintln!("unknown option: {other}");
                return usage();
            }
        }
    }

    let result = match command.as_str() {
        "status" => status().await,
        "up" => up(dry_run).await,
        "--help" | "-h" | "help" => return usage(),
        other => {
            eprintln!("unknown command: {other}");
            return usage();
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: daily-mirror-migrate <status|up> [--dry-run]\n\
         \n\
         The database comes from DAILY_MIRROR_DATABASE_URL (and\n\
         DAILY_MIRROR_DATABASE_AUTH_TOKEN for Turso). `status` never writes."
    );
    ExitCode::FAILURE
}

/// Never printed: the auth token. Only the host is echoed, so a mistyped
/// environment is obvious without leaking a credential into CI logs.
async fn connect() -> io::Result<(Connection, String)> {
    let url = std::env::var("DAILY_MIRROR_DATABASE_URL").unwrap_or_default();
    if url.starts_with("libsql://") || url.starts_with("https://") {
        let token = std::env::var("DAILY_MIRROR_DATABASE_AUTH_TOKEN").map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "DAILY_MIRROR_DATABASE_AUTH_TOKEN is required for a Turso URL",
            )
        })?;
        let host = url
            .split_once("://")
            .map(|(_, rest)| rest.to_owned())
            .unwrap_or_else(|| url.clone());
        let database = Builder::new_remote(url, token)
            .build()
            .await
            .map_err(io::Error::other)?;
        let connection = database.connect().map_err(io::Error::other)?;
        return Ok((connection, host));
    }
    let path = if url.is_empty() {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data/daily-mirror.db")
            .to_string_lossy()
            .into_owned()
    } else {
        url
    };
    if let Some(parent) = std::path::Path::new(&path).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let database = Builder::new_local(&path)
        .build()
        .await
        .map_err(io::Error::other)?;
    let connection = database.connect().map_err(io::Error::other)?;
    Ok((connection, path))
}

async fn status() -> io::Result<()> {
    let (connection, target) = connect().await?;
    let status = migrations::status(&connection).await?;
    println!("database: {target}");
    println!(
        "schema version: {} (this build expects {})",
        status.current_version(),
        status.expected_version
    );
    println!("\napplied:");
    if status.applied.is_empty() {
        println!("  (none — this database has never been migrated)");
    }
    for entry in &status.applied {
        println!(
            "  {:04} {:<24} {}",
            entry.version, entry.name, entry.applied_at
        );
    }
    println!("\npending:");
    if status.pending.is_empty() {
        println!("  (none)");
    }
    for migration in &status.pending {
        println!("  {:04} {}", migration.version, migration.name);
    }
    if !status.drift.is_empty() {
        println!("\nDRIFT — this build disagrees with the recorded history:");
        for entry in &status.drift {
            println!("  {entry}");
        }
        return Err(io::Error::other(
            "migration history does not match this build",
        ));
    }
    Ok(())
}

async fn up(dry_run: bool) -> io::Result<()> {
    let (connection, target) = connect().await?;
    let (status, ran) = migrations::apply(&connection, dry_run).await?;
    println!("database: {target}");
    if ran.is_empty() {
        println!(
            "already at version {} — nothing to do",
            status.current_version()
        );
        return Ok(());
    }
    for migration in &ran {
        println!(
            "{} {:04} {}",
            if dry_run { "would apply" } else { "applied" },
            migration.version,
            migration.name
        );
    }
    if dry_run {
        println!(
            "dry run: {} migration(s) pending; nothing was written",
            ran.len()
        );
    } else {
        println!("schema version is now {}", status.current_version());
    }
    Ok(())
}
