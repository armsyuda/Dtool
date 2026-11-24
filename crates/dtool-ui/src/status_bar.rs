use egui::{RichText, Ui};
use dtool_design::{color, size, status_bar_frame};

pub fn bar_height() -> f32 {
    size("bar.status.height")
}

pub fn left_slot_width() -> f32 {
    size("bar.status.left_slot")
}

pub fn zoom_label(zoom: f32) -> String {
    let percent = if zoom.is_finite() { (zoom * 100.0).round() as i64 } else { 100 };
    dtool_strings::format("status.zoom", &[("percent", &percent.to_string())])
}

pub struct StatusBar {
    pub visible: bool,
    pub hint_key: String,
    pub stats_text: Option<String>,
    pub zoom_text: Option<String>,
    base_hint_key: String,
    tool_id: String,
    shift_held: bool,
    alt_held: bool,
    ctrl_held: bool,
}

impl Default for StatusBar {
    fn default() -> Self {
        Self {
            visible: true,
            hint_key: "status.hint.default".to_string(),
            stats_text: None,
            zoom_text: None,
            base_hint_key: "status.hint.default".to_string(),
            tool_id: String::new(),
            shift_held: false,
            alt_held: false,
            ctrl_held: false,
        }
    }
}

impl StatusBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_hint_key(&mut self, key: &str) {
        self.base_hint_key = key.to_string();
        self.hint_key = key.to_string();
    }

    pub fn set_tool(&mut self, tool_id: &str) {
        self.tool_id = tool_id.to_string();
        self.recompute_hint();
    }

    pub fn set_modifiers(&mut self, shift: bool, alt: bool, ctrl: bool) {
        self.shift_held = shift;
        self.alt_held = alt;
        self.ctrl_held = ctrl;
        self.recompute_hint();
    }

    fn recompute_hint(&mut self) {
        if self.tool_id.is_empty() {
            self.hint_key = self.base_hint_key.clone();
            return;
        }

        let modifier_suffix = self.modifier_suffix();

        if !modifier_suffix.is_empty() {
            let variant_key = format!("status.hint.{}.{}", self.tool_id, modifier_suffix);
            let variant_text = dtool_strings::text(&variant_key);
            if variant_text != variant_key {
                self.hint_key = variant_key;
                return;
            }
        }

        let base_tool_key = format!("status.hint.{}", self.tool_id);
        let base_tool_text = dtool_strings::text(&base_tool_key);
        if base_tool_text != base_tool_key {
            self.hint_key = base_tool_key;
        } else {
            self.hint_key = self.base_hint_key.clone();
        }
    }

    fn modifier_suffix(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl_held {
            parts.push("ctrl");
        }
        if self.alt_held {
            parts.push("alt");
        }
        if self.shift_held {
            parts.push("shift");
        }
        parts.join("+")
    }

    pub fn hint_parts(hint_text: &str) -> Vec<(String, bool)> {
        let mut parts: Vec<(String, bool)> = Vec::new();
        let mut current_part = String::new();
        let mut is_bold = false;
        let mut i = 0;
        let chars: Vec<char> = hint_text.chars().collect();
        while i < chars.len() {
            if chars[i] == '<' && i + 2 < chars.len() && chars[i+1] == 'b' && chars[i+2] == '>' {
                if !current_part.is_empty() {
                    parts.push((current_part.clone(), is_bold));
                    current_part.clear();
                }
                is_bold = true;
                i += 3;
            } else if chars[i] == '<' && i + 3 < chars.len() && chars[i+1] == '/' && chars[i+2] == 'b' && chars[i+3] == '>' {
                if !current_part.is_empty() {
                    parts.push((current_part.clone(), is_bold));
                    current_part.clear();
                }
                is_bold = false;
                i += 4;
            } else {
                current_part.push(chars[i]);
                i += 1;
            }
        }
        if !current_part.is_empty() {
            parts.push((current_part, is_bold));
        }
        parts
    }

    pub fn show(&self, ui: &mut Ui) {
        if !self.visible {
            return;
        }
        let bar_height = size("bar.status.height");
        egui::Panel::bottom("status_bar")
            .exact_size(bar_height)
            .resizable(false)
            .show_separator_line(true)
            .show(ui, |ui| {
                status_bar_frame(ui, |ui| {
                    ui.allocate_space(egui::vec2(size("bar.status.left_slot"), size("bar.status.height")));

                    let hint_text = dtool_strings::text(&self.hint_key);
                    for (part, bold) in Self::hint_parts(&hint_text) {
                        let mut rich_text = RichText::new(part).color(color("status.hint"));
                        if bold {
                            rich_text = rich_text.strong();
                        }
                        ui.label(rich_text);
                    }

                    if self.stats_text.is_some() || self.zoom_text.is_some() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(zoom) = &self.zoom_text {
                                ui.label(RichText::new(zoom).color(color("status.hint")));
                            }
                            if let Some(stats) = &self.stats_text {
                                ui.label(RichText::new(stats).color(color("status.hint")));
                            }
                        });
                    }
                });
            });
    }
}
