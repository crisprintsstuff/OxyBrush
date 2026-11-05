//! stdio framing and the loopback HTTP listener.
//!
//! The listener binds `127.0.0.1` only. Every request needs the session
//! token. An `Origin` header, when a client sends one, must name this
//! loopback port — that is what stops a web page from driving the server
//! through DNS rebinding. Nothing here dials out except
//! [`forward_message`], which refuses any address other than `127.0.0.1`.

use crate::Host;
use crate::protocol::{Identity, ServerState, handle_message};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_BODY: usize = 8 * 1024 * 1024;
/// How long after the last authenticated request the app shows "agent connected".
pub const ACTIVE_WINDOW: Duration = Duration::from_secs(45);

/// Shared with the UI: the token, whether an agent has spoken recently, and stop.
pub struct Control {
    token: Mutex<String>,
    last_ms: AtomicU64,
    stop: AtomicBool,
}

impl Control {
    pub fn new(token: String) -> Self {
        Self { token: Mutex::new(token), last_ms: AtomicU64::new(0), stop: AtomicBool::new(false) }
    }

    pub fn token(&self) -> String {
        self.token.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Invalidate every client holding the old token and clear the indicator.
    pub fn rotate(&self) -> String {
        let next = new_token().unwrap_or_default();
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
        self.last_ms.store(0, Ordering::Relaxed);
        next
    }

    fn mark(&self) {
        self.last_ms.store(now_ms(), Ordering::Relaxed);
    }

    /// Forget the "agent connected" indicator without changing the token.
    fn clear_activity(&self) {
        self.last_ms.store(0, Ordering::Relaxed);
    }

    pub fn is_active(&self) -> bool {
        let last = self.last_ms.load(Ordering::Relaxed);
        last != 0 && now_ms().saturating_sub(last) < ACTIVE_WINDOW.as_millis() as u64
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

pub fn new_token() -> Result<String, String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|e| format!("could not generate a session token: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub struct Loopback {
    pub addr: SocketAddr,
    /// `http://127.0.0.1:<port>/mcp`
    pub endpoint: String,
    pub control: Arc<Control>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.control.request_stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Listen on `127.0.0.1` with an ephemeral port. `host` is called on the server thread.
pub fn serve_loopback(identity: Identity, host: Box<dyn Host>, control: Arc<Control>) -> Result<Loopback, String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| format!("could not bind 127.0.0.1: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    if addr.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err(format!("refusing to listen on {} (not 127.0.0.1)", addr.ip()));
    }
    let endpoint = format!("http://{}/mcp", SocketAddr::from((Ipv4Addr::LOCALHOST, addr.port())));
    let control_thread = Arc::clone(&control);
    let thread = std::thread::Builder::new()
        .name("oxy-mcp".into())
        .spawn(move || {
            let mut host = host;
            accept_loop(listener, identity, host.as_mut(), &control_thread);
        })
        .map_err(|e| e.to_string())?;
    Ok(Loopback { addr, endpoint, control, thread: Some(thread) })
}

fn accept_loop(listener: TcpListener, identity: Identity, host: &mut dyn Host, control: &Control) {
    let mut state = ServerState::default();
    while !control.stopped() {
        match listener.accept() {
            Ok((stream, peer)) => {
                // The bind address is already loopback; still drop anything else.
                if peer.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
                    continue;
                }
                let _ = stream.set_nonblocking(false);
                let _ = handle_connection(stream, identity, host, control, &mut state);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }
}

fn handle_connection(
    stream: TcpStream,
    identity: Identity,
    host: &mut dyn Host,
    control: &Control,
    state: &mut ServerState,
) -> std::io::Result<()> {
    let _ = stream.set_nonblocking(false);
    let port = stream.local_addr().map(|a| a.port()).unwrap_or(0);
    let mut reader = BufReader::new(stream);
    let request = match read_http(&mut reader) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    let origin = header(&request.headers, "origin").map(str::to_owned);
    let response = respond(&request, port, origin.as_deref(), identity, host, control, state);
    let stream = reader.get_mut();
    stream.write_all(&response)?;
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_http(reader: &mut BufReader<TcpStream>) -> Result<HttpRequest, String> {
    reader.get_ref().set_read_timeout(Some(Duration::from_secs(15))).ok();
    let mut first = String::new();
    if reader.read_line(&mut first).map_err(|e| e.to_string())? == 0 {
        return Err("closed".into());
    }
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    let mut used = first.len();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break;
        }
        used += line.len();
        if used > 64 * 1024 {
            return Err("headers too large".into());
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let len = header(&headers, "content-length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return Err("body too large".into());
    }
    let mut body = vec![0u8; len];
    if len > 0 {
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    }
    let path = target.split('?').next().unwrap_or("/").to_string();
    Ok(HttpRequest { method, path, headers, body })
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn respond(
    request: &HttpRequest,
    port: u16,
    origin: Option<&str>,
    identity: Identity,
    host: &mut dyn Host,
    control: &Control,
    state: &mut ServerState,
) -> Vec<u8> {
    if request.path != "/mcp" && request.path != "/mcp/" {
        return http(404, "Not Found", "text/plain", "", b"not found");
    }
    if let Some(origin) = origin
        && !origin_allowed(origin, port)
    {
        return http(403, "Forbidden", "application/json", "", br#"{"error":"origin not allowed"}"#);
    }
    let cors = origin.map(cors_headers).unwrap_or_default();
    if request.method.eq_ignore_ascii_case("OPTIONS") {
        let extra = format!(
            "{cors}Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nAccess-Control-Max-Age: 600\r\n"
        );
        return http(204, "No Content", "text/plain", &extra, b"");
    }
    let host_ok = header(&request.headers, "host").is_some_and(|h| host_allowed(h, port));
    if !host_ok {
        return http(400, "Bad Request", "application/json", &cors, br#"{"error":"host must be loopback"}"#);
    }
    if request.method.eq_ignore_ascii_case("GET") {
        let extra = format!("{cors}Allow: POST, DELETE, OPTIONS\r\n");
        return http(
            405,
            "Method Not Allowed",
            "application/json",
            &extra,
            br#"{"error":"POST JSON-RPC to /mcp. Clients that only speak stdio should run oxybrush-cli mcp --attach."}"#,
        );
    }
    let presented = header(&request.headers, "authorization").and_then(bearer).unwrap_or("");
    if !tokens_equal(presented, &control.token()) {
        return http(401, "Unauthorized", "application/json", &cors, br#"{"error":"unauthorized"}"#);
    }
    if request.method.eq_ignore_ascii_case("DELETE") {
        control.clear_activity();
        return http(200, "OK", "application/json", &cors, b"{}");
    }
    if !request.method.eq_ignore_ascii_case("POST") {
        return http(405, "Method Not Allowed", "application/json", &cors, br#"{"error":"method not allowed"}"#);
    }
    let text = match std::str::from_utf8(&request.body) {
        Ok(t) => t,
        Err(_) => return http(400, "Bad Request", "application/json", &cors, br#"{"error":"body is not utf-8"}"#),
    };
    control.mark();
    match handle_message(state, identity, host, text) {
        None => http(202, "Accepted", "application/json", &cors, b""),
        Some(body) => http(200, "OK", "application/json", &cors, body.as_bytes()),
    }
}

fn origin_allowed(origin: &str, port: u16) -> bool {
    let origin = origin.trim().trim_end_matches('/');
    let allowed = [format!("http://127.0.0.1:{port}"), format!("http://localhost:{port}")];
    allowed.iter().any(|item| origin.eq_ignore_ascii_case(item))
}

fn host_allowed(host: &str, port: u16) -> bool {
    let host = host.trim();
    host.eq_ignore_ascii_case(&format!("127.0.0.1:{port}")) || host.eq_ignore_ascii_case(&format!("localhost:{port}"))
}

fn bearer(value: &str) -> Option<&str> {
    let value = value.trim();
    value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer ")).map(str::trim)
}

fn tokens_equal(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= (x ^ y) as usize;
    }
    diff == 0
}

fn cors_headers(origin: &str) -> String {
    format!(
        "Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\nAccess-Control-Allow-Headers: Content-Type, Authorization, Mcp-Session-Id, MCP-Protocol-Version\r\nAccess-Control-Expose-Headers: Mcp-Session-Id\r\n"
    )
}

fn http(status: u16, reason: &str, content_type: &str, extra: &str, body: &[u8]) -> Vec<u8> {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
        body.len()
    );
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    out
}

/// Read one LSP-style `Content-Length` frame. `None` at clean EOF.
pub fn read_frame(reader: &mut impl BufRead) -> std::io::Result<Option<String>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            length = Some(value.trim().parse::<usize>().map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("bad Content-Length: {e}"))
            })?);
        }
    }
    let Some(len) = length else {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length"));
    };
    if len > MAX_BODY {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "message too large"));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    String::from_utf8(buf).map(Some).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub fn write_frame(mut writer: impl Write, body: &str) -> std::io::Result<()> {
    let bytes = body.as_bytes();
    write!(writer, "Content-Length: {}\r\n\r\n", bytes.len())?;
    writer.write_all(bytes)?;
    writer.flush()
}

/// Speak MCP on stdin/stdout. Log nothing to stdout; it is the protocol stream.
pub fn serve_stdio(identity: Identity, host: &mut dyn Host) -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    let mut state = ServerState::default();
    while let Some(body) = read_frame(&mut reader)? {
        if let Some(response) = handle_message(&mut state, identity, host, &body) {
            write_frame(&mut writer, &response)?;
        }
    }
    Ok(())
}

/// `127.0.0.1:PORT` or `localhost:PORT` (always connected as 127.0.0.1).
pub fn parse_attach(spec: &str) -> Result<SocketAddr, String> {
    let spec = spec.trim().trim_start_matches("http://").trim_end_matches("/mcp").trim_end_matches('/');
    let (host, port) =
        spec.rsplit_once(':').ok_or_else(|| format!("attach address must be 127.0.0.1:PORT, not `{spec}`"))?;
    if !matches!(host, "127.0.0.1" | "localhost") {
        return Err(format!("refusing to attach to {host}; only 127.0.0.1 or localhost"));
    }
    let port: u16 = port.parse().map_err(|_| format!("bad port in `{spec}`"))?;
    Ok(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
}

/// POST one JSON-RPC message to a loopback server. `None` for 202/204 (notifications).
pub fn forward_message(addr: SocketAddr, token: &str, body: &str) -> Result<Option<String>, String> {
    if addr.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err("refusing to connect anywhere but 127.0.0.1".into());
    }
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|e| format!("could not reach the OxyBrush app at {addr}: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(180))).map_err(|e| e.to_string())?;
    let bytes = body.as_bytes();
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        addr.port(),
        bytes.len()
    );
    stream.write_all(request.as_bytes()).and_then(|_| stream.write_all(bytes)).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    reader.read_line(&mut status).map_err(|e| e.to_string())?;
    let code: u16 = status.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let len = header(&headers, "content-length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return Err("response too large".into());
    }
    let mut buf = vec![0u8; len];
    if len > 0 {
        reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
    }
    let text = String::from_utf8(buf).map_err(|e| e.to_string())?;
    match code {
        202 | 204 => Ok(None),
        401 => Err("the app rejected the token (Disconnect rotates it)".into()),
        200..=299 => Ok(Some(text)),
        _ => Err(format!("the app returned HTTP {code}: {text}")),
    }
}

/// Bridge stdio to a running app. Refuses non-loopback addresses.
pub fn serve_stdio_attach(spec: &str, token: &str) -> Result<(), String> {
    let addr = parse_attach(spec)?;
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    while let Some(body) = read_frame(&mut reader).map_err(|e| e.to_string())? {
        if let Some(response) = forward_message(addr, token, &body)? {
            write_frame(&mut writer, &response).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::HeadlessSession;
    use std::io::Cursor;

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, "{\"ok\":true}").unwrap();
        let mut cursor = Cursor::new(buf);
        assert_eq!(read_frame(&mut cursor).unwrap().unwrap(), "{\"ok\":true}");
        assert!(read_frame(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn attach_address_stays_on_loopback() {
        assert_eq!(parse_attach("127.0.0.1:9").unwrap(), SocketAddr::from((Ipv4Addr::LOCALHOST, 9)));
        assert_eq!(parse_attach("http://localhost:9/mcp").unwrap().ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert!(parse_attach("192.168.1.5:9").is_err());
        assert!(parse_attach("example.com:9").is_err());
        assert!(forward_message(SocketAddr::from((Ipv4Addr::new(1, 1, 1, 1), 80)), "t", "{}").is_err());
    }

    #[test]
    fn loopback_requires_token_and_rejects_foreign_origins() {
        let session = HeadlessSession::with_piece("Sphere", oxy_mesh::primitives::icosphere(2, 1.0));
        let token = new_token().unwrap();
        let control = Arc::new(Control::new(token.clone()));
        let server = serve_loopback(Identity::oxybrush(), Box::new(session), Arc::clone(&control)).unwrap();
        assert_eq!(server.addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));

        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#;
        let denied = raw_http(server.addr, "POST", "/mcp", None, None, init);
        assert!(denied.starts_with("HTTP/1.1 401"), "{denied}");

        let evil = raw_http(server.addr, "POST", "/mcp", Some(&token), Some("https://evil.example"), init);
        assert!(evil.starts_with("HTTP/1.1 403"), "{evil}");

        let ok = raw_http(server.addr, "POST", "/mcp", Some(&token), None, init);
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert!(ok.contains("oxybrush"), "{ok}");
        assert!(control.is_active());

        let noted = raw_http(
            server.addr,
            "POST",
            "/mcp",
            Some(&token),
            Some(&format!("http://127.0.0.1:{}", server.addr.port())),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        );
        assert!(noted.starts_with("HTTP/1.1 202"), "{noted}");

        let tools = raw_http(
            server.addr,
            "POST",
            "/mcp",
            Some(&token),
            None,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        );
        assert!(tools.contains("screenshot"), "{tools}");
        assert!(tools.contains("stroke"), "{tools}");

        // A second listener must not be required to prove we never bind a wildcard.
        assert_ne!(server.addr.ip(), IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        drop(server);
    }

    fn raw_http(
        addr: SocketAddr,
        method: &str,
        path: &str,
        token: Option<&str>,
        origin: Option<&str>,
        body: &str,
    ) -> String {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        let origin = origin.map(|o| format!("Origin: {o}\r\n")).unwrap_or_default();
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{auth}{origin}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            addr.port(),
            body.len()
        );
        stream.write_all(req.as_bytes()).unwrap();
        stream.flush().unwrap();
        let mut buf = String::new();
        match stream.read_to_string(&mut buf) {
            Ok(_) => buf,
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset && !buf.is_empty() => buf,
            Err(e) => panic!("read failed: {e}"),
        }
    }
}
