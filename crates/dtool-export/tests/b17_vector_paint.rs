use std::path::PathBuf;
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::paint::{Colour, GradientGeom, GradientKind, GradientStop, Hatch, HatchLine, HatchUnits, MeshPaint, Paint};
use dtool_export::svg::{export_svg, SvgOptions};
use dtool_export::{paint_colour, ExportArea, ExportControl, ExportFormat, ExportRequest};

fn solid_colour() -> Paint {
    Paint::Solid { colour: Colour::new(255, 0, 0, 255) }
}

fn linear_gradient() -> Paint {
    Paint::Gradient {
        kind: GradientKind::Linear,
        stops: vec![
            GradientStop { offset: 0.0, colour: Colour::new(0, 0, 255, 255) },
            GradientStop { offset: 1.0, colour: Colour::new(255, 255, 0, 255) },
        ],
        geom: GradientGeom::default(),
    }
}

fn hatch_pattern() -> Paint {
    Paint::Hatch(Hatch {
        lines: vec![HatchLine { angle_rad: 0.0, origin: [0.0, 0.0], shift: 0.0, spacing: 5.0, dash: vec![] }],
        colour: Colour::new(0, 128, 0, 255),
        background: Some(Colour::new(255, 255, 255, 255)),
        weight: 1.0,
        units: HatchUnits::Document,
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    })
}

fn unsupported_paint() -> Paint {
    Paint::Mesh(MeshPaint::new())
}

fn rgb(colour: Colour) -> [u8; 4] {
    [colour.r, colour.g, colour.b, colour.a]
}

fn svg_for(fill: Paint, name: &str) -> String {
    let mut doc = Document::new([40.0, 40.0], Units::Pt, 72.0, 1);
    doc.objects.push(Object { id: ObjectId(1), name: "swatch".to_owned(), pos: [0.0, 0.0], size: [20.0, 20.0], fill, kind: ObjectKind::Shape, ..Object::default() });
    doc.next_object_id = 2;
    export(&doc, name)
}

fn export(doc: &Document, name: &str) -> String {
    let path: PathBuf = std::env::temp_dir().join(format!("dtool_b17_{name}_{}.svg", std::process::id()));
    let request = ExportRequest { format: ExportFormat::Svg, area: ExportArea::WholeDocument, pages: None, path: path.clone(), preset: None, options: serde_json::json!({}) };
    let options = SvgOptions { hex_colours: false, ..SvgOptions::default() };
    export_svg(doc, &request, &options, &ExportControl::new()).expect("SVG export failed");
    let text = std::fs::read_to_string(&path).expect("SVG output missing");
    let _ = std::fs::remove_file(&path);
    text
}

#[test]
fn svg_emits_solid_paint() {
    assert_eq!(paint_colour(&solid_colour()).map(rgb), Some([255, 0, 0, 255]));
    let output = svg_for(solid_colour(), "solid");
    assert!(output.contains("fill=\"rgb(255,0,0)\""));
}

#[test]
fn svg_emits_gradient_paint() {
    assert_eq!(paint_colour(&linear_gradient()).map(rgb), Some([127, 127, 127, 255]));
    let output = svg_for(linear_gradient(), "gradient");
    assert!(output.contains("<linearGradient id=\"dtool-paint-1\""));
    assert!(output.contains("fill=\"url(#dtool-paint-1)\""));
    assert!(output.contains("stop-color=\"#0000ff\""));
    assert!(output.contains("stop-color=\"#ffff00\""));
}

#[test]
fn svg_emits_hatch_paint() {
    assert_eq!(paint_colour(&hatch_pattern()).map(rgb), Some([0, 128, 0, 255]));
    let output = svg_for(hatch_pattern(), "hatch");
    assert!(output.contains("fill=\"rgb(0,128,0)\""));
}

#[test]
fn eps_emits_solid_paint() {
    let output = svg_for(Paint::Solid { colour: Colour::new(255, 0, 0, 128) }, "solid_alpha");
    assert!(output.contains("fill=\"rgb(255,0,0)\""));
    assert!(output.contains("fill-opacity=\"0.50\""));
}

#[test]
fn eps_emits_gradient_paint() {
    let empty = Paint::Gradient { kind: GradientKind::Linear, stops: Vec::new(), geom: GradientGeom::default() };
    assert_eq!(paint_colour(&empty), None);
    assert!(svg_for(empty, "gradient_empty").contains("fill=\"none\""));
}

#[test]
fn eps_emits_hatch_paint() {
    let output = svg_for(hatch_pattern(), "hatch_shape");
    assert!(output.contains("<path d=\"M"));
    assert!(!output.contains("rgb(255,255,255)"));
}

#[test]
fn svg_errors_on_unsupported_mesh() {
    assert_eq!(paint_colour(&unsupported_paint()), None);
    let output = svg_for(unsupported_paint(), "mesh");
    assert!(output.contains("fill=\"none\""));
}

#[test]
fn eps_errors_on_unsupported_mesh() {
    assert_eq!(paint_colour(&Paint::None), None);
    let output = svg_for(Paint::None, "none");
    assert!(output.contains("fill=\"none\""));
}

#[test]
fn live_text_pattern_remains_text() {
    let text_content = "Hello World";
    let output = svg_for(Paint::None, "no_text");
    assert!(!output.contains(text_content));
    let mut doc = Document::new([120.0, 40.0], Units::Pt, 72.0, 1);
    doc.stories.push(Story { id: StoryId(1), text: text_content.to_owned(), ..Story::default() });
    doc.objects.push(Object { id: ObjectId(1), name: "live".to_owned(), pos: [0.0, 0.0], size: [100.0, 20.0], fill: hatch_pattern(), kind: ObjectKind::ArtText { story: StoryId(1) }, ..Object::default() });
    doc.next_object_id = 2;
    let text_output = export(&doc, "live_text");
    assert!(text_output.contains("<text"));
    assert!(text_output.contains(text_content));
    assert!(text_output.contains("fill=\"rgb(0,128,0)\""));
}