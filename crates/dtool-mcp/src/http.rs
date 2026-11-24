use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use dtool_core::workspace::{host_channel, HostInbox, HostWake, Workspace};

use crate::Server;

pub const ACCEPT_POLL: Duration = Duration::from_millis(10);
pub const CONNECTION_IO_TIMEOUT: Duration = Duration::from_secs(5);
pub const STOP_WAIT: Duration = Duration::from_secs(2);
pub const LIVE_ADDR: &str = "127.0.0.1:0";

#[derive(Debug, Clone)]
pub struct HttpOptions {
    pub addr: SocketAddr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopReport {
    pub listener_joined: bool,
    pub in_flight_left: usize,
    pub elapsed: Duration,
}

pub struct HttpHandle {
    pub addr: SocketAddr,
    pub token: String,
    shutdown: Arc<AtomicBool>,
    in_flight: Arc<AtomicUsize>,
    join_handle: Option<JoinHandle<()>>,
    instance: Arc<std::sync::Mutex<crate::InstanceState>>,
    endpoint_file: Option<PathBuf>,
    stopped: Option<StopReport>,
}

impl HttpHandle {
    pub fn publish_endpoint(&mut self, path: &Path) -> std::io::Result<()> {
        crate::handoff::write_endpoint(path, self.addr, &self.token)?;
        self.endpoint_file = Some(path.to_path_buf());
        Ok(())
    }

    pub fn endpoint_file(&self) -> Option<&Path> { self.endpoint_file.as_deref() }

    pub fn in_flight(&self) -> usize { self.in_flight.load(Ordering::SeqCst) }

    pub fn is_shutting_down(&self) -> bool { self.shutdown.load(Ordering::SeqCst) }

    pub fn begin_shutdown(&mut self) {
        if self.shutdown.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(path) = self.endpoint_file.take() {
            crate::handoff::invalidate_endpoint(&path, &self.token);
        }
        if let Ok(mut state) = self.instance.lock() {
            if state.addr == Some(self.addr) {
                state.addr = None;
            }
        }
    }

    pub fn stop(&mut self, wait: Duration) -> StopReport {
        if let Some(report) = self.stopped {
            return report;
        }
        let started = Instant::now();
        self.begin_shutdown();
        let deadline = started + wait;
        let mut listener_joined = true;
        if let Some(handle) = self.join_handle.take() {
            while !handle.is_finished() && Instant::now() < deadline {
                std::thread::sleep(ACCEPT_POLL);
            }
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                listener_joined = false;
            }
        }
        while self.in_flight() > 0 && Instant::now() < deadline {
            std::thread::sleep(ACCEPT_POLL);
        }
        let report = StopReport { listener_joined, in_flight_left: self.in_flight(), elapsed: started.elapsed() };
        self.stopped = Some(report);
        report
    }
}

impl Drop for HttpHandle {
    fn drop(&mut self) {
        self.stop(STOP_WAIT);
    }
}

struct InFlight(Arc<AtomicUsize>);

impl InFlight {
    fn enter(counter: &Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(counter))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn new_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::AtomicU64;
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut parts = [0u64; 2];
    for (salt, part) in parts.iter_mut().enumerate() {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u8(salt as u8);
        hasher.write_u32(std::process::id());
        hasher.write_u128(nanos);
        hasher.write_u64(count);
        *part = hasher.finish();
    }
    format!("{:016x}{:016x}", parts[0], parts[1])
}

pub fn serve_http(server: &mut Server, opts: HttpOptions) -> Result<HttpHandle, String> {
    if opts.addr.ip().to_string() != "127.0.0.1" {
        return Err("BIND_NOT_ALLOWED".to_string());
    }

    let listener = match TcpListener::bind(opts.addr) {
        Ok(l) => l,
        Err(_) => {
            if let Ok(mut state) = server.instance.lock() {
                state.last_error = Some("PORT_IN_USE".to_string());
            }
            return Err("PORT_IN_USE".to_string());
        }
    };

    let actual_addr = match listener.local_addr() {
        Ok(a) => a,
        Err(_) => return Err("PORT_IN_USE".to_string()),
    };
    if listener.set_nonblocking(true).is_err() {
        return Err("PORT_IN_USE".to_string());
    }

    let token = new_token();
    let shutdown = Arc::new(AtomicBool::new(false));
    let in_flight = Arc::new(AtomicUsize::new(0));

    let server_arc = Arc::new(std::sync::Mutex::new(server.share()));

    let shutdown_clone = Arc::clone(&shutdown);
    let in_flight_clone = Arc::clone(&in_flight);
    let token_clone = token.clone();

    let join_handle = std::thread::spawn(move || {
        while !shutdown_clone.load(Ordering::SeqCst) {
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(ACCEPT_POLL);
                    continue;
                }
                Err(_) => {
                    std::thread::sleep(ACCEPT_POLL);
                    continue;
                }
            };
            if stream.set_nonblocking(false).is_err() {
                continue;
            }
            let _ = stream.set_read_timeout(Some(CONNECTION_IO_TIMEOUT));
            let _ = stream.set_write_timeout(Some(CONNECTION_IO_TIMEOUT));

            let guard = InFlight::enter(&in_flight_clone);
            let server_ref = Arc::clone(&server_arc);
            let token_ref = token_clone.clone();
            let shutdown_inner = Arc::clone(&shutdown_clone);

            std::thread::spawn(move || {
                let _guard = guard;
                handle_connection(stream, server_ref, &token_ref, &shutdown_inner);
            });
        }
    });

    if let Ok(mut state) = server.instance.lock() {
        state.addr = Some(actual_addr);
        state.last_error = None;
    }

    Ok(HttpHandle {
        addr: actual_addr,
        token,
        shutdown,
        in_flight,
        join_handle: Some(join_handle),
        instance: Arc::clone(&server.instance),
        endpoint_file: None,
        stopped: None,
    })
}

fn shutdown_body(request: &serde_json::Value) -> serde_json::Value {
    let id = request.get("id").cloned().unwrap_or(serde_json::Value::Null);
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": crate::error_value("SHUTDOWN", "error.shutdown") })
}

struct LiveRunning {
    inbox: HostInbox,
    http: HttpHandle,
}

#[derive(Default)]
pub struct LiveMcp {
    running: Option<LiveRunning>,
    last_error: Option<String>,
    last_stop: Option<StopReport>,
}

impl LiveMcp {
    pub fn new() -> Self { Self::default() }

    pub fn is_running(&self) -> bool { self.running.is_some() }

    pub fn addr(&self) -> Option<SocketAddr> { self.running.as_ref().map(|running| running.http.addr) }

    pub fn token(&self) -> Option<&str> { self.running.as_ref().map(|running| running.http.token.as_str()) }

    pub fn last_error(&self) -> Option<&str> { self.last_error.as_deref() }

    pub fn last_stop(&self) -> Option<StopReport> { self.last_stop }

    pub fn in_flight(&self) -> usize { self.running.as_ref().map_or(0, |running| running.http.in_flight()) }

    pub fn sync(&mut self, enabled: bool, workspace: &mut Workspace, wake: HostWake, config: crate::Config, endpoint: Option<&Path>) -> bool {
        match (enabled, self.running.is_some()) {
            (true, false) => self.start(workspace, wake, config, endpoint),
            (false, true) => {
                self.stop(workspace, STOP_WAIT);
                true
            }
            _ => false,
        }
    }

    pub fn start(&mut self, workspace: &mut Workspace, wake: HostWake, config: crate::Config, endpoint: Option<&Path>) -> bool {
        if self.running.is_some() {
            return false;
        }
        let connection = workspace.ai_connection();
        let (handle, inbox) = host_channel(wake);
        inbox.publish(workspace);
        workspace.set_host_confirms(Some(inbox.confirms()));
        let mut server = Server::with_host(config, handle, connection);
        let Ok(addr) = LIVE_ADDR.parse() else { return false };
        match serve_http(&mut server, HttpOptions { addr }) {
            Ok(mut http) => {
                self.last_error = None;
                if let Some(path) = endpoint {
                    if http.publish_endpoint(path).is_err() {
                        self.last_error = Some("HANDOFF_WRITE_FAILED".to_string());
                    }
                }
                self.running = Some(LiveRunning { inbox, http });
                true
            }
            Err(code) => {
                workspace.set_host_confirms(None);
                self.last_error = Some(code);
                false
            }
        }
    }

    pub fn drain(&self, workspace: &mut Workspace, budget: usize) -> usize {
        match &self.running {
            Some(running) => running.inbox.drain(workspace, budget),
            None => 0,
        }
    }

    pub fn next_request(&self) -> Option<dtool_core::workspace::HostRequest> {
        self.running.as_ref().and_then(|running| running.inbox.try_next())
    }

    pub fn publish(&self, workspace: &Workspace) {
        if let Some(running) = &self.running {
            running.inbox.publish(workspace);
        }
    }

    pub fn stop(&mut self, workspace: &mut Workspace, wait: Duration) -> Option<StopReport> {
        let LiveRunning { inbox, mut http } = self.running.take()?;
        let started = Instant::now();
        http.begin_shutdown();
        let deadline = started + wait;
        while Instant::now() < deadline {
            inbox.drain(workspace, usize::MAX);
            if http.in_flight() == 0 {
                break;
            }
            std::thread::sleep(ACCEPT_POLL);
        }
        workspace.set_host_confirms(None);
        drop(inbox);
        let left = deadline.saturating_duration_since(Instant::now());
        let mut report = http.stop(left);
        report.elapsed = started.elapsed();
        self.last_stop = Some(report);
        Some(report)
    }
}
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

fn handle_connection(
    stream: std::net::TcpStream,
    server: Arc<std::sync::Mutex<Server>>,
    token: &str,
    shutdown: &AtomicBool,
) {
    use std::io::{BufRead, BufReader};

    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();

    if reader.read_line(&mut request_line).is_err() {
        return;
    }

    let mut headers: Vec<String> = Vec::new();
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        if line.trim().is_empty() {
            break;
        }
        headers.push(line);
    }

    let auth_ok = headers.iter().any(|h| {
        let Some((name, value)) = h.split_once(':') else { return false };
        if !name.trim().eq_ignore_ascii_case("authorization") {
            return false;
        }
        let value = value.trim();
        let Some((scheme, provided)) = value.split_once(' ') else { return false };
        if !scheme.eq_ignore_ascii_case("bearer") {
            return false;
        }
        ct_eq(provided.trim().as_bytes(), token.as_bytes())
    });

    if !auth_ok {
        write_response(&stream, 401, "error.unauthorized");
        return;
    }

    let content_length: usize = headers
        .iter()
        .filter_map(|h| {
            let lower = h.to_lowercase();
            if lower.starts_with("content-length:") {
                lower.split(':').nth(1).and_then(|v| v.trim().parse::<usize>().ok())
            } else {
                None
            }
        })
        .next()
        .unwrap_or(0);

    if content_length > crate::UI_MAX_REQUEST_BYTES {
        write_response(&stream, 413, "error.request_too_large");
        return;
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        use std::io::Read;
        if reader.read_exact(&mut body).is_err() {
            write_response(&stream, 400, "error.invalid_params");
            return;
        }
    }

    let request: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            write_response(&stream, 400, "error.invalid_params");
            return;
        }
    };

    if shutdown.load(Ordering::SeqCst) {
        write_response_json(&stream, 200, &shutdown_body(&request));
        return;
    }

    let body = match server.lock() {
        Ok(_) if shutdown.load(Ordering::SeqCst) => shutdown_body(&request),
        Ok(mut server) => server.handle_request(request),
        Err(_) => serde_json::json!({ "error": crate::error_value("INTERNAL_ERROR", "error.internal_error") }),
    };
    write_response_json(&stream, 200, &body);
}

fn write_response(mut stream: &std::net::TcpStream, status: u16, message_key: &str) {
    use std::io::Write;
    let body = serde_json::json!({
        "error": crate::error_value(
            match status {
                401 => "UNAUTHORIZED",
                413 => "REQUEST_TOO_LARGE",
                _ => "INVALID_PARAMS",
            },
            message_key,
        )
    });
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        413 => "Payload Too Large",
        _ => "Internal Server Error",
    };
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status, status_text, body_str.len(), body_str
    );
    let _ = stream.write_all(response.as_bytes());
}

fn write_response_json(mut stream: &std::net::TcpStream, status: u16, body: &serde_json::Value) {
    use std::io::Write;
    let body_str = serde_json::to_string(body).unwrap_or_default();
    let status_text = match status {
        200 => "OK",
        _ => "Internal Server Error",
    };
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status, status_text, body_str.len(), body_str
    );
    let _ = stream.write_all(response.as_bytes());
}
