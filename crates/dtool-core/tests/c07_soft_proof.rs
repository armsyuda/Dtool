use dtool_core::colour::{proof, Colour, ProfileStore, SoftProof};

fn saturated_green() -> Colour {
    Colour::rgb(0.0, 1.0, 0.0, 1.0)
}

fn fogra_proof() -> SoftProof {
    SoftProof {
        profile: ProfileStore::FOGRA39L.to_string(),
        intent: dtool_core::colour::RenderingIntent::RelativeColorimetric,
        bpc: true,
        gamut_check: false,
    }
}

#[test]
fn c07_soft_proof() {
    let input = saturated_green();
    let proofed = proof::apply_soft_proof(&input, &fogra_proof()).expect("FOGRA39L proof");
    assert_ne!(proofed.values, input.values, "saturated RGB green must shift under a CMYK soft proof");
    assert_eq!(proofed.alpha, input.alpha);
    assert_eq!(proofed.model, dtool_core::colour::ColourModel::Rgb);

    let missing = SoftProof {
        profile: "no-such-proof-profile".to_string(),
        ..fogra_proof()
    };
    let err = proof::apply_soft_proof(&input, &missing).expect_err("missing profile must error");
    assert_eq!(err.code, "COLOUR_PROFILE_MISSING");

    let empty = SoftProof { profile: String::new(), ..SoftProof::default() };
    let empty_err = proof::apply_soft_proof(&input, &empty).expect_err("empty profile must not no-op");
    assert_eq!(empty_err.code, "COLOUR_PROFILE_MISSING");
}

#[test]
fn c07_soft_proof_set_stores_per_view_state() {
    use dtool_core::command::Session;
    use dtool_core::document::{Document, Units};
    use dtool_protocol::Command;
    use serde_json::json;

    let mut session = Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1));
    let result = proof::run(
        &mut session,
        &Command::with_params(
            "ui.soft_proof.set",
            json!({ "enabled": true, "profile": ProfileStore::FOGRA39L, "intent": "relative_colorimetric", "gamut_check": true }),
        ),
    )
    .expect("routed")
    .expect("set ok");
    assert_eq!(result["enabled"], true);
    assert_eq!(result["profile"], ProfileStore::FOGRA39L);
    assert_eq!(result["gamut_check"], true);
    assert!(session.view.soft_proof_enabled);
    assert_eq!(session.view.soft_proof_profile, ProfileStore::FOGRA39L);
    assert!(session.view.soft_proof_gamut_check);
    let from_view = proof::proof_from_view(&session.view);
    let proofed = proof::apply_soft_proof(&saturated_green(), &from_view).expect("view proof");
    assert_ne!(proofed.values, saturated_green().values);
}
