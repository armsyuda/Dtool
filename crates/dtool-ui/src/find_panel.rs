use dtool_strings::text;
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FindFormat {
    pub size_pt: String,
    pub family: String,
    pub weight: String,
    pub color: String,
    pub italic: bool,
    pub char_style: String,
    pub para_style: String,
}

impl FindFormat {
    pub fn to_value(&self, paragraph: bool) -> Option<Value> {
        let mut map = Map::new();
        if let Ok(size) = self.size_pt.trim().parse::<f64>() {
            map.insert("size_pt".into(), size.into());
        }
        for (key, value) in [("family", &self.family), ("color", &self.color), ("char_style", &self.char_style)] {
            if !value.trim().is_empty() {
                map.insert(key.into(), value.trim().into());
            }
        }
        if let Ok(weight) = self.weight.trim().parse::<u64>() {
            map.insert("weight".into(), weight.into());
        }
        if self.italic {
            map.insert("italic".into(), true.into());
        }
        if paragraph && !self.para_style.trim().is_empty() {
            map.insert("para_style".into(), self.para_style.trim().into());
        }
        (!map.is_empty()).then_some(Value::Object(map))
    }

    fn ui(&mut self, ui: &mut egui::Ui, paragraph: bool) {
        egui::Grid::new(("find.format.grid", paragraph)).num_columns(2).show(ui, |ui| {
            for (key, value) in [("find.format.size", &mut self.size_pt), ("find.format.family", &mut self.family), ("find.format.weight", &mut self.weight), ("find.format.color", &mut self.color), ("find.format.char_style", &mut self.char_style)] {
                ui.label(text(key));
                ui.text_edit_singleline(value);
                ui.end_row();
            }
            if paragraph {
                ui.label(text("find.format.para_style"));
                ui.text_edit_singleline(&mut self.para_style);
                ui.end_row();
            }
            ui.label(text("find.format.italic"));
            ui.checkbox(&mut self.italic, "");
            ui.end_row();
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindPanelScope {
    Document,
    Story,
    Selection,
}

pub const FIND_SCOPES: [(FindPanelScope, &str); 3] = [
    (FindPanelScope::Document, "find.scope.document"),
    (FindPanelScope::Story, "find.scope.story"),
    (FindPanelScope::Selection, "find.scope.selection"),
];

#[derive(Clone, Debug, PartialEq)]
pub enum FindRequest {
    Next { backwards: bool },
    Replace,
    ReplaceAll,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FindPanel {
    pub pattern: String,
    pub replacement: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub scope: FindPanelScope,
    pub status: Option<String>,
    pub format: FindFormat,
    pub replace_format: FindFormat,
}

impl Default for FindPanel {
    fn default() -> Self {
        Self { pattern: String::new(), replacement: String::new(), regex: false, case_sensitive: false, whole_word: false, scope: FindPanelScope::Document, status: None, format: FindFormat::default(), replace_format: FindFormat::default() }
    }
}

impl FindPanel {
    pub fn can_search(&self) -> bool {
        !self.pattern.is_empty() || self.format_value().is_some()
    }

    pub fn format_value(&self) -> Option<Value> {
        self.format.to_value(true)
    }

    pub fn replace_format_value(&self) -> Option<Value> {
        self.replace_format.to_value(false)
    }

    pub fn request(&self, request: FindRequest) -> Option<FindRequest> {
        self.can_search().then_some(request)
    }

    pub fn report_found(&mut self, count: Option<usize>) {
        self.status = Some(match count {
            Some(0) | None => text("find.status.none").to_owned(),
            Some(n) => format!("{} {n}", text("find.status.replaced")),
        });
    }

    pub fn report_error(&mut self) {
        self.status = Some(text("find.status.invalid").to_owned());
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<FindRequest> {
        let mut out = None;
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(text("find.find"));
                ui.text_edit_singleline(&mut self.pattern);
            });
            ui.horizontal(|ui| {
                ui.label(text("find.replace_with"));
                ui.text_edit_singleline(&mut self.replacement);
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.regex, text("find.regex"));
                ui.checkbox(&mut self.case_sensitive, text("find.match_case"));
                ui.checkbox(&mut self.whole_word, text("find.whole_word"));
                let labels: Vec<&str> = FIND_SCOPES.iter().map(|(_, key)| text(key)).collect();
                let mut index = FIND_SCOPES.iter().position(|(scope, _)| *scope == self.scope).unwrap_or(0);
                if dtool_design::dropdown(ui, "find.scope", &mut index, &labels).changed() {
                    self.scope = FIND_SCOPES[index].0;
                }
            });
            if dtool_strings::has("find.format") {
                egui::CollapsingHeader::new(text("find.format")).id_salt("find.format").show(ui, |ui| self.format.ui(ui, true));
            }
            if dtool_strings::has("find.replace_format") {
                egui::CollapsingHeader::new(text("find.replace_format")).id_salt("find.replace_format").show(ui, |ui| self.replace_format.ui(ui, false));
            }
            ui.add_enabled_ui(self.can_search(), |ui| {
                ui.horizontal(|ui| {
                    if ui.button(text("find.previous")).clicked() {
                        out = self.request(FindRequest::Next { backwards: true });
                    }
                    if ui.button(text("find.next")).clicked() {
                        out = self.request(FindRequest::Next { backwards: false });
                    }
                    if ui.button(text("find.replace")).clicked() {
                        out = self.request(FindRequest::Replace);
                    }
                    if ui.button(text("find.replace_all")).clicked() {
                        out = self.request(FindRequest::ReplaceAll);
                    }
                });
            });
            if let Some(status) = &self.status {
                ui.label(status.as_str());
            }
        });
        out
    }
}