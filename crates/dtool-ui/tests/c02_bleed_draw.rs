#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::document::{Document, Units};
use dtool_core::workspace::Scope;
use dtool_core::Command;
use dtool_export::pdf::{export_pdf, PdfOptions};
use dtool_export::png::export_png;
use dtool_export::{ExportArea, ExportControl, ExportFormat, ExportRequest};
use dtool_ui::rulers::{bleed_rects, BLEED_COLOUR, DEFAULT_BLEED_MM};
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn shell(units: Option<Units>) -> AppShell {
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("c02_bleed");
    shell.workspace.activate(id).unwrap();
    if let Some(units) = units {
        let session = shell.workspace.session_mut().unwrap();
        session.document = Document::new([210.0, 297.0], units, 300.0, 1);
        session.has_document = true;
    }
    shell
}

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command::with_params(name, params)).1.unwrap()
}

fn page(shell: &mut AppShell) -> [f32; 4] {
    let doc = &shell.workspace.session_mut().unwrap().document;
    let size = doc.pages[0].size;
    [0.0, 0.0, size[0], size[1]]
}

fn inflate(r: [f32; 4], [l, t, rr, b]: [f32; 4]) -> [f32; 4] {
    [r[0] - l, r[1] - t, r[2] + l + rr, r[3] + t + b]
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

fn bleed_shapes(shell: &mut AppShell) -> usize {
    let ctx = egui::Context::default();
    let canvas = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 1200.0));
    let input = egui::RawInput { screen_rect: Some(canvas), ..Default::default() };
    let rulers = &shell.rulers;
    let session = shell.workspace.session_mut().unwrap();
    let mut view = session.view.clone();
    view.zoom = 1.0;
    view.scroll = [100.0, 100.0];
    view.rotation_deg = 0.0;
    let session = &*session;
    let mut output = ctx.run_ui(input, |ui| rulers.paint_visible_guides(ui, session, &view, canvas));
    output.textures_delta.clear();
    format!("{:?}", output.shapes).matches(&format!("{:?}", BLEED_COLOUR)).count()
}

fn default_bleed(units: Units, dpi: f32) -> f32 {
    let px = dtool_core::units::to_px(DEFAULT_BLEED_MM, Units::Mm, dpi).unwrap();
    dtool_core::units::from_px(px, units, dpi).unwrap()
}

#[test]
fn bleed_box_follows_per_document_show_bleed_with_3mm_default_in_px_doc() {
    let _home = test_home::enter("bleed_box_follows_per_document_show_bleed_with_3mm_default_in_px_doc");
    let mut s = shell(None);
    assert!(bleed_rects(s.workspace.session_mut().unwrap()).is_empty());
    assert_eq!(bleed_shapes(&mut s), 0);

    run(&mut s, "view.flags.set", json!({ "show_bleed": true }));
    let (units, dpi) = { let d = &s.workspace.session_mut().unwrap().document; (d.units, d.dpi) };
    let d = default_bleed(units, dpi);
    assert!(d > 0.0);
    let rects = bleed_rects(s.workspace.session_mut().unwrap());
    let p = page(&mut s);
    assert_eq!(rects.len(), 1);
    assert!(close(rects[0], inflate(p, [d; 4])), "{rects:?} page {p:?} d {d}");
    assert_eq!(bleed_shapes(&mut s), 4);

    let version = s.workspace.session_mut().unwrap().document.version;
    s.workspace.session_mut().unwrap().document.setup.bleed.left = 10.0;
    s.workspace.session_mut().unwrap().document.setup.bleed.top = 5.0;
    let rects = bleed_rects(s.workspace.session_mut().unwrap());
    assert!(close(rects[0], inflate(p, [10.0, 5.0, 0.0, 0.0])), "{rects:?}");
    assert_eq!(s.workspace.session_mut().unwrap().document.version, version);

    run(&mut s, "view.flags.set", json!({ "show_bleed": false }));
    assert!(bleed_rects(s.workspace.session_mut().unwrap()).is_empty());
    assert_eq!(bleed_shapes(&mut s), 0);
}

#[test]
fn bleed_box_in_mm_doc_uses_3mm_default_and_custom_mm_bleed() {
    let _home = test_home::enter("bleed_box_in_mm_doc_uses_3mm_default_and_custom_mm_bleed");
    let mut s = shell(Some(Units::Mm));
    run(&mut s, "view.flags.set", json!({ "show_bleed": true }));
    let p = page(&mut s);
    assert_eq!(p, [0.0, 0.0, 210.0, 297.0]);
    let rects = bleed_rects(s.workspace.session_mut().unwrap());
    assert!((default_bleed(Units::Mm, 300.0) - 3.0).abs() < 1e-3);
    assert!(close(rects[0], [-3.0, -3.0, 216.0, 303.0]), "{rects:?}");
    let bleed = &mut s.workspace.session_mut().unwrap().document.setup.bleed;
    bleed.left = 5.0;
    bleed.right = 5.0;
    bleed.top = 2.0;
    bleed.bottom = 2.0;
    let rects = bleed_rects(s.workspace.session_mut().unwrap());
    assert!(close(rects[0], [-5.0, -2.0, 220.0, 301.0]), "{rects:?}");
    assert_eq!(bleed_shapes(&mut s), 4);
}

#[test]
fn export_output_is_unchanged_by_show_bleed() {
    let _home = test_home::enter("export_output_is_unchanged_by_show_bleed");
    let mut s = shell(None);
    run(&mut s, "object.create", json!({ "pos": [10.0, 10.0], "size": [40.0, 30.0] }));
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("c02_bleed_draw");
    std::fs::create_dir_all(&dir).unwrap();
    let mut outputs = Vec::new();
    for (i, on) in [false, true].into_iter().enumerate() {
        run(&mut s, "view.flags.set", json!({ "show_bleed": on }));
        let doc = s.workspace.session_mut().unwrap().document.clone();
        let png = export_png(&doc, None).unwrap();
        let path = dir.join(format!("bleed_{i}.pdf"));
        let request = ExportRequest { format: ExportFormat::Pdf, area: ExportArea::WholeDocument, pages: None, path: path.clone(), preset: None, options: json!({}) };
        export_pdf(&doc, &request, &PdfOptions::default(), &ExportControl::new()).unwrap();
        outputs.push((png, std::fs::read(&path).unwrap()));
    }
    assert_eq!(outputs[0].0, outputs[1].0);
    assert_eq!(outputs[0].1, outputs[1].1);
    assert!(s.workspace.session_mut().unwrap().view.canvas_flags.show_bleed);
}