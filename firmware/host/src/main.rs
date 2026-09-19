//! The host firmware binary.
//!
//! Live mode polls the button every 20 ms and runs the same
//! `daily_mirror_core::runtime` loop the board will run. Script mode replays a
//! file of timestamped events against a virtual clock, which is how the whole
//! pairing flow runs in `cargo test`.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use daily_mirror_core::runtime::Step;
use daily_mirror_core::timing::BUTTON_POLL_MS;
use daily_mirror_firmware_host::adapters::button::{SharedButton, spawn_stdin_reader};
use daily_mirror_firmware_host::adapters::clock::{HostClock, VirtualTime};
use daily_mirror_firmware_host::adapters::log::HostLog;
use daily_mirror_firmware_host::{HostConfig, build, script};

#[derive(Debug, Parser)]
#[command(
    name = "daily-mirror-firmware-host",
    about = "Run the Daily Mirror device firmware on Linux",
    long_about = "Type `p` to press the button, `r` to release, `c` for a short click, \
                  `hold 5000` to enter pairing, and `q` to quit. Send credentials with \
                  `curl -X POST http://127.0.0.1:<port>/provision -d '{...}'`."
)]
struct Cli {
    /// Directory holding the device id, credentials, and the pending queue.
    #[arg(
        long,
        env = "DAILY_MIRROR_HOST_STORE",
        default_value = "data/device-host"
    )]
    store: PathBuf,

    /// JPEG returned by every capture.
    #[arg(
        long,
        env = "DAILY_MIRROR_HOST_FIXTURE",
        default_value = "../../feed.jpg"
    )]
    fixture: PathBuf,

    /// Port for the local provisioning link. 0 lets the OS choose.
    #[arg(
        long,
        env = "DAILY_MIRROR_HOST_PROVISIONING_PORT",
        default_value_t = 8088
    )]
    port: u16,

    /// Replay a script against a virtual clock instead of reading the keyboard.
    #[arg(long)]
    script: Option<PathBuf>,

    /// How long a script keeps ticking after its last step.
    #[arg(long, default_value_t = 1_000)]
    settle_ms: u64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.script {
        Some(path) => run_script(&cli, path),
        None => run_live(&cli),
    }
}

fn run_script(cli: &Cli, path: &PathBuf) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read the script {}", path.display()))?;
    let steps = script::parse(&text)?;
    let time = VirtualTime::new();
    let button = SharedButton::new();
    let mut runtime = build(HostConfig {
        store_dir: cli.store.clone(),
        fixture: cli.fixture.clone(),
        provisioning_port: cli.port,
        clock: HostClock::virtual_clock(time.clone()),
        button: button.clone(),
        log: HostLog::new(),
    })?;
    let outcome = script::run(&mut runtime, &time, &button, &steps, cli.settle_ms)?;
    println!(
        "script finished after {} ticks in state {}{}",
        outcome.ticks,
        outcome.final_state,
        if outcome.rebooted {
            " (reset, rebooting)"
        } else {
            ""
        }
    );
    Ok(())
}

fn run_live(cli: &Cli) -> Result<()> {
    let button = SharedButton::new();
    spawn_stdin_reader(button.clone());

    // A full reset ends with a reboot, so the live loop rebuilds the runtime
    // and boots again — the same thing the board does, minus the power cycle.
    loop {
        let mut runtime = build(HostConfig {
            store_dir: cli.store.clone(),
            fixture: cli.fixture.clone(),
            provisioning_port: cli.port,
            clock: HostClock::system(),
            button: button.clone(),
            log: HostLog::new(),
        })?;
        eprintln!("device id {}", runtime.identity().device_id);

        let mut reboot = runtime.boot() == Step::Reboot;
        while !reboot && !button.quit_requested() {
            reboot = runtime.step() == Step::Reboot;
            thread::sleep(Duration::from_millis(BUTTON_POLL_MS));
        }
        if button.quit_requested() {
            eprintln!("stopped");
            return Ok(());
        }
        eprintln!("full reset complete; rebooting into first-boot pairing");
    }
}
