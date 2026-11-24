use dtool_core::boolean::{BoolOp, COMPOUND_EDIT};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_core::{Command, ObjectId, ObjectKind};
use dtool_strings::text;

use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};

struct Operand {
    id: ObjectId,
    label: String,
    pos: [f64; 2],
    locked: bool,
}

pub struct CompoundEdit {
    pub owner: ContextId,
    id: ObjectId,
    operation: BoolOp,
    operands: Vec<Operand>,
}

impl CompoundEdit {
    pub fn open(workspace: &Workspace) -> Option<Self> {
        let owner = workspace.active()?;
        let session = workspace.session()?;
        let [id] = session.selection.ids.as_slice() else { return None };
        let object = session.document.get_object(*id)?;
        let ObjectKind::Compound(spec) = &object.kind else { return None };
        let operands = spec.operands.iter().map(|operand| Operand {
            id: operand.id,
            label: crate::panels::layers::display_name(operand.id.0, &operand.name),
            pos: operand.pos.map(f64::from),
            locked: operand.locked,
        }).collect();
        Some(Self { owner, id: *id, operation: spec.op, operands })
    }

    pub fn show(&mut self, ctx: &egui::Context) -> (bool, Option<Command>) {
        let (result, (apply, cancel)) = show_modal(ctx, "dtool.dialog.compound", text("menu.vector.edit_compound"),
            ClosePolicy::CancelOnly, false, |ui| {
                ui.set_min_width(295.0);
                egui::ComboBox::from_label(text("compound.operation"))
                    .selected_text(operation_label(self.operation))
                    .show_ui(ui, |ui| {
                        for op in [BoolOp::Add, BoolOp::Subtract, BoolOp::Intersect, BoolOp::Xor] {
                            ui.selectable_value(&mut self.operation, op, operation_label(op));
                        }
                    });
                ui.separator();
                ui.label(text("compound.operand"));
                egui::ScrollArea::vertical().max_height(330.0).show(ui, |ui| {
                    for operand in &mut self.operands {
                        ui.push_id(operand.id.0, |ui| {
                            ui.label(&operand.label);
                            ui.horizontal(|ui| {
                                ui.add_enabled_ui(!operand.locked, |ui| {
                                    dtool_design::number_field(ui, &mut operand.pos[0], "X");
                                    dtool_design::number_field(ui, &mut operand.pos[1], "Y");
                                });
                            });
                        });
                    }
                });
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let apply = dtool_design::button(ui, text("dialog.ok")).clicked();
                    let cancel = dtool_design::button(ui, text("dialog.cancel")).clicked();
                    (apply, cancel)
                }).inner
            });
        let command = apply.then(|| {
            let operands: Vec<_> = self.operands.iter().filter(|operand| !operand.locked)
                .map(|operand| serde_json::json!({ "id": operand.id, "pos": operand.pos })).collect();
            Command::with_params(COMPOUND_EDIT, serde_json::json!({
                "id": self.id, "op": self.operation, "operands": operands
            }))
        });
        (cancel || result == DialogResult::Cancel, command)
    }
}

fn operation_label(op: BoolOp) -> &'static str {
    text(match op {
        BoolOp::Add => "menu.vector.geometry.add",
        BoolOp::Subtract => "menu.vector.geometry.subtract",
        BoolOp::Intersect => "menu.vector.geometry.intersect",
        BoolOp::Xor => "menu.vector.geometry.xor",
        BoolOp::Divide => "menu.vector.geometry.divide",
    })
}
