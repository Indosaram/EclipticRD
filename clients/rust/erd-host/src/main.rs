use std::io::{self, BufRead, Write};
use std::sync::mpsc;

use anyhow::{bail, Context, Result};
use clap::Parser;
use erd_host::{
    accessibility_is_trusted, random_pin, request_accessibility, ConsentPrompt, HostConfig,
    HostServer, PairingStore,
};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "erd-host",
    version,
    about = "EclipticRD v3 macOS screen streaming host",
    long_about = "EclipticRD v3 macOS screen streaming host.\n\nFirst run: macOS prompts for Screen Recording and Accessibility. Grant both in System Settings > Privacy & Security, then relaunch the host. Screen Recording is required for capture; Accessibility is required for remote input injection."
)]
struct Cli {
    /// Use this exact 8-digit bootstrap PIN.
    #[arg(long, value_name = "PIN", conflicts_with = "pin")]
    bootstrap_pin: Option<String>,

    /// Generate and display a fresh 8-digit bootstrap PIN (`--pin generate`).
    #[arg(long, value_name = "generate", conflicts_with = "bootstrap_pin")]
    pin: Option<String>,

    /// List persisted paired devices and exit.
    #[arg(long, conflicts_with_all = ["revoke", "bootstrap_pin", "pin"])]
    list_paired: bool,

    /// Revoke one pairing ID and exit.
    #[arg(long, value_name = "ID", conflicts_with_all = ["list_paired", "bootstrap_pin", "pin"])]
    revoke: Option<String>,

    /// LAN HEVC bitrate in Mbps. Accepted range: 50 through 150.
    /// Without this flag the compatibility default is 8 Mbps.
    #[arg(long, value_name = "MBPS", value_parser = clap::value_parser!(u32).range(50..=150))]
    lan_bitrate_mbps: Option<u32>,

    /// Disable ScreenCaptureKit host-audio capture.
    #[arg(long)]
    no_audio: bool,

    /// Automatically approve incoming pairing requests (non-interactive / automation).
    #[arg(long)]
    auto_approve: bool,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let store = PairingStore::host_default()?;
    if cli.list_paired {
        let mut records = store.load_all()?;
        records.sort_by(|left, right| right.added_at_unix_ms.cmp(&left.added_at_unix_ms));
        if records.is_empty() {
            println!("No paired devices.");
        } else {
            for record in records {
                println!(
                    "{}\t{}\t{}",
                    record.id, record.name, record.added_at_unix_ms
                );
            }
        }
        return Ok(());
    }
    if let Some(id) = cli.revoke {
        if store.revoke(&id)? {
            println!("Revoked pairing {id}");
        } else {
            bail!("pairing ID not found: {id}");
        }
        return Ok(());
    }

    let pin = match (cli.bootstrap_pin, cli.pin.as_deref()) {
        (Some(pin), None) => validate_pin(pin)?,
        (None, Some("generate")) | (None, None) => random_pin(),
        (None, Some(other)) => bail!("--pin accepts only 'generate', got '{other}'"),
        (Some(_), Some(_)) => unreachable!("clap enforces conflicts"),
    };

    onboard_permissions();

    let auto_approve = cli.auto_approve;
    let (consent_tx, consent_rx) = mpsc::channel::<ConsentPrompt>();
    std::thread::Builder::new()
        .name("erd-host-consent".into())
        .spawn(move || {
            if auto_approve {
                while let Ok(prompt) = consent_rx.recv() {
                    prompt.respond(true);
                }
            } else {
                consent_loop(consent_rx);
            }
        })
        .context("failed to start consent UI channel")?;

    let mut config = HostConfig::macos_default(Some(pin.clone()), store)?;
    config.capture_audio = !cli.no_audio;
    config.consent_sender = Some(consent_tx);
    if let Some(mbps) = cli.lan_bitrate_mbps {
        config.bitrate = mbps * 1_000_000;
    }

    let server = HostServer::bind(config)?;
    println!("EclipticRD bootstrap PIN: {pin}");
    println!("TCP listening on {}", server.tcp_addr()?);
    println!("UDP listening on {}", server.udp_addr()?);
    println!("Pairing approval requests will appear in this terminal.");
    server.serve()?;
    Ok(())
}

fn validate_pin(pin: String) -> Result<String> {
    if pin.len() != 8 || !pin.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("bootstrap PIN must be exactly 8 decimal digits");
    }
    Ok(pin)
}

fn consent_loop(receiver: mpsc::Receiver<ConsentPrompt>) {
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    while let Ok(prompt) = receiver.recv() {
        print!("Approve pairing for '{}' [y/N]? ", prompt.client_name);
        let _ = io::stdout().flush();
        let approved = lines
            .next()
            .and_then(Result::ok)
            .is_some_and(|line| matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"));
        prompt.respond(approved);
    }
}

#[cfg(target_os = "macos")]
fn onboard_permissions() {
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    let screen_recording = unsafe { CGPreflightScreenCaptureAccess() };
    if !screen_recording {
        eprintln!(
            "Screen Recording permission is required. macOS will prompt now; grant it in System Settings > Privacy & Security > Screen Recording, then relaunch erd-host."
        );
        let _ = unsafe { CGRequestScreenCaptureAccess() };
    }
    if !accessibility_is_trusted() {
        eprintln!(
            "Accessibility permission is required for remote input. macOS will prompt now; enable erd-host (or this terminal) in System Settings > Privacy & Security > Accessibility, then relaunch."
        );
        let _ = request_accessibility();
    }
}

#[cfg(not(target_os = "macos"))]
fn onboard_permissions() {
    eprintln!("erd-host capture and input require macOS; this build can run protocol tests only.");
}
