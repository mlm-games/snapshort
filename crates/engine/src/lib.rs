//! One entry point for every snapshort action.
//!
//! The UI, the CLI, the control channel and the MCP server all run commands
//! through [`Session::execute`] (JSON id + params) or [`Session::dispatch`]
//! (typed [`SessionCommand`]). The session owns the services and the event
//! orchestration, so headless automation behaves exactly like the app.
//!
//! [`SessionCommand`] is pure data and compiles everywhere (the web shell
//! speaks it); the session and the command registry are native, where the
//! media, render and file services live.

pub mod command;

pub mod presets;

#[cfg(not(target_arch = "wasm32"))]
pub mod commands;
#[cfg(not(target_arch = "wasm32"))]
pub mod inspect;
#[cfg(not(target_arch = "wasm32"))]
pub mod params;

#[cfg(not(target_arch = "wasm32"))]
mod session;

pub use command::SessionCommand;
#[cfg(not(target_arch = "wasm32"))]
#[cfg(not(target_arch = "wasm32"))]
pub use commands::{CommandSpec, command_specs, find as find_command};
#[cfg(not(target_arch = "wasm32"))]
pub use inspect::{media_info_json, project_json, timeline_json_from};
pub use presets::{Preset, preset_names};

#[cfg(not(target_arch = "wasm32"))]
pub use session::{RunningExport, Session};

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("command `{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error("no project is open")]
    NoProject,
    #[error("app: {0}")]
    App(#[from] snapshort_usecases::AppError),
    #[error("{0}")]
    Other(String),
}

#[cfg(not(target_arch = "wasm32"))]
pub type Result<T> = std::result::Result<T, EngineError>;

#[cfg(not(target_arch = "wasm32"))]
impl EngineError {
    pub(crate) fn bad(cmd: &str, msg: impl Into<String>) -> Self {
        Self::BadParams {
            cmd: cmd.to_string(),
            msg: msg.into(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<String> for EngineError {
    fn from(msg: String) -> Self {
        EngineError::Other(msg)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<&str> for EngineError {
    fn from(msg: &str) -> Self {
        EngineError::Other(msg.to_string())
    }
}
