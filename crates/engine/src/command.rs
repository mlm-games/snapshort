//! The command the app speaks, on every platform: pure data, no services.
//!
//! The UI sends these over a channel; the CLI and the MCP server send the JSON
//! form of them. Keeping the type free of the session means the web shell and
//! the desktop shell share one command vocabulary.

use miniter_usecases::EditCommand;
use snapshort_usecases::{
    AssetCommand, PlaybackCommand, PreviewCommand, ProjectCommand, RenderCommand,
};

#[derive(Debug, Clone)]
pub enum SessionCommand {
    Project(ProjectCommand),
    /// An edit plus its undo label; a non-empty label folds consecutive
    /// same-label edits (slider ticks, keystrokes) into one undo step.
    Edit {
        cmd: Box<EditCommand>,
        label: String,
    },
    Asset(AssetCommand),
    Playback(PlaybackCommand),
    Preview(PreviewCommand),
    Render(RenderCommand),
    /// The editor selection (clips and/or tracks).
    Select {
        clips: Vec<miniter_domain::ClipId>,
        tracks: Vec<miniter_domain::TrackId>,
    },
    /// A JSON command, for UI actions that are engine commands at heart.
    RunJson {
        id: String,
        params: serde_json::Value,
    },
    Undo,
    Redo,
}
