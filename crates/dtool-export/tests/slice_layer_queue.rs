use std::path::PathBuf;

use dtool_core::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::paint::{Colour, Paint};
use dtool_core::slice::{INCLUDE_CONTAINER, OutputId, OutputSize, SliceOutput, SliceTarget};
use dtool_export::raster::OverwriteDecision;
use dtool_export::slices;
use serde_json::json;

fn rect(id: u64, parent: Option<u64>, pos: [f32; 2], size: [f32; 2], rgba: [u8; 4]) -> Object {
    let [r, g, b, a] = rgba;
    Object {
        id: ObjectId(id),
        parent_id: parent.map(ObjectId),
        pos,
        size,
        fill: Paint::Solid {
            colour: Colour::new(r, g, b, a),
        },
        ..Object::default()
    }
}

fn add(session: &mut Session, source: u64, name: &str, outputs: Vec<SliceOutput>) {
    let result = session
        .run_named(
            "slice.create",
            json!({"source": source, "name": name, "layer": true}),
        )
        .unwrap();
    let id = &result["id"];
    assert!(session.document.slices.last().unwrap().outputs.is_empty());
    session
        .run_named("slice.update", json!({"id": id, "outputs": outputs}))
        .unwrap();
}

fn output(id: u64, format: &str, size: OutputSize, include: bool) -> SliceOutput {
    let mut options = json!({ (INCLUDE_CONTAINER): include });
    if format == "pdf" {
        options["pdf_colour_space"] = json!("RGB");
        options["pdf_raster_dpi"] = json!(300);
    }
    SliceOutput {
        id: OutputId(id),
        enabled: true,
        format: format.into(),
        size,
        suffix: String::new(),
        path: None,
        options: Some(options),
    }
}

#[test]
fn queue_exports_trimmed_groups_full_artboards_and_confirms_existing_files() {
    let mut doc = Document::new([160.0, 100.0], Units::Px, 72.0, 1);
    doc.objects = vec![
        Object {
            kind: ObjectKind::Artboard,
            ..rect(1, None, [0.0, 0.0], [100.0, 80.0], [0; 4])
        },
        Object {
            kind: ObjectKind::Group,
            ..rect(2, Some(1), [0.0, 0.0], [60.0, 45.0], [0; 4])
        },
        rect(3, Some(2), [10.0, 12.0], [20.0, 10.0], [255, 0, 0, 255]),
        rect(4, Some(2), [40.0, 36.0], [10.0, 8.0], [255, 0, 0, 0]),
        rect(5, Some(1), [15.0, 14.0], [6.0, 4.0], [0, 0, 255, 255]),
    ];
    doc.next_object_id = 6;
    let mut session = Session::new(doc);
    add(
        &mut session,
        2,
        "Group",
        vec![
            output(1, "png", OutputSize::Scale(1.0), false),
            output(2, "png", OutputSize::Scale(1.0), true),
            output(3, "png", OutputSize::Width(40), false),
            output(4, "pdf", OutputSize::Height(20), false),
        ],
    );
    add(
        &mut session,
        1,
        "Board",
        vec![output(5, "png", OutputSize::Scale(1.0), false)],
    );
    session
        .run_named(
            "slice.create",
            json!({"rect": [0, 0, 20, 20], "name": "Legacy"}),
        )
        .unwrap();

    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("slice-layer-queue-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Group.png"), b"old").unwrap();
    let receipt = slices::run_confirming(
        &session.document,
        &SliceTarget::LayerQueue,
        &folder,
        "Doc",
        "Doc",
        |item| {
            assert_eq!(item.path.file_name().unwrap(), "Group.png");
            OverwriteDecision::Skip
        },
    )
    .unwrap();
    assert_eq!(std::fs::read(folder.join("Group.png")).unwrap(), b"old");
    assert_eq!(receipt.written().len(), 4, "{receipt:?}");
    assert_eq!(receipt.skipped().len(), 1);
    assert!(!folder.join("Legacy.png").exists());

    let receipt = slices::run_confirming(
        &session.document,
        &SliceTarget::LayerQueue,
        &folder,
        "Doc",
        "Doc",
        |item| {
            if item.path.file_name().unwrap() == "Group.png" {
                OverwriteDecision::Overwrite
            } else {
                OverwriteDecision::Skip
            }
        },
    )
    .unwrap();
    assert_eq!(receipt.written().len(), 1);
    let image = |name| image::open(folder.join(name)).unwrap().to_rgba8();
    let trimmed = image("Group.png");
    assert_eq!(trimmed.dimensions(), (20, 10));
    assert_eq!(trimmed.get_pixel(7, 3).0, [255, 0, 0, 255]);
    let full = image("Group-2.png");
    assert_eq!(full.dimensions(), (100, 80));
    assert_eq!(full.get_pixel(0, 0)[3], 0);
    assert_eq!(full.get_pixel(17, 15).0, [255, 0, 0, 255]);
    assert_eq!(image("Group@40x20.png").dimensions(), (40, 20));
    let board = image("Board.png");
    assert_eq!(board.dimensions(), (100, 80));
    assert_eq!(board.get_pixel(17, 15).0, [0, 0, 255, 255]);
    let pdf = lopdf::Document::load(folder.join("Group@40x20.pdf")).unwrap();
    let page = *pdf.get_pages().values().next().unwrap();
    let media_box = pdf
        .get_dictionary(page)
        .unwrap()
        .get(b"MediaBox")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(media_box[2].as_float().unwrap(), 40.0);
    assert_eq!(media_box[3].as_float().unwrap(), 20.0);
}

#[test]
fn queue_uses_the_layers_page_and_exports_multiple_formats() {
    let mut doc = Document::new([120.0, 70.0], Units::Px, 72.0, 2);
    let (x, y, _, _) = dtool_export::page_region(&doc, 1).unwrap();
    let mut layer = rect(1, None, [x + 10.0, y + 8.0], [20.0, 10.0], [255, 0, 0, 255]);
    layer.page_id = Some(doc.pages[1].id);
    doc.objects.push(layer);
    doc.next_object_id = 2;
    let mut session = Session::new(doc);
    add(
        &mut session,
        1,
        "Layer",
        vec![
            output(1, "png", OutputSize::Scale(1.0), true),
            output(2, "jpeg", OutputSize::Height(20), false),
            output(3, "webp", OutputSize::Scale(0.5), false),
            output(4, "svg", OutputSize::Width(40), false),
        ],
    );
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("slice-layer-formats-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let receipt = slices::run(
        &session.document,
        &SliceTarget::LayerQueue,
        &folder,
        "Doc",
        "Doc",
    )
    .unwrap();
    assert_eq!(receipt.written().len(), 4, "{receipt:?}");
    let png = image::open(folder.join("Layer.png")).unwrap().to_rgba8();
    assert_eq!(png.dimensions(), (120, 70));
    assert_eq!(png.get_pixel(15, 12).0, [255, 0, 0, 255]);
    assert_eq!(png.get_pixel(0, 0)[3], 0);
    let jpeg = image::open(folder.join("Layer@40x20.jpg"))
        .unwrap()
        .to_rgb8();
    assert_eq!(jpeg.dimensions(), (40, 20));
    assert!(jpeg.get_pixel(10, 7)[0] > 230);
    let webp = image::open(folder.join("Layer@0.5x.webp"))
        .unwrap()
        .to_rgba8();
    assert_eq!(webp.dimensions(), (10, 5));
    let svg = std::fs::read_to_string(folder.join("Layer@40x20.svg")).unwrap();
    assert!(
        svg.contains("width=\"40\"") && svg.contains("height=\"20\""),
        "{svg}"
    );
}

#[test]
fn transparent_bounds_cross_measurement_tiles() {
    let mut doc = Document::new([5000.0, 80.0], Units::Px, 72.0, 1);
    doc.objects = vec![
        Object {
            kind: ObjectKind::Group,
            ..rect(1, None, [0.0, 0.0], [5000.0, 80.0], [0; 4])
        },
        rect(2, Some(1), [0.0, 0.0], [5000.0, 80.0], [0; 4]),
        rect(3, Some(1), [2046.0, 10.0], [6.0, 4.0], [255, 0, 0, 255]),
    ];
    doc.next_object_id = 4;
    let mut session = Session::new(doc);
    add(
        &mut session,
        1,
        "Wide",
        vec![output(1, "png", OutputSize::Scale(1.0), false)],
    );
    let job = slices::plan(
        &session.document,
        &SliceTarget::LayerQueue,
        std::path::Path::new("."),
        "Doc",
        "Doc",
    )
    .unwrap();
    let item = &job.list()[0];
    assert_eq!(item.options.width, Some(6));
    assert_eq!(item.options.height, Some(4));
    assert_eq!(
        item.options.area,
        dtool_export::ExportArea::Rect([2046.0, 10.0, 6.0, 4.0])
    );
}
