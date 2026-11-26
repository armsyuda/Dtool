use dtool_core::commands::ui::register_export_action_for;
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_core::{Command, CommandResult, Session};
use dtool_export::svg::{export_svg, SvgOptions};
use dtool_export::{ExportArea, ExportControl, ExportFormat, ExportRequest};
use serde_json::{json, Value};
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c13_b10_svg_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn host(_id: Option<ContextId>, session: &Session, command: &Command) -> CommandResult {
    let options: SvgOptions = serde_json::from_value(command.params["svg"].clone()).map_err(|_| dtool_core::CommandError::invalid_params())?;
    let path = PathBuf::from(command.params["path"].as_str().unwrap());
    let request = ExportRequest { format: ExportFormat::Svg, area: ExportArea::WholeDocument, pages: None, path, preset: None, options: json!({}) };
    let receipt = export_svg(&session.document, &request, &options, &ExportControl::new())?;
    Ok(json!({ "ok": true, "receipt": format!("{receipt:?}") }))
}

fn call(workspace: &mut Workspace, options: &SvgOptions, path: &PathBuf) -> CommandResult {
    workspace.run(Scope::active(), Command::with_params("ui.export.open", json!({ "svg": options, "path": path }))).1
}

#[test]
fn c13_b10_svg_rejects_unapplied_options_before_writing_on_live_path() {
    assert!(register_export_action_for(host));
    let mut workspace = Workspace::default();
    workspace.new_document("c13_b10_svg");
    workspace.session_mut().unwrap().document.create_object(dtool_core::Object { size: [20.0, 20.0], ..Default::default() });
    let mut variants: Vec<(&str, Value)> = vec![
        ("embed_images", json!(false)),
        ("raster_dpi", json!(0)),
        ("flatten_transforms", json!(true)),
        ("tile_patterns", json!(true)),
        ("longer_text_spans", json!(true)),
        ("skip_slice_hidden", json!(false)),
    ];
    for (field, value) in variants.drain(..) {
        let mut options = serde_json::to_value(SvgOptions::default()).unwrap();
        options[field] = value;
        let options: SvgOptions = serde_json::from_value(options).unwrap();
        let path = tmp(&format!("bad_{field}.svg"));
        let _ = std::fs::remove_file(&path);
        let error = call(&mut workspace, &options, &path).unwrap_err();
        assert_eq!(error.code, "EXPORT_OPTION_UNAVAILABLE", "{field}");
        assert!(!path.exists(), "{field}");
    }
    let path = tmp("default.svg");
    assert!(call(&mut workspace, &SvgOptions::default(), &path).is_ok());
    assert!(std::fs::read_to_string(&path).unwrap().contains("<svg"));
}
