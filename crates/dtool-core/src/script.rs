use std::path::PathBuf;
use std::time::Instant;

use boa_engine::object::ObjectInitializer;
use boa_engine::property::Attribute;
use boa_engine::{js_string, Context, JsError, JsNativeError, JsResult, JsValue, NativeFunction, Source};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use crate::inpaint::CancelToken;
use crate::workspace::{ContextId, Scope, Workspace};

pub const SCRIPT_RUN: &str = "script.run";
pub const SCRIPT_STOP: &str = "script.stop";
pub const SCRIPT_STOP_ALL: &str = "script.stop_all";
pub const SCRIPT_RUNNING: &str = "script.running";
pub const SCRIPT_TRUST: &str = "script.trust";
pub const SCRIPTING_OFF: &str = "SCRIPTING_OFF";
pub const SCRIPT_UNTRUSTED: &str = "SCRIPT_UNTRUSTED";
pub const SCRIPT_FAILED: &str = "SCRIPT_FAILED";
pub const SCRIPT_STOPPED: &str = "SCRIPT_STOPPED";
pub const LOOP_LIMIT: u64 = 50_000_000;
pub const LOG_CAP: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScriptId(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Script {
    pub id: ScriptId,
    pub name: String,
    pub category: String,
    pub description: String,
    pub source: String,
    pub trusted: bool,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            id: ScriptId(String::new()),
            name: String::new(),
            category: String::new(),
            description: String::new(),
            source: String::new(),
            trusted: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptLibrary {
    pub categories: Vec<String>,
    pub scripts: Vec<Script>,
}

impl ScriptLibrary {
    pub fn find(&self, id: &ScriptId) -> Option<&Script> {
        self.scripts.iter().find(|script| &script.id == id)
    }

    pub fn find_mut(&mut self, id: &ScriptId) -> Option<&mut Script> {
        self.scripts.iter_mut().find(|script| &script.id == id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptPolicy {
    pub enabled: bool,
    pub fs_folders: Vec<PathBuf>,
    pub network: bool,
    pub dynamic_code: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogLine {
    pub level: LogLevel,
    pub text: String,
    pub at_ms: u64,
}

pub trait JsEngine {
    fn eval(&mut self, src: &str, host: &mut dyn ScriptHostApi, stop: &CancelToken) -> Result<(), String>;
}

pub trait ScriptHostApi {
    fn run(&mut self, name: &str, params_json: &str) -> String;
    fn log(&mut self, l: LogLine);
}

#[derive(Debug, Clone)]
pub struct RunningScript {
    pub id: ScriptId,
    pub started: Instant,
    pub token: std::sync::Arc<CancelToken>,
}

impl std::fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("CancelToken").field(&self.is_cancelled()).finish()
    }
}

#[derive(Debug, Default)]
pub struct ScriptHost {
    pub running: Vec<RunningScript>,
    pub policy: ScriptPolicy,
    pub library: ScriptLibrary,
    pub log: Vec<LogLine>,
    next_run: u64,
}

impl ScriptHost {
    pub fn new(policy: ScriptPolicy) -> Self {
        Self { policy, ..Self::default() }
    }

    pub fn push_log(&mut self, line: LogLine) {
        self.log.push(line);
        if self.log.len() > LOG_CAP {
            let extra = self.log.len() - LOG_CAP;
            self.log.drain(..extra);
        }
    }

    pub fn stop(&mut self, id: &ScriptId) -> bool {
        let mut found = false;
        for running in &self.running {
            if &running.id == id {
                running.token.cancel();
                found = true;
            }
        }
        found
    }

    pub fn stop_all(&mut self) -> usize {
        for running in &self.running {
            running.token.cancel();
        }
        self.running.len()
    }

    fn start(&mut self) -> RunningScript {
        self.next_run += 1;
        let running = RunningScript {
            id: ScriptId(format!("run-{}", self.next_run)),
            started: Instant::now(),
            token: std::sync::Arc::new(CancelToken::new()),
        };
        self.running.push(running.clone());
        running
    }

    fn finish(&mut self, id: &ScriptId) {
        self.running.retain(|running| &running.id != id);
    }

    pub fn running_json(&self) -> Value {
        Value::Array(
            self.running
                .iter()
                .map(|running| json!({ "run_id": running.id.0, "elapsed_ms": running.started.elapsed().as_millis() as u64 }))
                .collect(),
        )
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BoaEngine {
    pub loop_limit: u64,
}

impl BoaEngine {
    pub fn new() -> Self {
        Self { loop_limit: LOOP_LIMIT }
    }
}

fn stopped_error() -> JsError {
    JsNativeError::error().with_message(SCRIPT_STOPPED).into()
}

fn arg_text(args: &[JsValue], index: usize, context: &mut Context) -> JsResult<String> {
    match args.get(index) {
        Some(value) if !value.is_undefined() => Ok(value.to_string(context)?.to_std_string_escaped()),
        _ => Ok(String::new()),
    }
}

fn arg_json(args: &[JsValue], index: usize, context: &mut Context) -> JsResult<String> {
    match args.get(index) {
        Some(value) if !value.is_undefined() && !value.is_null() => match value.to_json(context)? {
            Some(json) => Ok(json.to_string()),
            None => Ok("{}".to_string()),
        },
        _ => Ok("{}".to_string()),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct HostSlot {
    host: *mut (dyn ScriptHostApi + 'static),
    stop: *const CancelToken,
}

impl HostSlot {
    fn host(&self) -> JsResult<&mut (dyn ScriptHostApi + 'static)> {
        if unsafe { self.stop.as_ref() }.map(|stop| stop.is_cancelled()).unwrap_or(true) {
            return Err(stopped_error());
        }
        unsafe { self.host.as_mut() }.ok_or_else(stopped_error)
    }
}

fn log_function(slot: std::rc::Rc<HostSlot>, level: LogLevel) -> NativeFunction {
    unsafe {
        NativeFunction::from_closure(move |_this, args, context| {
            let mut parts = Vec::with_capacity(args.len());
            for index in 0..args.len() {
                parts.push(arg_text(args, index, context)?);
            }
            slot.host()?.log(LogLine { level, text: parts.join(" "), at_ms: now_ms() });
            Ok(JsValue::undefined())
        })
    }
}

fn run_function(slot: std::rc::Rc<HostSlot>) -> NativeFunction {
    unsafe {
        NativeFunction::from_closure(move |_this, args, context| {
            let name = arg_text(args, 0, context)?;
            let params = arg_json(args, 1, context)?;
            let reply = slot.host()?.run(&name, &params);
            let value: Value = serde_json::from_str(&reply).unwrap_or(Value::Null);
            JsValue::from_json(&value, context)
        })
    }
}

impl JsEngine for BoaEngine {
    fn eval(&mut self, src: &str, host: &mut dyn ScriptHostApi, stop: &CancelToken) -> Result<(), String> {
        if stop.is_cancelled() {
            return Err(SCRIPT_STOPPED.to_string());
        }
        let raw: *mut (dyn ScriptHostApi + '_) = host;
        let raw: *mut (dyn ScriptHostApi + 'static) = unsafe { std::mem::transmute(raw) };
        let slot = std::rc::Rc::new(HostSlot { host: raw, stop: stop as *const CancelToken });
        let result = {
            let mut context = Context::default();
            context.runtime_limits_mut().set_loop_iteration_limit(if self.loop_limit == 0 { LOOP_LIMIT } else { self.loop_limit });
            let dtool = ObjectInitializer::new(&mut context)
                .function(run_function(slot.clone()), js_string!("run"), 2)
                .function(log_function(slot.clone(), LogLevel::Info), js_string!("log"), 1)
                .function(log_function(slot.clone(), LogLevel::Warn), js_string!("warn"), 1)
                .function(log_function(slot.clone(), LogLevel::Error), js_string!("error"), 1)
                .build();
            let console = ObjectInitializer::new(&mut context)
                .function(log_function(slot.clone(), LogLevel::Info), js_string!("log"), 1)
                .function(log_function(slot.clone(), LogLevel::Info), js_string!("info"), 1)
                .function(log_function(slot.clone(), LogLevel::Warn), js_string!("warn"), 1)
                .function(log_function(slot.clone(), LogLevel::Error), js_string!("error"), 1)
                .build();
            let registered = context
                .register_global_property(js_string!("dtool"), dtool, Attribute::all())
                .and_then(|_| context.register_global_property(js_string!("console"), console, Attribute::all()));
            match registered {
                Err(err) => Err(err.to_string()),
                Ok(()) => match context.eval(Source::from_bytes(src)) {
                    Ok(_) => Ok(()),
                    Err(err) => {
                        if stop.is_cancelled() {
                            Err(SCRIPT_STOPPED.to_string())
                        } else {
                            Err(err.to_string())
                        }
                    }
                },
            }
        };
        drop(slot);
        result
    }
}

struct WorkspaceHost<'a> {
    workspace: &'a mut Workspace,
    scope: Scope,
    log: Vec<LogLine>,
}

impl ScriptHostApi for WorkspaceHost<'_> {
    fn run(&mut self, name: &str, params_json: &str) -> String {
        let params: Value = match serde_json::from_str(params_json) {
            Ok(value) => value,
            Err(_) => return json!({ "ok": false, "error": { "code": "INVALID_PARAMS", "message_key": "error.invalid_params" } }).to_string(),
        };
        if name.starts_with("script.") {
            return json!({ "ok": false, "error": { "code": "INVALID_PARAMS", "message_key": "error.invalid_params" } }).to_string();
        }
        let (_, result) = self.workspace.run(self.scope, Command::with_params(name, params));
        match result {
            Ok(value) => json!({ "ok": true, "result": value }).to_string(),
            Err(err) => json!({ "ok": false, "error": { "code": err.code, "message_key": err.message_key } }).to_string(),
        }
    }

    fn log(&mut self, l: LogLine) {
        self.log.push(l);
    }
}

pub fn is_script_command(name: &str) -> bool {
    matches!(name, SCRIPT_RUN | SCRIPT_STOP | SCRIPT_STOP_ALL | SCRIPT_RUNNING | SCRIPT_TRUST)
}

fn param_str(command: &Command, key: &str) -> Option<String> {
    command.params.get(key).and_then(Value::as_str).map(str::to_string)
}

pub fn run_source(workspace: &mut Workspace, scope: Scope, source: &str, engine: &mut dyn JsEngine) -> CommandResult {
    if !workspace.script_host().policy.enabled {
        return Err(CommandError::new(SCRIPTING_OFF));
    }
    let running = workspace.script_host_mut().start();
    let (outcome, lines) = {
        let mut host = WorkspaceHost { workspace: &mut *workspace, scope, log: Vec::new() };
        let outcome = engine.eval(source, &mut host, &running.token);
        (outcome, host.log)
    };
    let script_host = workspace.script_host_mut();
    script_host.finish(&running.id);
    let log_json: Vec<Value> = lines
        .iter()
        .map(|line| json!({ "level": line.level, "text": line.text, "at_ms": line.at_ms }))
        .collect();
    for line in lines {
        script_host.push_log(line);
    }
    match outcome {
        Ok(()) => Ok(json!({ "run_id": running.id.0, "ok": true, "log": log_json })),
        Err(message) => {
            let code = if message == SCRIPT_STOPPED { SCRIPT_STOPPED } else { SCRIPT_FAILED };
            script_host.push_log(LogLine { level: LogLevel::Error, text: message.clone(), at_ms: now_ms() });
            Err(CommandError::new(code))
        }
    }
}

fn run_command(workspace: &mut Workspace, scope: Scope, command: &Command) -> CommandResult {
    match command.name.as_str() {
        SCRIPT_RUN => {
            let source = if let Some(source) = param_str(command, "source") {
                source
            } else if let Some(id) = param_str(command, "id") {
                let host = workspace.script_host();
                if !host.policy.enabled {
                    return Err(CommandError::new(SCRIPTING_OFF));
                }
                let script = host.library.find(&ScriptId(id)).ok_or_else(CommandError::not_found)?;
                if !script.trusted {
                    return Err(CommandError::new(SCRIPT_UNTRUSTED));
                }
                script.source.clone()
            } else {
                return Err(CommandError::invalid_params());
            };
            let mut engine = BoaEngine::new();
            run_source(workspace, scope, &source, &mut engine)
        }
        SCRIPT_STOP => {
            let id = param_str(command, "run_id").ok_or_else(CommandError::invalid_params)?;
            if workspace.script_host_mut().stop(&ScriptId(id)) {
                Ok(json!({ "stopped": 1 }))
            } else {
                Err(CommandError::not_found())
            }
        }
        SCRIPT_STOP_ALL => Ok(json!({ "stopped": workspace.script_host_mut().stop_all() })),
        SCRIPT_RUNNING => Ok(json!({ "running": workspace.script_host().running_json() })),
        SCRIPT_TRUST => {
            let id = param_str(command, "id").ok_or_else(CommandError::invalid_params)?;
            let script = workspace.script_host_mut().library.find_mut(&ScriptId(id)).ok_or_else(CommandError::not_found)?;
            script.trusted = true;
            Ok(json!({ "trusted": true }))
        }
        _ => Err(CommandError::not_implemented()),
    }
}

pub fn run(workspace: &mut Workspace, scope: Scope, command: &Command) -> Option<(Option<ContextId>, CommandResult)> {
    if !is_script_command(&command.name) {
        return None;
    }
    Some((scope.context, run_command(workspace, scope, command)))
}
