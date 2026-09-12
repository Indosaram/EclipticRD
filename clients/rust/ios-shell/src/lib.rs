pub mod commands;
pub mod frame;
pub mod qa;
pub mod state;

#[cfg(test)]
mod tests;

pub use maho_app::{IpcError, IpcErrorCode, IpcErrorStage, PairingEndpoint, PairingSummary};
pub use maho_render::AudioOutputEvent;
pub use state::{AppState, ConnectionState, SessionStats, WorkerCompletion, WorkerKind};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::list_hosts,
            commands::stop_discovery,
            commands::list_pairings,
            commands::forget_pairing,
            commands::connect,
            commands::disconnect,
            commands::stats,
            commands::poll_frame,
            commands::touch,
            commands::set_touch_mode,
            commands::send_key,
            commands::set_muted,
            commands::presented,
            commands::startup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running MahoRD iOS application");
}
