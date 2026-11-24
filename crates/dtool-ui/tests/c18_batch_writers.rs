#[path = "support/test_home.rs"]
mod test_home;
use dtool_export::ExportFormat;
use dtool_export::batch::{self, BatchAction, BatchManifest, BatchOutput, BatchStatus, Destination, FileStatus, OverwritePolicy};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 6,
    0, 0, 0, 114, 182, 13, 36, 0, 0, 0, 17, 73, 68, 65, 84, 120, 156, 99, 16, 209, 136, 250,
    15, 194, 12, 48, 6, 0, 51, 32, 6, 85, 123, 239, 145, 110, 0, 0, 0, 0, 73, 69, 78, 68, 174,
    66, 96, 130,
];

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_root(tag: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("dtool-c18w-{tag}-{}-{nanos}-{seq}", std::process::id()));
    fs::create_dir_all(root.join("in")).unwrap();
    fs::create_dir_all(root.join("out")).unwrap();
    root
}

fn output(root: &Path, format: ExportFormat, name: &str, lossy: bool, size: Option<(u32, u32)>) -> BatchOutput {
    BatchOutput {
        format,
        width: size.map(|s| s.0),
        height: size.map(|s| s.1),
        keep_aspect: false,
        dest: Destination::Folder(root.join("out")),
        name: name.into(),
        overwrite: OverwritePolicy::Fail,
        webp_lossy: lossy,
    }
}

fn run(manifest: BatchManifest) -> batch::BatchSnapshot {
    let handle = batch::prepare(manifest).unwrap();
    batch::start(&handle).unwrap()
}

fn png_size(bytes: &[u8]) -> (u32, u32) {
    size_of(bytes, ExportFormat::Png)
}

fn size_of(bytes: &[u8], format: ExportFormat) -> (u32, u32) {
    dtool_export::raster::decode(bytes, format).unwrap().dimensions()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn every_offered_format_writes_its_own_encoding_of_the_input() {
    let _home = test_home::enter("every_offered_format_writes_its_own_encoding_of_the_input");
    let root = temp_root("all");
    let source = root.join("in").join("a.png");
    fs::write(&source, PNG).unwrap();
    let mut manifest = BatchManifest::default();
    manifest.add(source.clone()).unwrap();
    for (format, name, lossy) in [
        (ExportFormat::Png, "{name}.{format}", false),
        (ExportFormat::Jpeg, "{name}.{format}", false),
        (ExportFormat::Webp, "{name}_lossless.{format}", false),
        (ExportFormat::Webp, "{name}_lossy.{format}", true),
        (ExportFormat::Svg, "{name}.{format}", false),
        (ExportFormat::Pdf, "{name}.{format}", false),
        (ExportFormat::Eps, "{name}.{format}", false),
    ] {
        manifest.outputs.push(output(&root, format, name, lossy, None));
    }
    let snap = run(manifest);
    assert_eq!(snap.status, BatchStatus::Done);
    let file = &snap.files[0];
    assert_eq!(file.status, FileStatus::Done, "{file:?}");
    assert_eq!(file.outputs.len(), 7);
    for receipt in &file.outputs {
        assert!(receipt.complete);
        assert_eq!(receipt.bytes, fs::metadata(&receipt.path).unwrap().len());
    }
    let out = root.join("out");
    assert_eq!(fs::read(out.join("a.png")).unwrap(), PNG);
    let jpeg = fs::read(out.join("a.jpg")).unwrap();
    assert!(jpeg.starts_with(&[255, 216, 255]));
    assert_eq!(size_of(&jpeg, ExportFormat::Jpeg), (2, 2));
    let lossless = fs::read(out.join("a_lossless.webp")).unwrap();
    assert!(lossless.starts_with(b"RIFF") && &lossless[8..12] == b"WEBP" && contains(&lossless, b"VP8L"));
    assert_eq!(size_of(&lossless, ExportFormat::Webp), (2, 2));
    let lossy = fs::read(out.join("a_lossy.webp")).unwrap();
    assert!(lossy.starts_with(b"RIFF") && &lossy[8..12] == b"WEBP" && contains(&lossy, b"VP8 "));
    let svg = String::from_utf8(fs::read(out.join("a.svg")).unwrap()).unwrap();
    assert!(svg.contains("<svg") && svg.contains("width=\"2\"") && svg.contains("data:image/png;base64,"));
    let pdf = fs::read(out.join("a.pdf")).unwrap();
    assert!(pdf.starts_with(b"%PDF-") && contains(&pdf, b"/MediaBox [0 0 2 2]") && contains(&pdf, b"/Subtype /Image") && pdf.ends_with(b"%%EOF\n"));
    let eps = String::from_utf8(fs::read(out.join("a.eps")).unwrap()).unwrap();
    assert!(eps.starts_with("%!PS-Adobe-3.0 EPSF-3.0") && eps.contains("%%BoundingBox: 0 0 2 2") && eps.contains("colorimage"));
    assert_eq!(fs::read(&source).unwrap(), PNG);
}

#[test]
fn resize_action_and_output_size_change_the_written_pixels() {
    let _home = test_home::enter("resize_action_and_output_size_change_the_written_pixels");
    let root = temp_root("resize");
    let source = root.join("in").join("a.png");
    fs::write(&source, PNG).unwrap();
    let mut manifest = BatchManifest::default();
    manifest.add(source.clone()).unwrap();
    manifest.actions.push(BatchAction { command: "document.resize".into(), params: json!({"width": 6, "height": 4}) });
    manifest.outputs.push(output(&root, ExportFormat::Png, "{name}_action.{format}", false, None));
    manifest.outputs.push(output(&root, ExportFormat::Png, "{name}_sized.{format}", false, Some((3, 3))));
    manifest.outputs.push(output(&root, ExportFormat::Pdf, "{name}_sized.{format}", false, Some((3, 3))));
    let snap = run(manifest);
    assert_eq!(snap.files[0].status, FileStatus::Done, "{:?}", snap.files[0]);
    let out = root.join("out");
    assert_eq!(png_size(&fs::read(out.join("a_action.png")).unwrap()), (6, 4));
    assert_eq!(png_size(&fs::read(out.join("a_sized.png")).unwrap()), (3, 3));
    assert!(contains(&fs::read(out.join("a_sized.pdf")).unwrap(), b"/MediaBox [0 0 3 3]"));
    assert_eq!(fs::read(&source).unwrap(), PNG);
}

#[test]
fn undecodable_input_fails_without_writing_fake_outputs() {
    let _home = test_home::enter("undecodable_input_fails_without_writing_fake_outputs");
    let root = temp_root("bad");
    let source = root.join("in").join("a.png");
    fs::write(&source, b"not an image").unwrap();
    let mut manifest = BatchManifest::default();
    manifest.add(source).unwrap();
    manifest.outputs.push(output(&root, ExportFormat::Svg, "{name}.{format}", false, None));
    let snap = run(manifest);
    let file = &snap.files[0];
    assert_eq!(file.status, FileStatus::Failed);
    assert!(file.error.as_deref().unwrap_or("").contains("INPUT_DECODE_FAILED"), "{file:?}");
    assert!(!root.join("out").join("a.svg").exists());
}
