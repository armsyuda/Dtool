use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }

fn junk(extension: &str) -> Fixture {
    let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let path = PathBuf::from("C:/Users/armsy/AppData/Local/Temp/opencode").join(format!("dtool_c09fmt_{}_{sequence}.{extension}", std::process::id()));
    std::fs::write(&path, b"not-a-real-image-file").unwrap();
    Fixture(path)
}

fn assert_code(path: &std::path::Path, code: &str) {
    match dtool_io::image_import::decode_image(path) {
        Err(error) => assert_eq!(error.code, code, "{}", path.display()),
        Ok(_) => panic!("expected {code}"),
    }
}

#[test]
fn unenabled_formats_return_unsupported_profile() {
    let jxl = if cfg!(feature = "jxl") { "INVALID_FORMAT" } else { "UNSUPPORTED_PROFILE" };
    let heif = if cfg!(feature = "heif") { "INVALID_FORMAT" } else { "UNSUPPORTED_PROFILE" };
    assert_code(&junk("jxl").0, jxl);
    for extension in ["heic", "heif", "hif"] {
        assert_code(&junk(extension).0, heif);
    }
}

#[test]
fn enabled_image_formats_reject_junk_as_invalid_format() {
    for extension in ["bmp", "hdr", "exr"] {
        assert_code(&junk(extension).0, "INVALID_FORMAT");
    }
    assert_code(&junk("tga").0, "UNSUPPORTED_PROFILE");
}