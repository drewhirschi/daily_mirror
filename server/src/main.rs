// Local process entry point. Application routes and domain wiring live in
// src/app.rs so deployment adapters can run the exact same Router.

use std::net::{IpAddr, Ipv4Addr};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let app = server::app();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let host = std::env::var("HOST")
        .ok()
        .and_then(|value| value.parse::<IpAddr>().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let listener = bind_with_fallback(host, port).await;
    let local = listener.local_addr().expect("listener has a local addr");
    println!("listening on http://{local}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

/// Bind `0.0.0.0:start`, or the next free port up to `start + 20` if it's taken.
async fn bind_with_fallback(host: IpAddr, start: u16) -> tokio::net::TcpListener {
    for port in start..start.saturating_add(20) {
        match tokio::net::TcpListener::bind((host, port)).await {
            Ok(listener) => {
                if port != start {
                    eprintln!("Port {start} is in use; bound {port} instead (set PORT to choose).");
                }
                return listener;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(e) => {
                eprintln!("Failed to bind {host}:{port}: {e}");
                std::process::exit(1);
            }
        }
    }
    eprintln!(
        "No free port in {start}..{}. Stop the process using it, or set PORT.",
        start.saturating_add(20)
    );
    std::process::exit(1);
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
