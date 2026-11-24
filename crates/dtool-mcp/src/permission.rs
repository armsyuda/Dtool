use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use dtool_core::workspace::ContextId;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateClass {
    C1CloseDiscard,
    C2Overwrite,
    C3DeleteFiles,
    Ungated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny { message_key: String },
    Ask { class: GateClass, message_key: String },
}

pub fn classify(command_name: &str) -> GateClass {
    match command_name {
        "doc.close" | "doc.close_all" | "doc.close.discard" | "app.quit" | "app.exit" => GateClass::C1CloseDiscard,
        name if !is_query_command(name) && (name.starts_with("doc.save") || name.starts_with("doc.export") || name.starts_with("export.")) => GateClass::C2Overwrite,
        "file.delete" | "file.remove" | "files.delete" | "fs.delete" | "fs.remove" | "resource.delete_file" => GateClass::C3DeleteFiles,
        _ => GateClass::Ungated,
    }
}

fn gated(name: &str, params: &Value, depth: usize) -> Option<GateClass> {
    if depth > 32 { return Some(GateClass::C1CloseDiscard); }
    if name == "edit.batch" {
        if let Some(commands) = params.get("commands").and_then(Value::as_array) {
            for command in commands {
                let name = command.get("name").and_then(Value::as_str).unwrap_or("");
                if let Some(class) = gated(name, command.get("params").unwrap_or(&Value::Null), depth + 1) { return Some(class); }
            }
        }
    }
    let class = classify(name); if class == GateClass::Ungated { None } else { Some(class) }
}

fn command_from_request(request: &Value) -> (&str, &Value) {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").unwrap_or(&Value::Null);
    if matches!(method, "tools/call" | "dtool.run" | "dtool.query") {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let args = params.get("arguments").or_else(|| params.get("params")).unwrap_or(&Value::Null);
        if matches!(name, "dtool.run" | "dtool_run" | "dtool.query" | "dtool_query") {
            (args.get("name").and_then(Value::as_str).unwrap_or(""), args.get("params").or_else(|| args.get("arguments")).unwrap_or(&Value::Null))
        } else { (name, args) }
    } else { (method, params) }
}

pub fn confirmation_response(request: &Value, ai_enabled: bool) -> Option<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if !ai_enabled { return Some(serde_json::json!({"jsonrpc":"2.0","id":id,"error":{"code":"AI_CONNECTION_DISABLED","message_key":"error.ai_connection_disabled"}})); }
    let (name, params) = command_from_request(request);
    let store = confirm_store();
    let result = match name {
        "mcp.confirm.policy.query" => Ok(serde_json::json!({"classes":["C1CloseDiscard","C2Overwrite","C3DeleteFiles"],"token_execution_available":store.approver_attached(),"gated_error":"NEEDS_CONFIRMATION","ttl_ms":store.ttl.as_millis() as u64})),
        "mcp.confirm.list" => Ok(serde_json::json!({"pending":store.list()})),
        "mcp.confirm.status" => match params.get("confirm_id").and_then(Value::as_u64) {
            Some(confirm_id) => Ok(store.status(confirm_id)),
            None => Err(("INVALID_PARAMS", "error.invalid_params")),
        },
        "mcp.confirm.approve" | "mcp.confirm.decline" => Err(("PERMISSION_DENIED", "error.permission_denied")),
        _ => return None,
    };
    Some(match result {
        Ok(result) => serde_json::json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err((code, key)) => serde_json::json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message_key":key}}),
    })
}

pub fn gate_class(request: &Value) -> Option<GateClass> {
    let (name, params) = command_from_request(request);
    gated(name, params, 0)
}

pub fn confirm_token(request: &Value) -> Option<String> {
    let params = request.get("params");
    let args = params.and_then(|p| p.get("arguments").or_else(|| p.get("params")));
    let inner = args.and_then(|a| a.get("params").or_else(|| a.get("arguments")));
    [params, args, inner].into_iter().flatten().find_map(|v| v.get("confirm_token").and_then(Value::as_str)).map(str::to_string)
}

pub fn strip_confirm_token(request: &mut Value) {
    fn strip(value: Option<&mut Value>) -> Option<&mut Value> {
        let value = value?;
        if let Some(map) = value.as_object_mut() { map.remove("confirm_token"); }
        Some(value)
    }
    if let Some(params) = strip(request.get_mut("params")) {
        let args = if params.get("arguments").is_some() { params.get_mut("arguments") } else { params.get_mut("params") };
        if let Some(args) = strip(args) {
            let inner = if args.get("params").is_some() { args.get_mut("params") } else { args.get_mut("arguments") };
            strip(inner);
        }
    }
}

pub fn request_fingerprint(request: &Value) -> (String, u64) {
    use std::hash::{Hash, Hasher};
    let mut request = request.clone();
    strip_confirm_token(&mut request);
    let (name, params) = command_from_request(&request);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    params.to_string().hash(&mut hasher);
    (name.to_string(), hasher.finish())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmState { Pending, Approved, Declined }

#[derive(Debug, Clone)]
pub struct PendingConfirm {
    pub id: u64,
    pub name: String,
    pub class: GateClass,
    pub message_key: String,
    pub params_hash: u64,
    pub context: Option<ContextId>,
    pub generation: Option<u64>,
    pub expires_at: Instant,
    pub state: ConfirmState,
    token: Option<String>,
}

#[derive(Debug)]
pub struct ConfirmStore {
    pending: Mutex<Vec<PendingConfirm>>,
    next_id: AtomicU64,
    approver: AtomicBool,
    pub ttl: Duration,
}

impl ConfirmStore {
    pub fn new(ttl: Duration) -> Self {
        Self { pending: Mutex::new(Vec::new()), next_id: AtomicU64::new(1), approver: AtomicBool::new(false), ttl }
    }

    pub fn set_approver_attached(&self, attached: bool) { self.approver.store(attached, Ordering::SeqCst); }

    pub fn approver_attached(&self) -> bool { self.approver.load(Ordering::SeqCst) }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<PendingConfirm>> {
        self.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn prune(items: &mut Vec<PendingConfirm>) {
        let now = Instant::now();
        items.retain(|item| item.expires_at + Duration::from_secs(60) > now);
    }

    pub fn request(&self, request: &Value, class: GateClass, message_key: &str, context: Option<ContextId>, generation: Option<u64>) -> u64 {
        let (name, params_hash) = request_fingerprint(request);
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut items = self.lock();
        Self::prune(&mut items);
        items.push(PendingConfirm { id, name, class, message_key: message_key.to_string(), params_hash, context, generation, expires_at: Instant::now() + self.ttl, state: ConfirmState::Pending, token: None });
        id
    }

    pub fn approve(&self, id: u64) -> Result<(), &'static str> {
        let mut items = self.lock();
        let item = items.iter_mut().find(|item| item.id == id).ok_or("CONFIRM_INVALID")?;
        if item.expires_at <= Instant::now() { return Err("CONFIRM_EXPIRED"); }
        if item.state != ConfirmState::Pending { return Err("CONFIRM_INVALID"); }
        item.state = ConfirmState::Approved;
        item.token = Some(crate::http::new_token());
        Ok(())
    }

    pub fn decline(&self, id: u64) {
        let mut items = self.lock();
        if let Some(item) = items.iter_mut().find(|item| item.id == id) { item.state = ConfirmState::Declined; item.token = None; }
    }

    pub fn take(&self, request: &Value, token: &str, context: Option<ContextId>, generation: Option<u64>) -> Result<(), &'static str> {
        let (name, params_hash) = request_fingerprint(request);
        let mut items = self.lock();
        let index = items.iter().position(|item| item.token.as_deref() == Some(token)).ok_or("CONFIRM_INVALID")?;
        let item = items.remove(index);
        if item.expires_at <= Instant::now() { return Err("CONFIRM_EXPIRED"); }
        if item.name != name || item.params_hash != params_hash || item.context != context { return Err("CONFIRM_INVALID"); }
        if item.generation != generation { return Err("CONFIRM_STALE"); }
        Ok(())
    }

    pub fn status(&self, id: u64) -> Value {
        let items = self.lock();
        match items.iter().find(|item| item.id == id) {
            None => serde_json::json!({"confirm_id":id,"state":"unknown"}),
            Some(item) if item.expires_at <= Instant::now() => serde_json::json!({"confirm_id":id,"state":"expired"}),
            Some(item) => match item.state {
                ConfirmState::Pending => serde_json::json!({"confirm_id":id,"state":"pending"}),
                ConfirmState::Declined => serde_json::json!({"confirm_id":id,"state":"declined"}),
                ConfirmState::Approved => serde_json::json!({"confirm_id":id,"state":"approved","confirm_token":item.token}),
            },
        }
    }

    pub fn pending(&self) -> Vec<PendingConfirm> {
        let now = Instant::now();
        self.lock().iter().filter(|item| item.state == ConfirmState::Pending && item.expires_at > now).cloned().collect()
    }

    pub fn list(&self) -> Vec<Value> {
        self.pending().into_iter().map(|item| serde_json::json!({
            "confirm_id": item.id,
            "name": item.name,
            "class": format!("{:?}", item.class),
            "message_key": item.message_key,
            "context": item.context.map(|context| context.0),
            "expires_in_ms": item.expires_at.saturating_duration_since(Instant::now()).as_millis() as u64,
        })).collect()
    }
}

pub fn confirm_store() -> &'static ConfirmStore {
    static STORE: OnceLock<ConfirmStore> = OnceLock::new();
    STORE.get_or_init(|| ConfirmStore::new(Duration::from_secs(120)))
}

pub fn is_query_command(name: &str) -> bool {
    if name == "dtool.schema" || name == "dtool.commands" { return true; }
    if name.starts_with("doc.query") { return true; }
    if name.ends_with(".query") { return true; }
    if name.ends_with(".get") { return true; }
    if name.ends_with(".list") { return true; }
    if name.ends_with(".status") { return true; }
    if name.ends_with(".version") { return true; }
    false
}

pub fn is_mutating_command(name: &str) -> bool {
    !is_query_command(name)
}

pub fn classify_request(request: &Value, document_path: Option<&Path>) -> PermissionDecision {
    let (name, params) = command_from_request(request);
    if request.get("method").and_then(Value::as_str) == Some("dtool.query") && is_mutating_command(name) { return PermissionDecision::Deny { message_key: "error.invalid_params".into() }; }
    if let Some(class) = gated(name, params, 0) {
        match class {
            GateClass::C2Overwrite => {
                let target_path = params.get("path").and_then(Value::as_str);
                if let Some(doc_path) = document_path {
                    if let Some(tp) = target_path {
                        if Path::new(tp) == doc_path { return PermissionDecision::Allow; }
                    }
                }
                return PermissionDecision::Ask { class, message_key: "mcp.confirm.c2".into() };
            }
            GateClass::C1CloseDiscard => {
                let modified = params.get("modified").and_then(Value::as_bool).unwrap_or(true);
                if !modified { return PermissionDecision::Allow; }
                return PermissionDecision::Ask { class, message_key: "mcp.confirm.c1".into() };
            }
            GateClass::C3DeleteFiles => {
                return PermissionDecision::Ask { class, message_key: "mcp.confirm.c3".into() };
            }
            _ => {}
        }
    }
    PermissionDecision::Allow
}

pub fn validate_query_wrapper(command_name: &str) -> Result<(), String> {
    if is_mutating_command(command_name) { return Err("error.invalid_params".to_string()); }
    Ok(())
}

pub fn default_allow_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(cwd) = std::env::current_dir().ok() { dirs.push(cwd); }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home_path = PathBuf::from(home);
        let dtool_docs = home_path.join("Documents").join("dtool");
        dirs.push(dtool_docs);
    }
    dirs
}

fn is_remote_or_device_path(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::Prefix as PrefixKind;
        if let Some(prefix) = path.components().next() {
            if let std::path::Component::Prefix(p) = prefix {
                match p.kind() {
                    PrefixKind::Verbatim(_) | PrefixKind::VerbatimUNC(_, _) | PrefixKind::VerbatimDisk(_) | PrefixKind::DeviceNS(_) | PrefixKind::UNC(_, _) => return true,
                    _ => {}
                }
            }
        }
        let s = path.to_string_lossy();
        if s.starts_with("\\\\") || s.starts_with("//") { return true; }
    }
    #[cfg(unix)]
    {
        let s = path.to_string_lossy();
        if s.starts_with("/dev/") || s.starts_with("/proc/") || s.starts_with("/sys/") { return true; }
    }
    false
}

fn has_dot_escape(path: &Path) -> bool {
    for component in path.components() {
        if let std::path::Component::ParentDir = component { return true; }
    }
    false
}

pub fn is_path_allowed(path: &Path, allow_dirs: &[PathBuf]) -> bool {
    if allow_dirs.is_empty() { return false; }
    if has_dot_escape(path) { return false; }
    if is_remote_or_device_path(path) { return false; }
    let canonical = match path.canonicalize() { Ok(c) => c, Err(_) => return false };
    for dir in allow_dirs {
        let canonical_dir = match dir.canonicalize() { Ok(c) => c, Err(_) => continue };
        if canonical.starts_with(&canonical_dir) { return true; }
    }
    false
}

pub fn check_path_permission(path: &Path, allow_dirs: &[PathBuf]) -> PermissionDecision {
    let dirs = if allow_dirs.is_empty() { default_allow_dirs() } else { allow_dirs.to_vec() };
    if is_path_allowed(path, &dirs) { PermissionDecision::Allow } else { PermissionDecision::Deny { message_key: "error.path_not_allowed".to_string() } }
}

fn collect_paths_from_value(value: &Value, paths: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                if key == "path" || key == "href" || key == "file" || key == "filename" {
                    if let Some(s) = val.as_str() { paths.push(s.to_string()); }
                } else { collect_paths_from_value(val, paths); }
            }
        }
        Value::Array(arr) => { for item in arr { collect_paths_from_value(item, paths); } }
        _ => {}
    }
}

pub fn check_request_paths(request: &Value, allow_dirs: &[PathBuf]) -> PermissionDecision {
    let dirs = if allow_dirs.is_empty() { default_allow_dirs() } else { allow_dirs.to_vec() };
    let params = request.get("params");
    if let Some(p) = params {
        let mut paths = Vec::new();
        collect_paths_from_value(p, &mut paths);
        for path_str in paths {
            let path = Path::new(&path_str);
            if !is_path_allowed(path, &dirs) { return PermissionDecision::Deny { message_key: "error.path_not_allowed".to_string() }; }
        }
    }
    PermissionDecision::Allow
}

pub fn security_query(allow_dirs: &[PathBuf]) -> serde_json::Value {
    let dirs = if allow_dirs.is_empty() { default_allow_dirs() } else { allow_dirs.to_vec() };
    serde_json::json!({
        "allow_dirs": dirs.iter().map(|d| d.display().to_string()).collect::<Vec<String>>(),
        "is_default": allow_dirs.is_empty()
    })
}

pub fn gate_message_key(class: &GateClass) -> &'static str {
    match class {
        GateClass::C1CloseDiscard => "mcp.confirm.c1",
        GateClass::C2Overwrite => "mcp.confirm.c2",
        GateClass::C3DeleteFiles => "mcp.confirm.c3",
        GateClass::Ungated => "",
    }
}

pub fn needs_confirmation(class: &GateClass) -> bool {
    matches!(class, GateClass::C1CloseDiscard | GateClass::C2Overwrite | GateClass::C3DeleteFiles)
}
