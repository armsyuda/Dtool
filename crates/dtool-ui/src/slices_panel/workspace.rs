use std::collections::BTreeSet;

use dtool_core::slice::{
    INCLUDE_CONTAINER, OutputId, OutputSize, Slice, SliceOutput, SliceSource, SliceTarget,
};
use dtool_core::workspace::DocContext;
use dtool_core::{Object, ObjectId, Session, artboard};
use dtool_design::{
    ControlState, NumericSpec, button, button_with, icon_button, numeric_value,
    panel_frame, panel_header, tab, toggle,
};
use dtool_strings::text;
use serde_json::json;

use super::{OverwriteDecision, SlicesPanel};

const FORMATS: [(&str, &str, [u8; 3]); 5] = [
    ("png", "PNG", [99, 198, 221]),
    ("jpeg", "JPG", [239, 174, 94]),
    ("pdf", "PDF", [250, 131, 133]),
    ("svg", "SVG", [183, 154, 243]),
    ("webp", "WebP", [132, 211, 163]),
];
const SCALES: [f32; 7] = [0.2, 0.5, 1.5, 2.0, 3.0, 5.0, 10.0];

pub(super) struct OutputDraft {
    format: usize,
    size_kind: usize,
    scale: f32,
    pixels: f64,
    include_container: bool,
    quality: f64,
    progressive: bool,
    lossless: bool,
    alpha: bool,
    pdf_colour: usize,
    pdf_dpi: f64,
    outline_text: bool,
    outline_strokes: bool,
    svg_outline_text: bool,
    svg_dpi: f64,
}

impl Default for OutputDraft {
    fn default() -> Self {
        Self {
            format: 0,
            size_kind: 0,
            scale: 1.0,
            pixels: 1024.0,
            include_container: false,
            quality: 90.0,
            progressive: false,
            lossless: false,
            alpha: true,
            pdf_colour: 0,
            pdf_dpi: 300.0,
            outline_text: true,
            outline_strokes: false,
            svg_outline_text: false,
            svg_dpi: 72.0,
        }
    }
}

impl OutputDraft {
    fn size(&self) -> OutputSize {
        match self.size_kind {
            1 => OutputSize::Width(self.pixels.round() as u32),
            2 => OutputSize::Height(self.pixels.round() as u32),
            _ => OutputSize::Scale(self.scale),
        }
    }

    fn output(&self, id: OutputId, is_board: bool) -> SliceOutput {
        let (format, _, _) = FORMATS[self.format];
        let mut options = match format {
            "png" => json!({"transparent": self.alpha}),
            "jpeg" => {
                json!({"quality": self.quality.round() as u8, "progressive": self.progressive})
            }
            "webp" => {
                json!({"quality": self.quality.round() as u8, "lossless": self.lossless, "transparent": self.alpha})
            }
            "pdf" => json!({
                "pdf_colour_space": (["CMYK", "RGB"][self.pdf_colour]),
                "pdf_raster_dpi": self.pdf_dpi,
                "pdf_outline_text": self.outline_text,
                "pdf_outline_strokes": self.outline_strokes,
            }),
            "svg" => json!({
                "svg": {"text_as_curves": self.svg_outline_text, "raster_dpi": self.svg_dpi.round() as u32},
            }),
            _ => json!({}),
        };
        options[INCLUDE_CONTAINER] = json!(!is_board && self.include_container);
        SliceOutput {
            id,
            enabled: true,
            format: format.into(),
            size: self.size(),
            suffix: String::new(),
            path: None,
            options: Some(options),
        }
    }

    fn show(&mut self, ui: &mut egui::Ui, is_board: bool) {
        ui.horizontal_wrapped(|ui| {
            for (index, (_, label, [r, g, b])) in FORMATS.iter().copied().enumerate() {
                let label = egui::RichText::new(label).color(egui::Color32::from_rgb(r, g, b));
                if tab(ui, self.format == index, label).clicked() {
                    self.format = index;
                }
            }
        });
        use dtool_core::effect_raster::{MAX_DPI, MIN_DPI};
        let dpi_range = f64::from(MIN_DPI)..=f64::from(MAX_DPI);
        match FORMATS[self.format].0 {
            "png" => {
                toggle(ui, &mut self.alpha, text("slice.workspace.alpha"));
            }
            "jpeg" | "webp" => {
                if self.format == 1 {
                    toggle(
                        ui,
                        &mut self.progressive,
                        text("slice.workspace.progressive"),
                    );
                } else {
                    toggle(ui, &mut self.alpha, text("slice.workspace.alpha"));
                    toggle(ui, &mut self.lossless, text("slice.workspace.lossless"));
                }
                if self.format == 1 || !self.lossless {
                    number(
                        ui,
                        "slice.workspace.quality",
                        &mut self.quality,
                        1.0..=100.0,
                        "%",
                    );
                }
            }
            "pdf" => {
                ui.label(text("slice.workspace.colour"));
                ui.horizontal(|ui| {
                    for (index, label) in ["CMYK", "RGB"].into_iter().enumerate() {
                        if tab(ui, self.pdf_colour == index, label).clicked() {
                            self.pdf_colour = index;
                        }
                    }
                });
                number(
                    ui,
                    "slice.workspace.dpi",
                    &mut self.pdf_dpi,
                    dpi_range,
                    " DPI",
                );
                toggle(
                    ui,
                    &mut self.outline_text,
                    text("slice.workspace.outline_text"),
                );
                toggle(
                    ui,
                    &mut self.outline_strokes,
                    text("slice.workspace.outline_strokes"),
                );
            }
            "svg" => {
                number(
                    ui,
                    "slice.workspace.dpi",
                    &mut self.svg_dpi,
                    dpi_range,
                    " DPI",
                );
                toggle(
                    ui,
                    &mut self.svg_outline_text,
                    text("slice.workspace.outline_text"),
                );
            }
            _ => {}
        }

        ui.separator();
        ui.strong(text("slice.workspace.bounds"));
        if is_board {
            ui.label(text("slice.workspace.board_bounds"));
        } else {
            ui.horizontal_wrapped(|ui| {
                for (include, label) in [(false, "slice.workspace.trim"), (true, "slice.workspace.include")] {
                    if tab(ui, self.include_container == include, text(label)).clicked() {
                        self.include_container = include;
                    }
                }
            });
        }
        ui.separator();
        ui.strong(text("slice.workspace.size"));
        ui.horizontal_wrapped(|ui| {
            for scale in SCALES {
                if tab(
                    ui,
                    self.size_kind == 0 && self.scale == scale,
                    format!("{scale}×"),
                )
                .clicked()
                {
                    self.size_kind = 0;
                    self.scale = scale;
                }
            }
        });
        let kinds = [
            text("slice.size.scale"),
            text("slice.workspace.width"),
            text("slice.workspace.height"),
        ];
        ui.horizontal_wrapped(|ui| {
            for (index, label) in kinds.into_iter().enumerate() {
                if tab(ui, self.size_kind == index, label).clicked() {
                    self.size_kind = index;
                }
            }
        });
        if self.size_kind == 0 {
            let mut scale = f64::from(self.scale);
            let spec = NumericSpec::new("")
                .range(0.01..=1024.0)
                .decimals(2)
                .step(0.1)
                .suffix("×");
            let id = ui.make_persistent_id("slice.workspace.scale");
            if numeric_value(ui, id, &mut scale, &spec).changed() {
                self.scale = scale as f32;
            }
        } else {
            let max = f64::from(dtool_export::raster::MAX_EXPORT_SIDE);
            number(
                ui,
                kinds_key(self.size_kind),
                &mut self.pixels,
                1.0..=max,
                " px",
            );
        }
    }
}

fn kinds_key(kind: usize) -> &'static str {
    if kind == 1 {
        "slice.workspace.width"
    } else {
        "slice.workspace.height"
    }
}

fn number(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    unit: &str,
) {
    let spec = NumericSpec::new(text(label))
        .range(range)
        .decimals(0)
        .suffix(unit);
    let id = ui.make_persistent_id(label);
    numeric_value(ui, id, value, &spec);
}

fn layer_name(object: &Object) -> String {
    if !object.name.trim().is_empty() {
        return object.name.clone();
    }
    let key = if artboard::is_artboard(object) {
        "slice.workspace.artboard"
    } else if object.is_group() {
        "slice.workspace.group"
    } else {
        "slice.workspace.layer"
    };
    format!("{} {}", text(key), object.id.0)
}

pub(crate) fn select_source(
    session: &mut Session,
    id: Option<ObjectId>,
    modifiers: egui::Modifiers,
) {
    let selected = id.is_some_and(|id| session.selection.ids.contains(&id));
    let op = crate::panels::layers::select_op(selected, modifiers);
    let ids: Vec<_> = id.into_iter().collect();
    let _ = session.run_named("selection.set", json!({"ids": ids, "op": op}));
}

enum QueueAction {
    Pick,
    Remove,
    ClearOutputs,
    RemoveOutput(OutputId),
}

impl SlicesPanel {
    pub(crate) fn show_source_tree(&mut self, ui: &mut egui::Ui, context: &mut DocContext) {
        panel_header(ui, text("slice.workspace.layers"));
        let session = &mut context.session;
        let enabled = !session.selection.ids.is_empty();
        if button_with(
            ui,
            text("slice.workspace.add"),
            &ControlState::enabled(enabled),
        )
        .clicked()
        {
            let selected = session.selection.ids.clone();
            for id in selected {
                if let Some(object) = session.document.get_object(id) {
                    let params = json!({"source": id, "name": layer_name(object), "layer": true});
                    if !self.command(session, "slice.create", params) {
                        break;
                    }
                }
            }
        }
        ui.separator();
        let id = ui.make_persistent_id(("slice.tree", context.id.0));
        let mut collapsed = ui
            .data(|data| data.get_temp::<BTreeSet<u64>>(id))
            .unwrap_or_default();
        let rows = crate::panels::layers::layer_rows(
            &session.document,
            &session.selection.ids,
            &collapsed,
        );
        let mut picked = None;
        egui::ScrollArea::vertical()
            .id_salt("slice.source")
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                for row in rows {
                    let label = crate::panels::layers::row_label(&row);
                    ui.push_id(row.id, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(row.depth.min(7) as f32 * 9.0);
                            if row.has_children {
                                let closed = collapsed.contains(&row.id);
                                let icon = if closed { "\u{25b8}" } else { "\u{25be}" };
                                if icon_button(ui, icon).clicked() && !collapsed.remove(&row.id) {
                                    collapsed.insert(row.id);
                                }
                            } else {
                                ui.add_space(dtool_design::size("icon.size.md"));
                            }
                            if tab(ui, row.selected, &label)
                                .on_hover_text(&label)
                                .clicked()
                            {
                                picked = Some(ObjectId(row.id));
                            }
                        });
                    });
                }
            });
        ui.data_mut(|data| data.insert_temp(id, collapsed));
        if let Some(id) = picked {
            select_source(session, Some(id), ui.input(|input| input.modifiers));
        }
    }

    pub(crate) fn show_workspace(&mut self, ui: &mut egui::Ui, context: &mut DocContext) {
        let has_status = self.job.is_some() || self.receipt.is_some() || self.error.is_some();
        let footer_height = if has_status { 68.0 } else { 32.0 };
        let available = ui.available_size();
        let body_height = (available.y - footer_height - ui.spacing().item_spacing.y).max(0.0);
        let (body, _) = ui.allocate_exact_size(egui::vec2(available.x, body_height), egui::Sense::hover());
        let mut body_ui = ui.new_child(
            egui::UiBuilder::new().id_salt("slice.workspace.body").max_rect(body)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        body_ui.set_clip_rect(ui.clip_rect().intersect(body));
        self.show_workspace_body(&mut body_ui, context);
        ui.separator();
        self.show_export_footer(ui, context);
    }

    fn show_workspace_body(&mut self, ui: &mut egui::Ui, context: &mut DocContext) {
        if ui.available_width() < 320.0 {
            let id = ui.make_persistent_id("slice.workspace.compact_tab");
            let mut options = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
            ui.horizontal_wrapped(|ui| {
                if tab(ui, !options, text("slice.workspace.queue")).clicked() {
                    options = false;
                }
                if tab(ui, options, text("slice.workspace.options")).clicked() {
                    options = true;
                }
            });
            ui.data_mut(|data| data.insert_temp(id, options));
            self.show_workspace_column(ui, &mut context.session, options, false);
        } else {
            ui.columns(2, |columns| {
                self.show_workspace_column(&mut columns[0], &mut context.session, false, true);
                self.show_workspace_column(&mut columns[1], &mut context.session, true, true);
            });
        }
    }

    fn show_workspace_column(&mut self, ui: &mut egui::Ui, session: &mut Session, options: bool, heading: bool) {
        panel_frame(ui, |ui| {
            if heading {
                let label = if options { "slice.workspace.options" } else { "slice.workspace.queue" };
                panel_header(ui, text(label));
                ui.separator();
            }
            egui::ScrollArea::vertical()
                .id_salt(("slice.workspace.column", options))
                .max_height(ui.available_height().max(1.0))
                .show(ui, |ui| {
                    if options {
                        self.show_options(ui, session);
                    } else {
                        self.show_queue(ui, session);
                    }
                });
        });
    }

    fn show_export_footer(&mut self, ui: &mut egui::Ui, context: &DocContext) {
        let enabled = self.job.is_none()
            && context.session.document.slices.iter().any(|slice| {
                matches!(slice.source, SliceSource::Layer(_))
                    && slice.enabled
                    && slice.outputs.iter().any(|output| output.enabled)
            });
        let clicked = ui
            .allocate_ui_with_layout(
                egui::vec2(ui.available_width(), dtool_design::size("button.height")),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    button_with(ui, text("slice.workspace.export"), &ControlState::enabled(enabled)).clicked()
                },
            )
            .inner;
        if clicked {
            self.start_export(ui.ctx(), context, SliceTarget::LayerQueue);
        }
        if self.job.is_some() {
            ui.label(text("slice.export_running"));
        }
        if let Some(receipt) = &self.receipt {
            let counts = [
                receipt.written().len(),
                receipt.skipped().len(),
                receipt.failed().len(),
            ]
            .map(|n| n.to_string());
            let response = ui.label(dtool_strings::format(
                "slice.export_summary",
                &[
                    ("written", &counts[0]),
                    ("skipped", &counts[1]),
                    ("failed", &counts[2]),
                ],
            ));
            let paths = receipt
                .failed()
                .into_iter()
                .chain(receipt.skipped())
                .map(|result| result.path.display().to_string())
                .collect::<Vec<_>>();
            if !paths.is_empty() {
                response.on_hover_text(paths.join("\n"));
            }
            if receipt.cancelled {
                ui.label(text("slice.workspace.cancelled"));
            }
        }
        if let Some(error) = self.error.as_deref() {
            ui.label(text(error));
        }
    }

    pub(crate) fn update_export(&mut self, ctx: &egui::Context) {
        self.poll_export();
        if self.overwrite.is_none() {
            self.overwrite = self
                .overwrite_rx
                .as_ref()
                .and_then(|receiver| receiver.try_recv().ok());
        }
        let Some(request) = &self.overwrite else {
            return;
        };
        let (result, action) = crate::dialog_chrome::show_modal(
            ctx,
            "slice.workspace.overwrite",
            text("slice.workspace.overwrite"),
            crate::dialog_chrome::ClosePolicy::CancelOnly,
            false,
            |ui| {
                ui.label(text("slice.workspace.overwrite_prompt"));
                ui.label(request.path.display().to_string());
                ui.horizontal_wrapped(|ui| {
                    [
                        ("slice.workspace.replace", OverwriteDecision::Overwrite),
                        ("slice.workspace.skip", OverwriteDecision::Skip),
                        ("button.cancel", OverwriteDecision::Cancel),
                    ]
                    .into_iter()
                    .find_map(|(key, action)| button(ui, text(key)).clicked().then_some(action))
                })
                .inner
            },
        );
        let decision = action.or_else(|| {
            (result == crate::dialog_chrome::DialogResult::Cancel)
                .then_some(OverwriteDecision::Cancel)
        });
        if let Some(decision) = decision
            && let Some(request) = self.overwrite.take()
        {
            let _ = request.reply.send(decision);
        }
    }

    fn command(&mut self, session: &mut Session, name: &str, params: serde_json::Value) -> bool {
        match session.run_named(name, params) {
            Ok(_) => {
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(error.message_key);
                false
            }
        }
    }

    fn show_queue(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        let mut action = None;
        let mut entries = session
            .document
            .slices
            .iter()
            .filter(|slice| matches!(slice.source, SliceSource::Layer(_)))
            .peekable();
        if entries.peek().is_none() {
            ui.label(text("slice.workspace.queue_empty"));
        }
        for slice in entries {
            ui.push_id(slice.id.0, |ui| {
                let SliceSource::Layer(id) = slice.source else {
                    return;
                };
                let label = session
                    .document
                    .get_object(id)
                    .map_or_else(|| slice.name.clone(), layer_name);
                ui.horizontal_wrapped(|ui| {
                    if tab(ui, session.active_slice == Some(slice.id), &label)
                        .on_hover_text(&label)
                        .clicked()
                    {
                        action = Some((slice.id, QueueAction::Pick));
                    }
                    if icon_button(ui, "\u{00d7}")
                        .on_hover_text(text("slice.workspace.remove_layer"))
                        .clicked()
                    {
                        action = Some((slice.id, QueueAction::Remove));
                    }
                });
                if session.document.get_object(id).is_none() {
                    ui.label(text("slice.workspace.missing_layer"));
                }
                for output in &slice.outputs {
                    ui.push_id(output.id.0, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            show_output(ui, output);
                            if icon_button(ui, "\u{00d7}")
                                .on_hover_text(text("slice.delete_output"))
                                .clicked()
                            {
                                action = Some((slice.id, QueueAction::RemoveOutput(output.id)));
                            }
                        });
                    });
                }
                if !slice.outputs.is_empty() && button(ui, text("slice.workspace.clear")).clicked()
                {
                    action = Some((slice.id, QueueAction::ClearOutputs));
                }
                ui.separator();
            });
        }
        if let Some((id, action)) = action {
            match action {
                QueueAction::Pick => {
                    self.command(session, "slice.select", json!({"id": id}));
                }
                QueueAction::Remove => {
                    self.command(session, "slice.delete", json!({"id": id}));
                }
                QueueAction::ClearOutputs => {
                    self.command(session, "slice.update", json!({"id": id, "outputs": []}));
                }
                QueueAction::RemoveOutput(output) => {
                    if let Some(slice) = session.document.slices.iter().find(|slice| slice.id == id)
                    {
                        let outputs: Vec<_> = slice
                            .outputs
                            .iter()
                            .filter(|row| row.id != output)
                            .collect();
                        self.command(
                            session,
                            "slice.update",
                            json!({"id": id, "outputs": outputs}),
                        );
                    }
                }
            }
        }
    }

    fn show_options(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        let active = session
            .document
            .slices
            .iter()
            .find(|slice| Some(slice.id) == session.active_slice);
        let Some(Slice {
            id,
            source: SliceSource::Layer(object_id),
            outputs,
            ..
        }) = active
        else {
            ui.label(text("slice.workspace.pick_layer"));
            return;
        };
        let Some(object) = session.document.get_object(*object_id) else {
            ui.label(text("slice.workspace.missing_layer"));
            return;
        };
        let is_board = artboard::is_artboard(object);
        ui.strong(layer_name(object));
        self.draft.show(ui, is_board);
        if button(ui, text("slice.workspace.save")).clicked() {
            let next = session
                .document
                .slices
                .iter()
                .flat_map(|slice| &slice.outputs)
                .map(|output| output.id.0)
                .max()
                .unwrap_or(0)
                .checked_add(1);
            if let Some(next) = next {
                let mut outputs = outputs.clone();
                outputs.push(self.draft.output(OutputId(next), is_board));
                self.command(
                    session,
                    "slice.update",
                    json!({"id": id, "outputs": outputs}),
                );
            }
        }
    }
}

fn show_output(ui: &mut egui::Ui, output: &SliceOutput) {
    let (_, label, [r, g, b]) = FORMATS
        .iter()
        .find(|(format, _, _)| *format == output.format)
        .copied()
        .unwrap_or((&output.format, &output.format, [172, 172, 172]));
    let size = match output.size {
        OutputSize::Scale(scale) => format!("{scale}\u{00d7}"),
        OutputSize::Width(px) => format!("{} {px}px", text("slice.size.width")),
        OutputSize::Height(px) => format!("{} {px}px", text("slice.size.height")),
        OutputSize::Square(px) => format!("{px}\u{00d7}{px}px"),
    };
    let colour = egui::Color32::from_rgb(r, g, b);
    ui.label(
        egui::RichText::new(format!(" {label} "))
            .strong()
            .color(egui::Color32::from_gray(25))
            .background_color(colour),
    );
    ui.label(size);
    if let Some(options) = &output.options {
        if let Some(dpi) = options["pdf_raster_dpi"]
            .as_f64()
            .or_else(|| options["svg"]["raster_dpi"].as_f64())
        {
            ui.label(format!("{dpi} DPI"));
        }
        if let Some(colour) = options["pdf_colour_space"].as_str() {
            ui.label(colour);
        }
        if let Some(quality) = options["quality"]
            .as_u64()
            .filter(|_| options["lossless"].as_bool() != Some(true))
        {
            ui.label(format!("{quality}%"));
        }
        for (option, label) in [
            ("lossless", "slice.workspace.lossless"),
            ("progressive", "slice.workspace.progressive"),
            (INCLUDE_CONTAINER, "slice.workspace.include"),
        ] {
            if options[option].as_bool() == Some(true) {
                ui.label(text(label));
            }
        }
    }
}
