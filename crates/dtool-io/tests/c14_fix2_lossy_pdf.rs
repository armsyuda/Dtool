use dtool_core::colour::ColourModel;
use dtool_core::paint::Paint;
use dtool_io::{import_pdf, ImportOptions, IoErrorKind, LossKind};
use std::path::PathBuf;

const SHADING: &str = "/Shading << /Sh1 << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 1 0] /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >>";

fn build_pdf(resources: &str, body: &str, annots: bool) -> Vec<u8> {
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << {} >> {}/Contents 4 0 R >>",
        resources,
        if annots { "/Annots [5 0 R] " } else { "" }
    );
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        page,
        format!("<< /Length {} >>\nstream\n{}\nendstream", body.len(), body),
        "<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /Contents (note) >>".to_string(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", objects.len() + 1, xref).as_bytes());
    out
}

fn write(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dtool_c14_fix2_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn lossy_only_page_opens_with_reported_losses() {
    let resources = format!("/ExtGState << /G1 << /BM /Multiply >> >> {}", SHADING);
    let path = write("lossy_only.pdf", &build_pdf(&resources, "/G1 gs /Sh1 sh", true));
    let (doc, report) = import_pdf(&path, &ImportOptions::default()).expect("lossy-only page opens");
    assert_eq!(report.objects, 1);
    for kind in [LossKind::Gradient, LossKind::Blend, LossKind::Annotation] {
        assert!(report.losses.iter().any(|loss| loss.kind == kind), "{kind:?} missing: {:?}", report.losses);
    }
    assert!(report.losses.iter().any(|loss| loss.kind == LossKind::Gradient && loss.required));
    let shapes: Vec<_> = doc.objects.iter().filter(|o| !o.is_group()).collect();
    assert_eq!(shapes.len(), 1);
    assert_eq!(shapes[0].pos, [0.0, 0.0]);
    assert_eq!(shapes[0].size, [200.0, 200.0]);
    let Paint::Solid { colour } = &shapes[0].fill else { panic!("shading fallback has no solid fill") };
    assert_eq!(colour.model, ColourModel::Rgb);
    assert!(colour.values[0] > colour.values[2], "{:?}", colour.values);
}

#[test]
fn clipped_shading_fills_only_the_clip() {
    let path = write("clipped.pdf", &build_pdf(SHADING, "q 20 30 50 40 re W n /Sh1 sh Q", false));
    let (doc, report) = import_pdf(&path, &ImportOptions::default()).expect("clipped shading opens");
    assert_eq!(report.objects, 1);
    let shape = doc.objects.iter().find(|o| !o.is_group()).expect("fallback shape");
    assert_eq!(shape.size, [50.0, 40.0]);
    assert_eq!(shape.pos[0], 20.0);
}

#[test]
fn shading_without_resource_is_still_rejected() {
    let path = write("missing.pdf", &build_pdf("", "/Sh1 sh", false));
    let error = import_pdf(&path, &ImportOptions::default()).unwrap_err();
    assert_eq!(error.kind, IoErrorKind::UnsupportedProfile);
}