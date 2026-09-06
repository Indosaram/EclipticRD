//! Cross-platform Ecliptic Remote Desktop client orchestration.

mod abr;
pub mod agent_input;
pub mod agent_server;
mod audio;
mod clipboard;
mod input;
pub mod latency;
pub mod mcp_server;
mod media;
mod pairing;
mod session;

pub use abr::*;
pub use agent_input::*;
pub use agent_server::*;
pub use audio::*;
pub use clipboard::*;
pub use input::*;
pub use latency::*;
pub use mcp_server::*;
pub use media::*;
pub use pairing::*;
pub use session::*;
