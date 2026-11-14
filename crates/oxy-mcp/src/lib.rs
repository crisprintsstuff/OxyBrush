//! Local [Model Context Protocol](https://modelcontextprotocol.io) server for OxyBrush.
//!
//! The desktop app listens on `127.0.0.1` only, and only after the user opts
//! in. `oxybrush-cli mcp` speaks the same tools over stdio and never opens a
//! socket. Neither path makes an outbound connection. See
//! `docs/rfcs/0003-mcp.md`.

mod protocol;
mod session;
pub mod state;
mod stroke;
pub mod strokes;
mod tools;
mod transport;
mod view;

/// Prefix on every undo label an agent creates, so the history panel shows who did it.
pub const AGENT_LABEL: &str = "Agent: ";

/// Something that can run a tool. The CLI uses [`HeadlessSession`]; the app
/// posts the call to the UI thread and runs it on the editor.
pub trait Host: Send {
    fn call_tool(&mut self, name: &str, arguments: &serde_json::Value) -> tools::ToolResult;
}

pub use protocol::{Identity, ServerState, handle_message};
pub use session::HeadlessSession;
pub use stroke::{MaskOp, Space, StrokeParams, apply_samples, build_samples, primitive};
pub use strokes::{StrokeOp, parse_stroke_op};
pub use tools::{Action, CameraSet, ToolResult, brush_from_name, is_read_only, parse_action};
pub use transport::{
    Control, Loopback, forward_message, new_token, parse_attach, read_frame, serve_loopback, serve_stdio,
    serve_stdio_attach, write_frame,
};
pub use view::{View, symmetry_label};
