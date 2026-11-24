use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::{Command, CommandCatalog, CommandError};

pub const MODERN: &str = "2026-07-28";
pub const LEGACY: &str = "2025-11-25";
pub const SUPPORTED: &[&str] = &[MODERN, LEGACY];
pub const SERVER_NAME: &str = "dtool-mcp";
pub const UI_MAX_REQUEST_BYTES: usize = 256 * 1024;
pub const PREVIEW_DEFAULT_PX: f64 = 768.0;
pub const PREVIEW_MAX_PX: f64 = 2048.0;
pub const PREVIEW_MAX_BYTES: usize = 8 << 20;

const TOOL_NAMES: &[&str] = &[
    "dtool.commands",
    "dtool.schema",
    "dtool.run",
    "dtool.preview",
    "dtool.query",
    "dtool.undo",
];

#[derive(Debug, Clone)]
pub struct Config {
    pub allow_dirs: Vec<PathBuf>,
    pub busy_timeout: Duration,
    pub underscore_names: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            allow_dirs: Vec::new(),
            busy_timeout: Duration::from_secs(5),
            underscore_names: false,
        }
    }
}

pub fn default_session() -> Session {
    Session::new(Document::new([1920.0, 1080.0], Units::Px, 96.0, 1))
}

#[derive(Debug, Default)]
pub(crate) struct InstanceState {
    pub(crate) addr: Option<std::net::SocketAddr>,
    pub(crate) last_error: Option<String>,
}

#[derive(Clone)]
enum Backend {
    Shared(dtool_core::workspace::SharedWorkspace),
    Host(dtool_core::workspace::HostHandle),
}

pub struct Server {
    config: Config,
    pub(crate) instance: Arc<Mutex<InstanceState>>,
    backend: Backend,
    ai_connection: Arc<dtool_core::ai_connection::AiConnection>,
}

fn error_value(code: &str, message_key: &str) -> serde_json::Value {
    serde_json::json!({
        "code": code,
        "message_key": message_key
    })
}

pub(crate) fn parse_scope(params: &serde_json::Value) -> Result<Scope, serde_json::Value> {
    let invalid = || error_value("INVALID_PARAMS", "error.invalid_params");
    let context = |n: u64| Scope::of(dtool_core::workspace::ContextId(n));
    match params.get("scope") {
        None | Some(serde_json::Value::Null) => Ok(Scope::active()),
        Some(serde_json::Value::Number(n)) => n.as_u64().map(context).ok_or_else(invalid),
        Some(serde_json::Value::String(text)) if text == "active" => Ok(Scope::active()),
        Some(serde_json::Value::String(text)) => text.parse::<u64>().map(context).map_err(|_| invalid()),
        Some(serde_json::Value::Object(map)) => map.get("context").and_then(serde_json::Value::as_u64).map(context).ok_or_else(invalid),
        Some(_) => Err(invalid()),
    }
}

fn with_context(mut body: serde_json::Value, ctx_id: Option<dtool_core::workspace::ContextId>) -> serde_json::Value {
    if let (Some(id), Some(object)) = (ctx_id, body.as_object_mut()) { object.insert("context_id".into(), serde_json::json!(id.0)); }
    body
}

fn command_error_value(err: &CommandError) -> serde_json::Value {
    error_value(&err.code, &err.message_key)
}

pub fn is_ui_host_command(name: &str) -> bool {
    matches!(name, "ui.state.query" | "ui.screenshot.capture")
}

impl Server {
    pub fn new(config: Config, workspace: Workspace) -> Self {
        Self::with_workspace(config, workspace.into_shared())
    }

    pub fn with_workspace(config: Config, workspace: dtool_core::workspace::SharedWorkspace) -> Self {
        dtool_render::preview::install();
        dtool_text::text_host::install();
        dtool_text::canvas_text::install();
        dtool_export::macros::install();
        dtool_io::install_import_commands();
        let ai_connection = workspace.lock().expect("workspace lock").ai_connection();
        Self { config, instance: Arc::new(Mutex::new(InstanceState::default())), backend: Backend::Shared(workspace), ai_connection }
    }

    pub fn with_host(config: Config, host: dtool_core::workspace::HostHandle, ai_connection: Arc<dtool_core::ai_connection::AiConnection>) -> Self {
        dtool_render::preview::install();
        dtool_text::text_host::install();
        dtool_text::canvas_text::install();
        dtool_export::macros::install();
        dtool_io::install_import_commands();
        host.confirms().set_answer(Arc::new(|id, approve| {
            let store = permission::confirm_store();
            if approve { store.approve(id).is_ok() } else { store.decline(id); true }
        }));
        Self { config, instance: Arc::new(Mutex::new(InstanceState::default())), backend: Backend::Host(host), ai_connection }
    }

    pub fn share(&self) -> Server {
        Server {
            config: self.config.clone(),
            instance: Arc::clone(&self.instance),
            backend: self.backend.clone(),
            ai_connection: Arc::clone(&self.ai_connection),
        }
    }

    pub fn is_host_mode(&self) -> bool { matches!(self.backend, Backend::Host(_)) }

    fn run_ui_host(&self, scope: Scope, command: Command) -> serde_json::Value {
        match &self.backend {
            Backend::Host(host) => {
                let expected = if scope.context.is_none() { host.state().active_path } else { None };
                let reply = host.request(scope, command, expected, self.config.busy_timeout);
                match reply.result {
                    Ok(outcome) => with_context(serde_json::json!({ "result": outcome }), reply.context),
                    Err(err) => serde_json::json!({ "error": command_error_value(&err) }),
                }
            }
            Backend::Shared(_) => serde_json::json!({ "error": error_value("NO_WINDOW", "error.no_window") }),
        }
    }

    pub(crate) fn run_scoped(&self, scope: Scope, command: Command) -> (Option<dtool_core::workspace::ContextId>, dtool_protocol::CommandResult) {
        match &self.backend {
            Backend::Shared(workspace) => match workspace.lock() {
                Ok(mut workspace) => workspace.run(scope, command),
                Err(_) => (scope.context, Err(CommandError::new("INTERNAL_ERROR"))),
            },
            Backend::Host(host) => {
                let expected = if scope.context.is_none() { host.state().active_path } else { None };
                let reply = host.request(scope, command, expected, self.config.busy_timeout);
                (reply.context, reply.result)
            }
        }
    }

    fn context_generation(&self, id: dtool_core::workspace::ContextId) -> Option<u64> {
        match &self.backend {
            Backend::Shared(workspace) => workspace.lock().ok().and_then(|ws| ws.get(id).map(|context| context.session.generation)),
            Backend::Host(_) => None,
        }
    }

    fn host_state(&self) -> dtool_core::workspace::HostState {
        match &self.backend {
            Backend::Shared(workspace) => workspace.lock().map(|ws| ws.host_state()).unwrap_or_default(),
            Backend::Host(host) => host.state(),
        }
    }

    pub fn instance_reply(&self) -> serde_json::Value {
        let (addr, last_error) = match self.instance.lock() {
            Ok(state) => (state.addr, state.last_error.clone()),
            Err(_) => (None, None),
        };
        match addr {
            Some(addr) => serde_json::json!({
                "http": "up",
                "host": addr.ip().to_string(),
                "port": addr.port(),
                "addr": addr.to_string()
            }),
            None => serde_json::json!({
                "http": "down",
                "last_error": last_error
            }),
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn workspace(&self) -> Arc<Mutex<Workspace>> {
        self.shared_workspace().expect("shared workspace server")
    }

    pub fn shared_workspace(&self) -> Option<Arc<Mutex<Workspace>>> {
        match &self.backend {
            Backend::Shared(workspace) => Some(Arc::clone(workspace)),
            Backend::Host(_) => None,
        }
    }

    pub fn catalog(&self) -> CommandCatalog {
        match &self.backend {
            Backend::Shared(workspace) => workspace.lock().ok()
                .and_then(|ws| ws.session().map(|session| session.catalog.clone()))
                .unwrap_or_default(),
            Backend::Host(_) => CommandCatalog::default(),
        }
    }

    pub fn capabilities(&self) -> serde_json::Value {
        serde_json::json!({
            "protocol": SUPPORTED,
            "server_name": SERVER_NAME,
            "tools": TOOL_NAMES.to_vec(),
            "resources": {}
        })
    }

    pub fn server_info(&self) -> serde_json::Value {
        serde_json::json!({
            "name": SERVER_NAME,
            "version": env!("CARGO_PKG_VERSION"),
            "protocol": MODERN
        })
    }

    pub fn metadata(&self) -> serde_json::Value {
        let catalog_names = self.catalog().names();
        serde_json::json!({
            "server_name": SERVER_NAME,
            "protocol_versions": SUPPORTED,
            "catalog": catalog_names,
            "limits": {
                "ui_max_request_bytes": UI_MAX_REQUEST_BYTES,
                "preview_max_px": PREVIEW_MAX_PX,
                "preview_max_bytes": PREVIEW_MAX_BYTES
            }
        })
    }

    fn tool_definitions(&self) -> Vec<serde_json::Value> {
        TOOL_NAMES.iter().map(|name| {
            let schema = match *name {
                "dtool.commands" => serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
                "dtool.schema" => serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Command name to retrieve schema for"
                        }
                    },
                    "required": ["name"]
                }),
                "dtool.run" => serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Command name to execute"
                        },
                        "params": {
                            "type": "object",
                            "description": "Command parameters"
                        }
                    },
                    "required": ["name"]
                }),
                "dtool.preview" => serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "Node ID to preview"
                        },
                        "max_px": {
                            "type": "number",
                            "description": "Maximum preview size in pixels"
                        }
                    }
                }),
                "dtool.query" => serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Query command name"
                        },
                        "params": {
                            "type": "object",
                            "description": "Query parameters"
                        }
                    },
                    "required": ["name"]
                }),
                "dtool.undo" => serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
                _ => serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            };

            serde_json::json!({
                "name": name,
                "inputSchema": schema
            })
        }).collect()
    }

    pub fn schema_for_command(&self, name: &str) -> Result<serde_json::Value, serde_json::Value> {
        if !self.catalog().contains(name) {
            return Err(error_value("UNKNOWN_COMMAND", "error.unknown_command"));
        }

        let catalog = self.catalog();
        let spec = match catalog.get(name) {
            Some(spec) => spec,
            None => return Err(error_value("UNKNOWN_COMMAND", "error.unknown_command")),
        };
        let mut schema = serde_json::json!({
            "name": spec.name,
            "mutating": spec.mutating,
            "undoable": spec.undoable,
            "implemented": spec.implemented,
            "type": "object",
            "properties": {
                "scope": {
                    "type": ["integer", "string", "object"],
                    "description": "ContextId number, \"active\", or {\"context\": n}; default active"
                }
            }
        });
        if let Some(params) = spec.input_schema.and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok()) {
            schema["properties"]["params"] = params;
        }

        Ok(serde_json::json!({
            "name": name,
            "inputSchema": schema
        }))
    }

    pub fn resources(&self) -> Vec<serde_json::Value> {
        let mut resources: Vec<serde_json::Value> = Vec::new();

        for name in self.catalog().names() {
            resources.push(serde_json::json!({
                "uri": format!("dtool://command/{}", name),
                "name": name,
                "mimeType": "application/json"
            }));
        }

        for (context_id, path) in self.host_state().contexts {
            let ctx_id = context_id.0;
            {
                let uri = match &path {
                    Some(p) => format!("dtool://document/{}", p.display()),
                    None => format!("dtool://document/untitled/{}", ctx_id),
                };
                resources.push(serde_json::json!({
                    "uri": uri,
                    "name": match &path {
                        Some(p) => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| format!("document_{}", ctx_id)),
                        None => format!("untitled_{}", ctx_id),
                    },
                    "mimeType": "application/json"
                }));
            }
        }

        resources
    }

    pub fn read_resource(&self, uri: &str) -> Result<serde_json::Value, serde_json::Value> {
        if let Some(command_name) = uri.strip_prefix("dtool://command/") {
            if !self.catalog().contains(command_name) {
                return Err(error_value("CONTEXT_NOT_FOUND", "error.context_not_found"));
            }
            let catalog = self.catalog();
            match catalog.get(command_name) {
                Some(spec) => Ok(serde_json::json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string(&serde_json::json!({
                            "name": spec.name,
                            "mutating": spec.mutating,
                            "undoable": spec.undoable,
                            "implemented": spec.implemented
                        })).unwrap_or_default()
                    }]
                })),
                None => Err(error_value("CONTEXT_NOT_FOUND", "error.context_not_found")),
            }
        } else if uri.starts_with("dtool://document/") {
            for (context_id, path) in self.host_state().contexts {
                let ctx_id = context_id.0;
                {
                    let ctx_path = path;
                    let expected_uri = match &ctx_path {
                        Some(p) => format!("dtool://document/{}", p.display()),
                        None => format!("dtool://document/untitled/{}", ctx_id),
                    };
                    if expected_uri == uri {
                        return Ok(serde_json::json!({
                            "contents": [{
                                "uri": uri,
                                "mimeType": "application/json",
                                "text": serde_json::to_string(&serde_json::json!({
                                    "id": ctx_id,
                                    "path": ctx_path.as_ref().map(|p| p.display().to_string()),
                                    "has_document": true
                                })).unwrap_or_default()
                            }]
                        }));
                    }
                }
            }
            Err(error_value("CONTEXT_NOT_FOUND", "error.context_not_found"))
        } else {
            Err(error_value("CONTEXT_NOT_FOUND", "error.context_not_found"))
        }
    }

    pub fn handle_request(&mut self, request: serde_json::Value) -> serde_json::Value {
        let id = request.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let body = self.dispatch(&request);
        let mut reply = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id
        });
        if let (Some(target), Some(source)) = (reply.as_object_mut(), body.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }
        reply
    }

    pub fn dispatch(&mut self, request: &serde_json::Value) -> serde_json::Value {
        if let Some(reply) = self.connection_response(request) { return reply; }
        let mut request = request.clone();
        let id = request.get("id").cloned().unwrap_or(serde_json::Value::Null);

        let host_state = self.host_state();
        let document_path = host_state.active_path.clone();
        let decision = permission::classify_request(&request, document_path.as_deref());
        if let permission::PermissionDecision::Ask { class, message_key } = &decision {
            let store = permission::confirm_store();
            let scoped = request.get("params").and_then(|params| parse_scope(params).ok()).and_then(|scope| scope.context);
            let host_state = match scoped.filter(|id| Some(*id) != host_state.active) {
                Some(id) => dtool_core::workspace::HostState { active: Some(id), generation: self.context_generation(id), ..Default::default() },
                None => host_state.clone(),
            };
            match permission::confirm_token(&request) {
                Some(token) => {
                    if let Err(code) = store.take(&request, &token, host_state.active, host_state.generation) {
                        return serde_json::json!({"jsonrpc":"2.0","id":id,"error":error_value(code,&format!("error.{}",code.to_lowercase()))});
                    }
                }
                None => {
                    let confirm_id = store.request(&request, class.clone(), message_key, host_state.active, host_state.generation);
                    if let Backend::Host(host) = &self.backend {
                        host.request_confirm(dtool_core::workspace::HostConfirm {
                            id: confirm_id,
                            class: format!("{:?}", class),
                            message_key: message_key.clone(),
                            name: permission::request_fingerprint(&request).0,
                            context: host_state.active,
                            expires_at: std::time::Instant::now() + store.ttl,
                        });
                    }
                    let mut error = error_value("NEEDS_CONFIRMATION", message_key);
                    error["confirm_id"] = serde_json::json!(confirm_id);
                    error["class"] = serde_json::json!(format!("{:?}", class));
                    return serde_json::json!({"jsonrpc":"2.0","id":id,"error":error});
                }
            }
        }
        permission::strip_confirm_token(&mut request);
        let decision = if matches!(decision, permission::PermissionDecision::Ask { .. }) { permission::PermissionDecision::Allow } else { decision };
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("").to_string();
        let method = method.as_str();
        match decision {
            permission::PermissionDecision::Deny { message_key } => {
                return serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": error_value("INVALID_PARAMS", &message_key)
                });
            }
            permission::PermissionDecision::Ask { message_key, .. } => {
                return serde_json::json!({"jsonrpc":"2.0","id":id,"error":error_value("NEEDS_CONFIRMATION",&message_key)});
            }
            permission::PermissionDecision::Allow => {}
        }
        if let Some(error) = self.path_permission_error(&request) {
            return serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": error });
        }

        let body = match method {
            "initialize" => serde_json::json!({
                "result": {
                    "protocolVersion": MODERN,
                    "capabilities": self.capabilities(),
                    "serverInfo": self.server_info(),
                    "metadata": self.metadata()
                }
            }),
            "dtool.metadata" => {
                serde_json::json!({
                    "result": self.metadata()
                })
            }
            "tools/list" | "dtool.commands" => {
                let tools = self.tool_definitions();
                serde_json::json!({
                    "result": {
                        "tools": tools
                    }
                })
            }
            "dtool.schema" => {
                let params = request.get("params").cloned().unwrap_or(serde_json::Value::Null);
                let Some(name) = params.get("name").and_then(|value| value.as_str()) else {
                    return serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": error_value("INVALID_PARAMS", "error.invalid_params")
                    });
                };

                match self.schema_for_command(name) {
                    Ok(schema) => serde_json::json!({
                        "result": schema
                    }),
                    Err(err) => serde_json::json!({
                        "error": err
                    }),
                }
            }
            "resources/list" => {
                let resources = self.resources();
                serde_json::json!({
                    "result": {
                        "resources": resources
                    }
                })
            }
            "resources/read" => {
                let params = request.get("params").cloned().unwrap_or(serde_json::Value::Null);
                let Some(uri) = params.get("uri").and_then(|value| value.as_str()) else {
                    return serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": error_value("INVALID_PARAMS", "error.invalid_params")
                    });
                };

                match self.read_resource(uri) {
                    Ok(result) => serde_json::json!({
                        "result": result
                    }),
                    Err(err) => serde_json::json!({
                        "error": err
                    }),
                }
            }
            "tools/call" | "dtool.run" => {
                let params = request.get("params").cloned().unwrap_or(serde_json::Value::Null);
                self.execute_command(params)
            }
            "dtool.preview" => {
                let params = request.get("params").cloned().unwrap_or(serde_json::Value::Null);
                self.execute_preview(params)
            }
            "dtool.query" => {
                let params = request.get("params").cloned().unwrap_or(serde_json::Value::Null);
                self.execute_query(params)
            }
            "dtool.undo" => {
                let params = request.get("params").cloned().unwrap_or(serde_json::Value::Null);
                self.execute_undo(params)
            }
            _ => serde_json::json!({
                "error": error_value("UNKNOWN_COMMAND", "error.unknown_command")
            }),
        };
        body
    }

    fn path_permission_error(&self, request: &serde_json::Value) -> Option<serde_json::Value> {
        let method = request.get("method").and_then(serde_json::Value::as_str).unwrap_or("");
        if !matches!(method, "dtool.run" | "tools/call" | "dtool.query") { return None; }
        let params = request.get("params")?;
        let command_params = params.get("params").or_else(|| params.get("arguments"))?;
        let mut paths = Vec::new();
        collect_request_paths(command_params, &mut paths);
        if paths.is_empty() { return None; }
        let dirs = if self.config.allow_dirs.is_empty() { permission::default_allow_dirs() } else { self.config.allow_dirs.clone() };
        for raw in paths {
            let path = PathBuf::from(&raw);
            let allowed = permission::is_path_allowed(&path, &dirs)
                || (!path.exists() && path.parent().map(|parent| !parent.as_os_str().is_empty() && permission::is_path_allowed(parent, &dirs)).unwrap_or(false)
                    && !path.components().any(|c| matches!(c, std::path::Component::ParentDir)));
            if !allowed { return Some(error_value("PATH_NOT_ALLOWED", "error.path_not_allowed")); }
        }
        None
    }

    fn execute_command(&mut self, params: serde_json::Value) -> serde_json::Value {
        let scope = match parse_scope(&params) {
            Ok(scope) => scope,
            Err(error) => return serde_json::json!({ "error": error }),
        };
        let raw_name = match params.get("name") {
            None => "",
            Some(value) => match value.as_str() {
                Some(name) => name,
                None => {
                    return serde_json::json!({
                        "error": error_value("INVALID_PARAMS", "error.invalid_params")
                    });
                }
            },
        };
        let name = raw_name.to_string();

        if name == "app.instance.query" {
            return serde_json::json!({ "result": self.instance_reply() });
        }
        let in_catalog = self.catalog().contains(&name);

        let command_params = match params.get("params").or_else(|| params.get("arguments")) {
            None => serde_json::Value::Null,
            Some(value) => {
                if value.is_null() || value.is_object() {
                    value.clone()
                } else {
                    return serde_json::json!({
                        "error": error_value("INVALID_PARAMS", "error.invalid_params")
                    });
                }
            }
        };

        if name == "doc.open" {
            if let Some(path_str) = command_params.get("path").and_then(|v| v.as_str()) {
                let path = PathBuf::from(path_str);
                let decision = permission::check_path_permission(&path, &self.config.allow_dirs);
                match decision {
                    permission::PermissionDecision::Deny { message_key } => {
                        return serde_json::json!({
                            "error": error_value("PATH_NOT_ALLOWED", &message_key)
                        });
                    }
                    _ => {}
                }
            }
        }

        let command = Command {
            name,
            params: command_params,
        };

        if is_ui_host_command(&command.name) {
            return self.run_ui_host(scope, command);
        }

        let _editor = if is_catalog_mutating(&command.name) {
            let Some(guard) = EditorGuard::acquire(self.ai_connection.editor_lock(), self.config.busy_timeout) else {
                return serde_json::json!({
                    "error": error_value("EDITOR_BUSY", "error.editor_busy")
                });
            };
            if let Err(err) = self.preflight_scoped(scope, &command) {
                return serde_json::json!({
                    "error": command_error_value(&err)
                });
            }
            Some(guard)
        } else {
            None
        };

        let (ctx_id, result) = self.run_scoped(scope, command);

        match result {
            Ok(outcome) => with_context(serde_json::json!({
                "result": outcome
            }), ctx_id),
            Err(err) if !in_catalog && err.code == "NO_DOCUMENT" => serde_json::json!({
                "error": error_value("UNKNOWN_COMMAND", "error.unknown_command")
            }),
            Err(err) => serde_json::json!({
                "error": command_error_value(&err)
            }),
        }
    }

    fn execute_preview(&mut self, params: serde_json::Value) -> serde_json::Value {
        let scope = match parse_scope(&params) {
            Ok(scope) => scope,
            Err(error) => return serde_json::json!({ "error": error }),
        };
        let max_px = match params.get("max_px") {
            None => PREVIEW_DEFAULT_PX,
            Some(value) => match value.as_f64() {
                Some(px) if px > 0.0 && px <= PREVIEW_MAX_PX => px,
                _ => return serde_json::json!({
                    "error": error_value("INVALID_PARAMS", "error.invalid_params")
                }),
            },
        };

        let id = params.get("id").filter(|value| !value.is_null()).cloned();

        let mut command_params = serde_json::Map::new();
        if let Some(id) = id {
            command_params.insert("id".to_string(), id);
        }
        command_params.insert("max_px".to_string(), serde_json::json!(max_px));
        if let Some(size) = params.get("view_size") { command_params.insert("view_size".into(), size.clone()); }

        let command = Command {
            name: "render.node_preview".to_string(),
            params: serde_json::Value::Object(command_params),
        };

        let (ctx_id, result) = self.run_scoped(scope, command);

        match result {
            Ok(outcome) => with_context(serde_json::json!({
                "result": outcome
            }), ctx_id),
            Err(err) => serde_json::json!({
                "error": command_error_value(&err)
            }),
        }
    }

    fn execute_query(&mut self, params: serde_json::Value) -> serde_json::Value {
        let scope = match parse_scope(&params) {
            Ok(scope) => scope,
            Err(error) => return serde_json::json!({ "error": error }),
        };
        let raw_name = match params.get("name") {
            None => "",
            Some(value) => match value.as_str() {
                Some(name) => name,
                None => {
                    return serde_json::json!({
                        "error": error_value("INVALID_PARAMS", "error.invalid_params")
                    });
                }
            },
        };
        let name = raw_name.to_string();

        if name == "app.instance.query" {
            return serde_json::json!({ "result": self.instance_reply() });
        }

        if !self.catalog().contains(&name) {
            return serde_json::json!({
                "error": error_value("UNKNOWN_COMMAND", "error.unknown_command")
            });
        }

        if permission::is_mutating_command(&name) {
            return serde_json::json!({
                "error": error_value("INVALID_PARAMS", "error.invalid_params")
            });
        }

        let command_params = match params.get("params").or_else(|| params.get("arguments")) {
            None => serde_json::Value::Null,
            Some(value) => {
                if value.is_null() || value.is_object() {
                    value.clone()
                } else {
                    return serde_json::json!({
                        "error": error_value("INVALID_PARAMS", "error.invalid_params")
                    });
                }
            }
        };

        let command = Command {
            name,
            params: command_params,
        };

        if is_ui_host_command(&command.name) {
            return self.run_ui_host(scope, command);
        }

        let (ctx_id, result) = self.run_scoped(scope, command);

        match result {
            Ok(outcome) => with_context(serde_json::json!({
                "result": outcome
            }), ctx_id),
            Err(err) => serde_json::json!({
                "error": command_error_value(&err)
            }),
        }
    }

    fn execute_undo(&mut self, params: serde_json::Value) -> serde_json::Value {
        let scope = match parse_scope(&params) {
            Ok(scope) => scope,
            Err(error) => return serde_json::json!({ "result": error }),
        };
        let command = Command {
            name: "edit.undo".to_string(),
            params: serde_json::Value::Null,
        };

        let Some(_editor) = EditorGuard::acquire(self.ai_connection.editor_lock(), self.config.busy_timeout) else {
            return serde_json::json!({ "result": error_value("EDITOR_BUSY", "error.editor_busy") });
        };
        let (ctx_id, result) = self.run_scoped(scope, command);

        match result {
            Ok(outcome) => with_context(serde_json::json!({ "result": outcome }), ctx_id),
            Err(err) => serde_json::json!({ "result": command_error_value(&err) }),
        }
    }
}

struct EditorGuard(Arc<dtool_core::editor_lock::EditorLock>);

impl EditorGuard {
    fn acquire(lock: Arc<dtool_core::editor_lock::EditorLock>, timeout: Duration) -> Option<Self> {
        lock.acquire_with_timeout(timeout).then(|| Self(lock))
    }
}

impl Drop for EditorGuard {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn protocol_catalog() -> &'static CommandCatalog {
    static CATALOG: std::sync::OnceLock<CommandCatalog> = std::sync::OnceLock::new();
    CATALOG.get_or_init(CommandCatalog::new)
}

pub fn is_catalog_mutating(name: &str) -> bool {
    protocol_catalog().resolve(name).is_some_and(|spec| spec.mutating)
}

impl Server {
    fn preflight_scoped(&self, scope: Scope, command: &Command) -> Result<(), CommandError> {
        let Backend::Shared(workspace) = &self.backend else { return Ok(()); };
        let workspace = workspace.lock().map_err(|_| CommandError::new("INTERNAL_ERROR"))?;
        let Some(context) = scope.context.or_else(|| workspace.active()).and_then(|id| workspace.get(id)) else { return Ok(()); };
        let session = &context.session;
        if session.catalog.resolve(&command.name).is_some_and(|spec| !spec.implemented) {
            return Err(CommandError::not_implemented());
        }
        if command.name == "edit.batch" {
            return dtool_core::command::preflight(session, command);
        }
        dtool_core::command::preflight_targets(session, command)
    }
}
fn discard_line(input: &mut impl BufRead) -> bool {
    loop {
        let (found, used) = match input.fill_buf() {
            Ok([]) | Err(_) => return false,
            Ok(chunk) => match chunk.iter().position(|byte| *byte == b'\n') {
                Some(index) => (true, index + 1),
                None => (false, chunk.len()),
            },
        };
        input.consume(used);
        if found {
            return true;
        }
    }
}

pub fn serve_stdio(server: &mut Server, mut input: impl BufRead, output: impl Write) {
    let mut writer = std::io::BufWriter::new(output);
    let mut bytes = Vec::new();

    loop {
        bytes.clear();
        let read = match std::io::Read::take(&mut input, UI_MAX_REQUEST_BYTES as u64 + 1).read_until(b'\n', &mut bytes) {
            Ok(read) => read,
            Err(_) => break,
        };
        if read == 0 {
            break;
        }
        let complete = bytes.last() == Some(&b'\n');
        let oversized = !complete && bytes.len() > UI_MAX_REQUEST_BYTES;
        let more = !oversized || discard_line(&mut input);
        let Ok(line) = std::str::from_utf8(&bytes) else { break };
        let line = line.trim_end_matches(['\n', '\r']);
        if !oversized && line.trim().is_empty() {
            continue;
        }

        if oversized || line.len() > UI_MAX_REQUEST_BYTES {
            let reply = serde_json::json!({
                "jsonrpc": "2.0",
                "id": serde_json::Value::Null,
                "error": error_value("REQUEST_TOO_LARGE", "error.request_too_large")
            });
            if writeln!(writer, "{}", reply).is_err() { break; }
            if writer.flush().is_err() { break; }
            if !more { break; }
            continue;
        }

        let reply = match serde_json::from_str::<serde_json::Value>(line) {
            Ok(request) => server.handle_request(request),
            Err(_) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": serde_json::Value::Null,
                "error": error_value("INVALID_PARAMS", "error.invalid_params")
            }),
        };

        if writeln!(writer, "{}", reply).is_err() {
            break;
        }
        if writer.flush().is_err() {
            break;
        }
    }
}

pub mod handoff;
pub mod http;
pub mod permission;

impl Server {
    pub fn gui_connection_handle(&self) -> Arc<dtool_core::ai_connection::AiConnection> { Arc::clone(&self.ai_connection) }

    pub(crate) fn connection_response(&self, request: &serde_json::Value) -> Option<serde_json::Value> {
        let method = request.get("method").and_then(serde_json::Value::as_str).unwrap_or("");
        let name = if matches!(method, "dtool.run" | "dtool.query" | "tools/call") {
            request.get("params").and_then(|params| params.get("name")).and_then(serde_json::Value::as_str).unwrap_or("")
        } else { method };
        let id = request.get("id").cloned().unwrap_or(serde_json::Value::Null);
        if name == "app.ai_connection.query" {
            return Some(serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": { "enabled": self.ai_connection.enabled() } }));
        }
        if method == "initialize" { return None; }
        let code = if !self.ai_connection.enabled() {
            self.ai_connection.editor_lock().release();
            Some("AI_CONNECTION_DISABLED")
        } else if name == "app.ai_connection.set" { Some("NOT_IMPLEMENTED") } else { None }; // GUI-only switch; catalog implemented=false
        if let Some(code)=code { return Some(serde_json::json!({"jsonrpc":"2.0","id":id,"error":error_value(code,&format!("error.{}",code.to_lowercase()))})); }
        let mut reply = permission::confirmation_response(request,self.ai_connection.enabled());
        if let (Backend::Host(host), Some(result)) = (&self.backend, reply.as_mut().and_then(|reply| reply.get_mut("result"))) {
            if result.get("token_execution_available").is_some() && host.confirms().ui_attached() {
                result["token_execution_available"] = serde_json::json!(true);
            }
        }
        reply
    }
}

fn collect_request_paths(value: &serde_json::Value, paths: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                if matches!(key.as_str(), "path" | "file" | "filename" | "output_path" | "doc_dir") {
                    if let Some(text) = val.as_str().filter(|text| looks_like_file_path(text)) { paths.push(text.to_string()); continue; }
                }
                collect_request_paths(val, paths);
            }
        }
        serde_json::Value::Array(items) => { for item in items { collect_request_paths(item, paths); } }
        _ => {}
    }
}

fn looks_like_file_path(text: &str) -> bool {
    text.contains('/') || text.contains('\\') || std::path::Path::new(text).extension().is_some()
}
