use dtool_core::colour::swatch::Swatch;
use dtool_core::colour::{Colour as Managed, ColourFormat, ColourModel};
use dtool_core::document::{Document, Units};
use dtool_core::paint::Colour;
use dtool_core::{Command, Session};
use serde_json::json;

fn grey(level: f32) -> Colour {
    Colour::from_managed(Managed { model: ColourModel::Grey, values: [level, 0.0, 0.0, 0.0], alpha: 1.0 })
}

#[test]
fn c07_convert_keeps_spot_and_counts_skipped_with_one_undo() {
    let mut document = Document::new([100.0, 100.0], Units::Pt, 72.0, 1);
    let mut spot = Swatch::new(1, "PANTONE-like spot", Colour::new(200, 30, 40, 255));
    spot.spot = true;
    document.swatches.push(spot.clone());
    document.swatches.push(Swatch::new(2, "process", Colour::new(10, 120, 200, 255)));
    let mut registration = Swatch::new(4, "registration", Colour::new(0, 0, 0, 255));
    registration.registration = true;
    document.swatches.push(registration.clone());
    let mut stray = Swatch::new(3, "grey with no source profile", grey(40.0));
    stray.global = false;
    document.swatches.push(stray.clone());
    let mut s = Session::new(document);
    let before = serde_json::to_value(&s.document.swatches).unwrap();
    let depth = s.undo.applied_steps();
    let reply = s.run(&Command::with_params("color.profile.convert", json!({ "format": ColourFormat::LAB_16.to_string(), "profile": "LAB D50" }))).expect("convert");
    assert_eq!(reply["spot_kept"], 1);
    assert_eq!(reply["registration_kept"], 1);
    assert_eq!(s.document.swatches[2], registration);
    assert_eq!(reply["skipped"], 1, "{reply}");
    assert!(reply["converted"].as_u64().unwrap() >= 1);
    assert_eq!(serde_json::to_value(&s.document.swatches[0]).unwrap(), serde_json::to_value(&spot).unwrap());
    assert_eq!(s.document.swatches[1].colour.model, ColourModel::Lab);
    assert_eq!(s.document.swatches[3], stray);
    assert_eq!(s.undo.applied_steps(), depth + 1);
    s.run(&Command::with_params("edit.undo", json!({}))).expect("undo");
    assert_eq!(serde_json::to_value(&s.document.swatches).unwrap(), before);
}