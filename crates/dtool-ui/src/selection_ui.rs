use dtool_core::selection::PixelSelection;
use dtool_strings::text;
use dtool_design::{color, stroke_width};
use egui::{Stroke, StrokeKind};
use dtool_tools::selection_tools::SelectionTool;
use crate::Workspace;

pub struct SelectionContextBar {
    pub active_tool: Option<SelectionTool>,
    pub feather: bool,
    pub from_center: bool,
    pub antialias: bool,
    pub width: f64,
    pub height: f64,
}

impl Default for SelectionContextBar {
    fn default() -> Self {
        Self {
            active_tool: None,
            feather: false,
            from_center: false,
            antialias: true,
            width: 0.0,
            height: 0.0,
        }
    }
}

impl SelectionContextBar {
    pub fn show(&mut self, ui: &mut egui::Ui, ws: &mut Workspace) {
        if self.active_tool.is_none() {
            return;
        }

        dtool_design::context_bar_section(ui, |ui| {
            ui.horizontal(|ui| {
                let mode_keys = ["selection.mode.new", "selection.mode.add", "selection.mode.subtract", "selection.mode.intersect"];
                for key in mode_keys {
                    if dtool_design::button(ui, text(key)).clicked() {
                        ws.run_named("selection.set_op", serde_json::json!({}));
                    }
                }

                ui.separator();

                if dtool_design::toggle(ui, &mut self.feather, text("selection.feather")).clicked() {
                    ws.run_named("selection.modify", serde_json::json!({"operation": "feather"}));
                }

                if dtool_design::toggle(ui, &mut self.from_center, text("selection.from_center")).clicked() {
                    ws.run_named("selection.set_option", serde_json::json!({"from_center": self.from_center}));
                }

                if dtool_design::toggle(ui, &mut self.antialias, text("selection.antialias")).clicked() {
                    ws.run_named("selection.set_option", serde_json::json!({"antialias": self.antialias}));
                }

                ui.separator();

                let row_or_column = self.active_tool == Some(SelectionTool::Row)
                    || self.active_tool == Some(SelectionTool::Column);
                ui.label(text("selection.width"));
                if dtool_design::number_field(ui, &mut self.width, "").changed() {
                    let _ = ws.run_named(
                        "selection.set_option",
                        serde_json::json!({"width": self.width}),
                    );
                }
                if !row_or_column {
                    ui.label(text("selection.height"));
                    if dtool_design::number_field(ui, &mut self.height, "").changed() {
                        let _ = ws.run_named(
                            "selection.set_option",
                            serde_json::json!({"height": self.height}),
                        );
                    }
                }

                if dtool_design::button(ui, text("selection.refine_button")).clicked() {
                    ws.run_named("selection.refine", serde_json::json!({}));
                }
            });
        });
    }
}

pub fn draw_marching_ants(ui: &mut egui::Ui, selection: &PixelSelection) {
    if selection.is_empty() {
        return;
    }

    let bounds = match selection.bounds() {
        Some(b) => b,
        None => return,
    };

    let rect = egui::Rect::from_min_max(
        egui::pos2(bounds[0] as f32, bounds[1] as f32),
        egui::pos2((bounds[0] + bounds[2]) as f32, (bounds[1] + bounds[3]) as f32),
    );

    let stroke = Stroke {
        width: stroke_width("selection.ants"),
        color: color("accent"),
    };
    ui.painter().rect_stroke(rect, 0.0, stroke, StrokeKind::Middle);
}

pub struct QuickMaskToolbar {
    pub active: bool,
}

impl Default for QuickMaskToolbar {
    fn default() -> Self {
        Self { active: false }
    }
}

impl QuickMaskToolbar {
    pub fn show(&mut self, ui: &mut egui::Ui, ws: &mut Workspace) {
        if dtool_design::toggle(ui, &mut self.active, text("menu.selection.quick_mask")).clicked() {
            let _ = ws.run_named(
                "selection.quickmask.toggle",
                serde_json::json!({ "on": self.active }),
            );
        }
    }
}

pub struct QuickMaskMenu {
    pub show_overlay: bool,
    pub show_white: bool,
}

impl Default for QuickMaskMenu {
    fn default() -> Self {
        Self {
            show_overlay: true,
            show_white: false,
        }
    }
}

impl QuickMaskMenu {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let mut overlay = self.show_overlay;
        if dtool_design::toggle(ui, &mut overlay, text("quickmask.show_overlay")).clicked() {
            self.show_overlay = true;
            self.show_white = false;
        }
        let mut white = self.show_white;
        if dtool_design::toggle(ui, &mut white, text("quickmask.show_white")).clicked() {
            self.show_overlay = false;
            self.show_white = true;
        }
    }
}

pub fn add_to_curves_menu(items:&mut [crate::menu_tree::MenuItem]) {
    for item in items {
        if item.id.0=="layer.select" && !item.children.iter().any(|i|i.command==Some("selection.toCurves")) {
            item.children.push(crate::menu_tree::MenuItem {id:crate::menu_tree::MenuId("layer.select.to_curves"),key:"menu.select.to_curves",command:Some("selection.toCurves"),children:Vec::new(),separator:false,planned:false,checkable:false});
        }
        if item.id.0=="layer.select" { add_range_menus(&mut item.children); }
        add_to_curves_menu(&mut item.children);
    }
}

pub struct SelectionSaveLoadUi {
    pub save_path: String,
    pub load_path: String,
    pub file_dialog: bool,
    pub load_dialog: bool,
}

impl Default for SelectionSaveLoadUi {
    fn default() -> Self {
        Self {
            save_path: String::new(),
            load_path: String::new(),
            file_dialog: false,
            load_dialog: false,
        }
    }
}

impl SelectionSaveLoadUi {
    pub fn show(&mut self, ui: &mut egui::Ui, ws: &mut Workspace) {
        if dtool_design::button(ui, text("menu.selection.save_channel")).clicked() {
            let _ = ws.run_named("selection.save", serde_json::json!({ "to": "channel" }));
        }
        if dtool_design::button(ui, text("menu.selection.save_file")).clicked() {
            self.file_dialog = true;
        }
        if dtool_design::button(ui, text("menu.selection.load_file")).clicked() {
            self.load_dialog = true;
        }
        if self.file_dialog {
            dtool_design::dialog_frame(ui, |ui| {
                ui.label(text("menu.selection.save_file"));
                ui.text_edit_singleline(&mut self.save_path);
                ui.horizontal(|ui| {
                    if dtool_design::button(ui, text("button.apply")).clicked() {
                        let _ = ws.run_named(
                            "selection.save",
                            serde_json::json!({ "to": "file", "path": self.save_path }),
                        );
                        self.file_dialog = false;
                    }
                    if dtool_design::button(ui, text("button.cancel")).clicked() {
                        self.file_dialog = false;
                    }
                });
            });
        }
        if self.load_dialog {
            dtool_design::dialog_frame(ui, |ui| {
                ui.label(text("menu.selection.load_file"));
                ui.text_edit_singleline(&mut self.load_path);
                ui.horizontal(|ui| {
                    if dtool_design::button(ui, text("button.apply")).clicked() {
                        let _ = ws.run_named(
                            "selection.load",
                            serde_json::json!({ "from": "file", "path": self.load_path }),
                        );
                        self.load_dialog = false;
                    }
                    if dtool_design::button(ui, text("button.cancel")).clicked() {
                        self.load_dialog = false;
                    }
                });
            });
        }
        ui.menu_button(text("menu.selection.load_file"), |ui| {
            let channels = ws
                .core
                .session()
                .and_then(|session| session.document())
                .map(|doc| doc.spare_channels.clone())
                .unwrap_or_default();
            if channels.is_empty() {
                dtool_design::button_with(ui, text("menu.selection.load_file"), &dtool_design::ControlState::enabled(false));
            } else {
                for (index, (name, _)) in channels.iter().enumerate() {
                    let label = if name.is_empty() {
                        dtool_strings::format("selection.channel_name", &[("n", &index.to_string())])
                    } else {
                        name.clone()
                    };
                    if dtool_design::menu_item(ui, label).clicked() {
                        let _ = ws.run_named(
                            "selection.load",
                            serde_json::json!({ "from": "channel", "name": name }),
                        );
                    }
                }
            }
        });
    }
}

fn add_range_menus(items:&mut Vec<crate::menu_tree::MenuItem>) {
    use crate::menu_tree::{MenuItem,MenuId};
    if !items.iter().any(|item|item.id.0=="layer.select.colour_range") {
        items.push(MenuItem {id:MenuId("layer.select.colour_range"),key:"menu.select.colour_range",command:None,separator:false,planned:false,checkable:false,children:vec![
            MenuItem {id:MenuId("layer.select.colour.reds"),key:"menu.select.reds",command:Some("selection.hueRange.reds"),children:Vec::new(),separator:false,planned:false,checkable:false},
            MenuItem {id:MenuId("layer.select.colour.yellows"),key:"menu.select.yellows",command:Some("selection.hueRange.yellows"),children:Vec::new(),separator:false,planned:false,checkable:false},
            MenuItem {id:MenuId("layer.select.colour.greens"),key:"menu.select.greens",command:Some("selection.hueRange.greens"),children:Vec::new(),separator:false,planned:false,checkable:false},
            MenuItem {id:MenuId("layer.select.colour.cyans"),key:"menu.select.cyans",command:Some("selection.hueRange.cyans"),children:Vec::new(),separator:false,planned:false,checkable:false},
            MenuItem {id:MenuId("layer.select.colour.blues"),key:"menu.select.blues",command:Some("selection.hueRange.blues"),children:Vec::new(),separator:false,planned:false,checkable:false},
            MenuItem {id:MenuId("layer.select.colour.magentas"),key:"menu.select.magentas",command:Some("selection.hueRange.magentas"),children:Vec::new(),separator:false,planned:false,checkable:false},
        ]});
    }
    if !items.iter().any(|item|item.id.0=="layer.select.tonal_range") {
        items.push(MenuItem {id:MenuId("layer.select.tonal_range"),key:"menu.select.tonal_range",command:None,separator:false,planned:false,checkable:false,children:vec![
            MenuItem {id:MenuId("layer.select.tonal.highlights"),key:"menu.select.highlights",command:Some("selection.tonalRange.highlights"),children:Vec::new(),separator:false,planned:false,checkable:false},
            MenuItem {id:MenuId("layer.select.tonal.midtones"),key:"menu.select.midtones",command:Some("selection.tonalRange.midtones"),children:Vec::new(),separator:false,planned:false,checkable:false},
            MenuItem {id:MenuId("layer.select.tonal.shadows"),key:"menu.select.shadows",command:Some("selection.tonalRange.shadows"),children:Vec::new(),separator:false,planned:false,checkable:false},
        ]});
    }
    if !items.iter().any(|item|item.id.0=="layer.select.alpha_range") {
        items.push(MenuItem {id:MenuId("layer.select.alpha_range"),key:"menu.select.alpha_range",command:None,separator:false,planned:false,checkable:false,children:vec![
            MenuItem {id:MenuId("layer.select.alpha.transparent"),key:"menu.select.transparent",command:Some("selection.alphaRange.transparent"),children:Vec::new(),separator:false,planned:false,checkable:false},
            MenuItem {id:MenuId("layer.select.alpha.partial"),key:"menu.select.partial",command:Some("selection.alphaRange.partial"),children:Vec::new(),separator:false,planned:false,checkable:false},
            MenuItem {id:MenuId("layer.select.alpha.opaque"),key:"menu.select.opaque",command:Some("selection.alphaRange.opaque"),children:Vec::new(),separator:false,planned:false,checkable:false},
        ]});
    }
}
