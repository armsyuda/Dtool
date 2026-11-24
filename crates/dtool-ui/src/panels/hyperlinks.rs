use dtool_core::longdoc::anchors::{Hyperlink, LinkTarget};
use dtool_core::workspace::Workspace as CoreWorkspace;
use dtool_core::Document;
use dtool_design::{button, icon_button, panel_frame, panel_header};
use dtool_strings::text;
use std::cell::RefCell;

pub struct HyperlinksPanelState {
    pub open: bool,
    pub selected_index: Option<usize>,
}

impl Default for HyperlinksPanelState {
    fn default() -> Self {
        Self {
            open: true,
            selected_index: None,
        }
    }
}

pub fn target_label(target: &LinkTarget) -> (&'static str, String) {
    match target {
        LinkTarget::Url(s) => ("hyperlink.url", s.clone()),
        LinkTarget::Anchor(s) => ("hyperlink.anchor", s.clone()),
        LinkTarget::Page(p) => ("hyperlink.page", p.to_string()),
        LinkTarget::Email(s) => ("hyperlink.email", s.clone()),
    }
}

pub fn show_hyperlinks_panel(
    ui: &mut egui::Ui,
    doc: &Document,
    state: &mut HyperlinksPanelState,
    on_go_to: &dyn Fn(&LinkTarget),
    on_edit: &dyn Fn(usize, &Hyperlink),
    on_insert: &dyn Fn(),
) {
    if !state.open {
        return;
    }
    panel_frame(ui, |ui| {
        panel_header(ui, text("panel.hyperlinks.title"));
        let hyperlinks = doc.hyperlinks();
        if hyperlinks.is_empty() {
            ui.label(text("hyperlink.empty"));
        } else {
            for (i, link) in hyperlinks.iter().enumerate() {
                ui.horizontal(|ui| {
                    let (key, target) = target_label(&link.target);
                    if ui.selectable_label(state.selected_index == Some(i), format!("{}: {}", text(key), target)).clicked() {
                        state.selected_index = Some(i);
                    }
                    if icon_button(ui, text("hyperlink.goto")).clicked() {
                        on_go_to(&link.target);
                    }
                    if button(ui, text("hyperlink.edit")).clicked() {
                        state.selected_index = Some(i);
                        on_edit(i, link);
                    }
                });
            }
        }
        ui.horizontal(|ui| {
            if button(ui, text("hyperlink.insert")).clicked() {
                on_insert();
            }
        });
    });
}

#[derive(Default)]
pub struct HyperlinksPanel {
    pub state: HyperlinksPanelState,
}

pub enum HyperlinksRequest {
    Edit(usize, Hyperlink),
    Insert,
}

impl HyperlinksPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) -> Option<HyperlinksRequest> {
        let request: RefCell<Option<HyperlinksRequest>> = RefCell::new(None);
        let doc = workspace.session().and_then(|s| s.document())?;
        show_hyperlinks_panel(
            ui,
            doc,
            &mut self.state,
            &|_target| {},
            &|index, link| *request.borrow_mut() = Some(HyperlinksRequest::Edit(index, link.clone())),
            &|| *request.borrow_mut() = Some(HyperlinksRequest::Insert),
        );
        request.into_inner()
    }
}
