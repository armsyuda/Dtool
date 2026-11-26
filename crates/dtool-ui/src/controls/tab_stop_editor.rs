use dtool_design::{button, dropdown, numeric_field, studio_row, toggle, NumericEvent, NumericSpec, NumericValue};
use dtool_strings::text;
use dtool_tools::dtool_text::para_style::{Leader, TabAlign, TabStop};

pub const MM_PER_PT: f32 = 25.4 / 72.0;
pub const PT_PER_MM: f32 = 72.0 / 25.4;
pub const DEFAULT_INTERVAL_PT: f32 = 36.0;
pub const DEFAULT_NEW_POS_MM: f32 = 30.0;

pub fn mm_to_pt(mm: f32) -> f32 {
    mm * PT_PER_MM
}

pub fn pt_to_mm(pt: f32) -> f32 {
    pt * MM_PER_PT
}

fn tab_align_index(align: &TabAlign) -> usize {
    match align {
        TabAlign::Left => 0,
        TabAlign::Center => 1,
        TabAlign::Right => 2,
        TabAlign::Decimal => 3,
    }
}

fn tab_align_from_index(index: usize) -> TabAlign {
    match index {
        1 => TabAlign::Center,
        2 => TabAlign::Right,
        3 => TabAlign::Decimal,
        _ => TabAlign::Left,
    }
}

fn leader_index(leader: &Leader) -> usize {
    match leader {
        Leader::None => 0,
        Leader::Character(_) => 1,
        Leader::Underline => 2,
        Leader::Strikeout => 3,
    }
}

fn leader_from_index(index: usize, ch: char) -> Leader {
    match index {
        1 => Leader::Character(if ch == '\0' { '.' } else { ch }),
        2 => Leader::Underline,
        3 => Leader::Strikeout,
        _ => Leader::None,
    }
}

fn leader_character(leader: &Leader) -> char {
    match leader {
        Leader::Character(ch) => *ch,
        _ => '.',
    }
}

fn align_label(align: &TabAlign) -> &'static str {
    text(match align {
        TabAlign::Left => "para.tab.align.left",
        TabAlign::Center => "para.tab.align.center",
        TabAlign::Right => "para.tab.align.right",
        TabAlign::Decimal => "para.tab.align.decimal",
    })
}

fn consume_field_enter(ui: &egui::Ui, response: &egui::Response) {
    if response.lost_focus() {
        ui.input_mut(|input| {
            let modifiers = input.modifiers;
            input.consume_key(modifiers, egui::Key::Enter);
        });
    }
}

fn length_edit(ui: &mut egui::Ui, key: &str, value: f64, default: f64) -> NumericEvent {
    ui.horizontal(|ui| {
        let caption = ui.add_sized(
            [dtool_design::size("panel.label_width").min(ui.available_width() / 3.0), ui.spacing().interact_size.y],
            egui::Label::new(text(key)).wrap().sense(egui::Sense::click()),
        );
        let spec = NumericSpec::new("").suffix("mm").width(70.0).decimals(3).enabled(ui.is_enabled());
        let id = ui.make_persistent_id(key);
        let (response, event) = numeric_field(ui, id, NumericValue::Value(value), &spec);
        let response = response.labelled_by(caption.id);
        consume_field_enter(ui, &response);
        if ui.is_enabled() && caption.double_clicked() {
            return if default == value { NumericEvent::None } else { NumericEvent::Commit(default) };
        }
        match event {
            NumericEvent::None => ui.data(|data| data.get_temp::<dtool_design::NumericState>(id))
                .filter(|state| state.scrubbing && state.previewed)
                .map_or(NumericEvent::None, |state| NumericEvent::Preview(state.scrub)),
            NumericEvent::Cancel => {
                crate::app_canvas::clear_geometry_preview(ui);
                NumericEvent::Cancel
            }
            event => event,
        }
    }).inner
}

pub fn right_aligned_mm(mm: f32) -> TabStop {
    TabStop {
        pos: mm_to_pt(mm).max(0.0),
        align: TabAlign::Right,
        decimal: '.',
        leader: Leader::None,
        from_right: false,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TabStopEditorAction {
    None,
    Changed,
    IntervalChanged,
    PreviewTabs(Vec<TabStop>),
    PreviewInterval(f32),
    Add,
    Delete,
}

pub struct TabStopEditorState {
    pub stops: Vec<TabStop>,
    pub selected: Option<usize>,
    pub default_interval_mm: f64,
    pos_mm: f64,
    align: usize,
    decimal: String,
    leader: usize,
    leader_char: String,
    from_right: bool,
}

impl Default for TabStopEditorState {
    fn default() -> Self {
        Self {
            stops: Vec::new(),
            selected: None,
            default_interval_mm: pt_to_mm(DEFAULT_INTERVAL_PT) as f64,
            pos_mm: DEFAULT_NEW_POS_MM as f64,
            align: tab_align_index(&TabAlign::Right),
            decimal: ".".to_string(),
            leader: 0,
            leader_char: ".".to_string(),
            from_right: false,
        }
    }
}

impl TabStopEditorState {
    pub fn sync_from(&mut self, stops: &[TabStop], default_interval_pt: f32) {
        if self.stops != stops {
            self.load(stops.to_vec(), default_interval_pt);
        } else {
            self.default_interval_mm = interval_mm(default_interval_pt);
        }
    }

    pub fn load(&mut self, stops: Vec<TabStop>, default_interval_pt: f32) {
        self.stops = stops;
        self.default_interval_mm = interval_mm(default_interval_pt);
        if let Some(index) = self.selected {
            if index < self.stops.len() {
                self.load_draft(index);
            } else {
                self.selected = None;
            }
        }
    }

    fn load_draft(&mut self, index: usize) {
        let stop = &self.stops[index];
        self.pos_mm = pt_to_mm(stop.pos) as f64;
        self.align = tab_align_index(&stop.align);
        self.decimal = stop.decimal.to_string();
        self.leader = leader_index(&stop.leader);
        self.leader_char = leader_character(&stop.leader).to_string();
        self.from_right = stop.from_right;
    }

    fn draft_stop(&self) -> TabStop {
        let decimal = self.decimal.chars().next().unwrap_or('.');
        let leader_ch = self.leader_char.chars().next().unwrap_or('.');
        TabStop {
            pos: mm_to_pt(self.pos_mm as f32).max(0.0),
            align: tab_align_from_index(self.align),
            decimal,
            leader: leader_from_index(self.leader, leader_ch),
            from_right: self.from_right,
        }
    }

    pub fn default_interval_pt(&self) -> f32 {
        let pt = mm_to_pt(self.default_interval_mm as f32);
        if pt.is_finite() && pt > 0.0 {
            pt
        } else {
            DEFAULT_INTERVAL_PT
        }
    }

    pub fn sorted_stops(&self) -> Vec<TabStop> {
        let mut stops = self.stops.clone();
        stops.sort_by(|a, b| a.pos.partial_cmp(&b.pos).unwrap_or(std::cmp::Ordering::Equal));
        stops
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> TabStopEditorAction {
        let mut action = TabStopEditorAction::None;
        let align_options = [TabAlign::Left, TabAlign::Center, TabAlign::Right, TabAlign::Decimal]
            .map(|align| align_label(&align));
        let leader_options = ["tabs.leader.none", "tabs.leader.character", "tabs.leader.underline", "tabs.leader.strikeout"].map(text);

        let mut select: Option<usize> = None;
        egui::ScrollArea::vertical()
            .max_height(120.0)
            .show(ui, |ui| {
                for (index, stop) in self.stops.iter().enumerate() {
                    let label = format!("{:.3} mm  {}", pt_to_mm(stop.pos), align_label(&stop.align));
                    if ui
                        .selectable_label(self.selected == Some(index), label)
                        .clicked()
                    {
                        select = Some(index);
                    }
                }
            });
        if let Some(index) = select {
            self.selected = Some(index);
            self.load_draft(index);
        }

        ui.horizontal(|ui| {
            if button(ui, "+").on_hover_text(text("para.tab.add")).clicked() {
                action = TabStopEditorAction::Add;
            }
            if ui.add_enabled_ui(self.selected.is_some(), |ui| button(ui, text("tabs.delete")).clicked()).inner {
                action = TabStopEditorAction::Delete;
            }
        });
        if self.selected.is_some() {
            match length_edit(ui, "tabs.position", self.pos_mm, f64::from(DEFAULT_NEW_POS_MM)) {
                NumericEvent::Commit(value) => {
                    self.pos_mm = value.max(0.0);
                    action = TabStopEditorAction::Changed;
                }
                NumericEvent::Preview(value) if action == TabStopEditorAction::None => {
                    if let Some(index) = self.selected.filter(|index| *index < self.stops.len()) {
                        let mut stops = self.stops.clone();
                        stops[index].pos = mm_to_pt(value.max(0.0) as f32);
                        stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
                        action = TabStopEditorAction::PreviewTabs(stops);
                    }
                }
                _ => {}
            }
            let align_changed = studio_row(ui, text("tabs.align"), |ui, label| {
                dropdown(ui, "tabs.align", &mut self.align, &align_options).labelled_by(label).changed()
            });
            let leader_changed = studio_row(ui, text("tabs.leader"), |ui, label| {
                dropdown(ui, "tabs.leader", &mut self.leader, &leader_options).labelled_by(label).changed()
            });
            let mut field_changed = false;
            if self.align == tab_align_index(&TabAlign::Decimal) {
                field_changed |= scalar_edit(ui, "tabs.decimal", &mut self.decimal);
            }
            if self.leader == 1 {
                field_changed |= scalar_edit(ui, "tabs.leader.symbol", &mut self.leader_char);
            }
            let origin_changed = toggle(ui, &mut self.from_right, text("tabs.from_right")).changed();
            if align_changed || leader_changed || field_changed || origin_changed {
                action = TabStopEditorAction::Changed;
            }
        }
        match length_edit(ui, "tabs.default_interval", self.default_interval_mm, interval_mm(DEFAULT_INTERVAL_PT)) {
            NumericEvent::Commit(value) => {
                self.default_interval_mm = if value > 0.0 { value } else { interval_mm(DEFAULT_INTERVAL_PT) };
                action = TabStopEditorAction::IntervalChanged;
            }
            NumericEvent::Preview(value) if action == TabStopEditorAction::None => {
                let pt = mm_to_pt(value as f32);
                action = TabStopEditorAction::PreviewInterval(if pt > 0.0 && pt.is_finite() { pt } else { DEFAULT_INTERVAL_PT });
            }
            _ => {}
        }

        match action {
            TabStopEditorAction::Changed => {
                if let Some(mut index) = self.selected {
                    if index < self.stops.len() {
                        self.stops[index] = self.draft_stop();
                        while index > 0 && self.stops[index - 1].pos > self.stops[index].pos {
                            self.stops.swap(index - 1, index);
                            index -= 1;
                        }
                        while index + 1 < self.stops.len() && self.stops[index + 1].pos < self.stops[index].pos {
                            self.stops.swap(index, index + 1);
                            index += 1;
                        }
                        self.selected = Some(index);
                    }
                }
            }
            TabStopEditorAction::Add => {
                let mut stop = if self.selected.is_some() {
                    self.draft_stop()
                } else {
                    right_aligned_mm(DEFAULT_NEW_POS_MM)
                };
                if let Some(last) = self.stops.last() {
                    stop.pos = last.pos + self.default_interval_pt();
                }
                self.stops.push(stop);
                let index = self.stops.len() - 1;
                self.selected = Some(index);
                self.load_draft(index);
            }
            TabStopEditorAction::Delete => {
                if let Some(index) = self.selected {
                    if index < self.stops.len() {
                        self.stops.remove(index);
                    }
                }
                self.selected = None;
                self.pos_mm = DEFAULT_NEW_POS_MM as f64;
                self.align = tab_align_index(&TabAlign::Right);
                self.decimal = ".".to_string();
                self.leader = 0;
                self.leader_char = ".".to_string();
                self.from_right = false;
            }
            TabStopEditorAction::None | TabStopEditorAction::IntervalChanged
            | TabStopEditorAction::PreviewTabs(_) | TabStopEditorAction::PreviewInterval(_) => {}
        }
        action
    }
}

fn interval_mm(value: f32) -> f64 {
    let value = if value.is_finite() && value > 0.0 { value } else { DEFAULT_INTERVAL_PT };
    f64::from(pt_to_mm(value))
}

fn scalar_edit(ui: &mut egui::Ui, key: &str, value: &mut String) -> bool {
    let changed = studio_row(ui, text(key), |ui, label| {
        let response = ui.add(egui::TextEdit::singleline(value).desired_width(50.0)).labelled_by(label);
        consume_field_enter(ui, &response);
        response.changed()
    });
    if changed {
        *value = value.chars().find(|ch| !ch.is_control()).unwrap_or('.').to_string();
    }
    changed
}
