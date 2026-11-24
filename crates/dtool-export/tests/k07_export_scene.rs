use dtool_core::document::{Document, Story, Units};
use dtool_core::object::{Object, ObjectKind};
use dtool_core::paint::{Colour, Paint};
use dtool_core::pixel::PixelLayer;
use dtool_core::StoryId;
use dtool_export::raster::{area_region, render_preview, target_size, ExportOptions};
use dtool_export::{DocumentExportExt, ExportFormat};

#[test]
fn export_raster_reads_scene_image_and_text_box() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 72.0, 1);
    let pixels = vec![0u8, 255, 0, 255].repeat(4 * 4);
    let id = doc.create_object(Object {
        pos: [0.0, 0.0],
        size: [40.0, 40.0],
        raster_data: Some(pixels.clone()),
        raster_size: Some((4, 4)),
        ..Object::default()
    });
    doc.pixel_layers.insert(id, PixelLayer::from_rgba(4, 4, &pixels));
    let story = StoryId(1);
    doc.stories.push(Story { id: story, text: "ABC".into(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() });
    doc.create_object(Object {
        pos: [50.0, 10.0],
        size: [40.0, 20.0],
        kind: ObjectKind::FrameText { story },
        fill: Paint::Solid { colour: Colour::new(0, 0, 0, 255) },
        ..Object::default()
    });
    let rgba = doc.render_rgba_region((0.0, 0.0, 100.0, 100.0), 50, 50);
    assert_eq!(rgba.len(), 50 * 50 * 4);
    assert!(rgba.chunks(4).any(|px| px[3] > 0), "expected visible pixels");
}

#[test]
fn preview_matches_scene_raster() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 72.0, 1);
    let pixels = vec![0u8, 255, 0, 255].repeat(4 * 4);
    let id = doc.create_object(Object {
        pos: [0.0, 0.0],
        size: [40.0, 40.0],
        raster_data: Some(pixels.clone()),
        raster_size: Some((4, 4)),
        ..Object::default()
    });
    doc.pixel_layers.insert(id, PixelLayer::from_rgba(4, 4, &pixels));
    let story = StoryId(1);
    doc.stories.push(Story { id: story, text: "ABC".into(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() });
    doc.create_object(Object {
        pos: [50.0, 10.0],
        size: [40.0, 20.0],
        kind: ObjectKind::FrameText { story },
        fill: Paint::Solid { colour: Colour::new(0, 0, 0, 255) },
        ..Object::default()
    });
    let opts = ExportOptions::for_format(ExportFormat::Png);
    let preview = render_preview(&doc, &opts).expect("preview");
    let region = area_region(&doc, &opts.area).expect("region");
    let (width, height) = target_size(region, doc.dpi, &opts).expect("size");
    let rgba = doc.render_rgba_region(region, width as usize, height as usize);
    assert_eq!(preview.dimensions(), (width, height));
    assert_eq!(preview.as_raw(), &rgba);
}

#[test]
fn mixed_typography_table_and_image_agree_between_scene_and_export() {
    // The native application installs the renderer before building its first scene.
    assert!(dtool_render::scene::install());
    assert!(dtool_export::install_text_decorations());
    let mut doc = Document::new([120.0, 80.0], Units::Px, 72.0, 1);
    let pixels = vec![255u8, 0, 0, 255].repeat(8 * 8);
    let image_id = doc.create_object(Object {
        pos: [0.0, 0.0],
        size: [32.0, 32.0],
        raster_data: Some(pixels.clone()),
        raster_size: Some((8, 8)),
        ..Object::default()
    });
    doc.pixel_layers.insert(image_id, PixelLayer::from_rgba(8, 8, &pixels));

    let frame_story = StoryId(1);
    doc.stories.push(Story { id: frame_story, text: "Hi".into(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() });
    doc.create_object(Object {
        pos: [40.0, 4.0],
        size: [40.0, 16.0],
        kind: ObjectKind::FrameText { story: frame_story },
        fill: Paint::Solid { colour: Colour::new(0, 0, 0, 255) },
        ..Object::default()
    });

    let art_story = StoryId(2);
    doc.stories.push(Story { id: art_story, text: "Art".into(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() });
    doc.create_object(Object {
        pos: [84.0, 4.0],
        size: [30.0, 16.0],
        kind: ObjectKind::ArtText { story: art_story },
        fill: Paint::Solid { colour: Colour::new(0, 0, 120, 255) },
        ..Object::default()
    });

    let mut table: dtool_core::table::model::TableNode = serde_json::from_value(serde_json::json!({
        "col_widths_pt": [40.0],
        "row_heights_pt": [16.0],
        "cells": [{
            "id": 1,
            "row": 0,
            "col": 0,
            "row_span": 1,
            "col_span": 1,
            "story": 0
        }]
    })).expect("table");
    table.cells[0].local.fill = Some(Paint::Solid { colour: Colour::new(0, 180, 0, 255) });
    table.cells[0].mask.0 |= dtool_core::table::resolve::CELL_FILL;
    let table_id = doc.create_object(Object {
        pos: [40.0, 36.0],
        size: [40.0, 16.0],
        kind: ObjectKind::Table(table),
        fill: Paint::Solid { colour: Colour::new(0, 180, 0, 255) },
        ..Object::default()
    });

    let flags = dtool_core::CanvasFlags::default();
    let scene = dtool_core::scene::build_scene(&doc, &flags);
    assert!(scene.prims.iter().any(|prim| matches!(prim, dtool_core::scene::Prim::Image { object, .. } if *object == image_id)));
    assert!(scene.prims.iter().any(|prim| matches!(prim, dtool_core::scene::Prim::Text { .. })));
    assert!(scene.quads.iter().any(|(id, _)| *id == table_id), "table must be on the scene: {:?}", scene.quads);

    let bounds = [0.0, 0.0, 120.0, 80.0];
    let from_scene = dtool_core::scene::rasterize_scene(&doc, &scene, bounds, 60, 40, None);
    let from_export = doc.render_rgba_region((0.0, 0.0, 120.0, 80.0), 60, 40);
    assert_eq!(from_scene.len(), 60 * 40 * 4);
    assert_eq!(from_scene, from_export, "export raster must reuse the scene rasterizer for typography, table, and image");
    assert!(from_scene.chunks(4).any(|px| px[0] > 200 && px[3] > 0), "image red");
    assert!(from_scene.chunks(4).any(|px| px[3] > 0), "mixed scene must produce visible pixels");
}
