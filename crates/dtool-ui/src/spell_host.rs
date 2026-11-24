use dtool_core::serde_json::{json, Value};
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_core::{Command, CommandResult, Session};
use dtool_strings::text;

use crate::spelling_panel::{show_spelling_panel, SpellPanelAction, SpellPanelView};

pub const CHECK_SPELLING_MENU_ID: &str = "text.spelling.check_spelling";
pub const IGNORE_WORD_MENU_ID: &str = "text.spelling.ignore_misspelled_word";
pub const DICTIONARY_UNAVAILABLE: &str = "SPELL_DICTIONARY_UNAVAILABLE";

pub fn handles(id: &str) -> bool {
    matches!(id, CHECK_SPELLING_MENU_ID | IGNORE_WORD_MENU_ID)
}

pub fn window_id() -> egui::Id {
    egui::Id::unique("dtool.spelling_panel")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpellMark {
    pub story: u64,
    pub start: usize,
    pub end: usize,
    pub word: String,
}

#[derive(Default)]
pub struct SpellHost {
    pub marks: Vec<SpellMark>,
    pub grammar_suggestions: std::collections::BTreeMap<(u64, usize, usize), (String, String)>,
    pub grammar_disabled: bool,
    pub notice: Option<String>,
    pub selected: Option<usize>,
    pub suggestions: Vec<String>,
    pub error: Option<String>,
    pub story: Option<u64>,
    pub choice: String,
    pub open: bool,
    owner: Option<ContextId>,
}

fn active_story(session: &Session) -> Option<u64> {
    if let Some(edit) = &session.text_edit {
        return Some(edit.story.0);
    }
    session.selection.ids.iter().find_map(|id| session.document.get_object(*id).and_then(|object| object.story_id())).map(|story| story.0)
}

fn mark(value: &Value) -> Option<SpellMark> {
    let get = |key: &str| value.get(key).and_then(Value::as_u64);
    Some(SpellMark { story: get("story")?, start: get("start")? as usize, end: get("end")? as usize, word: value.get("word")?.as_str()?.to_owned() })
}

impl SpellHost {
    pub fn rows(&self) -> Vec<String> {
        self.marks.iter().map(|mark| self.grammar_suggestions.get(&(mark.story, mark.start, mark.end)).map(|(key, _)| format!("{}: {}", text(key), mark.word)).unwrap_or_else(|| mark.word.clone())).collect()
    }

    pub fn current(&self) -> Option<&SpellMark> {
        self.selected.and_then(|index| self.marks.get(index))
    }

    fn run(workspace: &mut Workspace, id: ContextId, name: &str, params: Value) -> CommandResult {
        workspace.run(Scope::of(id), Command::with_params(name, params)).1
    }

    pub fn handle_menu(&mut self, id: &str, workspace: &mut Workspace) -> bool {
        match id {
            CHECK_SPELLING_MENU_ID => {
                self.check(workspace);
                true
            }
            IGNORE_WORD_MENU_ID => {
                self.ignore_at_caret(workspace);
                true
            }
            _ => false,
        }
    }

    pub fn check(&mut self, workspace: &mut Workspace) {
        let Some(id) = workspace.active() else { return };
        self.story = workspace.get(id).and_then(|context| active_story(&context.session));
        self.owner = Some(id);
        self.open = true;
        self.recheck(workspace);
    }

    fn recheck(&mut self, workspace: &mut Workspace) {
        let Some(id) = self.owner else { return };
        let mut params = self.story.map_or_else(|| json!({}), |story| json!({ "story": story }));
        params["grammar"] = json!(!self.grammar_disabled);
        let keep = self.selected.unwrap_or(0);
        self.marks.clear(); self.grammar_suggestions.clear(); self.notice = None;
        match Self::run(workspace, id, "text.spell.check", params) {
            Ok(reply) if reply["available"].as_bool() == Some(false) => self.error = Some(DICTIONARY_UNAVAILABLE.to_owned()),
            Ok(reply) => {
                self.marks = reply["marks"].as_array().map(|marks| marks.iter().filter_map(mark).collect()).unwrap_or_default();
                for issue in reply["issues"].as_array().into_iter().flatten() {
                    if let (Some(mark), Some(suggestion), Some(key)) = (mark(issue), issue["suggestion"].as_str(), issue["label_key"].as_str()) {
                        let identity = (mark.story, mark.start, mark.end);
                        self.grammar_suggestions.insert(identity, (key.to_string(), suggestion.to_string()));
                        if !self.marks.iter().any(|m| (m.story, m.start, m.end) == identity) { self.marks.push(mark); }
                    }
                }
                self.marks.sort_by_key(|mark| (mark.story, mark.start, mark.end));
                if reply["ko_available"] == false { self.notice = reply["ko_message_key"].as_str().map(str::to_string); }
                self.error = None;
            }
            Err(error) => self.error = Some(error.code),
        }
        let next = (!self.marks.is_empty()).then(|| keep.min(self.marks.len() - 1));
        self.select(workspace, next);
    }

    fn select(&mut self, workspace: &mut Workspace, index: Option<usize>) {
        self.selected = index.filter(|index| *index < self.marks.len());
        self.suggestions.clear();
        let grammar = self.current().and_then(|mark| self.grammar_suggestions.get(&(mark.story, mark.start, mark.end))).map(|(_, suggestion)| suggestion.clone());
        if let Some(suggestion) = grammar { self.suggestions.push(suggestion); }
        else if let (Some(id), Some(word)) = (self.owner, self.current().map(|mark| mark.word.clone())) {
            if let Ok(reply) = Self::run(workspace, id, "text.spell.suggest", json!({ "word": word })) {
                self.suggestions = reply["suggestions"].as_array().map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect()).unwrap_or_default();
            }
        }
        self.choice = self.suggestions.first().cloned().unwrap_or_default();
    }

    fn replace(&mut self, workspace: &mut Workspace, with: &str) -> bool {
        let (Some(id), Some(mark)) = (self.owner, self.current().cloned()) else { return false };
        let params = json!({ "story": mark.story, "start": mark.start, "end": mark.end, "word": mark.word, "with": with });
        let result = Self::run(workspace, id, "text.spell.replace", params);
        let error = result.as_ref().err().map(|error| error.code.clone());
        self.recheck(workspace);
        if error.is_some() {
            self.error = error;
        }
        result.is_ok()
    }

    fn ignore(&mut self, workspace: &mut Workspace) {
        let (Some(id), Some(word)) = (self.owner, self.current().map(|mark| mark.word.clone())) else { return };
        if Self::run(workspace, id, "text.spell.ignore", json!({ "word": word })).is_ok() {
            self.recheck(workspace);
        }
    }

    fn add_word(&mut self, workspace: &mut Workspace) -> bool {
        let (Some(id), Some(word)) = (self.owner, self.current().map(|mark| mark.word.clone())) else { return false };
        let result = Self::run(workspace, id, "text.spell.add_word", json!({ "word": word }));
        let error = result.as_ref().err().map(|error| error.code.clone());
        self.recheck(workspace);
        if error.is_some() {
            self.error = error;
        }
        result.is_ok()
    }

    pub fn ignore_at_caret(&mut self, workspace: &mut Workspace) {
        if self.owner != workspace.active() || self.owner.is_none() {
            self.check(workspace);
        }
        let caret = self.owner.and_then(|id| workspace.get(id)).and_then(|context| context.session.text_edit.as_ref().map(|edit| (edit.story.0, edit.caret)));
        if let Some((story, caret)) = caret {
            if let Some(index) = self.marks.iter().position(|mark| mark.story == story && mark.start <= caret && caret <= mark.end) {
                self.select(workspace, Some(index));
            }
        }
        self.ignore(workspace);
    }

    pub fn apply(&mut self, workspace: &mut Workspace, action: SpellPanelAction) -> bool {
        match action {
            SpellPanelAction::Select(index) => {
                self.select(workspace, Some(index));
                false
            }
            SpellPanelAction::Replace(word) => self.replace(workspace, &word),
            SpellPanelAction::Ignore => {
                self.ignore(workspace);
                false
            }
            SpellPanelAction::AddWord => {
                self.add_word(workspace);
                false
            }
            SpellPanelAction::Recheck => {
                self.recheck(workspace);
                false
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, workspace: &mut Workspace) -> bool {
        if !self.open {
            return false;
        }
        if self.owner.is_none_or(|id| workspace.get(id).is_none()) {
            self.open = false;
            self.owner = None;
            return false;
        }
        let rows = self.rows();
        let suggestions = self.suggestions.clone();
        let error = self.error.clone();
        let view = SpellPanelView { rows: &rows, selected: self.selected, suggestions: &suggestions, error: error.as_deref() };
        let mut open = self.open;
        let mut action = None;
        let choice = &mut self.choice;
        let mut grammar = !self.grammar_disabled; let mut grammar_changed = false; let notice = self.notice.clone();
        egui::Window::new(text("menu.text.spelling"))
            .id(window_id())
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                grammar_changed = dtool_design::toggle(ui, &mut grammar, text("menu.text.spelling.korean_grammar")).changed();
                if let Some(key) = &notice { ui.label(text(key)); }
                action = show_spelling_panel(ui, &view, choice);
            });
        if grammar_changed {
            self.grammar_disabled = !grammar; crate::canvas::frame_text_input::set_grammar_enabled(grammar); self.recheck(workspace); action = None;
        }
        self.open = open;
        action.is_some_and(|action| self.apply(workspace, action))
    }
}