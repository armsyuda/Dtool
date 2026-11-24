use dtool_export::batch::{self, adjustment_params, apply_adjustments, Adjustments, BatchAction, BatchManifest, BatchOutput, BatchStatus, Destination, FileStatus, OverwritePolicy};
use dtool_export::ExportFormat;
use serde_json::json;
use std::fs;

fn home() -> std::path::PathBuf {
    let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("c18adj_{}", std::process::id()));
    unsafe {
        std::env::set_var("APPDATA", root.join("appdata"));
        std::env::set_var("LOCALAPPDATA", root.join("appdata"));
    }
    root
}

fn image(rgba: [u8; 4]) -> image::RgbaImage {
    image::RgbaImage::from_pixel(2, 2, image::Rgba(rgba))
}

#[test]
fn adjustment_contract_validates_keys_and_ranges() {
    assert_eq!(adjustment_params(&json!({ "brightness": 0.5 })), Some(Adjustments { brightness: 0.5, contrast: 0.0, saturation: 0.0, gamma: 1.0 }));
    for bad in [json!({}), json!({ "brightness": 2.0 }), json!({ "gamma": 0.0 }), json!({ "hue": 0.1 }), json!({ "contrast": "x" }), json!(null)] {
        assert!(adjustment_params(&bad).is_none(), "{bad}");
    }
    home();
    let mut manifest = BatchManifest::default();
    manifest.actions.push(BatchAction { command: "document.adjustments".into(), params: json!({ "hue": 1.0 }) });
    assert!(batch::validate_manifest(&manifest).iter().any(|p| p.code == "ACTION_PARAMS_INVALID"));
}

#[test]
fn pixel_math_brightness_contrast_saturation_gamma() {
    let mut img = image([100, 100, 100, 200]);
    apply_adjustments(&mut img, Adjustments { brightness: 0.2, contrast: 0.0, saturation: 0.0, gamma: 1.0 });
    assert_eq!(img.get_pixel(0, 0).0, [151, 151, 151, 200]);
    let mut grey = image([200, 50, 50, 255]);
    apply_adjustments(&mut grey, Adjustments { brightness: 0.0, contrast: 0.0, saturation: -1.0, gamma: 1.0 });
    let p = grey.get_pixel(0, 0).0;
    assert!(p[0] == p[1] && p[1] == p[2], "{p:?}");
    let mut flat = image([200, 60, 128, 255]);
    apply_adjustments(&mut flat, Adjustments { brightness: 0.0, contrast: -1.0, saturation: 0.0, gamma: 1.0 });
    assert_eq!(flat.get_pixel(0, 0).0, [128, 128, 128, 255]);
    let mut g = image([64, 64, 64, 255]);
    apply_adjustments(&mut g, Adjustments { brightness: 0.0, contrast: 0.0, saturation: 0.0, gamma: 2.0 });
    assert_eq!(g.get_pixel(0, 0).0[0], 128);
}

#[test]
fn batch_applies_document_adjustments_to_the_written_pixels() {
    let root = home().join("run");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("out")).unwrap();
    let source = root.join("in.png");
    image([100, 100, 100, 255]).save(&source).unwrap();
    let mut manifest = BatchManifest::default();
    manifest.add(source).unwrap();
    manifest.actions.push(BatchAction { command: "document.adjustments".into(), params: json!({ "brightness": 0.2 }) });
    manifest.outputs.push(BatchOutput { format: ExportFormat::Png, width: None, height: None, keep_aspect: false, dest: Destination::Folder(root.join("out")), name: "{name}.{format}".into(), overwrite: OverwritePolicy::Fail, webp_lossy: false });
    let handle = batch::prepare(manifest).unwrap();
    let snap = batch::start(&handle).unwrap();
    assert_eq!(snap.status, BatchStatus::Done);
    assert_eq!(snap.files[0].status, FileStatus::Done, "{:?}", snap.files[0]);
    let out = image::open(root.join("out").join("in.png")).unwrap().to_rgba8();
    assert_eq!(out.get_pixel(1, 1).0, [151, 151, 151, 255]);
}