use dtool_core::Session;
use dtool_strings::text;
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRow {
    pub id: u64,
    pub name: String,
}

#[derive(Default)]
pub struct SnapshotsPanel {
    pub open: bool,
}

pub fn rows(session: &mut Session) -> Vec<SnapshotRow> {
    let Ok(list) = session.run_named("history.snapshot.list", json!({})) else { return Vec::new() };
    list["snapshots"].as_array().map(|items| items.iter().filter_map(|item| {
        Some(SnapshotRow { id: item["id"].as_u64()?, name: item["name"].as_str()?.to_string() })
    }).collect()).unwrap_or_default()
}

pub fn create(session: &mut Session) -> Option<u64> {
    session.run_named("history.snapshot.create", json!({})).ok().and_then(|out| out["id"].as_u64())
}

pub fn restore(session: &mut Session, id: u64) -> bool {
    session.run_named("history.snapshot.restore", json!({"id": id})).is_ok()
}

pub fn layer_from(session: &mut Session, id: u64) -> Option<u64> {
    session.run_named("history.snapshot.layer_from", json!({"id": id})).ok().and_then(|out| out["group_id"].as_u64())
}

impl SnapshotsPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        if dtool_design::button(ui, text("snapshot.create")).clicked() {
            create(session);
        }
        let rows = rows(session);
        let mut restore_id: Option<u64> = None;
        let mut layer_id: Option<u64> = None;
        egui::ScrollArea::vertical().id_salt("snapshot_rows").show(ui, |ui| {
            for row in &rows {
                ui.horizontal(|ui| {
                    ui.label(row.name.as_str());
                    if dtool_design::button(ui, text("snapshot.restore")).clicked() {
                        restore_id = Some(row.id);
                    }
                    if dtool_design::button(ui, text("snapshot.layer_from")).clicked() {
                        layer_id = Some(row.id);
                    }
                });
            }
        });
        if let Some(id) = restore_id {
            restore(session, id);
        }
        if let Some(id) = layer_id {
            layer_from(session, id);
        }
    }
}
