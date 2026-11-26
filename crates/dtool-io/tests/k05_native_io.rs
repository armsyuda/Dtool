use dtool_core::colour::ColourPrefs;
use dtool_core::document::{Document, Story, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::paint::{range_fill, set_range_fill, Colour, Paint, StrokeStyle};
use dtool_core::pixel::PixelLayer;
use dtool_core::raster_source::LayerMask;
use dtool_core::resources::{ImageFormat, ImageResource, ResourceId};
use dtool_core::selection::PixelSelection;
use dtool_core::table::model::TableNode;
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::{Command, Session};
use dtool_io::native::{
    IoErrorKind, OpenOutcome, cleanup_stale_temp, open, open_with_status, save, save_with_extras, temp_path,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const SRGB: &str = "sRGB IEC61966-2.1";

fn solid(r: u8, g: u8, b: u8) -> Paint {
    Paint::Solid {
        colour: Colour::new(r, g, b, 255),
    }
}

fn unique_path(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "dtool_k05_{}_{}_{}.dtool",
        tag,
        std::process::id(),
        nanos
    ))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
    let tmp = temp_path(path);
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_dir(&tmp);
}

fn table_with_cell_fill(fill: Paint, story: u64) -> TableNode {
    let mut table: TableNode = serde_json::from_value(json!({
        "col_widths_pt": [80.0],
        "row_heights_pt": [20.0],
        "cells": [{
            "id": 1,
            "row": 0,
            "col": 0,
            "row_span": 1,
            "col_span": 1,
            "story": story
        }]
    }))
    .expect("table node");
    table.cells[0].local.fill = Some(fill);
    table
}

fn story_with_fill(id: u64, text: &str, fill: Paint) -> Story {
    let mut range = json!({ "start": 0, "end": text.len() });
    set_range_fill(&mut range, Some(&fill));
    Story {
        id: dtool_core::document::StoryId(id),
        text: text.to_string(),
        ranges: vec![range],
        specials: Vec::new(),
        paras: Vec::new(),
    }
}

fn sample_document() -> (Document, ObjectId) {
    let mut doc = Document::new([400.0, 300.0], Units::Pt, 72.0, 1);
    let story_fill = solid(10, 80, 200);
    doc.stories.push(story_with_fill(1, "Hello", story_fill.clone()));
    doc.next_story_id = 2;

    let shape_id = ObjectId(1);
    let mut shape = Object {
        id: shape_id,
        name: "shape".into(),
        pos: [8.0, 9.0],
        size: [40.0, 30.0],
        fill: solid(200, 40, 10),
        stroke: StrokeStyle {
            paint: solid(0, 0, 0),
            width: 2.0,
            ..StrokeStyle::default()
        },
        page_id: Some(doc.pages[0].id),
        ..Object::default()
    };
    shape.kind = ObjectKind::Shape;

    let table_id = ObjectId(3);
    let table = Object {
        id: table_id,
        name: "table".into(),
        pos: [10.0, 80.0],
        size: [80.0, 20.0],
        kind: ObjectKind::Table(table_with_cell_fill(solid(16, 32, 48), 0)),
        page_id: Some(doc.pages[0].id),
        ..Object::default()
    };

    let pixel_id = ObjectId(4);
    let pixel = Object {
        id: pixel_id,
        name: "pixels".into(),
        pos: [200.0, 10.0],
        size: [2.0, 2.0],
        raster_size: Some((2, 2)),
        raster_data: Some(vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255]),
        page_id: Some(doc.pages[0].id),
        ..Object::default()
    };

    doc.objects.push(shape);
    let text_id = ObjectId(2);
    doc.objects.push(Object {
        id: text_id,
        name: "frame".into(),
        pos: [50.0, 10.0],
        size: [120.0, 40.0],
        kind: ObjectKind::FrameText {
            story: dtool_core::document::StoryId(1),
        },
        page_id: Some(doc.pages[0].id),
        ..Object::default()
    });
    doc.objects.push(table);
    doc.objects.push(pixel);
    doc.next_object_id = 5;

    doc.pixel_layers.insert(pixel_id, PixelLayer::from_rgba(2, 2, &[
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
    ]));
    doc.layer_masks.push(LayerMask::new(pixel_id, PixelSelection::all(2, 2)));

    let resource = ImageResource {
        id: ResourceId(0),
        name: "dot".into(),
        embedded: true,
        format: ImageFormat::Png,
        pixel_size: [1, 1],
        data: Some(vec![137, 80, 78, 71, 13, 10, 26, 10]),
        ..ImageResource::default()
    };
    let resource_id = doc.add_image_resource(resource);
    doc.place_image(shape_id, resource_id).expect("place image");

    (doc, shape_id)
}

fn assert_solid(paint: &Paint, r: u8, g: u8, b: u8) {
    match paint {
        Paint::Solid { colour } => {
            assert_eq!((colour.r, colour.g, colour.b), (r, g, b));
        }
        other => panic!("expected solid paint, got {other:?}"),
    }
}

#[test]
fn failed_tmp_rename_keeps_existing_file() {
    let path = unique_path("failed_rename");
    let (doc, _) = sample_document();
    save(&doc, &path).unwrap();
    let original = std::fs::read(&path).unwrap();
    let tmp = temp_path(&path);
    let _ = std::fs::remove_file(&tmp);
    std::fs::create_dir(&tmp).unwrap();
    let err = save(&doc, &path).expect_err("dir at tmp should block write");
    assert_ne!(err.kind, IoErrorKind::NotFound);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    cleanup(&path);
}

#[test]
fn leftover_temp_file_is_removed_on_open() {
    let path = unique_path("stale_tmp");
    let (doc, _) = sample_document();
    save(&doc, &path).unwrap();
    let tmp = temp_path(&path);
    std::fs::write(&tmp, b"stale-partial").unwrap();
    assert!(tmp.is_file());
    let loaded = open(&path).unwrap();
    assert_eq!(loaded.objects.len(), doc.objects.len());
    assert!(!tmp.exists());
    cleanup(&path);
}

#[test]
fn corrupt_or_truncated_file_is_invalid_format() {
    let path = unique_path("corrupt");
    std::fs::write(&path, b"{").unwrap();
    let err = open(&path).expect_err("truncated json");
    assert_eq!(err.kind, IoErrorKind::InvalidFormat);
    assert_eq!(err.code(), "INVALID_FORMAT");
    assert_eq!(std::fs::read(&path).unwrap(), b"{");

    std::fs::write(&path, b"{\"version\":0}").unwrap();
    let err = open(&path).expect_err("missing required keys");
    assert_eq!(err.kind, IoErrorKind::InvalidFormat);
    cleanup(&path);
}

#[test]
fn missing_file_is_not_found_and_creates_nothing() {
    let path = unique_path("missing");
    cleanup(&path);
    let err = open(&path).expect_err("missing");
    assert_eq!(err.kind, IoErrorKind::NotFound);
    assert!(!path.exists());
    assert!(!temp_path(&path).exists());
}

#[test]
fn paint_story_table_and_ledger_survive_native_roundtrip() {
    let path = unique_path("roundtrip");
    let (doc, shape_id) = sample_document();
    let resource_id = doc.placed_images[0].resource;
    let next_resource = doc.next_resource_id;
    save(&doc, &path).unwrap();
    let loaded = open(&path).unwrap();

    let shape = loaded.get_object(shape_id).unwrap();
    assert_solid(&shape.fill, 200, 40, 10);
    assert_solid(&shape.stroke.paint, 0, 0, 0);
    assert_eq!(shape.stroke.width, 2.0);

    let story = loaded.stories.iter().find(|s| s.id.0 == 1).unwrap();
    let fill = range_fill(&story.ranges[0]).unwrap();
    assert_solid(&fill, 10, 80, 200);
    assert_eq!(story.text, "Hello");

    let table = loaded
        .objects
        .iter()
        .find_map(|o| match &o.kind {
            ObjectKind::Table(table) => Some(table),
            _ => None,
        })
        .unwrap();
    assert_solid(table.cells[0].local.fill.as_ref().unwrap(), 16, 32, 48);
    assert_eq!(table.alloc.high_water, table.cells.iter().map(|c| c.id.0).max().unwrap());

    let pixel_id = ObjectId(4);
    let layer = loaded.pixel_layers.get(pixel_id).unwrap();
    assert_eq!((layer.width, layer.height), (2, 2));
    let mask = loaded.layer_masks.iter().find(|m| m.object == pixel_id).unwrap();
    assert_eq!(mask.coverage.width, 2);
    let resource = loaded.image_resource(resource_id).unwrap();
    assert_eq!(resource.pixel_size, [1, 1]);
    assert_eq!(resource.data.as_ref().map(|d| d.len()), Some(8));
    assert_eq!(loaded.next_resource_id, next_resource);
    assert_eq!(loaded.next_story_id, doc.next_story_id.max(2));
    assert_eq!(loaded.next_object_id, doc.next_object_id.max(5));
    cleanup(&path);
}

#[test]
fn object_fill_undo_then_native_reopen_restores_paint() {
    let path = unique_path("undo_paint");
    let (doc, shape_id) = sample_document();
    let mut workspace = Workspace::default();
    let ctx = workspace.new_document("k05-paint");
    workspace.get_mut(ctx).unwrap().session = Session::new(doc.clone());
    let before = workspace
        .get(ctx)
        .unwrap()
        .session
        .document
        .get_object(shape_id)
        .unwrap()
        .fill
        .clone();
    let (_, result) = workspace.run(
        Scope::of(ctx),
        Command::with_params(
            "object.update",
            json!({
                "id": shape_id.0,
                "fill": solid(1, 2, 3)
            }),
        ),
    );
    result.expect("object.update");
    assert_solid(
        &workspace
            .get(ctx)
            .unwrap()
            .session
            .document
            .get_object(shape_id)
            .unwrap()
            .fill,
        1,
        2,
        3,
    );
    let (_, undone) = workspace.run(Scope::of(ctx), Command::with_params("edit.undo", json!({})));
    undone.expect("edit.undo");
    let restored = workspace
        .get(ctx)
        .unwrap()
        .session
        .document
        .get_object(shape_id)
        .unwrap()
        .fill
        .clone();
    assert_eq!(restored, before);
    save(
        &workspace.get(ctx).unwrap().session.document,
        &path,
    )
    .unwrap();
    let loaded = open(&path).unwrap();
    assert_eq!(loaded.get_object(shape_id).unwrap().fill, before);
    cleanup(&path);
}

struct PrefsGuard {
    previous: ColourPrefs,
}

impl Drop for PrefsGuard {
    fn drop(&mut self) {
        if let Ok(mut global) = dtool_core::colour::services().write() {
            global.preferences = self.previous.clone();
        }
    }
}

fn guard_convert_prefs(convert_on_open: bool) -> PrefsGuard {
    let mut global = dtool_core::colour::services().write().unwrap();
    let bytes = global.profiles.find(SRGB).unwrap().bytes.to_vec();
    let _ = global.profiles.insert("Old RGB".to_string(), bytes);
    let previous = global.preferences.clone();
    global.preferences.rgb = SRGB.to_string();
    global.preferences.convert_on_open = convert_on_open;
    global.preferences.warn_on_convert = false;
    PrefsGuard { previous }
}

#[test]
fn convert_on_open_reports_dirty_and_leaves_disk_unchanged() {
    let _guard = guard_convert_prefs(true);
    let path = unique_path("convert");
    let mut doc = Document::new([200.0, 100.0], Units::Pt, 72.0, 1);
    doc.colour.profile = "Old RGB".to_string();
    doc.objects.push(Object {
        id: ObjectId(3),
        fill: solid(200, 40, 10),
        ..Object::default()
    });
    doc.next_object_id = 4;
    save(&doc, &path).unwrap();
    let on_disk = std::fs::read(&path).unwrap();
    let outcome: OpenOutcome = open_with_status(&path).unwrap();
    assert!(outcome.converted);
    assert_eq!(outcome.file_version, doc.version);
    assert_eq!(outcome.document.version, doc.version + 1);
    assert_eq!(outcome.document.colour.profile, SRGB);
    assert_eq!(std::fs::read(&path).unwrap(), on_disk);
    assert!(outcome.report.as_ref().unwrap()["action"] == "converted");
    cleanup(&path);
}

#[test]
fn save_with_extras_keeps_unknown_keys_plain_save_drops_them() {
    let path = unique_path("unknown");
    let doc = Document::new([100.0, 100.0], Units::Pt, 72.0, 1);
    let mut raw = serde_json::to_value(&doc).unwrap();
    raw.as_object_mut()
        .unwrap()
        .insert("forward_field".into(), json!(123));
    std::fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();
    let outcome = open_with_status(&path).unwrap();
    assert_eq!(outcome.unknown_fields["forward_field"], 123);
    save_with_extras(&outcome.document, &outcome.unknown_fields, &path).unwrap();
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(kept["forward_field"], 123);
    save(&outcome.document, &path).unwrap();
    let dropped: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(dropped.get("forward_field").is_none());
    cleanup(&path);
}

#[test]
fn cleanup_stale_temp_ignores_directory_blockers() {
    let path = unique_path("tmp_dir");
    let tmp = temp_path(&path);
    std::fs::create_dir(&tmp).unwrap();
    cleanup_stale_temp(&path);
    assert!(tmp.is_dir());
    let _ = std::fs::remove_dir(&tmp);
}
