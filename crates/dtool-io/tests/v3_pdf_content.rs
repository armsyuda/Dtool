use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dtool_core::colour::ColourModel;
use dtool_core::object::ObjectKind;
use dtool_core::paint::Paint;
use dtool_core::resources::ImageFormat;
use dtool_io::IoErrorKind;
use dtool_io::import_pdf::{
    FontReplacement, ImportAs, ImportColour, ImportFormat, ImportOptions, PageSel, admit,
    import_pdf,
};
use dtool_io::import_report::LossKind;
use lopdf::encryption::{
    EncryptionState, EncryptionVersion, Permissions,
    crypt_filters::{Aes128CryptFilter, CryptFilter},
};
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, dictionary};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn save(pdf: &mut Pdf) -> Self {
        let directory = std::env::temp_dir().join("opencode");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!(
            "v3_pdf_content_{}_{}.pdf",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        pdf.save(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn pdf_with_pages(
    pages: &[([i64; 4], &[u8])],
    resources: Dictionary,
) -> (Pdf, Vec<ObjectId>, ObjectId) {
    let mut pdf = Pdf::with_version("1.7");
    let root = pdf.new_object_id();
    let resources = pdf.add_object(resources);
    let mut ids = Vec::new();
    for (media, bytes) in pages {
        let content = pdf.add_object(Stream::new(Dictionary::new(), bytes.to_vec()));
        ids.push(pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => root, "MediaBox" => media.iter().map(|v| Object::Integer(*v)).collect::<Vec<_>>(),
            "Contents" => content,
        }));
    }
    pdf.objects.insert(root, Object::Dictionary(dictionary! {
        "Type" => "Pages", "Kids" => ids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
        "Count" => ids.len() as i64, "Resources" => resources,
    }));
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
    pdf.trailer.set("Root", catalog);
    (pdf, ids, root)
}

fn font_resources() -> Dictionary {
    dictionary! { "Font" => dictionary! { "F1" => dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
        "FirstChar" => 32, "Widths" => vec![Object::Integer(600); 95],
    } } }
}

fn image(bytes: Vec<u8>, space: &str, width: i64, height: i64) -> Stream {
    Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => width, "Height" => height,
        "ColorSpace" => space, "BitsPerComponent" => 8 },
        bytes,
    )
}

fn close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(expected) {
        assert!(
            (a - e).abs() < 0.02,
            "actual {actual:?}, expected {expected:?}"
        );
    }
}

fn assert_counts(doc: &dtool_core::Document, report: &dtool_io::ImportReport) {
    let content = doc.objects.iter().filter(|o| !o.is_group() && o.clip_role.is_none()).count();
    assert_eq!(
        content,
        (report.objects + report.texts + report.images) as usize
    );
    assert_eq!(
        report.texts as usize,
        doc.objects
            .iter()
            .filter(|o| o.story_id().is_some())
            .count()
    );
    assert_eq!(
        report.images as usize,
        doc.objects
            .iter()
            .filter(|o| o.raster_data.is_some())
            .count()
    );
}

#[test]
fn rectangle_and_bezier_have_real_geometry_and_paint() {
    let (mut pdf, _, _) = pdf_with_pages(
        &[(
            [0, 0, 200, 200],
            b"0.1 0.2 0.3 0.4 k 1 0 0 RG 2 w 10 20 80 40 re B 10 100 m 20 130 40 130 50 100 c S",
        )],
        Dictionary::new(),
    );
    let file = Fixture::save(&mut pdf);
    let original = std::fs::read(&file.0).unwrap();
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    assert_eq!(doc.pages[0].size, [200.0, 200.0]);
    let rect = doc
        .objects
        .iter()
        .find(|o| o.name == "rect_10_140")
        .unwrap();
    close(&rect.pos, &[10.0, 140.0]);
    close(&rect.size, &[80.0, 40.0]);
    assert_eq!(rect.stroke.width, 2.0);
    let Paint::Solid { colour } = rect.fill else {
        panic!("missing fill")
    };
    assert_eq!(colour.model, ColourModel::Cmyk);
    close(&colour.values, &[10.0, 20.0, 30.0, 40.0]);
    let curve = doc.objects.iter().find_map(|o| o.curve()).unwrap();
    assert_eq!(curve.contour.points.len(), 2);
    assert!(curve.contour.points[0].handle_out.is_some());
    assert!(curve.contour.points[1].handle_in.is_some());
    assert_eq!(report.objects, 2);
    assert_counts(&doc, &report);
    assert!(
        dtool_io::import_pdf::fidelity_check(
            &doc,
            &dtool_io::import_pdf::FidelityManifest {
                objects: vec![("rect_10_140".into(), [10.0, 140.0, 80.0, 40.0])],
                texts: vec![],
                colours: vec!["CMYK(0.1, 0.2, 0.3, 0.4)".into()],
            },
            0.01
        )
        .is_empty()
    );
    assert!(report.is_acceptable(), "{:?}", report.losses);
    assert_eq!(std::fs::read(&file.0).unwrap(), original);
}

#[test]
fn graphics_stack_transform_and_even_odd_contours_are_mapped() {
    let (mut pdf, _, _) = pdf_with_pages(&[([0, 0, 200, 200], b"q 2 0 0 2 10 20 cm 10 20 30 40 re f Q 0 0 10 10 re f 60 60 50 50 re 70 70 10 10 re f*")], Dictionary::new());
    let file = Fixture::save(&mut pdf);
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    let rect = doc.objects.iter().find(|o| o.name == "rect_30_60").unwrap();
    close(&rect.pos, &[30.0, 60.0]);
    close(&rect.size, &[60.0, 80.0]);
    assert!(doc.objects.iter().any(|o| o.name == "rect_0_190"));
    let compound = doc.objects.iter().find(|o| o.curve().is_some()).unwrap();
    let curve = compound.curve().unwrap();
    assert_eq!(curve.holes.len(), 1);
    assert!(curve.contour.closed);
    assert_eq!(compound.fill_rule, dtool_core::boolean::FillRule::EvenOdd);
    assert!(report.is_acceptable(), "{:?}", report.losses);
    assert_counts(&doc, &report);
}

#[test]
fn text_creates_story_and_style_ranges_not_font_inventory_counts() {
    let bytes = b"BT /F1 12 Tf 0 0 1 rg 1 0 0 1 10 150 Tm (Hello PDF) Tj ET";
    let (mut pdf, _, _) = pdf_with_pages(&[([0, 0, 200, 200], bytes)], font_resources());
    let file = Fixture::save(&mut pdf);
    let opts = ImportOptions {
        fonts: vec![FontReplacement {
            missing: "Helvetica".into(),
            family: "Test Family".into(),
            style: "Bold".into(),
        }],
        ..ImportOptions::default()
    };
    let (doc, report) = import_pdf(&file.0, &opts).unwrap();
    assert_eq!(doc.stories.len(), 1);
    assert_eq!(doc.stories[0].text, "Hello PDF");
    assert_eq!(doc.stories[0].ranges[0]["family"], "Test Family");
    assert_eq!(doc.stories[0].ranges[0]["style"], "Bold");
    assert_eq!(doc.stories[0].ranges[0]["size_pt"], 12.0);
    let text = doc.objects.iter().find(|o| o.story_id().is_some()).unwrap();
    assert_eq!(text.story_id(), Some(doc.stories[0].id));
    close(&text.pos, &[10.0, 40.4]);
    close(&text.size, &[64.8, 12.0]);
    assert_eq!(text.fill, Paint::None);
    assert_eq!(doc.stories[0].ranges[0]["fill"], dtool_core::serde_json::json!({
        "type": "solid",
        "colour": { "r": 0, "g": 0, "b": 255, "a": 255 }
    }));
    assert_eq!(report.texts, 1);
    assert_counts(&doc, &report);
    assert!(
        report
            .losses
            .iter()
            .any(|l| l.kind == LossKind::Font && l.required)
    );
    assert!(!report.is_acceptable());
}

#[test]
fn type0_to_unicode_decodes_actual_korean_text() {
    let (mut pdf, _, root) = pdf_with_pages(
        &[(
            [0, 0, 200, 200],
            b"BT /F1 12 Tf 1 0 0 1 10 100 Tm <000100020003> Tj ET",
        )],
        Dictionary::new(),
    );
    let cmap = pdf.add_object(Stream::new(Dictionary::new(), b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /Test def /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange 3 beginbfchar <0001> <D55C> <0002> <AE00> <0003> <0041> endbfchar endcmap CMapName currentdict /CMap defineresource pop end end".to_vec()));
    let widths = vec![1.into(), Object::Array(vec![630.into()]), 2.into(), 2.into(), 870.into()];
    let font = pdf.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "KoreanTest",
        "Encoding" => "Identity-H", "ToUnicode" => cmap,
        "DescendantFonts" => vec![Object::Dictionary(dictionary! { "Type" => "Font", "Subtype" => "CIDFontType2", "DW" => 930, "W" => widths })] });
    pdf.get_dictionary_mut(root).unwrap().set(
        "Resources",
        dictionary! { "Font" => dictionary! { "F1" => font } },
    );
    let file = Fixture::save(&mut pdf);
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    assert_eq!(doc.stories[0].text, "한글A");
    assert_eq!(doc.stories[0].ranges[0]["end"], 3);
    let object = doc.objects.iter().find(|o| o.story_id().is_some()).unwrap();
    close(&object.size, &[29.16, 12.0]);
    assert_eq!(report.texts, 1);
    assert_counts(&doc, &report);
    assert!(
        !report
            .losses
            .iter()
            .any(|l| l.detail.contains("fallback font metrics"))
    );
}

#[test]
fn rgb_image_has_rgba_pixels_resource_and_placement() {
    let (mut pdf, _, root) = pdf_with_pages(
        &[([0, 0, 200, 200], b"q 40 0 0 20 10 30 cm /Im1 Do Q")],
        Dictionary::new(),
    );
    let image = pdf.add_object(image(vec![255, 0, 0, 0, 0, 255], "DeviceRGB", 2, 1));
    pdf.get_dictionary_mut(root).unwrap().set(
        "Resources",
        dictionary! { "XObject" => dictionary! { "Im1" => image } },
    );
    let file = Fixture::save(&mut pdf);
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    let obj = doc
        .objects
        .iter()
        .find(|o| o.raster_data.is_some())
        .unwrap();
    close(&obj.pos, &[10.0, 150.0]);
    close(&obj.size, &[40.0, 20.0]);
    assert_eq!(obj.raster_size, Some((2, 1)));
    assert_eq!(
        obj.raster_data.as_deref(),
        Some([255, 0, 0, 255, 0, 0, 255, 255].as_slice())
    );
    assert!(doc.pixel_layers.get(obj.id).is_some());
    let resource = doc
        .image_resource(doc.placed_resource(obj.id).unwrap())
        .unwrap();
    assert!(resource.embedded);
    assert_eq!(resource.format, ImageFormat::Bmp);
    assert_eq!(resource.pixel_size, [2, 1]);
    let bytes = resource.data.as_ref().unwrap();
    assert_eq!(&bytes[..2], b"BM");
    assert_eq!(bytes.len(), 62);
    assert_eq!(&bytes[54..], &[0, 0, 255, 255, 255, 0, 0, 255]);
    assert_eq!(report.images, 1);
    assert_counts(&doc, &report);
    assert!(report.is_acceptable());
}

#[test]
fn flate_grey_image_and_decode_array_are_applied() {
    let (mut pdf, _, root) = pdf_with_pages(
        &[([0, 0, 50, 50], b"20 0 0 20 0 0 cm /Im1 Do")],
        Dictionary::new(),
    );
    let mut stream = image(vec![0; 256], "DeviceGray", 16, 16);
    stream
        .dict
        .set("Decode", vec![Object::Integer(1), Object::Integer(0)]);
    stream.compress().unwrap();
    assert!(stream.dict.has(b"Filter"));
    let image = pdf.add_object(stream);
    pdf.get_dictionary_mut(root).unwrap().set(
        "Resources",
        dictionary! { "XObject" => dictionary! { "Im1" => image } },
    );
    let file = Fixture::save(&mut pdf);
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    let rgba = doc
        .objects
        .iter()
        .find_map(|o| o.raster_data.as_ref())
        .unwrap();
    assert_eq!(rgba.len(), 16 * 16 * 4);
    assert!(rgba.iter().all(|v| *v == 255));
    assert_counts(&doc, &report);
}

#[test]
fn unsupported_images_are_required_losses_without_placeholder_objects() {
    for mode in ["codec", "bits", "huge", "truncated"] {
        let (mut pdf, _, root) = pdf_with_pages(
            &[([0, 0, 50, 50], b"0 0 5 5 re f /Im1 Do")],
            Dictionary::new(),
        );
        let mut stream = image(vec![255], "DeviceRGB", 1, 1);
        match mode {
            "codec" => stream.dict.set("Filter", "DCTDecode"),
            "bits" => stream.dict.set("BitsPerComponent", 1),
            "huge" => stream.dict.set("Width", i32::MAX as i64),
            _ => {}
        }
        let image = pdf.add_object(stream);
        pdf.get_dictionary_mut(root).unwrap().set(
            "Resources",
            dictionary! { "XObject" => dictionary! { "Im1" => image } },
        );
        let file = Fixture::save(&mut pdf);
        let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
        assert_eq!(report.images, 0, "{mode}");
        assert!(doc.resources.is_empty());
        assert_eq!(report.objects, 1);
        assert!(
            report
                .losses
                .iter()
                .any(|l| l.kind == LossKind::Image && l.required)
        );
        assert!(!report.is_acceptable());
        assert_counts(&doc, &report);
    }
}

#[test]
fn unsupported_only_body_is_an_error_not_an_empty_success() {
    let (mut pdf, _, _) = pdf_with_pages(&[([0, 0, 50, 50], b"/Sh1 sh")], Dictionary::new());
    let file = Fixture::save(&mut pdf);
    let error = import_pdf(&file.0, &ImportOptions::default()).unwrap_err();
    assert_eq!(error.kind, IoErrorKind::UnsupportedProfile);
    assert_eq!(error.message_key, "error.unsupported_profile");
}

#[test]
fn inherited_boxes_rotation_and_user_unit_determine_selected_page_size() {
    let (mut pdf, pages, root) = pdf_with_pages(
        &[
            ([0, 0, 100, 100], b""),
            ([0, 0, 300, 200], b"10 20 30 40 re f"),
        ],
        Dictionary::new(),
    );
    pdf.get_dictionary_mut(pages[1])
        .unwrap()
        .remove(b"MediaBox");
    pdf.get_dictionary_mut(root)
        .unwrap()
        .set("MediaBox", vec![0.into(), 0.into(), 300.into(), 200.into()]);
    pdf.get_dictionary_mut(root).unwrap().set(
        "CropBox",
        vec![10.into(), 20.into(), 210.into(), 120.into()],
    );
    pdf.get_dictionary_mut(root).unwrap().set("Rotate", 90);
    pdf.get_dictionary_mut(pages[1]).unwrap().set("UserUnit", 2);
    let file = Fixture::save(&mut pdf);
    let opts = ImportOptions {
        pages: PageSel::Range(2, 2),
        dpi: 144,
        import_as: ImportAs::Artboards,
        colour: ImportColour::Cmyk,
        ..ImportOptions::default()
    };
    let (doc, report) = import_pdf(&file.0, &opts).unwrap();
    assert_eq!(doc.pages.len(), 1);
    close(&doc.pages[0].size, &[200.0, 400.0]);
    assert_eq!(doc.dpi, 144.0);
    assert_eq!(doc.colour.format.model, ColourModel::Cmyk);
    assert!(doc.objects.iter().any(|o| o.name == "Artboard 1"));
    let rect = doc.objects.iter().find(|o| !o.is_group()).unwrap();
    close(&rect.pos, &[0.0, 0.0]);
    close(&rect.size, &[80.0, 60.0]);
    assert_counts(&doc, &report);
}

#[test]
fn mixed_page_sizes_use_cumulative_non_overlapping_origins() {
    let bytes = b"0 0 10 10 re f";
    let (mut pdf, _, _) = pdf_with_pages(
        &[
            ([0, 0, 100, 100], bytes),
            ([0, 0, 150, 90], bytes),
            ([0, 0, 40, 50], bytes),
        ],
        Dictionary::new(),
    );
    let file = Fixture::save(&mut pdf);
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    assert_eq!(
        doc.pages.iter().map(|p| p.size).collect::<Vec<_>>(),
        [[100.0, 100.0], [150.0, 90.0], [40.0, 50.0]]
    );
    let groups: Vec<_> = doc.objects.iter().filter(|o| o.is_group()).collect();
    assert_eq!(
        groups.iter().map(|o| o.pos).collect::<Vec<_>>(),
        [[0.0, 0.0], [100.0, 0.0], [250.0, 0.0]]
    );
    for group in groups {
        assert!(doc.objects.iter().any(|o| o.parent_id == Some(group.id)));
    }
    assert_eq!(report.objects, 3);
    assert_counts(&doc, &report);
}

#[test]
fn rectangular_clip_and_normal_alpha_are_kept() {
    let resources = dictionary! { "ExtGState" => dictionary! { "GS1" => dictionary! { "ca" => 0.5, "BM" => "Normal" } } };
    let (mut pdf, _, _) = pdf_with_pages(
        &[(
            [0, 0, 100, 100],
            b"q 10 20 30 40 re W n /GS1 gs 0 0 100 100 re f Q 0 0 5 5 re f",
        )],
        resources,
    );
    let file = Fixture::save(&mut pdf);
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    let clip = doc.objects.iter().find(|o| o.name == "clip").unwrap();
    let mask = doc
        .objects
        .iter()
        .find(|o| o.parent_id == Some(clip.id) && !o.clip_role.is_none())
        .unwrap();
    assert_eq!(mask.clip_role, dtool_core::object::ClipRole::Mask);
    assert_eq!(mask.pos, [10.0, 40.0]);
    assert_eq!(mask.size, [30.0, 40.0]);
    let clipped = doc
        .objects
        .iter()
        .find(|o| o.parent_id == Some(clip.id) && o.clip_role.is_none())
        .unwrap();
    assert_eq!(clipped.opacity, 0.5);
    let restored = doc.objects.iter().find(|o| o.name == "rect_0_95").unwrap();
    assert_ne!(restored.parent_id, Some(clip.id));
    assert_eq!(restored.opacity, 1.0);
    assert_counts(&doc, &report);
    assert!(report.is_acceptable());
}

#[test]
fn form_xobject_uses_its_matrix_bbox_and_resources() {
    let (mut pdf, _, root) = pdf_with_pages(&[([0, 0, 200, 200], b"/Fm1 Do")], Dictionary::new());
    let form = pdf.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 40.into(), 40.into()],
            "Matrix" => vec![1.into(), 0.into(), 0.into(), 1.into(), 30.into(), 40.into()],
            "Resources" => font_resources(),
        },
        b"1 0 0 rg 0 0 20 10 re f BT /F1 10 Tf 1 0 0 1 0 20 Tm (Form) Tj ET".to_vec(),
    ));
    pdf.get_dictionary_mut(root).unwrap().set(
        "Resources",
        dictionary! { "XObject" => dictionary! { "Fm1" => form } },
    );
    let file = Fixture::save(&mut pdf);
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    let rect = doc
        .objects
        .iter()
        .find(|o| o.name == "rect_30_150")
        .unwrap();
    close(&rect.pos, &[30.0, 150.0]);
    close(&rect.size, &[20.0, 10.0]);
    assert_eq!(doc.stories[0].text, "Form");
    assert_eq!(report.objects, 1);
    assert_eq!(report.texts, 1);
    assert_eq!(report.images, 0);
    assert_counts(&doc, &report);
}

#[test]
fn editable_line_grouping_preserves_each_run_style() {
    let content = b"BT /F1 10 Tf 1 0 0 1 10 150 Tm 1 0 0 rg (Red) Tj 0 0 1 rg (Blue) Tj 0 -20 Td (Line) Tj ET";
    let (mut pdf, _, _) = pdf_with_pages(&[([0, 0, 200, 200], content)], font_resources());
    let file = Fixture::save(&mut pdf);
    let opts = ImportOptions {
        favour_editable_text: true,
        group_text_lines: true,
        ..ImportOptions::default()
    };
    let (doc, report) = import_pdf(&file.0, &opts).unwrap();
    assert_eq!(doc.stories[0].text, "RedBlue\nLine");
    assert_eq!(doc.stories[0].ranges.len(), 3);
    assert_ne!(
        doc.stories[0].ranges[0]["fill"],
        doc.stories[0].ranges[1]["fill"]
    );
    assert!(
        doc.objects
            .iter()
            .any(|o| matches!(o.kind, ObjectKind::FrameText { .. }))
    );
    assert_eq!(report.texts, 1);
    assert_counts(&doc, &report);
    assert!(
        report
            .losses
            .iter()
            .any(|l| l.required && l.detail.contains("reflows"))
    );
}

#[test]
fn unsupported_paints_blends_masks_and_operators_are_required_losses() {
    let resources = dictionary! { "ExtGState" => dictionary! { "GS1" => dictionary! { "BM" => "UnrecognizedBlend", "SMask" => dictionary! { "S" => "Luminosity" } } } };
    let content = b"/GS1 gs 0 0 m 50 0 l 0 50 l h W n 0 0 20 20 re f /Sh1 sh /Pattern cs /P1 scn 30 30 20 20 re f 1 2 unsupported";
    let (mut pdf, _, _) = pdf_with_pages(&[([0, 0, 100, 100], content)], resources);
    let file = Fixture::save(&mut pdf);
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    for kind in [
        LossKind::Blend,
        LossKind::Mask,
        LossKind::Gradient,
        LossKind::Pattern,
        LossKind::Other,
    ] {
        assert!(
            report.losses.iter().any(|l| l.kind == kind && l.required),
            "{:?}",
            report.losses
        );
    }
    assert!(!report.is_acceptable());
    assert_counts(&doc, &report);
}

#[test]
fn unused_resources_are_not_counted_and_annotations_are_reported() {
    let (mut pdf, pages, _) = pdf_with_pages(&[([0, 0, 100, 100], b"")], font_resources());
    pdf.get_dictionary_mut(pages[0]).unwrap().set(
        "Annots",
        vec![
            Object::Dictionary(dictionary! { "Subtype" => "Text" }),
            Object::Dictionary(dictionary! { "Subtype" => "Link" }),
        ],
    );
    let file = Fixture::save(&mut pdf);
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    assert_eq!((report.objects, report.texts, report.images), (0, 0, 0));
    let annotation = report
        .losses
        .iter()
        .find(|l| l.kind == LossKind::Annotation)
        .unwrap();
    assert_eq!(annotation.count, 2);
    assert!(!annotation.required);
    assert_counts(&doc, &report);
}

#[test]
fn broken_content_or_unknown_stream_codec_never_becomes_blank_success() {
    for mode in ["syntax", "dangling", "codec"] {
        let (mut pdf, pages, _) = pdf_with_pages(
            &[([0, 0, 100, 100], b"10 20 30 40 re f (unterminated")],
            Dictionary::new(),
        );
        let contents = pdf
            .get_dictionary(pages[0])
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap();
        if mode == "dangling" {
            pdf.objects.remove(&contents);
        }
        if mode == "codec" {
            pdf.get_object_mut(contents)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .dict
                .set("Filter", "UnsupportedDecode");
        }
        let file = Fixture::save(&mut pdf);
        assert!(
            import_pdf(&file.0, &ImportOptions::default()).is_err(),
            "{mode}"
        );
    }
}

#[test]
fn invalid_ranges_and_invalid_page_boxes_are_rejected() {
    let (mut pdf, pages, _) = pdf_with_pages(&[([0, 0, 100, 100], b"")], Dictionary::new());
    let file = Fixture::save(&mut pdf);
    for pages in [
        PageSel::Range(0, 1),
        PageSel::Range(1, 2),
        PageSel::Range(2, 1),
    ] {
        let opts = ImportOptions {
            pages,
            ..ImportOptions::default()
        };
        assert_eq!(
            import_pdf(&file.0, &opts).unwrap_err().kind,
            IoErrorKind::InvalidFormat
        );
    }
    pdf.get_dictionary_mut(pages[0])
        .unwrap()
        .set("MediaBox", vec![0.into(), 0.into(), 0.into(), 100.into()]);
    let file = Fixture::save(&mut pdf);
    assert!(import_pdf(&file.0, &ImportOptions::default()).is_err());
}

#[test]
fn aes128_password_is_required_and_decrypted_body_is_imported() {
    let (mut pdf, _, _) = pdf_with_pages(
        &[([0, 0, 100, 100], b"10 20 30 40 re f")],
        Dictionary::new(),
    );
    pdf.trailer.set(
        "ID",
        vec![
            Object::string_literal("v3-content-id-01"),
            Object::string_literal("v3-content-id-01"),
        ],
    );
    let crypt_filter: Arc<dyn CryptFilter> = Arc::new(Aes128CryptFilter);
    let state = EncryptionState::try_from(EncryptionVersion::V4 {
        document: &pdf,
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), crypt_filter)]),
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: "fixture-owner",
        user_password: "fixture-user",
        permissions: Permissions::all(),
    })
    .unwrap();
    pdf.encrypt(&state).unwrap();
    let file = Fixture::save(&mut pdf);
    let original = std::fs::read(&file.0).unwrap();
    let profile = admit(&file.0, ImportFormat::Pdf).unwrap();
    assert!(profile.encrypted);
    assert!(!profile.supported);
    let missing = import_pdf(&file.0, &ImportOptions::default()).unwrap_err();
    assert_eq!(missing.kind, IoErrorKind::Permission);
    assert_eq!(missing.message_key, "error.password_required");
    let wrong = ImportOptions {
        password: Some("wrong".into()),
        ..ImportOptions::default()
    };
    assert_eq!(
        import_pdf(&file.0, &wrong).unwrap_err().message_key,
        "error.wrong_password"
    );
    let right = ImportOptions {
        password: Some("fixture-user".into()),
        ..ImportOptions::default()
    };
    let (doc, report) = import_pdf(&file.0, &right).unwrap();
    assert_eq!(report.objects, 1);
    assert_counts(&doc, &report);
    assert_eq!(std::fs::read(&file.0).unwrap(), original);
}

#[test]
fn one_page_combines_paths_stories_and_images_and_survives_serialization() {
    let content = b"0.5 g 10 20 30 40 re f BT /F1 10 Tf 1 0 0 1 10 90 Tm (Body) Tj ET q 10 0 0 10 80 0 cm /Im1 Do Q";
    let (mut pdf, _, root) = pdf_with_pages(&[([0, 0, 100, 100], content)], font_resources());
    let im = pdf.add_object(image(vec![100, 150, 200], "DeviceRGB", 1, 1));
    let resource = pdf
        .get_dictionary(root)
        .unwrap()
        .get(b"Resources")
        .unwrap()
        .as_reference()
        .unwrap();
    pdf.get_dictionary_mut(resource)
        .unwrap()
        .set("XObject", dictionary! { "Im1" => im });
    let file = Fixture::save(&mut pdf);
    let original = std::fs::read(&file.0).unwrap();
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    assert_eq!((report.objects, report.texts, report.images), (1, 1, 1));
    let shape = doc.objects.iter().find(|o| o.name == "rect_10_40").unwrap();
    let Paint::Solid { colour } = shape.fill else {
        panic!("missing grey fill")
    };
    assert_eq!(colour.model, ColourModel::Grey);
    assert_eq!(colour.values[0], 50.0);
    assert_eq!(colour.r, 128);
    let encoded = serde_json::to_vec(&doc).unwrap();
    let restored: dtool_core::Document = serde_json::from_slice(&encoded).unwrap();
    assert_counts(&restored, &report);
    assert_eq!(restored.stories[0].text, "Body");
    assert!(
        restored.resources[0]
            .data
            .as_ref()
            .unwrap()
            .starts_with(b"BM")
    );
    assert!(
        restored
            .objects
            .iter()
            .any(|o| o.raster_data.as_deref() == Some(&[100, 150, 200, 255]))
    );
    assert_eq!(std::fs::read(&file.0).unwrap(), original);
}

#[test]
fn content_stream_arrays_preserve_graphics_state_across_streams() {
    let (mut pdf, pages, _) = pdf_with_pages(&[([0, 0, 100, 100], b"")], Dictionary::new());
    let first = pdf.add_object(Stream::new(
        Dictionary::new(),
        b"q 1 0 0 1 10 20 cm 1 0 0 rg".to_vec(),
    ));
    let second = pdf.add_object(Stream::new(Dictionary::new(), b"0 0 20 10 re f Q".to_vec()));
    let contents = pdf.add_object(vec![Object::Reference(first), Object::Reference(second)]);
    pdf.get_dictionary_mut(pages[0])
        .unwrap()
        .set("Contents", contents);
    let file = Fixture::save(&mut pdf);
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    let rect = doc.objects.iter().find(|o| !o.is_group()).unwrap();
    close(&rect.pos, &[10.0, 70.0]);
    close(&rect.size, &[20.0, 10.0]);
    assert_counts(&doc, &report);
    assert!(report.is_acceptable());
}

#[test]
fn missing_single_byte_glyph_is_not_silently_deleted() {
    let content = b"BT /F1 12 Tf 1 0 0 1 10 40 Tm <410042> Tj ET";
    let (mut pdf, _, _) = pdf_with_pages(&[([0, 0, 100, 100], content)], font_resources());
    let file = Fixture::save(&mut pdf);
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    assert_eq!(doc.stories[0].text, "A\u{fffd}B");
    assert!(
        report
            .losses
            .iter()
            .any(|l| l.kind == LossKind::Glyph && l.required)
    );
    assert_counts(&doc, &report);
}

#[test]
fn colour_key_masks_are_applied_and_unsupported_inline_images_are_reported() {
    let mut content = b"q 10 0 0 10 0 0 cm /Im1 Do Q BI /W 1 /H 1 /BPC 8 /CS /RGB ID ".to_vec();
    content.extend([255, 0, 0]);
    content.extend(b" EI\n");
    let (mut pdf, _, root) = pdf_with_pages(&[([0, 0, 100, 100], &content)], Dictionary::new());
    let mut stream = image(vec![1, 2, 3], "DeviceRGB", 1, 1);
    stream.dict.set(
        "Mask",
        vec![1.into(), 1.into(), 2.into(), 2.into(), 3.into(), 3.into()],
    );
    let im = pdf.add_object(stream);
    pdf.get_dictionary_mut(root).unwrap().set(
        "Resources",
        dictionary! { "XObject" => dictionary! { "Im1" => im } },
    );
    let file = Fixture::save(&mut pdf);
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    assert_eq!(report.images, 1);
    let rgba = doc.objects.iter().find_map(|o| o.raster_data.as_deref()).unwrap();
    assert_eq!(rgba, &[1, 2, 3, 0]);
    assert!(!report.losses.iter().any(|l| l.kind == LossKind::Mask));
    assert!(
        report
            .losses
            .iter()
            .any(|l| l.kind == LossKind::Image && l.detail.contains("inline") && l.required)
    );
    assert_counts(&doc, &report);
}

#[test]
fn independent_fill_and_stroke_alpha_are_preserved() {
    let resources = dictionary! { "ExtGState" => dictionary! { "GS1" => dictionary! { "ca" => 0.5, "CA" => 0.25 } } };
    let (mut pdf, _, _) =
        pdf_with_pages(&[([0, 0, 100, 100], b"/GS1 gs 0 0 10 10 re B")], resources);
    let file = Fixture::save(&mut pdf);
    let (doc, report) = import_pdf(&file.0, &ImportOptions::default()).unwrap();
    let obj = doc.objects.iter().find(|o| !o.is_group()).unwrap();
    let (Paint::Solid { colour: fill }, Paint::Solid { colour: stroke }) =
        (&obj.fill, &obj.stroke.paint)
    else {
        panic!("missing paints")
    };
    assert_eq!(fill.a, 128);
    assert_eq!(stroke.a, 64);
    assert_eq!(obj.opacity, 1.0);
    assert!(report.is_acceptable());
    assert_counts(&doc, &report);
}

#[test]
fn only_selected_pages_are_decoded() {
    let (mut pdf, _, _) = pdf_with_pages(
        &[
            ([0, 0, 50, 50], b"(unterminated"),
            ([0, 0, 100, 200], b"10 20 30 40 re f"),
        ],
        Dictionary::new(),
    );
    let file = Fixture::save(&mut pdf);
    assert!(import_pdf(&file.0, &ImportOptions::default()).is_err());
    let opts = ImportOptions {
        pages: PageSel::Range(2, 2),
        ..ImportOptions::default()
    };
    let (doc, report) = import_pdf(&file.0, &opts).unwrap();
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(doc.pages[0].size, [100.0, 200.0]);
    assert_eq!(report.objects, 1);
    assert_counts(&doc, &report);
}
