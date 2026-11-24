use std::path::PathBuf;

use dtool_export::cmyk::{
    cmyk_profile_licence_path, cmyk_profile_list, cmyk_profile_list_value, cmyk_profiles, default_cmyk_profile, find_cmyk_profile,
    resolve_cmyk_profile, CMYK_PROFILE_NAMES, DEFAULT_CMYK_PROFILE_ID,
};

fn repo_icc_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("dtool-core").join("assets").join("icc")
}

#[test]
fn c13_cmyk_list_has_fogra_default_and_adobe_swop_second() {
    let list = cmyk_profiles();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, DEFAULT_CMYK_PROFILE_ID);
    assert_eq!(default_cmyk_profile().icc_name, "FOGRA39L Coated");
    assert_eq!(list[1].icc_name, "U.S. Web Coated (SWOP) v2");
    assert_eq!(list[1].icc_file, "USWebCoatedSWOP.icc");
    assert_eq!(list[1].licence_file, "USWebCoatedSWOP.LICENSE.txt");
    assert!(list[1].vendor_supplied);
    assert!(!list[0].vendor_supplied);
    assert_eq!(CMYK_PROFILE_NAMES, ["FOGRA39L Coated", "U.S. Web Coated (SWOP) v2"]);
    for entry in list {
        assert!(entry.label_key.starts_with("colour.cmyk_profile."));
        assert!(entry.licence_key.starts_with("colour.cmyk_profile.licence."));
    }
    let defaults = cmyk_profile_list().into_iter().filter(|status| status.is_default).count();
    assert_eq!(defaults, 1);
}

#[test]
fn c13_cmyk_selection_by_id_name_or_file() {
    for key in ["us_web_coated_swop_v2", "U.S. Web Coated (SWOP) v2", "uswebcoatedswop.icc"] {
        assert_eq!(find_cmyk_profile(key).map(|entry| entry.id), Some("us_web_coated_swop_v2"), "{key}");
    }
    assert_eq!(find_cmyk_profile("FOGRA39L Coated").map(|entry| entry.id), Some("fogra39l_coated"));
    assert!(find_cmyk_profile("Coated FOGRA39").is_none());
    assert!(find_cmyk_profile("Japan Color 2001 Coated").is_none());
}

#[test]
fn c13_adobe_licence_text_ships_beside_the_profile() {
    let path = repo_icc_dir().join("USWebCoatedSWOP.LICENSE.txt");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("COLOR PROFILE BUNDLING AGREEMENT"));
    assert!(text.contains("https://www.adobe.com/support/downloads/iccprofiles/icc_eula_win_dist.html"));
    assert!(text.contains("EXHIBIT B"));
    assert!(text.contains("U.S. Web Coated (SWOP) v2"));
    assert!(!text.starts_with('\u{feff}'));
    assert_eq!(cmyk_profile_licence_path("us_web_coated_swop_v2").map(|found| found.file_name().unwrap().to_owned()), Some("USWebCoatedSWOP.LICENSE.txt".into()));
}

#[test]
fn c13_missing_profile_is_reported_not_substituted_then_found_when_supplied() {
    if !repo_icc_dir().join("FOGRA39L_coated.icc").is_file() {
        let error = resolve_cmyk_profile(Some("fogra39l_coated")).unwrap_err();
        assert_eq!(error.code, "NOT_FOUND");
        assert_eq!(error.message_key, "error.colour.cmyk_profile_missing");
        let value = cmyk_profile_list_value();
        let fogra = value.as_array().unwrap().iter().find(|item| item["id"] == "fogra39l_coated").unwrap();
        assert_eq!(fogra["available"], false);
    }
    let error = resolve_cmyk_profile(Some("Not A Profile")).unwrap_err();
    assert_eq!(error.code, "NOT_FOUND");

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c13_icc_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("FOGRA39L_coated.icc"), b"supplied").unwrap();
    std::fs::write(dir.join("FOGRA39L_coated.LICENSE.txt"), b"CC0-1.0").unwrap();
    unsafe { std::env::set_var("DTOOL_ICC_DIR", &dir) };
    let found = resolve_cmyk_profile(None);
    unsafe { std::env::set_var("DTOOL_ICC_DIR", "") };
    assert_eq!(found.unwrap(), dir.join("FOGRA39L_coated.icc"));
}

#[test]
fn c13_bundled_adobe_swop_is_the_unmodified_adobe_profile() {
    let path = repo_icc_dir().join("USWebCoatedSWOP.icc");
    if !path.is_file() {
        return;
    }
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.len(), 557_168);
    assert_eq!(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize, bytes.len());
    assert_eq!(&bytes[4..8], b"ADBE");
    assert_eq!(&bytes[12..16], b"prtr");
    assert_eq!(&bytes[16..20], b"CMYK");
    assert_eq!(&bytes[36..40], b"acsp");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("U.S. Web Coated (SWOP) v2"));
    assert!(text.contains("Copyright 2000 Adobe Systems, Inc."));
    let resolved = resolve_cmyk_profile(Some("U.S. Web Coated (SWOP) v2")).unwrap();
    assert_eq!(resolved.file_name().unwrap(), "USWebCoatedSWOP.icc");
    let value = cmyk_profile_list_value();
    let swop = value.as_array().unwrap().iter().find(|item| item["id"] == "us_web_coated_swop_v2").unwrap();
    assert_eq!(swop["available"], true);
}

#[test]
fn c13_cmyk_tiff_export_embeds_the_selected_swop_profile() {
    use dtool_core::document::{Document, Units};
    use dtool_export::cmyk::{export_tiff_cmyk, CmykOptions};
    use dtool_export::{ExportArea, ExportControl, ExportFormat, ExportRequest};
    if !repo_icc_dir().join("USWebCoatedSWOP.icc").is_file() {
        return;
    }
    let doc = Document::new([16.0, 16.0], Units::Px, 96.0, 1);
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c13_swop_{}.tif", std::process::id()));
    let request = ExportRequest { format: ExportFormat::Tiff, area: ExportArea::WholeDocument, pages: None, path: out.clone(), preset: None, options: serde_json::json!({}) };
    let options = CmykOptions { profile: Some("us_web_coated_swop_v2".into()), ..CmykOptions::default() };
    let receipt = export_tiff_cmyk(&doc, &request, &options, &ExportControl::new()).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[0..4], b"II*\0");
    assert_eq!(receipt.bytes as usize, bytes.len());
    assert!(bytes.len() > 557_168 + 16 * 16 * 4);
    let icc = std::fs::read(repo_icc_dir().join("USWebCoatedSWOP.icc")).unwrap();
    assert!(bytes.windows(64).any(|window| window == &icc[128..192]));
}
