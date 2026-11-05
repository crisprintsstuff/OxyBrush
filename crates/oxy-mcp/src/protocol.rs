//! JSON-RPC and the MCP handshake.
//!
//! Speaks protocol versions 2024-11-05, 2025-03-26 and 2025-06-18. A single
//! message or a batch array is accepted. Notifications (no id) produce no
//! response.

use crate::Host;
use crate::tools::{ToolResult, tool_defs};
use serde_json::{Value, json};

pub const INSTRUCTIONS: &str = "\
You control OxyBrush, a sculpting app, on this computer, alongside the user. \
Edits are undoable and labeled \"Agent:\" in the shared history. \
Prefer a screenshot, then small screen-space strokes, and check again. \
Paths are local files only. This server makes no network calls.\
";

const VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

#[derive(Debug, Clone, Copy)]
pub struct Identity {
    pub name: &'static str,
    pub version: &'static str,
}

impl Identity {
    pub fn oxybrush() -> Self {
        Self { name: "oxybrush", version: env!("CARGO_PKG_VERSION") }
    }

    pub fn cli() -> Self {
        Self { name: "oxybrush-cli", version: env!("CARGO_PKG_VERSION") }
    }
}

#[derive(Debug, Default)]
pub struct ServerState {
    pub initialized: bool,
    pub client_name: String,
}

/// Handle one JSON-RPC body. `None` means the message was a notification.
pub fn handle_message(state: &mut ServerState, identity: Identity, host: &mut dyn Host, raw: &str) -> Option<String> {
    let value: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return Some(rpc_error(&Value::Null, -32700, "parse error")),
    };
    if let Some(batch) = value.as_array() {
        let mut out = Vec::new();
        for msg in batch {
            if let Some(resp) = handle_one(state, identity, host, msg) {
                match serde_json::from_str(&resp) {
                    Ok(v) => out.push(v),
                    Err(_) => out.push(
                        json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32603, "message": "internal error"}}),
                    ),
                }
            }
        }
        if out.is_empty() {
            return None;
        }
        return Some(
            serde_json::to_string(&Value::Array(out))
                .unwrap_or_else(|_| rpc_error(&Value::Null, -32603, "internal error")),
        );
    }
    handle_one(state, identity, host, &value)
}

fn handle_one(state: &mut ServerState, identity: Identity, host: &mut dyn Host, msg: &Value) -> Option<String> {
    if !msg.is_object() {
        return Some(rpc_error(&Value::Null, -32600, "invalid request"));
    }
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let has_id = msg.get("id").is_some_and(|v| !v.is_null());
    let method = match msg.get("method").and_then(Value::as_str) {
        Some(m) => m,
        None => return Some(rpc_error(&id, -32600, "missing method")),
    };
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    let response = match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str);
            let version = requested.filter(|v| VERSIONS.contains(v)).unwrap_or(VERSIONS[0]);
            state.client_name = params
                .get("clientInfo")
                .and_then(|c| c.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("client")
                .to_owned();
            // The client is not initialized until it sends the notification.
            state.initialized = false;
            rpc_result(
                &id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": identity.name, "version": identity.version },
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "notifications/initialized" | "initialized" => {
            state.initialized = true;
            return None;
        }
        "ping" => rpc_result(&id, json!({})),
        "logging/setLevel" => rpc_result(&id, json!({})),
        "notifications/cancelled" => return None,
        "tools/list" => {
            if let Some(err) = need_init(state, &id) {
                err
            } else {
                rpc_result(&id, json!({ "tools": tool_defs() }))
            }
        }
        "tools/call" => {
            if let Some(err) = need_init(state, &id) {
                err
            } else {
                call_tool(host, &id, &params)
            }
        }
        _ => rpc_error(&id, -32601, "method not found"),
    };
    if has_id { Some(response) } else { None }
}

fn need_init(state: &ServerState, id: &Value) -> Option<String> {
    if state.initialized { None } else { Some(rpc_error(id, -32600, "server not initialized")) }
}

fn call_tool(host: &mut dyn Host, id: &Value, params: &Value) -> String {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return rpc_error(id, -32602, "tools/call needs a name");
    };
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let result = host.call_tool(name, &args);
    tool_result(id, result)
}

fn tool_result(id: &Value, result: ToolResult) -> String {
    let mut content = vec![json!({ "type": "text", "text": result.text })];
    if let Some(png) = result.image_png {
        content.push(json!({
            "type": "image",
            "data": base64_encode(&png),
            "mimeType": "image/png",
        }));
    }
    rpc_result(id, json!({ "content": content, "isError": result.is_error }))
}

fn rpc_result(id: &Value, result: Value) -> String {
    serde_json::to_string(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))
        .unwrap_or_else(|_| rpc_error(id, -32603, "internal error"))
}

fn rpc_error(id: &Value, code: i32, message: &str) -> String {
    serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    }))
    .unwrap_or_else(|_| {
        "{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32603,\"message\":\"internal error\"}}".into()
    })
}

fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push(T[(n & 63) as usize] as char);
        i += 3;
    }
    let rest = data.len() - i;
    if rest == 1 {
        let n = (data[i] as u32) << 16;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rest == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(&[0, 1, 2, 253, 254, 255]), "AAEC/f7/");
    }
}
