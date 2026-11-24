use dtool_core::colour::{convert, ColourModel, ConvertCtx, ProfileStore, RenderingIntent};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::Command;
use dtool_ui::dock::PanelId;
use dtool_ui::panels::colour::{ColourPanel, ColourTarget, PanelModel};
use dtool_ui::panels::{window_panel, workspace_panel, PanelRegistry, WINDOW_COLOUR, WINDOW_PANELS};
use serde_json::json;

fn workspace() -> (Workspace, ObjectId, ObjectId) {
    dtool_core::palette::set_app_path(Some(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("c07_b10_palettes_{}.json", std::process::id()))));
    let mut workspace = Workspace::default();
    workspace.new_document("c07_b10");
    let session = workspace.session_mut().unwrap();
    let black = Paint::Solid { colour: Colour::new(0, 0, 0, 255) };
    let white = Paint::Solid { colour: Colour::new(255, 255, 255, 255) };
    let mut first = Object { name: "a".into(), size: [10.0, 10.0], fill: black.clone(), ..Object::default() };
    first.stroke.paint = white.clone();
    let mut second = Object { name: "b".into(), pos: [20.0, 0.0], size: [10.0, 10.0], fill: black, ..Object::default() };
    second.stroke.paint = white;
    let a = session.document.create_object(first);
    let b = session.document.create_object(second);
    session.selection.ids = vec![a, b];
    (workspace, a, b)
}

fn fill(workspace: &Workspace, id: ObjectId) -> Paint {
    workspace.session().unwrap().document.get_object(id).unwrap().fill.clone()
}

fn run(workspace: &mut Workspace, command: Command) -> serde_json::Value {
    let name = command.name.clone();
    workspace.run(Scope::active(), command).1.unwrap_or_else(|error| panic!("{name}: {error:?}"))
}

#[test]
fn c07_b10_colour_panel_is_a_live_window_panel() {
    assert_eq!(window_panel(WINDOW_COLOUR), Some(PanelId::Colour));
    assert!(WINDOW_PANELS.contains(&WINDOW_COLOUR));
    let mut registry = PanelRegistry::with_existing_bodies();
    assert!(registry.has_body(&PanelId::Colour));
    let (mut workspace, a, _) = workspace();
    let ctx = egui::Context::default();
    let mut shown = false;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        shown = workspace_panel(ui, &PanelId::Colour, &mut registry, &mut workspace);
    });
    output.textures_delta.clear();
    assert!(shown);
    assert_eq!(registry.colour.hex, "#000000FF");
    assert_eq!(fill(&workspace, a), Paint::Solid { colour: Colour::new(0, 0, 0, 255) });
    let ratio = registry.colour.contrast.expect("contrast computed through color.contrast");
    assert!((ratio - 21.0).abs() < 0.05, "{ratio}");
}

#[test]
fn c07_b10_hex_alpha_applies_to_selection_in_one_undo_step() {
    let (mut workspace, a, b) = workspace();
    let mut panel = ColourPanel::default();
    panel.sync(&workspace);
    panel.set_hex("#33669980").unwrap();
    assert_eq!(panel.hex, "#33669980");
    assert!(panel.set_hex("#12345").is_err());
    assert!(panel.set_hex("nope").is_err());
    let command = panel.apply_command(&workspace).expect("changed selection");
    assert_eq!(command.name, "edit.batch");
    assert!(panel.apply(&mut workspace));
    let expected = Paint::Solid { colour: Colour::new(0x33, 0x66, 0x99, 0x80) };
    assert_eq!(fill(&workspace, a), expected);
    assert_eq!(fill(&workspace, b), expected);
    let mut synced = ColourPanel::default();
    assert!(synced.sync(&workspace));
    assert_eq!(synced.hex, "#33669980");
    assert!(panel.apply_command(&workspace).is_none());
    run(&mut workspace, Command::new("edit.undo"));
    assert_eq!(fill(&workspace, a), Paint::Solid { colour: Colour::new(0, 0, 0, 255) });
    assert_eq!(fill(&workspace, b), Paint::Solid { colour: Colour::new(0, 0, 0, 255) });
    let recent = panel.recent(&mut workspace);
    assert!(recent.iter().any(|colour| colour.a == 0x80 && colour.r == 0x33), "{recent:?}");
}

#[test]
fn c07_b10_stroke_target_updates_stroke_only() {
    let (mut workspace, a, _) = workspace();
    workspace.session_mut().unwrap().selection.ids = vec![a];
    let mut panel = ColourPanel::default();
    panel.set_target(ColourTarget::Stroke);
    assert!(panel.sync(&workspace));
    assert_eq!(panel.hex, "#FFFFFFFF");
    panel.set_alpha(0x40);
    let command = panel.apply_command(&workspace).unwrap();
    assert_eq!(command.name, "object.update");
    run(&mut workspace, command);
    let object = workspace.session().unwrap().document.get_object(a).unwrap().clone();
    assert_eq!(object.stroke.paint, Paint::Solid { colour: Colour::new(255, 255, 255, 0x40) });
    assert_eq!(object.fill, Paint::Solid { colour: Colour::new(0, 0, 0, 255) });
}

#[test]
fn c07_b10_cmyk_lab_grey_values_come_from_profiles() {
    let mut panel = ColourPanel::default();
    panel.set_hex("#FF0000").unwrap();
    let services = dtool_core::colour::services().read().unwrap();
    for (model, target, profile) in [(PanelModel::Cmyk, ColourModel::Cmyk, ProfileStore::FOGRA39L), (PanelModel::Lab, ColourModel::Lab, ProfileStore::LAB_D50), (PanelModel::Grey, ColourModel::Grey, ProfileStore::GREY_D50)] {
        panel.model = model;
        let values = panel.model_values().unwrap();
        let ctx = ConvertCtx { from: ProfileStore::SRGB.to_string(), to: profile.to_string(), intent: RenderingIntent::RelativeColorimetric, bpc: true };
        let expected = convert(&Colour::new(255, 0, 0, 255).managed(), target, &ctx, &services.profiles).unwrap();
        assert_eq!(values.len(), model.ranges().len());
        for (value, want) in values.iter().zip(expected.values.iter()) {
            assert!((value - want).abs() < 1e-3, "{model:?} {values:?} {:?}", expected.values);
        }
    }
    drop(services);
    panel.model = PanelModel::Cmyk;
    assert_ne!(panel.model_values().unwrap(), vec![0.0, 100.0, 100.0, 0.0]);
    panel.model = PanelModel::Lab;
    let lab = panel.model_values().unwrap();
    panel.set_model_values(&lab).unwrap();
    assert!(panel.colour.r >= 250 && panel.colour.g <= 5 && panel.colour.b <= 5, "{:?}", panel.colour);
    assert!(panel.set_model_values(&[200.0, 0.0, 0.0]).is_err());
    panel.model = PanelModel::Hsl;
    panel.set_model_values(&[120.0, 100.0, 50.0]).unwrap();
    assert_eq!((panel.colour.r, panel.colour.g, panel.colour.b), (0, 255, 0));
    assert_eq!(panel.model_values().unwrap(), vec![120.0, 100.0, 50.0]);
}

#[test]
fn c07_b10_soft_proof_controls_drive_view_state() {
    let (mut workspace, _, _) = workspace();
    let mut panel = ColourPanel::default();
    let profiles = ColourPanel::proof_profiles();
    assert!(profiles.iter().any(|name| name == ProfileStore::FOGRA39L), "{profiles:?}");
    panel.proof.enabled = true;
    panel.proof.intent = RenderingIntent::Perceptual;
    panel.proof.gamut_check = true;
    run(&mut workspace, panel.proof_command());
    let view = workspace.session().unwrap().view.clone();
    assert!(view.soft_proof_enabled);
    assert_eq!(view.soft_proof_profile, ProfileStore::FOGRA39L);
    assert_eq!(view.soft_proof_intent, "perceptual");
    panel.proof.enabled = false;
    run(&mut workspace, panel.proof_command());
    assert!(!workspace.session().unwrap().view.soft_proof_enabled);
    let bad = workspace.run(Scope::active(), Command::with_params("ui.soft_proof.set", json!({ "enabled": true, "intent": "nope" }))).1;
    assert!(bad.is_err());
}