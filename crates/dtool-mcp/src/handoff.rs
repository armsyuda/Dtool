
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "mcp_endpoint.json";

pub fn default_endpoint_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA").filter(|value| !value.is_empty())?;
    Some(PathBuf::from(appdata).join("DTool").join(FILE_NAME))
}

pub fn endpoint_json(addr: SocketAddr, token: &str) -> serde_json::Value {
    let started = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    serde_json::json!({
        "version": 1,
        "pid": std::process::id(),
        "addr": addr.to_string(),
        "token": token,
        "started_unix_ms": started
    })
}

pub fn write_endpoint(path: &Path, addr: SocketAddr, token: &str) -> std::io::Result<()> {
    let dir = path.parent().filter(|dir| !dir.as_os_str().is_empty()).ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    std::fs::create_dir_all(dir)?;
    let text = serde_json::to_string(&endpoint_json(addr, token)).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
    let temp = dir.join(format!("{FILE_NAME}.{}.tmp", std::process::id()));
    std::fs::write(&temp, text.as_bytes())?;
    if let Err(err) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(err);
    }
    Ok(())
}

pub fn remove_endpoint(path: &Path, token: &str) {
    let owned = std::fs::read_to_string(path).ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .map(|value| value.get("token").and_then(serde_json::Value::as_str) == Some(token))
        .unwrap_or(false);
    if owned { let _ = std::fs::remove_file(path); }
}

pub fn stopped_json() -> serde_json::Value {
    let stopped = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    serde_json::json!({
        "version": 1,
        "pid": std::process::id(),
        "addr": serde_json::Value::Null,
        "token": serde_json::Value::Null,
        "stopped": true,
        "stopped_unix_ms": stopped
    })
}

pub fn is_live(value: &serde_json::Value) -> bool {
    value.get("token").and_then(serde_json::Value::as_str).is_some_and(|token| !token.is_empty())
        && value.get("addr").and_then(serde_json::Value::as_str).is_some()
        && value.get("stopped").and_then(serde_json::Value::as_bool) != Some(true)
}

pub fn invalidate_endpoint(path: &Path, token: &str) -> bool {
    let owned = std::fs::read_to_string(path).ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .map(|value| value.get("token").and_then(serde_json::Value::as_str) == Some(token))
        .unwrap_or(false);
    if !owned {
        return false;
    }
    match serde_json::to_string(&stopped_json()) {
        Ok(text) => std::fs::write(path, text.as_bytes()).is_ok(),
        Err(_) => false,
    }
}