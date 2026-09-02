use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use clap::Parser;
use erd_app::{ClientSession, LatencyRecorder, PairingRecord, SessionConfig, SessionEvent};
use erd_decode::HevcDecoder;
use erd_proto::{Capabilities, ControlMessage, InputEvent, InputEventType, Modifiers};
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

const DEFAULT_TCP_PORT: u16 = 19730;
const DEFAULT_UDP_PORT: u16 = 19731;
const DEFAULT_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Parser)]
#[command(
    name = "erd-client",
    about = "Ecliptic Remote Desktop headless testing client for VM E2E driving",
    version
)]
struct Cli {
    /// Remote host address (IPv4, IPv6, or hostname).
    #[arg(long)]
    host: String,

    /// Remote host TCP TLS-PSK signaling port.
    #[arg(long, default_value_t = DEFAULT_TCP_PORT)]
    tcp_port: u16,

    /// Remote host UDP media port.
    #[arg(long)]
    udp_port: Option<u16>,

    /// 8-digit bootstrap PIN for pairing with the host.
    #[arg(long, conflicts_with = "psk_hex")]
    pin: Option<String>,

    /// 64-character hexadecimal pre-shared key (32 bytes) if pairing store is unavailable.
    #[arg(long, conflicts_with = "pin")]
    psk_hex: Option<String>,

    /// Send a tiny alternating mouse-move every N ms once streaming starts.
    /// Drives screens that only produce frames on change (static wlroots
    /// sessions) and exercises the host input-injection path.
    #[arg(long)]
    nudge_ms: Option<u64>,

    /// Pairing ID associated with psk-hex or pairing store reconnect.
    #[arg(long)]
    pairing_id: Option<String>,

    /// Optional explicit path to pairing store file.
    #[arg(long)]
    pairing_store: Option<PathBuf>,

    /// Exit 0 immediately after successfully decoding N video frames.
    #[arg(long)]
    frames: Option<u64>,

    /// Path to write JSON latency statistics upon completion.
    #[arg(long)]
    stats_json: Option<PathBuf>,

    /// Overall execution timeout in seconds.
    #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS)]
    timeout_secs: u64,

    /// Name of client sent in handshake.
    #[arg(long, default_value = "erd-headless-client")]
    client_name: String,
}

fn parse_hex_32(hex_str: &str) -> Result<Vec<u8>> {
    let trimmed = hex_str.trim();
    if trimmed.len() != 64 {
        bail!(
            "psk-hex must be exactly 64 hex characters (32 bytes), got length {}",
            trimmed.len()
        );
    }
    let mut bytes = Vec::with_capacity(32);
    for i in (0..64).step_by(2) {
        let byte = u8::from_str_radix(&trimmed[i..i + 2], 16)
            .with_context(|| format!("invalid hex digit pair '{}'", &trimmed[i..i + 2]))?;
        bytes.push(byte);
    }
    Ok(bytes)
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    run_client(cli)
}

fn run_client(cli: Cli) -> Result<()> {
    let udp_port = cli.udp_port.unwrap_or_else(|| {
        if cli.tcp_port == DEFAULT_TCP_PORT {
            DEFAULT_UDP_PORT
        } else {
            cli.tcp_port.saturating_add(1)
        }
    });

    let target_frames = cli.frames.unwrap_or(u64::MAX);
    let timeout = Duration::from_secs(cli.timeout_secs);
    let start_time = Instant::now();

    let session_config = SessionConfig {
        host: cli.host.clone(),
        tcp_port: cli.tcp_port,
        udp_port,
        client_name: cli.client_name.clone(),
        capabilities: Capabilities::STREAM_CONFIGURATION | Capabilities::TEXT_CLIPBOARD_SYNC,
        pairing_store_path: cli.pairing_store.clone(),
        connect_timeout: Duration::from_secs(10),
        handshake_ack_timeout: Duration::from_secs(10),
    };

    info!(
        host = %cli.host,
        tcp_port = cli.tcp_port,
        udp_port = udp_port,
        timeout_secs = cli.timeout_secs,
        "Initializing headless client session"
    );

    let session =
        ClientSession::new(session_config).context("failed to construct ClientSession")?;

    let ready = match (&cli.pin, &cli.psk_hex) {
        (Some(pin), None) => {
            info!("Initiating bootstrap pairing with host via PIN");
            session
                .pair_with_pin(pin)
                .context("bootstrap pairing failed")?
        }
        (None, Some(psk_hex)) => {
            let key = parse_hex_32(psk_hex).context("invalid --psk-hex argument")?;
            let pairing_id = cli
                .pairing_id
                .clone()
                .unwrap_or_else(|| "00000000-0000-0000-0000-000000000000".to_string());
            let pairing = PairingRecord {
                id: pairing_id,
                name: cli.host.clone(),
                key,
                added_at_unix_ms: 0,
            };
            info!(pairing_id = %pairing.id, "Connecting with explicit PSK");
            session
                .connect_with_pairing(pairing)
                .context("handshake with explicit PSK failed")?
        }
        (None, None) => {
            if let Some(pairing_id) = &cli.pairing_id {
                info!(pairing_id = %pairing_id, "Reconnecting with stored pairing record");
                session
                    .reconnect(pairing_id)
                    .context("reconnect with stored pairing failed")?
            } else {
                bail!("either --pin, --psk-hex, or --pairing-id must be provided");
            }
        }
        (Some(_), Some(_)) => {
            unreachable!("clap enforces conflicts between pin and psk_hex");
        }
    };

    info!(
        host_name = %ready.server.name,
        width = ready.server.width,
        height = ready.server.height,
        version = ready.server.version,
        "Handshake completed, session ready"
    );

    // Spawn TCP control loop
    let mut tcp_runtime = session
        .spawn_tcp_runtime()
        .context("failed to spawn TCP runtime")?;

    session
        .set_udp_read_timeout(Some(Duration::from_millis(5)))
        .context("failed to set UDP read timeout")?;

    let running = Arc::new(AtomicBool::new(true));
    let r_ctrl = running.clone();
    let _ = ctrlc_handler(move || {
        r_ctrl.store(false, Ordering::SeqCst);
    });

    let first_frame_seen = Arc::new(AtomicBool::new(false));

    let mut latency_recorder = LatencyRecorder::new();
    let mut decoder: Option<HevcDecoder> = None;
    let mut decoded_frames: u64 = 0;

    let (frame_tx, frame_rx) = mpsc::sync_channel::<(erd_app::AssembledFrame, Instant)>(1024);
    let session_udp = session.clone();
    let r_udp = running.clone();

    // Optional cursor-nudge driver: keeps compositors that only produce frames
    // on change (static wlroots sessions) feeding the pipeline, and exercises
    // the host input-injection path end-to-end.
    if let Some(nudge_ms) = cli.nudge_ms.filter(|value| *value > 0) {
        let session_nudge = session.clone();
        let r_nudge = running.clone();
        let first = Arc::clone(&first_frame_seen);
        std::thread::Builder::new()
            .name("erd-client-nudge".into())
            .spawn(move || {
                while !first.load(Ordering::Relaxed) && r_nudge.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(100));
                }
                let mut flip = false;
                while r_nudge.load(Ordering::Relaxed) {
                    let x = if flip { 0.501_5 } else { 0.5 };
                    flip = !flip;
                    let event = InputEvent {
                        event_type: InputEventType::MouseMove,
                        x,
                        y: 0.5,
                        key_code: 0,
                        modifiers: Modifiers::empty(),
                        scroll_dx: 0.0,
                        scroll_dy: 0.0,
                    };
                    if let Err(error) = session_nudge.send_input(event) {
                        debug!(%error, "nudge input rejected");
                    }
                    std::thread::sleep(Duration::from_millis(nudge_ms));
                }
            })
            .ok();
    }

    let udp_receiver_handle = std::thread::Builder::new()
        .name("erd-client-udp-receiver".into())
        .spawn(move || {
            while r_udp.load(Ordering::Relaxed) {
                match session_udp.receive_udp_event() {
                    Ok(SessionEvent::Frame(assembled_frame)) => {
                        let receive_ts = Instant::now();
                        if frame_tx.send((assembled_frame, receive_ts)).is_err() {
                            break;
                        }
                    }
                    Ok(SessionEvent::Ping) => {
                        debug!("UDP ping received");
                    }
                    Ok(_) => {}
                    Err(err) => {
                        if let erd_app::SessionError::Io(ref io_err) = err {
                            if matches!(
                                io_err.kind(),
                                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                            ) {
                                std::thread::yield_now();
                                continue;
                            }
                        }
                    }
                }
            }
        })
        .context("failed to spawn UDP receiver thread")?;

    info!("Awaiting UDP media datagrams and decoding frames...");

    while running.load(Ordering::Relaxed) {
        if start_time.elapsed() >= timeout {
            error!(
                decoded_frames,
                target_frames, "Session timed out after {} seconds", cli.timeout_secs
            );
            break;
        }

        if decoded_frames >= target_frames {
            info!(decoded_frames, "Reached requested target frame count");
            break;
        }

        // Check if TCP runtime emitted an error
        while let Ok(event_res) = tcp_runtime.events().try_recv() {
            match event_res {
                Ok(SessionEvent::Ping) => {
                    debug!("TCP heartbeat ping acknowledged");
                }
                Ok(SessionEvent::Clipboard(_text)) => {
                    debug!("Received clipboard update");
                }
                Ok(_) => {}
                Err(err) => {
                    // Non-fatal or timeout errors in polling shouldn't abort immediately unless fatal
                    warn!(%err, "TCP runtime poll error");
                }
            }
        }

        match frame_rx.recv_timeout(Duration::from_millis(5)) {
            Ok((assembled_frame, receive_ts)) => {
                let is_key = assembled_frame.header.is_key_frame;
                let data = &assembled_frame.data;
                debug!(
                    is_key,
                    size = data.len(),
                    "Received assembled video frame from UDP"
                );

                if decoder.is_none() {
                    match HevcDecoder::from_keyframe_auto(data) {
                        Ok((codec_kind, dec)) => {
                            info!(?codec_kind, "decoder initialized from keyframe");
                            decoder = Some(dec);
                        }
                        Err(err) => {
                            if is_key {
                                warn!(%err, "Failed to initialize HEVC decoder from frame");
                            }
                        }
                    }
                }

                let decode_start = Instant::now();
                let mut decoded_any = false;
                if let Some(dec) = decoder.as_mut() {
                    match dec.decode(data, assembled_frame.timestamp_ms as i64) {
                        Ok(nv12_frames) => {
                            if !nv12_frames.is_empty() {
                                decoded_any = true;
                                decoded_frames =
                                    decoded_frames.saturating_add(nv12_frames.len() as u64);
                                first_frame_seen.store(true, Ordering::Relaxed);
                            }
                        }
                        Err(err) => {
                            debug!(%err, is_key, size = data.len(), "Frame decode error");
                        }
                    }
                }

                let present_ts = Instant::now();
                if decoded_any {
                    // Record latency sample: receive/capture -> decode -> present
                    latency_recorder.record_sample(Some(receive_ts), decode_start, present_ts);
                    if decoded_frames <= 10
                        || decoded_frames % 20 == 0
                        || decoded_frames >= target_frames
                    {
                        info!(decoded_frames, target_frames, "Decoded frame progress");
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }

    running.store(false, Ordering::SeqCst);
    let reached_target = decoded_frames >= target_frames;

    info!(
        decoded_frames,
        target_frames, "Cleaning up session transport..."
    );
    teardown(&session, &mut tcp_runtime)?;
    let _ = udp_receiver_handle.join();

    let stats_json = latency_recorder.stats_json();
    info!(stats = %stats_json, "Latency statistics summary");

    write_stats_file(cli.stats_json.as_deref(), &latency_recorder)?;

    if !reached_target && start_time.elapsed() >= timeout {
        bail!("timeout expired before decoding requested frames (got {decoded_frames}/{target_frames})");
    }

    let stats_json = latency_recorder.stats_json();
    info!(stats = %stats_json, "Latency statistics summary");

    write_stats_file(cli.stats_json.as_deref(), &latency_recorder)?;

    Ok(())
}

fn teardown(session: &ClientSession, tcp_runtime: &mut erd_app::SessionRuntime) -> Result<()> {
    // Send Disconnect / BYE control message
    let _ = session.send_control(ControlMessage::Disconnect);
    tcp_runtime.stop();
    let _ = session.disconnect();
    Ok(())
}

fn write_stats_file(path: Option<&std::path::Path>, recorder: &LatencyRecorder) -> Result<()> {
    if let Some(stats_path) = path {
        let json = recorder.stats_json();
        if let Some(parent) = stats_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(stats_path, json.as_bytes())
            .with_context(|| format!("failed to write stats JSON to {}", stats_path.display()))?;
        info!("Stats JSON written to {}", stats_path.display());
    }
    Ok(())
}

fn ctrlc_handler<F>(f: F) -> Result<()>
where
    F: Fn() + Send + Sync + 'static,
{
    // Best-effort ctrl-c handler without extra crate
    // On Unix, standard signal hooks can be set or ignored gracefully.
    #[cfg(unix)]
    {
        use std::sync::Once;
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            // Nothing required if signal handler isn't needed, standard SIGINT exits process
        });
    }
    let _ = f;
    Ok(())
}
