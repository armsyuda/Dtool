use super::{Colour, ColourModel, ProfileStore, RenderingIntent, services};
use crate::command::Session;
use crate::view_state::ViewState;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::HashMap, sync::Arc};

pub const SOFT_PROOF_SET: &str = "ui.soft_proof.set";
const COLOUR_PROFILE_MISSING: &str = "COLOUR_PROFILE_MISSING";
const TRANSFORM_UNSUPPORTED: &str = "COLOUR_PROOF_UNSUPPORTED";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SoftProof {
    pub profile: String,
    pub intent: RenderingIntent,
    pub bpc: bool,
    pub gamut_check: bool,
}

impl Default for SoftProof {
    fn default() -> Self {
        Self {
            profile: String::new(),
            intent: RenderingIntent::RelativeColorimetric,
            bpc: true,
            gamut_check: false,
        }
    }
}

type Key = (Arc<[u8]>, Arc<[u8]>, Arc<[u8]>, RenderingIntent, RenderingIntent, bool, bool);
thread_local! {
    static CACHE: RefCell<HashMap<Key, lcms2::Transform<u8, u8>>> = RefCell::new(HashMap::new());
}

fn channels(model: ColourModel) -> usize {
    match model {
        ColourModel::Rgb | ColourModel::Lab => 3,
        ColourModel::Cmyk => 4,
        ColourModel::Grey => 1,
    }
}

fn format(model: ColourModel) -> lcms2::PixelFormat {
    match model {
        ColourModel::Rgb => lcms2::PixelFormat::RGB_FLT,
        ColourModel::Cmyk => lcms2::PixelFormat::CMYK_FLT,
        ColourModel::Lab => lcms2::PixelFormat::Lab_FLT,
        ColourModel::Grey => lcms2::PixelFormat::GRAY_FLT,
    }
}

fn lcms_intent(intent: RenderingIntent) -> lcms2::Intent {
    match intent {
        RenderingIntent::Perceptual => lcms2::Intent::Perceptual,
        RenderingIntent::RelativeColorimetric => lcms2::Intent::RelativeColorimetric,
        RenderingIntent::Saturation => lcms2::Intent::Saturation,
        RenderingIntent::AbsoluteColorimetric => lcms2::Intent::AbsoluteColorimetric,
    }
}

fn proof_flags(proof: &SoftProof) -> lcms2::Flags {
    let mut flags = lcms2::Flags::SOFT_PROOFING;
    if proof.gamut_check {
        flags = flags | lcms2::Flags::GAMUT_CHECK;
    }
    if proof.bpc {
        flags = flags | lcms2::Flags::BLACKPOINT_COMPENSATION;
    }
    flags
}

fn resolve_proof_profile<'a>(store: &'a ProfileStore, name: &str) -> Result<&'a crate::colour::IccProfile, CommandError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CommandError::new(COLOUR_PROFILE_MISSING));
    }
    if let Some(profile) = store.find(trimmed) {
        return Ok(profile);
    }
    let key = trimmed.to_ascii_lowercase().replace('_', " ").replace('-', " ");
    let key = key.split_whitespace().collect::<Vec<_>>().join(" ");
    if key == "fogra39l" || key == "fogra39l coated" || key == "fogra 39l" || key == "fogra39 l" {
        return store.find(ProfileStore::FOGRA39L).ok_or_else(|| CommandError::new(COLOUR_PROFILE_MISSING));
    }
    Err(CommandError::new(COLOUR_PROFILE_MISSING))
}

fn document_profile<'a>(store: &'a ProfileStore, colour: &Colour) -> Result<&'a crate::colour::IccProfile, CommandError> {
    let name = match colour.model {
        ColourModel::Rgb => ProfileStore::SRGB,
        ColourModel::Cmyk => ProfileStore::FOGRA39L,
        ColourModel::Lab => ProfileStore::LAB_D50,
        ColourModel::Grey => ProfileStore::GREY_D50,
    };
    store.find(name).ok_or_else(|| CommandError::new(COLOUR_PROFILE_MISSING))
}

fn display_profile<'a>(store: &'a ProfileStore) -> Result<&'a crate::colour::IccProfile, CommandError> {
    store.find(ProfileStore::SRGB).ok_or_else(|| CommandError::new(COLOUR_PROFILE_MISSING))
}

pub fn apply_soft_proof(colour: &Colour, proof: &SoftProof) -> Result<Colour, CommandError> {
    let services = services().read().map_err(|_| CommandError::new("INTERNAL"))?;
    apply_soft_proof_with(colour, proof, &services.profiles)
}

pub fn apply_soft_proof_with(colour: &Colour, proof: &SoftProof, store: &ProfileStore) -> Result<Colour, CommandError> {
    if !colour.alpha.is_finite() || !(0.0..=1.0).contains(&colour.alpha) || colour.values.iter().any(|value| !value.is_finite()) {
        return Err(CommandError::invalid_params());
    }
    let proof_icc = resolve_proof_profile(store, &proof.profile)?;
    let source = document_profile(store, colour)?;
    if source.model != colour.model {
        return Err(CommandError::new("PROFILE_MODEL_MISMATCH"));
    }
    let display = display_profile(store)?;
    let key = (
        source.bytes.clone(),
        display.bytes.clone(),
        proof_icc.bytes.clone(),
        proof.intent,
        RenderingIntent::RelativeColorimetric,
        proof.bpc,
        proof.gamut_check,
    );
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(&key) {
            let input = lcms2::Profile::new_icc(&source.bytes).map_err(|_| CommandError::new(TRANSFORM_UNSUPPORTED))?;
            let output = lcms2::Profile::new_icc(&display.bytes).map_err(|_| CommandError::new(TRANSFORM_UNSUPPORTED))?;
            let proofing = lcms2::Profile::new_icc(&proof_icc.bytes).map_err(|_| CommandError::new(TRANSFORM_UNSUPPORTED))?;
            let flags = proof_flags(proof);
            let transform = lcms2::Transform::new_proofing(
                &input,
                format(colour.model),
                &output,
                format(display.model),
                &proofing,
                lcms_intent(proof.intent),
                lcms_intent(RenderingIntent::RelativeColorimetric),
                flags,
            )
            .map_err(|_| CommandError::new(TRANSFORM_UNSUPPORTED))?;
            if cache.len() >= 16 {
                cache.clear();
            }
            cache.insert(key.clone(), transform);
        }
        let input: Vec<u8> = colour.values[..channels(colour.model)].iter().flat_map(|value| {
            let packed = if colour.model == ColourModel::Grey { *value / 100.0 } else { *value };
            packed.to_ne_bytes()
        }).collect();
        let mut output = vec![0u8; channels(display.model) * 4];
        cache[&key].transform_pixels(&input, &mut output);
        let mut values = [0.0; 4];
        for (index, bytes) in output.chunks_exact(4).enumerate() {
            values[index] = f32::from_ne_bytes(bytes.try_into().map_err(|_| CommandError::invalid_params())?);
            if display.model == ColourModel::Grey {
                values[index] *= 100.0;
            }
        }
        if values.iter().any(|value| !value.is_finite()) {
            return Err(CommandError::new(TRANSFORM_UNSUPPORTED));
        }
        Ok(Colour { model: display.model, values, alpha: colour.alpha })
    })
}

pub fn is_active(proof: &SoftProof) -> bool {
    !proof.profile.is_empty()
}

pub fn proof_from_view(view: &ViewState) -> SoftProof {
    SoftProof {
        profile: view.soft_proof_profile.clone(),
        intent: crate::colour::parse_intent(&view.soft_proof_intent).unwrap_or(RenderingIntent::RelativeColorimetric),
        bpc: view.soft_proof_bpc,
        gamut_check: view.soft_proof_gamut_check,
    }
}

fn bool_param(params: &Value, key: &str, fallback: bool) -> Result<bool, CommandError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(fallback),
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(CommandError::invalid_params()),
    }
}

pub fn set_from_params(view: &mut ViewState, params: &Value) -> Result<SoftProof, CommandError> {
    let profile = match params.get("profile") {
        None | Some(Value::Null) => view.soft_proof_profile.clone(),
        Some(Value::String(value)) => value.clone(),
        _ => return Err(CommandError::invalid_params()),
    };
    let enabled = match params.get("enabled") {
        None | Some(Value::Null) => {
            if params.get("profile").and_then(Value::as_str).is_some() {
                !profile.is_empty()
            } else {
                view.soft_proof_enabled
            }
        }
        Some(Value::Bool(value)) => *value,
        _ => return Err(CommandError::invalid_params()),
    };
    let resolved = if enabled && profile.is_empty() {
        ProfileStore::FOGRA39L.to_string()
    } else {
        profile
    };
    if enabled {
        let services = crate::colour::services().read().map_err(|_| CommandError::new("COLOUR_PROFILE_MISSING"))?;
        if services.profiles.find(&resolved).is_none() {
            return Err(CommandError::new("COLOUR_PROFILE_MISSING"));
        }
    }
    let intent = match params.get("intent") {
        None | Some(Value::Null) => {
            if view.soft_proof_intent.is_empty() {
                "relative_colorimetric".to_string()
            } else {
                view.soft_proof_intent.clone()
            }
        }
        Some(Value::String(value)) => {
            if crate::colour::parse_intent(value).is_none() && !value.is_empty() {
                return Err(CommandError::invalid_params());
            }
            value.clone()
        }
        _ => return Err(CommandError::invalid_params()),
    };
    let gamut_check = bool_param(params, "gamut_check", view.soft_proof_gamut_check)?;
    let bpc = bool_param(params, "bpc", view.soft_proof_bpc)?;
    view.soft_proof_enabled = enabled;
    view.soft_proof_profile = if enabled { resolved } else { String::new() };
    view.soft_proof_intent = intent;
    view.soft_proof_gamut_check = gamut_check;
    view.soft_proof_bpc = bpc;
    if enabled && view.soft_proof_profile.is_empty() {
        view.soft_proof_profile = ProfileStore::FOGRA39L.to_string();
    }
    Ok(proof_from_view(view))
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if command.name != SOFT_PROOF_SET {
        return None;
    }
    Some(session.has_document)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if command.name != SOFT_PROOF_SET {
        return None;
    }
    Some((|| {
        let proof = set_from_params(&mut session.view, &command.params)?;
        Ok(json!({
            "enabled": session.view.soft_proof_enabled,
            "profile": session.view.soft_proof_profile,
            "intent": session.view.soft_proof_intent,
            "gamut_check": session.view.soft_proof_gamut_check,
            "active": session.view.soft_proof_enabled && is_active(&proof)
        }))
    })())
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    run(session, command).map(|result| result.map(|_| ()))
}
