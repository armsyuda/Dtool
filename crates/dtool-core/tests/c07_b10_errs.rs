use dtool_core::colour::ProfileStore;
use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::{Command, CommandResult};
use serde_json::{json, Value};
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c07_b10_errs_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn fresh() -> Workspace {
    dtool_core::palette::set_app_path(Some(tmp("app_palettes.json")));
    let mut workspace = Workspace::default();
    workspace.new_document("c07_b10_errs");
    workspace
}

fn call(workspace: &mut Workspace, name: &str, params: Value) -> CommandResult {
    workspace.run(Scope::active(), Command::with_params(name, params)).1
}

#[test]
fn c07_b10_soft_proof_unknown_profile_is_refused_without_state_change() {
    let mut workspace = fresh();
    call(&mut workspace, "ui.soft_proof.set", json!({ "enabled": true, "profile": ProfileStore::FOGRA39L, "intent": "perceptual" })).unwrap();
    let before = workspace.session().unwrap().view.clone();
    let error = call(&mut workspace, "ui.soft_proof.set", json!({ "enabled": true, "profile": "No such profile", "intent": "saturation", "gamut_check": true })).unwrap_err();
    assert_eq!(error.code, "COLOUR_PROFILE_MISSING");
    let after = workspace.session().unwrap().view.clone();
    assert_eq!(after.soft_proof_profile, before.soft_proof_profile);
    assert_eq!(after.soft_proof_intent, before.soft_proof_intent);
    assert_eq!(after.soft_proof_gamut_check, before.soft_proof_gamut_check);
    assert!(after.soft_proof_enabled);
    call(&mut workspace, "ui.soft_proof.set", json!({ "enabled": false, "profile": "No such profile" })).unwrap();
    assert!(!workspace.session().unwrap().view.soft_proof_enabled);
}

#[test]
fn c07_b10_palette_export_afpalette_is_unsupported_and_writes_nothing() {
    let mut workspace = fresh();
    call(&mut workspace, "palette.create", json!({ "name": "Brand" })).unwrap();
    call(&mut workspace, "swatch.add", json!({ "name": "Red", "colour": "#FF0000", "palette": "Brand" })).unwrap();
    for (path, params) in [
        (tmp("brand_fmt.afpalette"), json!({ "format": "afpalette" })),
        (tmp("brand_ext.afpalette"), json!({})),
    ] {
        let _ = std::fs::remove_file(&path);
        let mut params = params;
        params["path"] = json!(path);
        params["scope"] = json!("document");
        params["palette"] = json!("Brand");
        let error = call(&mut workspace, "palette.export", params).unwrap_err();
        assert_eq!(error.code, "UNSUPPORTED_FORMAT");
        assert!(!path.exists(), "{path:?}");
    }
    let json_path = tmp("brand.json");
    let ok = call(&mut workspace, "palette.export", json!({ "path": json_path, "format": "json", "scope": "document", "palette": "Brand" })).unwrap();
    assert_eq!(ok["count"], 1);
    assert!(json_path.exists());
}