use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::clipboard::AppClipboard;
use crate::command::Session;
use crate::document::Document;
use crate::selection::Selection;
use crate::view::ViewTarget;
use crate::view_state::ViewState;
use dtool_protocol::{Command, CommandError, CommandResult};


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ContextId(pub u64);

pub const NATIVE_GUIDES_KEY: &str = "guides";
pub const NATIVE_VIEW_KEY: &str = "view";

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Scope {
    pub context: Option<ContextId>,
}

impl Scope {
    pub fn active() -> Self { Self { context: None } }
    pub fn of(id: ContextId) -> Self { Self { context: Some(id) } }
}

#[derive(Debug, Default)]
pub struct SharedServices {}

#[derive(Debug)]
pub struct DocContext {
    pub id: ContextId,
    pub title: String,
    pub path: Option<PathBuf>,
    pub session: Session,
    pub saved_version: u64,
    saved_content: Option<SavedContent>,
    checkpoint_match: std::sync::Mutex<Option<((u64, u64, u64), bool)>>,
    pub view_state: ViewState,
    pub reopen_view_target: Option<ViewTarget>,
    view_file_modified: Option<std::time::SystemTime>,
    pub initial_view_pending: bool,
    pub pending_edit: bool,
}

#[derive(Debug)]
struct SavedContent {
    undo_position: u64,
    undo_revision: u64,
    digest: [u8; 32],
}

fn content_digest(session: &Session) -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};
    use std::io::{self, BufWriter, Write};

    struct HashWriter(Sha256);
    impl Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }

    let mut doc = session.document.clone();
    // Revisions and allocation counters can advance while undo restores the same content.
    doc.version = 0;
    doc.next_object_id = 0;
    doc.next_page_id = 0;
    doc.next_story_id = 0;
    doc.next_resource_id = 0;
    doc.next_slice_id = 0;
    doc.next_swatch_id = 0;
    let mut guides = session.guides.clone();
    guides.next_guide_id = 0;
    let mut writer = BufWriter::new(HashWriter(Sha256::new()));
    serde_json::to_writer(&mut writer, &(&doc, &guides)).ok()?;
    let HashWriter(digest) = writer.into_inner().ok()?;
    Some(digest.finalize().into())
}

impl DocContext {
    pub fn is_modified(&self) -> bool {
        let version = self.session.document.version;
        if !self.session.has_document {
            return false;
        }
        let Some(checkpoint) = &self.saved_content else { return version != self.saved_version };
        let position = self.session.undo.position();
        let revision = self.session.undo.revision();
        if version != self.saved_version
            && (position != checkpoint.undo_position || revision == checkpoint.undo_revision)
        {
            return true;
        }
        let key = (version, revision, position);
        if key == (self.saved_version, checkpoint.undo_revision, checkpoint.undo_position) {
            return false;
        }
        let mut cache = self.checkpoint_match.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((cached, matches)) = *cache && cached == key {
            return !matches;
        }
        let matches = content_digest(&self.session) == Some(checkpoint.digest);
        *cache = Some((key, matches));
        !matches
    }

    fn current_view_target(&self) -> Option<ViewTarget> {
        self.session.view_canvas_size.and_then(|size| {
            ViewTarget::at_view_center_with_hint(
                &self.session.document, &self.session.view, size, self.session.view_target_hint,
            )
        }).or(self.reopen_view_target)
    }

    pub fn mark_saved(&mut self) {
        if self.session.has_document {
            self.saved_version = self.session.document.version;
            self.record_saved_content();
        }
    }

    fn record_saved_content(&mut self) {
        self.saved_content = content_digest(&self.session).map(|digest| SavedContent {
            undo_position: self.session.undo.position(),
            undo_revision: self.session.undo.revision(),
            digest,
        });
        self.checkpoint_match = Default::default();
    }

    pub(crate) fn reconnect_saved_history(&mut self) {
        if !self.is_modified() && let Some(checkpoint) = &mut self.saved_content
        {
            checkpoint.undo_position = self.session.undo.position();
            if self.saved_version == self.session.document.version {
                checkpoint.undo_revision = self.session.undo.revision();
            }
            self.checkpoint_match = Default::default();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CloseCheck { Closed, NeedsAnswer }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CloseAnswer { Yes, No, Cancel }

fn normalized_path(path: &Path) -> PathBuf {
    if let Ok(path) = std::fs::canonicalize(path) { return path; }
    let absolute = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(path) };
    let mut result = PathBuf::new();
    for part in absolute.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => { result.pop(); }
            other => result.push(other.as_os_str()),
        }
    }
    if let (Some(parent), Some(name)) = (result.parent(), result.file_name()) {
        if let Ok(parent) = std::fs::canonicalize(parent) { return parent.join(name); }
    }
    result
}

fn same_path(a: &Path, b: &Path) -> bool {
    let a = normalized_path(a);
    let b = normalized_path(b);
    if cfg!(windows) { a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()) } else { a == b }
}

pub fn attach_outcome(session: &Session, command: &Command, generation_before: u64, mut value: serde_json::Value) -> serde_json::Value {
    let mutating = session.catalog.resolve(&command.name).is_some_and(|spec| spec.mutating);
    if let Some(object) = value.as_object_mut() {
        let has_outcome = object.contains_key("doc_version") || object.contains_key("changes");
        if mutating || has_outcome || session.generation != generation_before {
            object.entry("doc_version").or_insert_with(|| serde_json::json!(session.generation));
            let name = command.name.clone();
            let changed = session.generation != generation_before;
            object.entry("changes").or_insert_with(|| if changed { serde_json::json!([{ "name": name }]) } else { serde_json::json!([]) });
        }
    }
    value
}

const RECENT_CAP: usize = 10;

pub type UiSessionSlot = Option<Box<dyn std::any::Any + Send>>;

pub type UiSessionRoute = fn(&mut UiSessionSlot, &Command) -> Option<CommandResult>;

static UI_SESSION_ROUTE: std::sync::OnceLock<UiSessionRoute> = std::sync::OnceLock::new();

pub fn register_ui_session_route(route: UiSessionRoute) -> bool {
    UI_SESSION_ROUTE.set(route).is_ok() || UI_SESSION_ROUTE.get().is_some()
}

pub fn ui_session_route_ready() -> bool { UI_SESSION_ROUTE.get().is_some() }

#[derive(Default)]
pub struct UiSessionState(pub UiSessionSlot);

impl std::fmt::Debug for UiSessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("UiSessionState").field(&self.0.is_some()).finish()
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct RecentView {
    path: PathBuf,
    modified: std::time::SystemTime,
    target: ViewTarget,
}

#[derive(Debug)]
pub struct Workspace {
    services: Arc<SharedServices>,
    contexts: Vec<DocContext>,
    active_id: Option<ContextId>,
    next_id: u64,
    recent: Vec<PathBuf>,
    recent_views: Vec<RecentView>,
    recent_cap: usize,
    recent_store: Option<PathBuf>,
    ai_connection: Arc<crate::ai_connection::AiConnection>,
    script_host: crate::script::ScriptHost,
    clipboard: AppClipboard,
    snap_prefs: Option<crate::snap_prefs::SnapPrefsService>,
    native_extras: std::collections::HashMap<ContextId, serde_json::Value>,
    paired_page_moves: Vec<[(ContextId, u64); 2]>,
    jobs: crate::job::JobRegistry,
    host_confirms: Option<HostConfirms>,
    ui_session: UiSessionState,
    display_dpi: Option<f32>,
}

impl Workspace {
    pub fn ai_connection(&self) -> Arc<crate::ai_connection::AiConnection> { Arc::clone(&self.ai_connection) }

    pub fn set_display_dpi(&mut self, dpi: Option<f32>) {
        if self.display_dpi != dpi {
            self.display_dpi = dpi;
            for context in &mut self.contexts {
                context.session.display_dpi = dpi;
            }
        }
    }

    pub fn ui_session(&self) -> &UiSessionState { &self.ui_session }

    pub fn ui_session_mut(&mut self) -> &mut UiSessionState { &mut self.ui_session }

    pub fn register_job(&mut self, pin: crate::job::JobPin) -> Result<std::sync::Arc<crate::inpaint::CancelToken>, CommandError> {
        if self.get(pin.context).is_none() {
            return Err(CommandError::context_not_found());
        }
        let record = crate::job::JobRecord::new(pin);
        let token = std::sync::Arc::clone(&record.cancel);
        self.jobs.register(record)?;
        Ok(token)
    }

    pub fn jobs(&self) -> &crate::job::JobRegistry {
        &self.jobs
    }

    pub fn jobs_mut(&mut self) -> &mut crate::job::JobRegistry {
        &mut self.jobs
    }

    pub fn apply_job(&mut self, job: crate::job::JobId) -> Result<serde_json::Value, CommandError> {
        let record = self.jobs.get(job).ok_or_else(CommandError::not_found)?;
        match record.state {
            crate::job::JobState::Completed => {}
            crate::job::JobState::Cancelled => return Err(CommandError::new("CANCELLED")),
            crate::job::JobState::Failed => return Err(CommandError::new("FAILED")),
            _ => return Err(CommandError::new("NOT_READY")),
        }
        let pin = record.pin.clone();
        let preview = record.preview.clone().ok_or_else(|| CommandError::new("NOT_READY"))?;
        let context = self.get_mut(pin.context).ok_or_else(CommandError::context_not_found)?;
        let reply = context.session.apply_job_result(&pin, preview)?;
        if let Some(record) = self.jobs.get_mut(job) {
            record.preview = None;
        }
        Ok(reply)
    }
    pub fn new(services: Arc<SharedServices>) -> Self {
        Self {
            services,
            contexts: Vec::new(),
            active_id: None,
            next_id: 1,
            recent: Vec::new(),
            recent_views: Vec::new(),
            recent_cap: RECENT_CAP,
            recent_store: None,
            ai_connection: Arc::new(crate::ai_connection::AiConnection::default()),
            script_host: crate::script::ScriptHost::default(),
            clipboard: AppClipboard::new(),
            snap_prefs: None,
            native_extras: std::collections::HashMap::new(),
            paired_page_moves: Vec::new(),
            jobs: crate::job::JobRegistry::default(),
            host_confirms: None,
            ui_session: UiSessionState::default(),
            display_dpi: None,
        }
    }

    pub fn script_host(&self) -> &crate::script::ScriptHost {
        &self.script_host
    }

    pub fn script_host_mut(&mut self) -> &mut crate::script::ScriptHost {
        &mut self.script_host
    }

    pub fn services(&self) -> Arc<SharedServices> {
        self.services.clone()
    }

    pub fn contexts(&self) -> &[DocContext] {
        &self.contexts
    }

    pub fn get(&self, id: ContextId) -> Option<&DocContext> {
        self.contexts.iter().find(|c| c.id == id)
    }

    pub fn get_mut(&mut self, id: ContextId) -> Option<&mut DocContext> {
        self.contexts.iter_mut().find(|c| c.id == id)
    }

    pub(crate) fn apply_linked_page_change<T>(
        &mut self, first: ContextId, second: ContextId,
        apply: impl FnOnce(&mut Session, &mut Session) -> Result<T, CommandError>,
    ) -> Result<T, CommandError> {
        let index = |id| self.contexts.iter().position(|context| context.id == id)
            .ok_or_else(CommandError::context_not_found);
        let indices = [index(first)?, index(second)?];
        let [first, second] = self.contexts.get_disjoint_mut(indices).map_err(|_| CommandError::invalid_params())?;
        let previous_first = crate::command::BatchRollback::capture(&first.session);
        let previous_second = crate::command::BatchRollback::capture(&second.session);
        match apply(&mut first.session, &mut second.session) {
            Ok(value) => {
                for context in [first, second] {
                    crate::longdoc::fields::refresh_if_stale(&mut context.session);
                    context.view_state = context.session.view.clone();
                }
                Ok(value)
            }
            Err(error) => {
                previous_first.restore(&mut first.session);
                previous_second.restore(&mut second.session);
                Err(error)
            }
        }
    }

    pub(crate) fn record_paired_page_move(&mut self, source: ContextId, destination: ContextId) {
        if let (Some(source), Some(destination)) = (self.get(source), self.get(destination)) {
            self.paired_page_moves.push([
                (source.id, source.session.undo.position()),
                (destination.id, destination.session.undo.position()),
            ]);
        }
    }

    fn run_paired_page_history(&mut self, id: ContextId, command: &Command) -> Option<CommandResult> {
        use crate::longdoc::page_import::MOVE_TO_DOCUMENT;

        let redo = match command.name.as_str() {
            "edit.undo" => false,
            "edit.redo" => true,
            _ => return None,
        };
        let current = &self.get(id)?.session;
        if current.edit_active() || current.text_edit.is_some() {
            return None;
        }
        let (name, step) = current.undo.next_step(redo)?;
        if name != MOVE_TO_DOCUMENT {
            return None;
        }
        let peer = self.paired_page_moves.iter().find_map(|[first, second]| {
            if *first == (id, step) { Some(*second) }
            else if *second == (id, step) { Some(*first) }
            else { None }
        });
        let Some((peer_id, peer_step)) = peer else {
            return Some(Err(CommandError::new("PAGE_MOVE_HISTORY_NEEDS_PEER")));
        };
        if !self.get(peer_id).is_some_and(|peer| {
            !peer.session.edit_active() && peer.session.text_edit.is_none()
                && peer.session.undo.next_step(redo) == Some((MOVE_TO_DOCUMENT, peer_step))
        }) {
            return Some(Err(CommandError::new("PAGE_MOVE_HISTORY_NEEDS_PEER")));
        }
        Some(self.apply_linked_page_change(id, peer_id, |current, peer| {
            let mut result = current.run_with(command, |session, command| session.run_history_step(command, true))?;
            let other = peer.run_with(command, |session, command| session.run_history_step(command, true))?;
            result["context_id"] = serde_json::json!(id);
            result["other_context"] = serde_json::json!(peer_id);
            result["other_doc_version"] = other["doc_version"].clone();
            Ok(result)
        }))
    }

    pub fn view_mut(&mut self, id: ContextId) -> Option<&mut ViewState> {
        self.get_mut(id).map(|ctx| &mut ctx.view_state)
    }

    pub fn active(&self) -> Option<ContextId> {
        self.active_id
    }

    pub fn activate(&mut self, id: ContextId) -> Result<(), CommandError> {
        if !self.contexts.iter().any(|c| c.id == id) {
            return Err(CommandError::context_not_found());
        }

        if let Some(previous) = self.active_id.filter(|previous| *previous != id) {
            if self.contexts.iter().any(|c| c.id == previous) {
                self.finish_pending_edit(previous, true)?;
            }
        }

        for ctx in self.contexts.iter_mut().filter(|ctx| ctx.id != id) {
            ctx.pending_edit = false;
        }

        self.switch_active(Some(id));
        Ok(())
    }

    fn switch_active(&mut self, target: Option<ContextId>) {
        if self.active_id != target {
            self.active_id = target;
            if let Some(context) = target.and_then(|id| self.get_mut(id)) {
                crate::commands::persona::sync_document_mode(&mut context.session);
            }
        }
    }

    pub fn finish_pending_edit(&mut self, id: ContextId, commit: bool) -> Result<(), CommandError> {
        let ctx = self.get_mut(id).ok_or_else(CommandError::context_not_found)?;
        if ctx.session.edit_active() {
            ctx.session.end_edit(if commit { crate::edit_session::EditEnd::Commit } else { crate::edit_session::EditEnd::Cancel })?;
        }
        if ctx.session.text_edit.is_some() {
            if commit {
                ctx.session.run(&Command::new("text.edit.end"))?;
            }
            ctx.session.text_edit = None;
        }
        ctx.session.text_composing = None;
        ctx.pending_edit = false;
        Ok(())
    }

    pub fn finish_pending_edit_with(&mut self, id: ContextId, commit: bool, preedit: Option<(crate::table::model::StoryId, usize, String)>) -> Result<(), CommandError> {
        let ctx = self.get_mut(id).ok_or_else(CommandError::context_not_found)?;
        if let Some((story, at, text)) = preedit {
            ctx.session.text_composing = None;
            if commit && !text.is_empty() {
                ctx.session.run(&Command::with_params("text.insert", serde_json::json!({ "story": story.0, "at": at, "text": text })))?;
            }
        }
        self.finish_pending_edit(id, commit)
    }

    pub fn insert_context(&mut self, title: &str, path: Option<PathBuf>, document: Document, saved_version: u64) -> ContextId {
        let generation = document.version;
        self.insert_context_with_generation(title, path, document, saved_version, generation)
    }

    pub fn insert_context_with_generation(&mut self, title: &str, path: Option<PathBuf>, mut document: Document, saved_version: u64, generation: u64) -> ContextId {
        let id = ContextId(self.next_id);
        self.next_id += 1;
        crate::commands::pages::normalize_layout_order(&mut document);
        crate::commands::masters::materialize_groups(&mut document);
        let mut session = Session::with_clipboard(document, self.clipboard.share());
        match &self.snap_prefs {
            Some(store) => session.snap_prefs = store.share(),
            None => self.snap_prefs = Some(session.snap_prefs.share()),
        }
        session.generation = generation;
        session.display_dpi = self.display_dpi;
        session.context = Some(id);
        let prefs = crate::commands::prefs_cmd::snapshot();
        if let Some(prefs) = &prefs {
            crate::commands::prefs_cmd::apply_to_session(&mut session, prefs);
        }
        if let Some(mode) = crate::commands::persona::current_default_view_mode() {
            crate::commands::persona::apply_default_view_mode(&mut session.view, mode);
        }
        let view_state = session.view.clone();
        let mut context = DocContext {
            id,
            title: title.to_string(),
            path,
            session,
            saved_version,
            saved_content: None,
            checkpoint_match: Default::default(),
            view_state,
            reopen_view_target: None,
            view_file_modified: None,
            initial_view_pending: false,
            pending_edit: false,
        };
        if context.path.is_none() && context.session.document.version == saved_version {
            context.record_saved_content();
        }
        self.contexts.push(context);
        self.active_id = Some(id);
        if let Some(prefs) = &prefs {
            self.apply_prefs(prefs);
        }
        id
    }

    pub fn snap_prefs(&self) -> Option<&crate::snap_prefs::SnapPrefsService> {
        self.snap_prefs.as_ref()
    }

    pub fn new_document(&mut self, title: &str) -> ContextId {
        self.insert_context(title, None, Document::default(), 0)
    }

    pub fn open_path(&mut self, path: &Path) -> Result<ContextId, CommandError> {
        crate::colour::convert::set_open_report(None);
        let canonical_path = std::fs::canonicalize(path).ok();

        for ctx in &self.contexts {
            if let Some(ctx_path) = &ctx.path {
                let is_same = match (&canonical_path, ctx_path) {
                    (Some(cp), cp_p) => {
                        if let Ok(ccp) = std::fs::canonicalize(cp_p) {
                            *cp == ccp
                        } else {
                            cp.as_path() == cp_p.as_path()
                        }
                    }
                    (None, cp_p) => path == cp_p.as_path(),
                };

                if is_same {
                    let id = ctx.id;
                    self.activate(id)?;
                    return Ok(id);
                }
            }
        }

        let mut opened = crate::native_io::open_file(path).map_err(|err| err.read_error())?;
        crate::text_cmd::refresh_legacy_art_widths(&mut opened.document);
        let saved_version = opened.document.version;
        let title = path
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());

        let id = self.insert_context_with_generation(&title, Some(path.to_path_buf()), opened.document, saved_version, opened.file_version);
        let mut extras = opened.unknown_fields;
        if let Some(guides) = extras.as_object_mut().and_then(|map| map.remove(NATIVE_GUIDES_KEY)) {
            if let (Ok(guides), Some(ctx)) = (serde_json::from_value::<crate::guides::Guides>(guides), self.get_mut(id)) {
                ctx.session.guides = guides;
            }
        }
        let modified = std::fs::metadata(path).and_then(|meta| meta.modified()).ok();
        let last_view = self.recent_views.iter()
            .find(|entry| Some(entry.modified) == modified && same_path(&entry.path, path))
            .map(|entry| entry.target);
        let saved_view = extras.as_object_mut().and_then(|map| map.remove(NATIVE_VIEW_KEY));
        if let Some(ctx) = self.get_mut(id) {
            ctx.initial_view_pending = true;
            if let Some(mut value) = saved_view {
                ctx.reopen_view_target = value.as_object_mut().and_then(|map| map.remove("target"))
                    .and_then(|target| serde_json::from_value(target).ok());
                if let Ok(view) = serde_json::from_value::<ViewState>(value) {
                    ctx.session.view = view.clone();
                    ctx.view_state = view;
                }
            }
            ctx.record_saved_content();
            ctx.view_file_modified = modified;
            ctx.reopen_view_target = last_view
                .filter(|target| target.bounds(&ctx.session.document).is_some())
                .or(ctx.reopen_view_target);
            ctx.session.view_target_hint = ctx.reopen_view_target;
            crate::commands::persona::sync_document_mode(&mut ctx.session);
        }
        if extras.as_object().is_some_and(|map| !map.is_empty()) {
            self.native_extras.insert(id, extras);
        }
        self.push_recent(path);
        Ok(id)
    }

    pub fn run(&mut self, scope: Scope, command: Command) -> (Option<ContextId>, CommandResult) {
        if let Err(err) = self.catalog_gate(&command) {
            return (scope.context.or(self.active_id), Err(err));
        }
        let target = scope.context.or(self.active_id);
        if let Some(result) = UI_SESSION_ROUTE.get().and_then(|route| route(&mut self.ui_session.0, &command)) {
            return (target, result);
        }
        let before = target.and_then(|id| self.get(id)).map(|context| (context.id, context.session.generation));
        let (id, result) = self.run_hooks(scope, command.clone());
        if let Ok(value) = &result
            && let Some(mode) = value.get("view_mode_default")
                .and_then(|value| serde_json::from_value::<crate::ViewMode>(value.clone()).ok())
        {
            for context in self.contexts.iter_mut().filter(|context| Some(context.id) != id) {
                crate::commands::persona::apply_default_view_mode(&mut context.session.view, mode);
                crate::commands::persona::apply_default_view_mode(&mut context.view_state, mode);
            }
        }
        (id, result.map(|value| self.attach_workspace_outcome(&command, id, before, value)))
    }

    fn attach_workspace_outcome(&self, command: &Command, id: Option<ContextId>, before: Option<(ContextId, u64)>, mut value: serde_json::Value) -> serde_json::Value {
        static CATALOG: std::sync::OnceLock<dtool_protocol::CommandCatalog> = std::sync::OnceLock::new();
        let catalog = CATALOG.get_or_init(dtool_protocol::CommandCatalog::new);
        let mutating = catalog.resolve(&command.name).is_some_and(|spec| spec.mutating);
        let (Some(id), Some(object)) = (id, value.as_object_mut()) else { return value };
        let Some(context) = self.get(id) else { return value };
        if !mutating || object.contains_key("doc_version") { return value; }
        let generation = context.session.generation;
        let changed = before.map_or(true, |(before_id, before_generation)| before_id != id || before_generation != generation);
        object.insert("doc_version".into(), serde_json::json!(generation));
        let name = command.name.clone();
        object.entry("changes").or_insert_with(|| if changed { serde_json::json!([{ "name": name }]) } else { serde_json::json!([]) });
        object.entry("context_id").or_insert_with(|| serde_json::json!(id.0));
        value
    }

    fn catalog_gate(&self, command: &Command) -> Result<(), CommandError> {
        static CATALOG: std::sync::OnceLock<dtool_protocol::CommandCatalog> = std::sync::OnceLock::new();
        let catalog = CATALOG.get_or_init(dtool_protocol::CommandCatalog::new);
        if catalog.resolve(&command.name).is_some() { Ok(()) } else { Err(CommandError::new("UNKNOWN_COMMAND")) }
    }

    fn run_hooks(&mut self, scope: Scope, command: Command) -> (Option<ContextId>, CommandResult) {
        if let Some(result) = crate::script::run(self, scope, &command) { return result; }
        if let Some(result) = self.run_recent(&command) { return (scope.context.or(self.active_id), result); }
        if let Some(result) = super::command::native_open::run(self, &command) { return result; }
        if let Some(result) = crate::import_hook::run(self, &command) { return result; }
        if let Some(result) = crate::commands::document_files::run(self, scope, &command) { return result; }
        if let Some(result) = crate::clipboard::run_workspace(self, scope, &command) { return result; }
        if let Some(result) = crate::longdoc::page_import::run_workspace(self, scope, &command) { return result; }
        if let Some(result) = crate::commands::export_batch::run(self, scope, &command) {
            if result.1.is_ok() {
                if let Some(ctx) = result.0.and_then(|id| self.get_mut(id)) {
                    crate::longdoc::fields::stamp_printed(&mut ctx.session);
                }
            }
            return result;
        }
        if let Some(result) = crate::commands::recovery::run_workspace(self, scope, &command) { return result; }
        if let Some(result) = crate::commands::ai_connection::run_workspace(self, &command) { return result; }
        if let Some(result) = crate::commands::document_colour::run_workspace(self, &command) { return result; }
        if let Some(result) = crate::commands::preview::run_workspace(self, scope, &command) { return result; }
        if let Some(result) = crate::data_merge::commands::run_workspace(self, scope, &command) { return result; }

        let target_id = match scope.context {
            Some(id) => Some(id),
            None => self.active_id,
        };

        if let Some(id) = target_id {
            if !self.contexts.iter().any(|c| c.id == id) {
                let issued = id.0 >= 1 && id.0 < self.next_id;
                return (issued.then_some(id), Err(CommandError::context_not_found()));
            }
            if let Some(result) = self.run_paired_page_history(id, &command) {
                return (Some(id), result);
            }

            match command.name.as_str() {
                "doc.save" => {
                    let result = self.save(id, command.path_param().as_deref()).map(|path| self.saved_outcome(id, &path));
                    return (Some(id), result);
                }
                "doc.save_as" => {
                    let result = match command.path_param() {
                        Some(path) => self.save_as(id, &path).map(|()| self.saved_outcome(id, &path)),
                        None => Err(CommandError::invalid_params()),
                    };
                    return (Some(id), result);
                }
                _ => {}
            }

            let context = self.get_mut(id).expect("validated context");
            context.session.context = Some(id);
            let view_before = context.session.view.clone();
            let generation_before = context.session.generation;
            let result = context.session.run(&command).map(|value| {
                let mut value = attach_outcome(&context.session, &command, generation_before, value);
                if let Some(object) = value.as_object_mut() { object.insert("context_id".into(), serde_json::json!(id.0)); }
                value
            });
            if result.is_ok() {
                if crate::longdoc::fields::is_print_command(&command.name) {
                    crate::longdoc::fields::stamp_printed(&mut context.session);
                } else {
                    crate::longdoc::fields::refresh_if_stale(&mut context.session);
                }
            }
            if context.session.view != view_before {
                context.view_state = context.session.view.clone();
            } else if context.view_state != context.session.view {
                context.session.view = context.view_state.clone();
            }
            return (Some(id), result);
        } else {
            return (None, self.run_app_global(&command).unwrap_or_else(|| Err(CommandError::no_document())));
        }
    }

    fn run_app_global(&mut self, command: &Command) -> Option<CommandResult> {
        if let Some(result) = crate::commands::persona::run_without_document(command) {
            return Some(result);
        }
        let empty = |command: &Command| command.params.as_object().map(|map| map.is_empty()).unwrap_or(command.params.is_null());
        let result = match command.name.as_str() {
            "app.about" | "app.version" if !empty(command) => Err(CommandError::invalid_params()),
            "app.about" => Ok(crate::command::app_about()),
            "app.version" => Ok(crate::command::app_version()),
            "snap.prefs.query" => {
                let store = self.snap_prefs.get_or_insert_with(crate::snap_prefs::SnapPrefsService::new);
                serde_json::to_value(store.query()).map_err(|_| CommandError::new("INTERNAL_ERROR"))
            }
            "snap.prefs.set" => {
                let args: crate::snap_prefs::SnapPrefsSetArgs = match serde_json::from_value(command.params.clone()) {
                    Ok(args) => args,
                    Err(_) => return Some(Err(CommandError::invalid_params())),
                };
                if !args.is_valid() { return Some(Err(CommandError::invalid_params())); }
                let store = self.snap_prefs.get_or_insert_with(crate::snap_prefs::SnapPrefsService::new);
                store.set(args).map_err(|_| CommandError::new("INTERNAL_ERROR"))
                    .map(|()| serde_json::json!({ "changes": [{ "name": "snap.prefs.set" }] }))
            }
            _ => return None,
        };
        Some(result)
    }

    fn saved_outcome(&self, id: ContextId, path: &Path) -> serde_json::Value {
        let generation = self.get(id).map(|ctx| ctx.session.generation).unwrap_or_default();
        serde_json::json!({ "doc_version": generation, "changes": [], "context_id": id.0, "path": path.to_string_lossy() })
    }

    pub fn request_close(&mut self, id: ContextId) -> Result<CloseCheck, CommandError> {
        let ctx = self.get(id).ok_or(CommandError::context_not_found())?;
        if ctx.is_modified() || ctx.pending_edit {
            Ok(CloseCheck::NeedsAnswer)
        } else {
            self.remove_context(id);
            Ok(CloseCheck::Closed)
        }
    }

    pub fn close_with(&mut self, id: ContextId, answer: CloseAnswer) -> Result<bool, CommandError> {
        if !self.contexts.iter().any(|c| c.id == id) {
            return Err(CommandError::context_not_found());
        }

        match answer {
            CloseAnswer::Yes => {
                self.save_context(id)?;
                self.remove_context(id);
                Ok(true)
            }
            CloseAnswer::No => {
                self.remove_context(id);
                Ok(true)
            }
            CloseAnswer::Cancel => {
                Ok(false)
            }
        }
    }

    pub fn close_others(&mut self, keep: ContextId) -> Vec<ContextId> {
        let mut modified_ids = Vec::new();
        let mut to_close = Vec::new();

        for ctx in &self.contexts {
            if ctx.id != keep {
                if ctx.is_modified() || ctx.pending_edit {
                    modified_ids.push(ctx.id);
                } else {
                    to_close.push(ctx.id);
                }
            }
        }

        self.remember_views(|id| to_close.contains(&id));
        for id in &to_close {
            self.jobs.cancel_for_context(*id);
            self.native_extras.remove(id);
        }
        self.contexts.retain(|c| !to_close.contains(&c.id));

        self.switch_active(self.contexts.iter().any(|ctx| ctx.id == keep).then_some(keep));

        modified_ids
    }

    pub fn close_all(&mut self) -> Vec<ContextId> {
        let mut modified_ids = Vec::new();
        let mut to_close = Vec::new();

        for ctx in &self.contexts {
            if ctx.is_modified() || ctx.pending_edit {
                modified_ids.push(ctx.id);
            } else {
                to_close.push(ctx.id);
            }
        }

        self.remember_views(|id| to_close.contains(&id));
        for id in &to_close {
            self.jobs.cancel_for_context(*id);
            self.native_extras.remove(id);
        }
        self.contexts.retain(|c| !to_close.contains(&c.id));

        self.switch_active(self.contexts.first().map(|context| context.id));

        modified_ids
    }

    pub fn path_clash(&self, id: ContextId, path: &Path) -> Option<ContextId> {
        self.contexts
            .iter()
            .filter(|c| c.id != id)
            .find(|c| c.path.as_deref().is_some_and(|p| same_path(p, path)))
            .map(|c| c.id)
    }

    pub fn path_clashes(&self) -> Vec<(ContextId, ContextId)> {
        let mut clashes = Vec::new();
        for (i, a) in self.contexts.iter().enumerate() {
            let Some(pa) = a.path.as_deref() else { continue };
            for b in self.contexts.iter().skip(i + 1) {
                if let Some(pb) = b.path.as_deref() {
                    if same_path(pa, pb) {
                        clashes.push((a.id, b.id));
                    }
                }
            }
        }
        clashes
    }

    pub fn recent(&self) -> &[PathBuf] {
        &self.recent
    }

    pub fn remove_recent(&mut self, path: &Path) -> bool {
        let before = self.recent.len();
        self.recent.retain(|p| !same_path(p, path));
        self.recent_views.retain(|entry| !same_path(&entry.path, path));
        let removed = self.recent.len() != before;
        if removed {
            self.save_recent();
        }
        removed
    }

    /// Recent-files store next to prefs.json (%APPDATA%\DTool\recent.json).
    pub fn recent_default_path() -> PathBuf {
        crate::prefs::Prefs::default_path().with_file_name("recent.json")
    }

    /// Load the recent list from `path` and keep it there on every change (open/save/remove/clear/cap).
    /// Off by default so tests and headless runs never touch the user's settings; the app turns it on at startup.
    pub fn enable_recent_persistence(&mut self, path: PathBuf) {
        let mut stored = std::fs::read_to_string(&path).ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .unwrap_or_default();
        let stored = stored.as_object_mut();
        let (recent, views) = stored.map(|map| (map.remove("recent"), map.remove("views"))).unwrap_or_default();
        self.recent_views = views.and_then(|views| serde_json::from_value(views).ok()).unwrap_or_default();
        self.recent_views.truncate(RECENT_CAP);
        let loaded: Vec<PathBuf> = recent
            .and_then(|list| serde_json::from_value::<Vec<String>>(list).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .collect();
        let mut merged = std::mem::take(&mut self.recent);
        for path in loaded {
            if !merged.iter().any(|p| same_path(p, &path)) {
                merged.push(path);
            }
        }
        merged.truncate(self.recent_cap);
        self.recent = merged;
        self.recent_store = Some(path);
        self.save_recent();
    }

    pub fn recent_store(&self) -> Option<&Path> {
        self.recent_store.as_deref()
    }

    fn save_recent(&self) {
        let Some(path) = &self.recent_store else { return };
        let paths: Vec<String> = self.recent.iter().map(|p| p.display().to_string()).collect();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(&serde_json::json!({ "recent": paths, "views": self.recent_views })) {
            let _ = std::fs::write(path, text);
        }
    }

    pub fn clear_recent(&mut self) {
        self.recent.clear();
        self.recent_views.clear();
        self.save_recent();
    }

    fn recent_json(&self) -> serde_json::Value {
        let paths: Vec<String> = self.recent.iter().map(|p| p.display().to_string()).collect();
        let missing: Vec<String> = self.recent.iter().filter(|p| !p.exists()).map(|p| p.display().to_string()).collect();
        serde_json::json!({ "recent": paths, "missing": missing })
    }

    fn run_recent(&mut self, command: &Command) -> Option<CommandResult> {
        match command.name.as_str() {
            crate::commands::recent::DOC_RECENT_LIST => Some(Ok(self.recent_json())),
            crate::commands::recent::DOC_RECENT_CLEAR => {
                self.clear_recent();
                Some(Ok(self.recent_json()))
            }
            crate::commands::recent::DOC_RECENT_REMOVE => Some(match command.path_param() {
                Some(path) => {
                    let removed = self.remove_recent(&path);
                    let mut reply = self.recent_json();
                    reply["removed"] = serde_json::json!(removed);
                    Ok(reply)
                }
                None => Err(CommandError::invalid_params()),
            }),
            crate::commands::recent::DOC_OPEN_RECENT => match command.path_param() {
                Some(path) if !path.exists() => Some(Err(CommandError::not_found())),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn push_recent(&mut self, path: &Path) {
        self.recent.retain(|p| !same_path(p, path));
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(self.recent_cap);
        self.save_recent();
    }

    pub fn set_recent_count(&mut self, count: u32) {
        self.recent_cap = count.max(1) as usize;
        let before = self.recent.len();
        self.recent.truncate(self.recent_cap);
        if self.recent.len() != before {
            self.save_recent();
        }
    }

    pub fn apply_prefs(&mut self, prefs: &crate::prefs::Prefs) {
        self.set_recent_count(prefs.recent_count);
    }

    pub fn write_document(doc: &Document, path: &Path) -> Result<(), CommandError> {
        Self::write_document_with_extras(doc, &serde_json::Value::Null, path)
    }

    pub fn write_document_with_extras(doc: &Document, extras: &serde_json::Value, path: &Path) -> Result<(), CommandError> {
        crate::native_io::save_file(doc, extras, path).map_err(|err| err.write_error())
    }

    pub fn native_extras(&self, id: ContextId) -> Option<&serde_json::Value> {
        self.native_extras.get(&id)
    }

    fn write_context(&mut self, id: ContextId, target: &Path) -> Result<(), CommandError> {
        let ctx = self.get(id).ok_or(CommandError::context_not_found())?;
        let doc = ctx.session.document().ok_or(CommandError::no_document())?;
        if target.as_os_str().is_empty() || self.path_clash(id, target).is_some() {
            return Err(CommandError::invalid_params());
        }
        let mut extras = self.native_extras.get(&id).cloned().filter(serde_json::Value::is_object).unwrap_or_else(|| serde_json::json!({}));
        extras[NATIVE_GUIDES_KEY] = serde_json::to_value(&ctx.session.guides).map_err(|_| CommandError::new("INTERNAL_ERROR"))?;
        let mut view = serde_json::to_value(&ctx.view_state).map_err(|_| CommandError::new("INTERNAL_ERROR"))?;
        if let Some(target) = ctx.current_view_target() {
            view["target"] = serde_json::json!(target);
        }
        extras[NATIVE_VIEW_KEY] = view;
        Self::write_document_with_extras(doc, &extras, target)?;
        if ctx.session.history.save_with_document {
            crate::history_store::HistoryStore::from_undo(&ctx.session.undo).save_sidecar(target)?;
        }
        if let Some(ctx) = self.get_mut(id) {
            ctx.mark_saved();
            ctx.view_file_modified = std::fs::metadata(target).and_then(|meta| meta.modified()).ok();
        }
        self.push_recent(target);
        Ok(())
    }

    pub fn save(&mut self, id: ContextId, path: Option<&Path>) -> Result<PathBuf, CommandError> {
        let ctx = self.get(id).ok_or(CommandError::context_not_found())?;
        if !ctx.session.has_document {
            return Err(CommandError::no_document());
        }
        let target = match path {
            Some(p) if ctx.path.as_deref().is_some_and(|owned| !same_path(owned, p)) => {
                return Err(CommandError::invalid_params());
            }
            Some(p) => p.to_path_buf(),
            None => ctx.path.clone().ok_or(CommandError::invalid_params())?,
        };
        let info = self.stamp_fields(id, &target);
        if let Err(err) = self.write_context(id, &target) {
            self.restore_fields(id, info);
            return Err(err);
        }
        if let Some(ctx) = self.get_mut(id) {
            if ctx.path.is_none() {
                ctx.path = Some(target.clone());
            }
        }
        Ok(target)
    }

    fn stamp_fields(&mut self, id: ContextId, path: &Path) -> Option<crate::longdoc::fields::FieldDocInfo> {
        let ctx = self.get_mut(id)?;
        Some(crate::longdoc::fields::stamp_saved(&mut ctx.session, path))
    }

    fn restore_fields(&mut self, id: ContextId, info: Option<crate::longdoc::fields::FieldDocInfo>) {
        if let (Some(ctx), Some(info)) = (self.get_mut(id), info) {
            ctx.session.document.field_info = info;
            crate::longdoc::fields::refresh_fields(&mut ctx.session);
        }
    }

    pub fn save_as(&mut self, id: ContextId, path: &Path) -> Result<(), CommandError> {
        let info = self.stamp_fields(id, path);
        if let Err(err) = self.write_context(id, path) {
            self.restore_fields(id, info);
            return Err(err);
        }
        if let Some(ctx) = self.get_mut(id) {
            ctx.path = Some(path.to_path_buf());
            if let Some(name) = path.file_stem() {
                ctx.title = name.to_string_lossy().to_string();
            }
        }
        Ok(())
    }

    pub fn save_context(&mut self, id: ContextId) -> Result<(), CommandError> {
        if !self.contexts.iter().any(|c| c.id == id) {
            return Err(CommandError::context_not_found());
        }
        self.save(id, None).map(|_| ())
    }

    pub fn session(&self) -> Option<&Session> {
        self.active().and_then(|id| self.get(id)).map(|c| &c.session)
    }

    pub fn session_mut(&mut self) -> Option<&mut Session> {
        let id = self.active()?;
        self.get_mut(id).map(|c| &mut c.session)
    }

    pub fn selection(&self) -> Option<&Selection> {
        self.session().map(|s| &s.selection)
    }

    pub fn clipboard(&self) -> Option<&AppClipboard> {
        self.session().map(|s| &s.clipboard)
    }

    pub fn app_clipboard(&self) -> &AppClipboard {
        &self.clipboard
    }

    pub fn remember_open_views(&mut self) {
        self.remember_views(|_| true);
    }

    fn remember_views(&mut self, includes: impl Fn(ContextId) -> bool) {
        for ctx in self.contexts.iter().filter(|ctx| includes(ctx.id)) {
            let (Some(path), Some(modified), Some(target)) = (
                &ctx.path, ctx.view_file_modified, ctx.current_view_target(),
            ) else { continue };
            self.recent_views.retain(|entry| !same_path(&entry.path, path));
            self.recent_views.insert(0, RecentView { path: path.clone(), modified, target });
        }
        self.recent_views.truncate(RECENT_CAP);
        self.save_recent();
    }

    fn remove_context(&mut self, id: ContextId) {
        self.remember_views(|owner| owner == id);
        self.native_extras.remove(&id);
        self.jobs.cancel_for_context(id);
        let index = self.contexts.iter().position(|c| c.id == id);
        if let Some(idx) = index {
            self.contexts.remove(idx);

            if self.active_id == Some(id) {
                let target = self.contexts.get(idx).or_else(|| self.contexts.last()).map(|context| context.id);
                self.switch_active(target);
            }
        }
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new(Arc::new(SharedServices::default()))
    }
}

pub type SharedWorkspace = std::sync::Arc<std::sync::Mutex<Workspace>>;

impl Workspace {
    pub fn into_shared(self) -> SharedWorkspace { std::sync::Arc::new(std::sync::Mutex::new(self)) }
}

pub fn run_shared(workspace: &SharedWorkspace, scope: Scope, command: Command) -> (Option<ContextId>, CommandResult) {
    if crate::commands::export_batch::is_ours(&command) {
        return crate::commands::export_batch::run_detached(scope, &command);
    }
    match workspace.lock() {
        Ok(mut workspace) => workspace.run(scope, command),
        Err(_) => (scope.context, Err(CommandError::new("INTERNAL_ERROR"))),
    }
}

pub struct HostRequest {
    pub scope: Scope,
    pub command: Command,
    pub expected_active_path: Option<PathBuf>,
    pub reply: std::sync::mpsc::Sender<HostReply>,
}

#[derive(Debug)]
pub struct HostReply {
    pub context: Option<ContextId>,
    pub result: CommandResult,
    pub generation: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostState {
    pub active: Option<ContextId>,
    pub active_path: Option<PathBuf>,
    pub contexts: Vec<(ContextId, Option<PathBuf>)>,
    pub generation: Option<u64>,
}

pub type HostWake = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostConfirm {
    pub id: u64,
    pub class: String,
    pub message_key: String,
    pub name: String,
    pub context: Option<ContextId>,
    pub expires_at: std::time::Instant,
}

pub type HostConfirmAnswer = Arc<dyn Fn(u64, bool) -> bool + Send + Sync>;

#[derive(Default)]
struct HostConfirmsInner {
    items: Vec<HostConfirm>,
    answer: Option<HostConfirmAnswer>,
    ui_attached: bool,
}

#[derive(Clone, Default)]
pub struct HostConfirms {
    inner: Arc<std::sync::Mutex<HostConfirmsInner>>,
}

impl std::fmt::Debug for HostConfirms {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("HostConfirms") }
}

impl HostConfirms {
    fn lock(&self) -> std::sync::MutexGuard<'_, HostConfirmsInner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn push(&self, item: HostConfirm) {
        let mut inner = self.lock();
        let now = std::time::Instant::now();
        inner.items.retain(|existing| existing.id != item.id && existing.expires_at > now);
        inner.items.push(item);
    }

    pub fn withdraw(&self, id: u64) { self.lock().items.retain(|item| item.id != id); }

    pub fn set_answer(&self, answer: HostConfirmAnswer) { self.lock().answer = Some(answer); }

    pub fn pending(&self) -> Vec<HostConfirm> {
        let now = std::time::Instant::now();
        self.lock().items.iter().filter(|item| item.expires_at > now).cloned().collect()
    }

    pub fn answer(&self, id: u64, approve: bool) -> bool {
        let answer = {
            let mut inner = self.lock();
            let before = inner.items.len();
            inner.items.retain(|item| item.id != id);
            if inner.items.len() == before { return false; }
            inner.answer.clone()
        };
        answer.map(|answer| answer(id, approve)).unwrap_or(false)
    }

    pub fn set_ui_attached(&self, attached: bool) { self.lock().ui_attached = attached; }

    pub fn ui_attached(&self) -> bool { self.lock().ui_attached }
}

#[derive(Clone)]
pub struct HostHandle {
    tx: std::sync::mpsc::Sender<HostRequest>,
    wake: HostWake,
    state: Arc<std::sync::Mutex<HostState>>,
    confirms: HostConfirms,
}

impl std::fmt::Debug for HostHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("HostHandle") }
}

impl HostHandle {
    pub fn send(&self, request: HostRequest) -> Result<(), HostRequest> {
        match self.tx.send(request) {
            Ok(()) => { (self.wake)(); Ok(()) }
            Err(std::sync::mpsc::SendError(request)) => Err(request),
        }
    }

    pub fn state(&self) -> HostState {
        self.state.lock().map(|state| state.clone()).unwrap_or_default()
    }

    pub fn confirms(&self) -> HostConfirms { self.confirms.clone() }

    pub fn request_confirm(&self, item: HostConfirm) {
        self.confirms.push(item);
        (self.wake)();
    }

    pub fn request(&self, scope: Scope, command: Command, expected_active_path: Option<PathBuf>, timeout: std::time::Duration) -> HostReply {
        let (reply, rx) = std::sync::mpsc::channel();
        if self.send(HostRequest { scope, command, expected_active_path, reply }).is_err() {
            return HostReply { context: scope.context, result: Err(CommandError::shutdown()), generation: None };
        }
        match rx.recv_timeout(timeout) {
            Ok(reply) => reply,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => HostReply { context: scope.context, result: Err(CommandError::ui_timeout()), generation: None },
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => HostReply { context: scope.context, result: Err(CommandError::shutdown()), generation: None },
        }
    }
}

pub struct HostInbox {
    rx: std::sync::mpsc::Receiver<HostRequest>,
    state: Arc<std::sync::Mutex<HostState>>,
    confirms: HostConfirms,
}

impl HostInbox {
    pub fn try_next(&self) -> Option<HostRequest> { self.rx.try_recv().ok() }

    pub fn confirms(&self) -> HostConfirms { self.confirms.clone() }

    pub fn publish(&self, workspace: &Workspace) {
        if let Ok(mut state) = self.state.lock() { *state = workspace.host_state(); }
    }

    pub fn drain(&self, workspace: &mut Workspace, budget: usize) -> usize {
        let mut applied = 0;
        while applied < budget {
            let Some(request) = self.try_next() else { break };
            workspace.apply_host_request(request);
            applied += 1;
        }
        self.publish(workspace);
        applied
    }
}

pub fn host_channel(wake: HostWake) -> (HostHandle, HostInbox) {
    let (tx, rx) = std::sync::mpsc::channel();
    let state = Arc::new(std::sync::Mutex::new(HostState::default()));
    let confirms = HostConfirms::default();
    (HostHandle { tx, wake, state: Arc::clone(&state), confirms: confirms.clone() }, HostInbox { rx, state, confirms })
}

impl Workspace {
    pub fn set_host_confirms(&mut self, confirms: Option<HostConfirms>) { self.host_confirms = confirms; }

    pub fn host_confirms(&self) -> Option<&HostConfirms> { self.host_confirms.as_ref() }

    pub fn host_state(&self) -> HostState {
        let active = self.active();
        let active_ctx = active.and_then(|id| self.get(id));
        HostState {
            active,
            active_path: active_ctx.and_then(|ctx| ctx.path.clone()),
            contexts: self.contexts.iter().map(|ctx| (ctx.id, ctx.path.clone())).collect(),
            generation: active_ctx.map(|ctx| ctx.session.generation),
        }
    }

    pub fn apply_host_request(&mut self, request: HostRequest) {
        let HostRequest { scope, command, expected_active_path, reply } = request;
        if scope.context.is_none() {
            if let Some(expected) = expected_active_path {
                let actual = self.active().and_then(|id| self.get(id)).and_then(|ctx| ctx.path.clone());
                if actual.as_deref() != Some(expected.as_path()) {
                    let _ = reply.send(HostReply { context: self.active(), result: Err(CommandError::new("NEEDS_CONFIRMATION")), generation: None });
                    return;
                }
            }
        }
        let (context, result) = if crate::commands::export_batch::is_ours(&command) {
            crate::commands::export_batch::run_detached(scope, &command)
        } else {
            self.run(scope, command)
        };
        let generation = context.and_then(|id| self.get(id)).map(|ctx| ctx.session.generation);
        let _ = reply.send(HostReply { context, result, generation });
    }
}
