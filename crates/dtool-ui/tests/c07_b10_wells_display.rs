use dtool_core::colour::model::ObjectIcc;
use dtool_core::colour::{ColourModel, ConvertCtx, ProfileStore};
use dtool_core::document::Document;
use dtool_core::object::Object;
use dtool_core::paint::{Colour, Paint, PaintRole};
use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::Command;
use dtool_ui::dock::PanelId;
use dtool_ui::fill_stroke::{paint_well_colour, FillStrokeWells, PaintOwners};
use dtool_ui::panels::{workspace_panel, PanelRegistry};
use serde_json::json;
use std::sync::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

fn cmyk(c: f32, m: f32, y: f32, k: f32) -> Colour {
    Colour::from_managed(dtool_core::colour::Colour::cmyk(c, m, y, k, 1.0))
}

fn expected(colour: &Colour, profile: &str) -> egui::Color32 {
    let services = dtool_core::colour::services().read().unwrap();
    let ctx = ConvertCtx { from: profile.to_string(), to: ProfileStore::SRGB.to_string(), intent: services.preferences.intent, bpc: services.preferences.bpc };
    let out = dtool_core::colour::convert::convert(&colour.managed(), ColourModel::Rgb, &ctx, &services.profiles).unwrap();
    let to_u8 = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgba_unmultiplied(to_u8(out.values[0]), to_u8(out.values[1]), to_u8(out.values[2]), colour.a)
}

fn naive(colour: &Colour) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(colour.r, colour.g, colour.b, colour.a)
}

fn app_cmyk() -> String {
    dtool_core::colour::services().read().unwrap().preferences.cmyk.clone()
}

fn doc_profile() -> &'static str {
    let has_swop = dtool_core::colour::services().read().unwrap().profiles.find(ProfileStore::SWOP_V2).is_some();
    if has_swop { ProfileStore::SWOP_V2 } else { ProfileStore::FOGRA39L }
}

fn collect(shape: &egui::Shape, out: &mut Vec<egui::Color32>) {
    match shape {
        egui::Shape::Rect(rect) => out.push(rect.fill),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, out)),
        _ => {}
    }
}

fn fills_of(run: impl FnMut(&mut egui::Ui)) -> Vec<egui::Color32> {
    let ctx = egui::Context::default();
    let mut run = run;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| run(ui));
    output.textures_delta.clear();
    let mut fills = Vec::new();
    output.shapes.iter().for_each(|clipped| collect(&clipped.shape, &mut fills));
    fills
}

fn run(workspace: &mut Workspace, command: Command) -> serde_json::Value {
    let name = command.name.clone();
    workspace.run(Scope::active(), command).1.unwrap_or_else(|error| panic!("{name}: {error:?}"))
}

#[test]
fn c07_b10_swatch_previews_use_document_then_app_cmyk_profile() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    dtool_core::palette::set_app_path(Some(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("c07_b10_wells_palettes_{}.json", std::process::id()))));
    let mut workspace = Workspace::default();
    workspace.new_document("c07_b10_wells");
    let ink = cmyk(10.0, 80.0, 20.0, 5.0);
    let rgb = Colour::new(12, 200, 40, 255);
    run(&mut workspace, Command::with_params("swatch.add", json!({ "name": "ink", "colour": serde_json::to_value(ink).unwrap() })));
    run(&mut workspace, Command::with_params("swatch.add", json!({ "name": "screen", "colour": serde_json::to_value(rgb).unwrap() })));
    run(&mut workspace, Command::with_params("palette.view", json!({ "scope": "document" })));
    let mut registry = PanelRegistry::with_existing_bodies();
    let fills = fills_of(|ui| { workspace_panel(ui, &PanelId::Swatches, &mut registry, &mut workspace); });
    assert!(fills.contains(&expected(&ink, &app_cmyk())), "CMYK swatch preview goes through the app default CMYK profile");
    assert!(!fills.contains(&naive(&ink)), "naive CMYK swatch preview");
    assert!(fills.contains(&naive(&rgb)), "RGB swatch unchanged");
    let profile = doc_profile();
    {
        let session = workspace.session_mut().unwrap();
        session.document.colour.format.model = ColourModel::Cmyk;
        session.document.colour.profile = profile.to_string();
    }
    let fills = fills_of(|ui| { workspace_panel(ui, &PanelId::Swatches, &mut registry, &mut workspace); });
    assert!(fills.contains(&expected(&ink, profile)), "CMYK swatch preview goes through the document CMYK profile");
    assert!(fills.contains(&naive(&rgb)));
}

#[test]
fn c07_b10_fill_and_stroke_wells_use_display_path() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let fill_ink = cmyk(0.0, 90.0, 80.0, 0.0);
    let stroke_ink = cmyk(80.0, 0.0, 20.0, 10.0);
    let mut object = Object { fill: Paint::Solid { colour: fill_ink }, ..Object::default() };
    object.stroke.paint = Paint::Solid { colour: stroke_ink };
    assert_eq!(paint_well_colour(&object.fill), Some(expected(&fill_ink, &app_cmyk())));
    assert_eq!(paint_well_colour(&Paint::Solid { colour: Colour::new(255, 0, 0, 255) }), Some(egui::Color32::from_rgba_unmultiplied(255, 0, 0, 255)));
    let profile = doc_profile();
    let mut doc = Document::new([100.0, 100.0], dtool_core::document::Units::Px, 72.0, 1);
    doc.colour.format.model = ColourModel::Cmyk;
    doc.colour.profile = profile.to_string();
    let mut fill = object.fill.clone();
    let mut stroke = object.stroke.clone();
    let owners = PaintOwners { source: Some(&mut fill), frame: None, cell: None, edge: Some(&mut stroke), glyph: None };
    let mut wells = FillStrokeWells::new(&owners);
    wells.set_display(Some(&doc), None);
    wells.open_popup_for(&owners, PaintRole::SourceObject);
    let fills = fills_of(|ui| wells.show_popup(ui));
    assert!(fills.contains(&expected(&fill_ink, profile)), "fill well uses the document CMYK profile");
    assert!(!fills.contains(&naive(&fill_ink)));
    wells.open_popup_for(&owners, PaintRole::Edge);
    let fills = fills_of(|ui| wells.show_popup(ui));
    assert!(fills.contains(&expected(&stroke_ink, profile)), "stroke well uses the document CMYK profile");
    wells.set_display(Some(&doc), Some(ObjectIcc { profile: Some(ProfileStore::FOGRA39L.to_string()), intent: None }));
    let fills = fills_of(|ui| wells.show_popup(ui));
    assert!(fills.contains(&expected(&stroke_ink, ProfileStore::FOGRA39L)), "object profile wins in the well");
    assert_eq!(object.fill, Paint::Solid { colour: fill_ink }, "stored paint untouched");
}
