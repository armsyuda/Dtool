use dtool_core::shortcuts::{Focus, Modifiers};
use dtool_core::workspace::{Scope, Workspace as CoreWorkspace};
use dtool_protocol::Command;
use serde_json::json;

pub(crate) fn primary_button_positions(ui: &egui::Ui) -> (Option<egui::Pos2>, Option<egui::Pos2>) {
    ui.input(|i| {
        let mut down = None;
        let mut up = None;
        for event in &i.events {
            if let egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, .. } = event {
                if *pressed {
                    down = Some(*pos);
                } else {
                    up = Some(*pos);
                }
            }
        }
        (down, up)
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    KeyDown { key: String, modifiers: Modifiers },
    KeyUp { key: String, modifiers: Modifiers },
    PointerMove { x: f32, y: f32 },
    PointerDown { x: f32, y: f32 },
    PointerUp { x: f32, y: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImeEvent {
    CompositionStart { text: String },
    CompositionUpdate { text: String },
    CompositionCommit { text: String },
    CompositionCancel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputState {
    pub focus: Focus,
    pub ime_composing: bool,
    pub ime_text: String,
    pub last_pointer_pos: Option<(f32, f32)>,
    pub pointer_down: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            focus: Focus::Canvas,
            ime_composing: false,
            ime_text: String::new(),
            last_pointer_pos: None,
            pointer_down: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InputHandler {
    state: InputState,
    document_mutated_on_commit: bool,
}

impl InputHandler {
    pub fn new() -> Self {
        Self {
            state: InputState::default(),
            document_mutated_on_commit: false,
        }
    }

    pub fn state(&self) -> &InputState {
        &self.state
    }

    pub fn handle_input_event(&mut self, event: InputEvent) {
        match event {
            InputEvent::KeyDown { key, modifiers } => {
                if self.state.ime_composing {
                    return;
                }
                let _ = (key, modifiers);
            }
            InputEvent::KeyUp { key, modifiers } => {
                if self.state.ime_composing {
                    return;
                }
                let _ = (key, modifiers);
            }
            InputEvent::PointerMove { x, y } => {
                self.state.last_pointer_pos = Some((x, y));
            }
            InputEvent::PointerDown { x, y } => {
                if self.state.ime_composing {
                    return;
                }
                self.state.pointer_down = true;
                self.state.last_pointer_pos = Some((x, y));
            }
            InputEvent::PointerUp { x, y } => {
                if self.state.ime_composing {
                    return;
                }
                self.state.pointer_down = false;
                self.state.last_pointer_pos = Some((x, y));
            }
        }
    }

    pub fn handle_ime_event(&mut self, event: ImeEvent) {
        match event {
            ImeEvent::CompositionStart { text } => {
                self.state.ime_composing = true;
                self.state.ime_text = text;
            }
            ImeEvent::CompositionUpdate { text } => {
                if !self.state.ime_composing {
                    self.state.ime_composing = true;
                }
                self.state.ime_text = text;
            }
            ImeEvent::CompositionCommit { text } => {
                self.state.ime_composing = false;
                self.state.ime_text = String::new();
                self.document_mutated_on_commit = true;
                let _ = text;
            }
            ImeEvent::CompositionCancel => {
                if !self.state.ime_composing {
                    return;
                }
                self.state.ime_composing = false;
                self.state.ime_text = String::new();
            }
        }
    }

    pub fn is_ime_composing(&self) -> bool {
        self.state.ime_composing
    }

    pub fn document_mutated_on_commit(&self) -> bool {
        self.document_mutated_on_commit
    }

    pub fn reset_document_mutation_flag(&mut self) {
        self.document_mutated_on_commit = false;
    }

    pub fn ingest(&mut self, ctx: &egui::Context, focus: Focus) -> Vec<ImeEvent> {
        self.state.focus = focus;
        let events = ctx.input(|input| input.events.clone());
        self.ingest_events(&events, focus)
    }

    pub fn ingest_into(&mut self, ctx: &egui::Context, focus: Focus, workspace: &mut CoreWorkspace) -> Vec<ImeEvent> {
        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let events = self.ingest(ctx, focus);
        if focus != Focus::Canvas {
            return events;
        }
        for event in &events {
            let _ = self.apply_ime(workspace, event);
        }
        if escape {
            if self.is_ime_composing() {
                self.commit_pending(workspace);
            }
            self.end_edit(workspace);
        }
        events
    }

    pub fn ingest_events(&mut self, events: &[egui::Event], focus: Focus) -> Vec<ImeEvent> {
        self.state.focus = focus;
        let mut ime_events = Vec::new();
        for event in events {
            match event {
                egui::Event::Ime(ime) => {
                    for mapped in map_egui_ime(ime, self.state.ime_composing, &self.state.ime_text) {
                        self.handle_ime_event(mapped.clone());
                        ime_events.push(mapped);
                    }
                }
                egui::Event::Text(text) => {
                    if !self.state.ime_composing && !text.is_empty() && focus == Focus::Canvas {
                        let mapped = ImeEvent::CompositionCommit { text: text.clone() };
                        self.handle_ime_event(mapped.clone());
                        ime_events.push(mapped);
                    }
                }
                egui::Event::Key { key, pressed, modifiers, .. } => {
                    let key = map_key(*key);
                    let modifiers = map_modifiers(*modifiers);
                    if *pressed {
                        self.handle_input_event(InputEvent::KeyDown { key, modifiers });
                    } else {
                        self.handle_input_event(InputEvent::KeyUp { key, modifiers });
                    }
                }
                egui::Event::PointerMoved(pos) => {
                    self.handle_input_event(InputEvent::PointerMove { x: pos.x, y: pos.y });
                }
                egui::Event::PointerButton { pos, pressed, .. } => {
                    if *pressed {
                        self.handle_input_event(InputEvent::PointerDown { x: pos.x, y: pos.y });
                    } else {
                        self.handle_input_event(InputEvent::PointerUp { x: pos.x, y: pos.y });
                    }
                }
                _ => {}
            }
        }
        ime_events
    }

    pub fn apply_ime(&mut self, workspace: &mut CoreWorkspace, event: &ImeEvent) -> bool {
        match event {
            ImeEvent::CompositionCommit { text } if !text.is_empty() => self.insert_at_focus(workspace, text),
            _ => false,
        }
    }

    pub fn insert_at_focus(&mut self, workspace: &mut CoreWorkspace, text: &str) -> bool {
        let Some(session) = workspace.session() else {
            return false;
        };
        let Some((story, at)) = focused_insert_at(session) else {
            return false;
        };
        let command = insert_command(story, at, text);
        let (_, result) = workspace.run(Scope::active(), command);
        match result {
            Ok(_) => {
                self.document_mutated_on_commit = true;
                if let Some(session) = workspace.session_mut() {
                    let caret = at + text.chars().count();
                    place_caret(session, caret, caret);
                }
                true
            }
            Err(_) => false,
        }
    }

    pub fn commit_pending(&mut self, workspace: &mut CoreWorkspace) -> bool {
        if !self.state.ime_composing {
            return false;
        }
        let text = std::mem::take(&mut self.state.ime_text);
        self.state.ime_composing = false;
        if text.is_empty() {
            false
        } else {
            self.insert_at_focus(workspace, &text)
        }
    }

    pub fn end_edit(&mut self, workspace: &mut CoreWorkspace) {
        if let Some(session) = workspace.session_mut() {
            let _ = session.run(&Command::new("text.edit.end"));
        }
    }

    pub fn commit_pending_and_end_edit(&mut self, workspace: &mut CoreWorkspace) -> bool {
        let inserted = self.commit_pending(workspace);
        self.end_edit(workspace);
        inserted
    }

    pub fn commit_on_escape(&mut self, workspace: &mut CoreWorkspace) -> bool {
        self.commit_pending_and_end_edit(workspace)
    }

    pub fn commit_on_tool_switch(&mut self, workspace: &mut CoreWorkspace) -> bool {
        self.commit_pending_and_end_edit(workspace)
    }

    pub fn commit_on_tab_switch(&mut self, workspace: &mut CoreWorkspace) -> bool {
        self.commit_pending_and_end_edit(workspace)
    }

    pub fn commit_on_ime_disabled(&mut self, workspace: &mut CoreWorkspace) -> bool {
        self.commit_pending_and_end_edit(workspace)
    }

    pub fn compose(&mut self, workspace: &mut CoreWorkspace, params: &serde_json::Value) -> bool {
        if let Some(preedit) = params.get("preedit").and_then(serde_json::Value::as_str) {
            self.handle_ime_event(ImeEvent::CompositionStart { text: preedit.to_string() });
        }
        if let Some(commit) = params.get("commit").and_then(serde_json::Value::as_str) {
            if !commit.is_empty() {
                if let (Some(story), Some(at)) = (
                    params.get("story").and_then(serde_json::Value::as_u64),
                    params.get("at").and_then(serde_json::Value::as_u64),
                ) {
                    self.state.ime_composing = false;
                    self.state.ime_text.clear();
                    let (_, result) = workspace.run(Scope::active(), insert_command(story, at as usize, commit));
                    if result.is_ok() {
                        self.document_mutated_on_commit = true;
                        return true;
                    }
                    return false;
                }
                self.state.ime_composing = true;
                self.state.ime_text = commit.to_string();
                return self.commit_pending(workspace);
            }
            self.state.ime_composing = false;
            self.state.ime_text.clear();
        }
        self.is_ime_composing() || self.document_mutated_on_commit
    }
}

pub fn insert_command(story: u64, at: usize, text: &str) -> Command {
    Command::with_params("text.insert", json!({ "story": story, "at": at, "text": text }))
}

fn focused_insert_at(session: &dtool_core::Session) -> Option<(u64, usize)> {
    session.text_edit.as_ref().map(|edit| (edit.story.0, edit.caret))
}

fn map_key(key: egui::Key) -> String {
    let name = key.name();
    if name.len() == 1 {
        name.to_lowercase()
    } else {
        name.to_string()
    }
}

fn map_modifiers(modifiers: egui::Modifiers) -> Modifiers {
    Modifiers { ctrl: modifiers.ctrl || modifiers.command, shift: modifiers.shift, alt: modifiers.alt }
}

fn map_egui_ime(event: &egui::ImeEvent, composing: bool, ime_text: &str) -> Vec<ImeEvent> {
    match event {
        #[allow(deprecated)]
        egui::ImeEvent::Enabled => vec![ImeEvent::CompositionStart { text: String::new() }],
        egui::ImeEvent::Preedit { text, .. } => {
            if text.is_empty() {
                if composing {
                    vec![ImeEvent::CompositionCancel]
                } else {
                    Vec::new()
                }
            } else if composing {
                vec![ImeEvent::CompositionUpdate { text: text.clone() }]
            } else {
                vec![ImeEvent::CompositionStart { text: text.clone() }]
            }
        }
        egui::ImeEvent::Commit(text) => vec![ImeEvent::CompositionCommit { text: text.clone() }],
        #[allow(deprecated)]
        egui::ImeEvent::Disabled => {
            if composing {
                vec![ImeEvent::CompositionCommit { text: ime_text.to_string() }]
            } else {
                Vec::new()
            }
        }
        egui::ImeEvent::DeleteSurrounding { .. } => Vec::new(),
    }
}

pub const IME_COMPOSE: &str = "ui.ime.compose";

pub fn compose_command(params: serde_json::Value) -> Command {
    Command::with_params(IME_COMPOSE, params)
}

pub fn ime_output(rect: egui::Rect, cursor_rect: egui::Rect) -> egui::output::IMEOutput {
    egui::output::IMEOutput {
        purpose: egui::IMEPurpose::Normal,
        rect,
        cursor_rect,
        should_interrupt_composition: false,
    }
}

pub fn publish_ime_caret(ctx: &egui::Context, rect: egui::Rect) {
    ctx.output_mut(|output| output.ime = Some(ime_output(rect, rect)));
}

pub struct ShortcutDispatcher {
    handler: InputHandler,
    map: crate::shortcuts::ShortcutMap,
}

impl ShortcutDispatcher {
    pub fn new(handler: InputHandler) -> Self {
        Self { handler, map: crate::shortcuts::ShortcutMap::affinity_default() }
    }

    pub fn with_map(handler: InputHandler, map: crate::shortcuts::ShortcutMap) -> Self {
        Self { handler, map }
    }

    pub fn dispatch_shortcut(
        &self,
        focus: Focus,
        key: String,
        modifiers: Modifiers,
    ) -> Option<String> {
        if self.handler.is_ime_composing() {
            return None;
        }
        self.map.dispatch(focus, &key, modifiers)
    }

    pub fn handler(&self) -> &InputHandler {
        &self.handler
    }

    pub fn handler_mut(&mut self) -> &mut InputHandler {
        &mut self.handler
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKey {
    Backspace,
    Delete,
    Left,
    Right,
    PreviousColumn,
    NextColumn,
    Home,
    End,
    Enter,
    Tab,
    Escape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditAction {
    Preedit(String),
    Commit(String),
    Cancel,
    Key { key: EditKey, shift: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditRoute {
    #[default]
    Session,
    Core,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComposeMachine {
    preedit: String,
    composing: bool,
}

impl ComposeMachine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_composing(&self) -> bool {
        self.composing
    }

    pub fn preedit(&self) -> Option<&str> {
        if self.composing && !self.preedit.is_empty() {
            Some(&self.preedit)
        } else {
            None
        }
    }

    pub fn clear(&mut self) {
        self.composing = false;
        self.preedit.clear();
    }

    pub fn take_pending(&mut self) -> Option<String> {
        self.composing = false;
        let text = std::mem::take(&mut self.preedit);
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    pub fn feed(&mut self, events: &[egui::Event]) -> Vec<EditAction> {
        self.feed_oriented(events, false)
    }

    pub(crate) fn feed_oriented(&mut self, events: &[egui::Event], vertical: bool) -> Vec<EditAction> {
        let mut actions = Vec::new();
        let mut committed: Vec<String> = Vec::new();
        let mut ime_seen = false;
        for event in events {
            match event {
                egui::Event::Ime(ime) => {
                    ime_seen |= self.feed_ime(ime, &mut actions, &mut committed);
                }
                egui::Event::Text(text) => {
                    if self.composing || text.is_empty() || text.chars().any(char::is_control) {
                        continue;
                    }
                    if let Some(index) = committed.iter().position(|done| done == text) {
                        committed.remove(index);
                        continue;
                    }
                    actions.push(EditAction::Commit(text.clone()));
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    let key = match (vertical, *key) {
                        (true, egui::Key::ArrowUp) => Some(EditKey::Left),
                        (true, egui::Key::ArrowDown) => Some(EditKey::Right),
                        (true, egui::Key::ArrowLeft) => Some(EditKey::NextColumn),
                        (true, egui::Key::ArrowRight) => Some(EditKey::PreviousColumn),
                        _ => edit_key(*key),
                    };
                    let Some(key) = key else {
                        continue;
                    };
                    if key == EditKey::Escape {
                        if let Some(text) = self.take_pending() {
                            actions.push(EditAction::Commit(text));
                        }
                        actions.push(EditAction::Key { key, shift: modifiers.shift });
                        continue;
                    }
                    if self.composing || ime_seen || modifiers.ctrl || modifiers.command || modifiers.alt || key == EditKey::Tab && modifiers.shift {
                        continue;
                    }
                    actions.push(EditAction::Key { key, shift: modifiers.shift });
                }
                _ => {}
            }
        }
        actions
    }

    fn feed_ime(&mut self, ime: &egui::ImeEvent, actions: &mut Vec<EditAction>, committed: &mut Vec<String>) -> bool {
        match ime {
            egui::ImeEvent::Preedit { text, .. } => {
                if text.is_empty() {
                    if !self.composing {
                        return false;
                    }
                    self.clear();
                    actions.push(EditAction::Cancel);
                } else {
                    self.composing = true;
                    self.preedit = text.clone();
                    actions.push(EditAction::Preedit(text.clone()));
                }
                true
            }
            egui::ImeEvent::Commit(text) => {
                self.clear();
                if !text.is_empty() {
                    committed.push(text.clone());
                    actions.push(EditAction::Commit(text.clone()));
                }
                true
            }
            #[allow(deprecated)]
            egui::ImeEvent::Disabled => match self.take_pending() {
                Some(text) => {
                    actions.push(EditAction::Commit(text));
                    true
                }
                None => false,
            },
            _ => false,
        }
    }
}

fn edit_key(key: egui::Key) -> Option<EditKey> {
    Some(match key {
        egui::Key::Backspace => EditKey::Backspace,
        egui::Key::Delete => EditKey::Delete,
        egui::Key::ArrowLeft => EditKey::Left,
        egui::Key::ArrowRight => EditKey::Right,
        egui::Key::Home => EditKey::Home,
        egui::Key::End => EditKey::End,
        egui::Key::Enter => EditKey::Enter,
        egui::Key::Tab => EditKey::Tab,
        egui::Key::Escape => EditKey::Escape,
        _ => return None,
    })
}

pub trait TextEditSink {
    fn preedit(&mut self, text: &str);
    fn cancel_preedit(&mut self);
    fn commit_text(&mut self, text: &str) -> bool;
    fn edit_key(&mut self, key: EditKey, shift: bool) -> bool;
}

pub fn apply_edit_actions(sink: &mut dyn TextEditSink, actions: &[EditAction]) -> bool {
    let mut changed = false;
    for action in actions {
        match action {
            EditAction::Preedit(text) => sink.preedit(text),
            EditAction::Cancel => sink.cancel_preedit(),
            EditAction::Commit(text) => changed |= sink.commit_text(text),
            EditAction::Key { key, shift } => changed |= sink.edit_key(*key, *shift),
        }
    }
    changed
}

pub struct SessionEditSink<'a> {
    pub session: &'a mut dtool_core::Session,
}

impl TextEditSink for SessionEditSink<'_> {
    fn preedit(&mut self, _text: &str) {}

    fn cancel_preedit(&mut self) {}

    fn commit_text(&mut self, text: &str) -> bool {
        let typed = type_at_caret(self.session, text);
        if typed {
            autocorrect_typed(self.session, text);
        }
        typed
    }

    fn edit_key(&mut self, key: EditKey, shift: bool) -> bool {
        edit_at_caret(self.session, key, shift)
    }
}

pub const CORE_EDIT_COMMANDS: [&str; 7] = [
    "text.edit.compose",
    "text.edit.compose_commit",
    "text.edit.compose_cancel",
    "text.edit.type",
    "text.edit.backspace",
    "text.edit.delete_forward",
    "text.edit.caret",
];

pub fn core_edit_command(session: &dtool_core::Session, action: &EditAction) -> Option<Command> {
    Some(match action {
        EditAction::Preedit(text) => Command::with_params(CORE_EDIT_COMMANDS[0], json!({ "text": text })),
        EditAction::Commit(text) => Command::with_params(CORE_EDIT_COMMANDS[1], json!({ "text": text })),
        EditAction::Cancel => Command::new(CORE_EDIT_COMMANDS[2]),
        EditAction::Key { key, shift } => match key {
            EditKey::Enter => Command::with_params(CORE_EDIT_COMMANDS[3], json!({ "text": "\n" })),
            EditKey::Backspace => Command::new(CORE_EDIT_COMMANDS[4]),
            EditKey::Delete => Command::new(CORE_EDIT_COMMANDS[5]),
            EditKey::Tab => Command::with_params(CORE_EDIT_COMMANDS[3], json!({ "text": "\t" })),
            EditKey::Escape => return None,
            _ => {
                let span = edit_span(session)?;
                let to = caret_target(session, &span, *key, *shift);
                Command::with_params(CORE_EDIT_COMMANDS[6], json!({ "to": to, "extend": shift }))
            }
        },
    })
}

pub fn apply_to_session(session: &mut dtool_core::Session, actions: &[EditAction], route: EditRoute) -> bool {
    match route {
        EditRoute::Session => apply_edit_actions(&mut SessionEditSink { session }, actions),
        EditRoute::Core => {
            let mut changed = false;
            for action in actions {
                let Some(command) = core_edit_command(session, action) else {
                    continue;
                };
                let ok = session.run(&command).is_ok();
                if let (true, EditAction::Commit(text)) = (ok, action) {
                    autocorrect_typed(session, text);
                }
                changed |= ok && !matches!(action, EditAction::Preedit(_) | EditAction::Cancel);
            }
            changed
        }
    }
}

pub fn autocorrect_typed(session: &mut dtool_core::Session, inserted: &str) -> bool {
    dtool_core::commands::prefs_cmd::snapshot()
        .map(|prefs| dtool_tools::dtool_text::autocorrect::expand_in_edit(session, inserted, &prefs).unwrap_or(false))
        .unwrap_or(false)
}

struct EditSpan {
    story: u64,
    caret: usize,
    anchor: usize,
    chars: Vec<char>,
}

fn edit_span(session: &dtool_core::Session) -> Option<EditSpan> {
    let edit = session.text_edit.as_ref()?;
    let story = session.document.stories.iter().find(|item| item.id == edit.story)?;
    let chars: Vec<char> = story.text.chars().collect();
    Some(EditSpan { story: edit.story.0, caret: edit.caret.min(chars.len()), anchor: edit.anchor.min(chars.len()), chars })
}

fn caret_target(session: &dtool_core::Session, span: &EditSpan, key: EditKey, shift: bool) -> usize {
    let EditSpan { chars, caret, anchor, .. } = span;
    let (caret, anchor) = (*caret, *anchor);
    let (start, end) = (caret.min(anchor), caret.max(anchor));
    match key {
        EditKey::Left if !shift && start != end => start,
        EditKey::Right if !shift && start != end => end,
        EditKey::Left => caret.saturating_sub(1),
        EditKey::Right => (caret + 1).min(chars.len()),
        EditKey::PreviousColumn | EditKey::NextColumn => column_target(session, caret, key).unwrap_or(caret),
        EditKey::Home => chars[..caret].iter().rposition(|c| *c == '\n').map_or(0, |index| index + 1),
        EditKey::End => chars[caret..].iter().position(|c| *c == '\n').map_or(chars.len(), |index| caret + index),
        _ => caret,
    }
}

fn column_target(session: &dtool_core::Session, caret: usize, key: EditKey) -> Option<usize> {
    use dtool_tools::dtool_text as text;
    let edit = session.text_edit.as_ref()?;
    let core = session.document.stories.iter().find(|story| story.id == edit.story)?;
    let story = text::bridge::story_in_document(core, &session.document);
    let lines = text::layout_story(&text::FontLibrary::shared(), &story, None);
    let current = lines.iter().position(|line| line.range.start <= caret && caret <= line.range.end)?;
    let next = if key == EditKey::PreviousColumn { current.checked_sub(1)? } else { current + 1 };
    let target = lines.get(next)?;
    let advance = text::frame::caret_x(&lines[current], caret);
    Some(text::frame::caret_at_point(std::slice::from_ref(target), [advance, target.baseline]))
}

pub fn place_caret(session: &mut dtool_core::Session, caret: usize, anchor: usize) -> bool {
    let Some(edit) = session.text_edit.as_ref() else {
        return false;
    };
    let mut params = json!({ "story": edit.story.0, "range": { "start": caret.min(anchor), "end": caret.max(anchor) } });
    if let Some(id) = edit.object {
        params["id"] = id.0.into();
    }
    if session.run(&Command::with_params("text.select", params)).is_err() {
        return false;
    }
    if let Some(edit) = session.text_edit.as_mut() {
        edit.caret = caret;
        edit.anchor = anchor;
    }
    true
}

fn delete_span(session: &mut dtool_core::Session, story: u64, start: usize, end: usize) -> bool {
    let command = Command::with_params("text.delete", json!({ "story": story, "range": { "start": start, "end": end } }));
    session.run(&command).is_ok()
}

pub fn type_at_caret(session: &mut dtool_core::Session, text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let Some(span) = edit_span(session) else {
        return false;
    };
    let start = span.caret.min(span.anchor);
    let end = span.caret.max(span.anchor);
    if start != end && !delete_span(session, span.story, start, end) {
        return false;
    }
    if session.run(&insert_command(span.story, start, text)).is_err() {
        return false;
    }
    let caret = start + text.chars().count();
    place_caret(session, caret, caret);
    true
}

pub fn edit_at_caret(session: &mut dtool_core::Session, key: EditKey, shift: bool) -> bool {
    let Some(span) = edit_span(session) else {
        return false;
    };
    let (start, end) = (span.caret.min(span.anchor), span.caret.max(span.anchor));
    match key {
        EditKey::Backspace | EditKey::Delete => {
            let (from, to) = if start != end {
                (start, end)
            } else if key == EditKey::Backspace {
                if span.caret == 0 {
                    return false;
                }
                (span.caret - 1, span.caret)
            } else {
                if span.caret >= span.chars.len() {
                    return false;
                }
                (span.caret, span.caret + 1)
            };
            if !delete_span(session, span.story, from, to) {
                return false;
            }
            place_caret(session, from, from);
            true
        }
        EditKey::Enter => session.run(&Command::with_params(CORE_EDIT_COMMANDS[3], json!({ "text": "\n" }))).is_ok(),
        EditKey::Left | EditKey::Right | EditKey::PreviousColumn | EditKey::NextColumn | EditKey::Home | EditKey::End => {
            let target = caret_target(session, &span, key, shift);
            let anchor = if shift { span.anchor } else { target };
            place_caret(session, target, anchor);
            false
        }
        EditKey::Tab => type_at_caret(session, "\t"),
        EditKey::Escape => false,
    }
}
