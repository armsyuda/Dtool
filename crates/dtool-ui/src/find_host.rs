use dtool_core::serde_json::{json, Value};
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_core::{Command, CommandResult, Session};
use dtool_strings::text;

use crate::find_panel::{FindPanel, FindPanelScope, FindRequest};

pub const FIND_MENU_ID: &str = "window.text.find_and_replace";

pub fn window_id() -> egui::Id {
    egui::Id::unique("dtool.find_panel")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoundSpan {
    pub story: u64,
    pub start: usize,
    pub end: usize,
}

#[derive(Default)]
pub struct FindHost {
    pub panel: FindPanel,
    pub open: bool,
    last: Option<(ContextId, FoundSpan)>,
}

fn span(value: &Value) -> Option<FoundSpan> {
    let get = |key: &str| value.get(key).and_then(Value::as_u64);
    Some(FoundSpan { story: get("story")?, start: get("start")? as usize, end: get("end")? as usize })
}

fn edited_story(session: &Session) -> Option<u64> {
    if let Some(edit) = &session.text_edit {
        return Some(edit.story.0);
    }
    session.selection.ids.iter().find_map(|id| session.document.get_object(*id).and_then(|object| object.story_id())).map(|story| story.0)
}

impl FindHost {
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    pub fn last_match(&self) -> Option<FoundSpan> {
        self.last.map(|(_, found)| found)
    }

    pub fn base_params(&self, session: &Session) -> Option<Value> {
        let mut params = json!({ "pattern": self.panel.pattern, "regex": self.panel.regex, "case_sensitive": self.panel.case_sensitive, "whole_word": self.panel.whole_word });
        if let Some(format) = self.panel.format_value() {
            params["format"] = format;
        }
        match self.panel.scope {
            FindPanelScope::Document => params["scope"] = "document".into(),
            FindPanelScope::Story => {
                params["scope"] = "story".into();
                params["story"] = edited_story(session)?.into();
            }
            FindPanelScope::Selection => params["scope"] = "selection".into(),
        }
        Some(params)
    }

    pub fn replace_params(&self, params: &mut Value) {
        let replace_format = self.panel.replace_format_value();
        if !(self.panel.replacement.is_empty() && replace_format.is_some()) {
            params["replacement"] = self.panel.replacement.clone().into();
        }
        if let Some(format) = replace_format {
            params["replace_format"] = format;
        }
    }

    fn select(workspace: &mut Workspace, id: ContextId, found: FoundSpan) {
        let Some(context) = workspace.get(id) else { return };
        let object = context.session.document.objects.iter().find(|object| object.story_id().map(|story| story.0) == Some(found.story)).map(|object| object.id.0);
        let mut params = json!({ "story": found.story, "range": { "start": found.start, "end": found.end } });
        if let Some(object) = object {
            params["id"] = object.into();
        }
        let _ = workspace.run(Scope::of(id), Command::with_params("text.select", params));
    }

    fn run(workspace: &mut Workspace, id: ContextId, name: &str, params: Value) -> CommandResult {
        workspace.run(Scope::of(id), Command::with_params(name, params)).1
    }

    fn fail(&mut self, code: &str) {
        self.last = None;
        if code == "INVALID_PATTERN" || code == "INVALID_PARAMS" {
            self.panel.report_error();
        } else {
            self.panel.report_found(None);
        }
    }

    fn find(&mut self, workspace: &mut Workspace, id: ContextId, mut params: Value, from: Option<(u64, usize)>, backwards: bool) -> Option<FoundSpan> {
        if let Some((story, at)) = from {
            params["from"] = json!({ "story": story, "at": at });
        }
        params["backwards"] = backwards.into();
        match Self::run(workspace, id, "text.find", params) {
            Ok(reply) => match span(&reply["match"]) {
                Some(found) => {
                    self.last = Some((id, found));
                    self.panel.status = None;
                    Self::select(workspace, id, found);
                    Some(found)
                }
                None => {
                    self.fail("");
                    None
                }
            },
            Err(error) => {
                self.fail(&error.code);
                None
            }
        }
    }

    pub fn request(&mut self, workspace: &mut Workspace, request: FindRequest) -> bool {
        let Some(request) = self.panel.request(request) else { return false };
        let Some(id) = workspace.active() else { return false };
        let Some(params) = workspace.get(id).and_then(|context| self.base_params(&context.session)) else {
            self.fail("INVALID_PARAMS");
            return false;
        };
        let last = self.last.filter(|(owner, _)| *owner == id).map(|(_, found)| found);
        match request {
            FindRequest::Next { backwards } => {
                let from = last.map(|found| (found.story, if backwards { found.start } else { found.end }));
                self.find(workspace, id, params, from, backwards);
                false
            }
            FindRequest::Replace => {
                let Some(target) = last else {
                    self.find(workspace, id, params, None, false);
                    return false;
                };
                let mut replace = params.clone();
                self.replace_params(&mut replace);
                replace["target"] = json!({ "story": target.story, "start": target.start, "end": target.end });
                match Self::run(workspace, id, "text.replace", replace) {
                    Ok(reply) => {
                        let done = span(&reply["match"]).unwrap_or(target);
                        self.last = None;
                        self.find(workspace, id, params, Some((done.story, done.end)), false);
                        true
                    }
                    Err(error) => {
                        self.fail(&error.code);
                        false
                    }
                }
            }
            FindRequest::ReplaceAll => {
                let mut replace = params;
                self.replace_params(&mut replace);
                self.last = None;
                match Self::run(workspace, id, "text.replace_all", replace) {
                    Ok(reply) => {
                        let count = reply["replaced"].as_u64().unwrap_or(0) as usize;
                        self.panel.report_found(Some(count));
                        count > 0
                    }
                    Err(error) => {
                        self.fail(&error.code);
                        false
                    }
                }
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, workspace: &mut Workspace) -> bool {
        if !self.open {
            return false;
        }
        if self.last.is_some_and(|(owner, _)| workspace.get(owner).is_none()) {
            self.last = None;
        }
        let mut open = self.open;
        let mut request = None;
        egui::Window::new(text("menu.window.text.find_and_replace"))
            .id(window_id())
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| request = self.panel.ui(ui));
        self.open = open;
        request.is_some_and(|request| self.request(workspace, request))
    }
}
