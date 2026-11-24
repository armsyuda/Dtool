use super::{Colour, ColourModel, ConvertCtx, ProfileStore, RenderingIntent};
use crate::CommandError;
use std::{cell::RefCell, collections::HashMap, sync::Arc};

type Key = (Arc<[u8]>, Arc<[u8]>, RenderingIntent, bool);
thread_local! { static CACHE: RefCell<HashMap<Key, lcms2::Transform<u8, u8>>> = RefCell::new(HashMap::new()); }

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

pub fn convert(c: &Colour, to_model: ColourModel, ctx: &ConvertCtx, store: &ProfileStore) -> Result<Colour, CommandError> {
    if !c.alpha.is_finite() || !(0.0..=1.0).contains(&c.alpha) || c.values.iter().any(|v| !v.is_finite()) {
        return Err(CommandError::invalid_params());
    }
    for index in 0..channels(c.model) {
        let (lo, hi) = match c.model {
            ColourModel::Rgb => (0.0, 1.0),
            ColourModel::Cmyk | ColourModel::Grey => (0.0, 100.0),
            ColourModel::Lab if index == 0 => (0.0, 100.0),
            ColourModel::Lab => (-128.0, 127.0),
        };
        if !(lo..=hi).contains(&c.values[index]) {
            return Err(CommandError::invalid_params());
        }
    }
    let from = store.find(&ctx.from).ok_or_else(|| CommandError::new("PROFILE_NOT_FOUND"))?;
    let to = store.find(&ctx.to).ok_or_else(|| CommandError::new("PROFILE_NOT_FOUND"))?;
    if from.model != c.model || to.model != to_model {
        return Err(CommandError::new("PROFILE_MODEL_MISMATCH"));
    }
    if from.bytes == to.bytes && c.model == to_model {
        return Ok(c.clone());
    }
    let key = (from.bytes.clone(), to.bytes.clone(), ctx.intent, ctx.bpc);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(&key) {
            let source = lcms2::Profile::new_icc(&from.bytes).map_err(|_| CommandError::new("PROFILE_MODEL_MISMATCH"))?;
            let target = lcms2::Profile::new_icc(&to.bytes).map_err(|_| CommandError::new("PROFILE_MODEL_MISMATCH"))?;
            let intent = match ctx.intent {
                RenderingIntent::Perceptual => lcms2::Intent::Perceptual,
                RenderingIntent::RelativeColorimetric => lcms2::Intent::RelativeColorimetric,
                RenderingIntent::Saturation => lcms2::Intent::Saturation,
                RenderingIntent::AbsoluteColorimetric => lcms2::Intent::AbsoluteColorimetric,
            };
            let flags = if ctx.bpc { lcms2::Flags::BLACKPOINT_COMPENSATION } else { lcms2::Flags::default() };
            let transform = lcms2::Transform::new_flags(&source, format(c.model), &target, format(to_model), intent, flags).map_err(|_| CommandError::new("PROFILE_MODEL_MISMATCH"))?;
            if cache.len() >= 16 { cache.clear(); }
            cache.insert(key.clone(), transform);
        }
        let input: Vec<u8> = c.values[..channels(c.model)].iter().flat_map(|v| {
            let value = if c.model == ColourModel::Grey { *v / 100.0 } else { *v };
            value.to_ne_bytes()
        }).collect();
        let mut output = vec![0u8; channels(to_model) * 4];
        cache[&key].transform_pixels(&input, &mut output);
        let mut values = [0.0; 4];
        for (index, bytes) in output.chunks_exact(4).enumerate() {
            values[index] = f32::from_ne_bytes(bytes.try_into().map_err(|_| CommandError::invalid_params())?);
            if to_model == ColourModel::Grey { values[index] *= 100.0; }
        }
        if values.iter().any(|v| !v.is_finite()) { return Err(CommandError::new("PROFILE_MODEL_MISMATCH")); }
        Ok(Colour { model: to_model, values, alpha: c.alpha })
    })
}

fn srgb_luminance(c: &Colour) -> f32 {
    let r = if c.model == ColourModel::Grey { c.values[0] / 100.0 } else { c.values[0] };
    let g = if c.model == ColourModel::Grey { c.values[0] / 100.0 } else { c.values[1] };
    let b = if c.model == ColourModel::Grey { c.values[0] / 100.0 } else { c.values[2] };

    let r_lin = if r <= 0.04045 { r / 12.92 } else { ((r + 0.055) / 1.055).powf(2.4) };
    let g_lin = if g <= 0.04045 { g / 12.92 } else { ((g + 0.055) / 1.055).powf(2.4) };
    let b_lin = if b <= 0.04045 { b / 12.92 } else { ((b + 0.055) / 1.055).powf(2.4) };

    0.2126 * r_lin + 0.7152 * g_lin + 0.0722 * b_lin
}

pub fn contrast_ratio(a: &Colour, b: &Colour, store: &ProfileStore) -> f32 {
    let ctx_rgb = ConvertCtx { from: "sRGB".to_string(), to: "sRGB".to_string(), intent: RenderingIntent::RelativeColorimetric, bpc: false };

    let a_srgb = convert(a, ColourModel::Rgb, &ctx_rgb, store).unwrap_or_else(|_| a.clone());
    let b_srgb = convert(b, ColourModel::Rgb, &ctx_rgb, store).unwrap_or_else(|_| b.clone());

    let la = srgb_luminance(&a_srgb);
    let lb = srgb_luminance(&b_srgb);

    let l1 = la.max(lb);
    let l2 = la.min(lb);

    (l1 + 0.05) / (l2 + 0.05)
}

pub const PROFILE_CONVERT: &str = "color.profile.convert";
pub const PROFILE_CONVERT_RESTORE: &str = "color.profile.convert.restore";

fn colour_shape(map: &serde_json::Map<String, serde_json::Value>) -> bool {
    let rgb = map.len() == 4 && ["r", "g", "b", "a"].iter().all(|key| map.get(*key).is_some_and(serde_json::Value::is_u64));
    let managed = map.len() == 3 && map.contains_key("model") && map.get("values").is_some_and(serde_json::Value::is_array) && map.get("alpha").is_some_and(serde_json::Value::is_number);
    rgb || managed
}

fn convert_value(value: &mut serde_json::Value, doc: &super::DocColour, target: super::ColourFormat, profile: &str, intent: RenderingIntent, bpc: bool, services: &super::ColourServices, count: &mut usize, skipped: &mut usize) -> Result<(), CommandError> {
    match value {
        serde_json::Value::Object(map) if colour_shape(map) => {
            let paint: crate::paint::Colour = serde_json::from_value(serde_json::Value::Object(map.clone())).map_err(|_| CommandError::invalid_params())?;
            let managed = paint.managed();
            let from = if managed.model == doc.format.model {
                doc.profile.clone()
            } else {
                super::ColourFormat { model: managed.model, depth: super::BitDepth::U8 }.default_profile(&services.preferences)
            };
            if services.profiles.find(&from).is_none() {
                *skipped += 1;
                return Ok(());
            }
            let ctx = ConvertCtx { from, to: profile.to_string(), intent, bpc };
            let converted = convert(&managed, target.model, &ctx, &services.profiles)?;
            *value = serde_json::to_value(crate::paint::Colour::from_managed(converted)).map_err(|_| CommandError::invalid_params())?;
            *count += 1;
            Ok(())
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                convert_value(item, doc, target, profile, intent, bpc, services, count, skipped)?;
            }
            Ok(())
        }
        serde_json::Value::Array(items) => {
            for item in items {
                convert_value(item, doc, target, profile, intent, bpc, services, count, skipped)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn convert_document(doc: &mut crate::document::Document, target: super::ColourFormat, profile: &str, intent: RenderingIntent, bpc: bool, services: &super::ColourServices) -> Result<usize, CommandError> {
    convert_document_report(doc, target, profile, intent, bpc, services).map(|(count, _)| count)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConvertReport {
    pub converted: usize,
    pub skipped: usize,
    pub spot_kept: usize,
    pub registration_kept: usize,
}

pub fn convert_document_report(doc: &mut crate::document::Document, target: super::ColourFormat, profile: &str, intent: RenderingIntent, bpc: bool, services: &super::ColourServices) -> Result<(usize, usize), CommandError> {
    convert_document_full(doc, target, profile, intent, bpc, services).map(|report| (report.converted, report.skipped))
}

pub fn convert_document_full(doc: &mut crate::document::Document, target: super::ColourFormat, profile: &str, intent: RenderingIntent, bpc: bool, services: &super::ColourServices) -> Result<ConvertReport, CommandError> {
    let found = services.profiles.find(profile).ok_or_else(|| CommandError::new("PROFILE_NOT_FOUND"))?;
    if found.model != target.model {
        return Err(CommandError::new("PROFILE_MODEL_MISMATCH"));
    }
    let mut objects = serde_json::to_value(&doc.objects).map_err(|_| CommandError::invalid_params())?;
    let mut count = 0;
    let mut skipped = 0;
    convert_value(&mut objects, &doc.colour, target, profile, intent, bpc, services, &mut count, &mut skipped)?;
    let mut spot_kept = 0;
    let mut registration_kept = 0;
    let mut swatches = Vec::with_capacity(doc.swatches.len());
    for swatch in &doc.swatches {
        if swatch.spot || swatch.registration {
            if swatch.spot { spot_kept += 1; } else { registration_kept += 1; }
            swatches.push(serde_json::to_value(swatch).map_err(|_| CommandError::invalid_params())?);
            continue;
        }
        let mut value = serde_json::to_value(swatch).map_err(|_| CommandError::invalid_params())?;
        convert_value(&mut value, &doc.colour, target, profile, intent, bpc, services, &mut count, &mut skipped)?;
        swatches.push(value);
    }
    let swatches = serde_json::Value::Array(swatches);
    let mut table_formats = serde_json::to_value(&doc.table_formats).map_err(|_| CommandError::invalid_params())?;
    convert_value(&mut table_formats, &doc.colour, target, profile, intent, bpc, services, &mut count, &mut skipped)?;
    let table_formats = serde_json::from_value(table_formats).map_err(|_| CommandError::invalid_params())?;
    let objects = serde_json::from_value(objects).map_err(|_| CommandError::invalid_params())?;
    let swatches = serde_json::from_value(swatches).map_err(|_| CommandError::invalid_params())?;
    let mut stories = doc.stories.clone();
    for story in &mut stories {
        for range in &mut story.ranges {
            let Some(paint) = crate::paint::range_fill(range) else { continue };
            let mut value = serde_json::to_value(&paint).map_err(|_| CommandError::invalid_params())?;
            convert_value(&mut value, &doc.colour, target, profile, intent, bpc, services, &mut count, &mut skipped)?;
            let converted: crate::paint::Paint = serde_json::from_value(value).map_err(|_| CommandError::invalid_params())?;
            if converted != paint {
                crate::paint::set_range_fill(range, Some(&converted));
            }
        }
    }
    doc.objects = objects;
    doc.swatches = swatches;
    doc.stories = stories;
    doc.table_formats = table_formats;
    doc.colour = super::DocColour { format: target, profile: services.profiles.canonical(profile) };
    Ok(ConvertReport { converted: count, skipped, spot_kept, registration_kept })
}

pub fn parse_convert_params(params: &serde_json::Value, prefs: &super::ColourPrefs) -> Result<(super::ColourFormat, String, RenderingIntent, bool), CommandError> {
    let format = params.get("format").and_then(serde_json::Value::as_str).and_then(super::ColourFormat::parse).ok_or_else(CommandError::invalid_params)?;
    let profile = match params.get("profile") {
        Some(serde_json::Value::String(name)) if !name.is_empty() => name.clone(),
        None => format.default_profile(prefs),
        _ => return Err(CommandError::invalid_params()),
    };
    let intent = match params.get("intent") {
        None | Some(serde_json::Value::Null) => prefs.intent,
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| if value.is_string() { CommandError::new("UNSUPPORTED_INTENT") } else { CommandError::invalid_params() })?,
    };
    let bpc = match params.get("bpc") {
        None | Some(serde_json::Value::Null) => prefs.bpc,
        Some(value) => value.as_bool().ok_or_else(CommandError::invalid_params)?,
    };
    Ok((format, profile, intent, bpc))
}

fn convert_snapshot(doc: &crate::document::Document) -> Result<serde_json::Value, CommandError> {
    Ok(serde_json::json!({
        "objects": serde_json::to_value(&doc.objects).map_err(|_| CommandError::invalid_params())?,
        "colour": serde_json::to_value(&doc.colour).map_err(|_| CommandError::invalid_params())?,
        "swatches": serde_json::to_value(&doc.swatches).map_err(|_| CommandError::invalid_params())?,
        "stories": serde_json::to_value(&doc.stories).map_err(|_| CommandError::invalid_params())?,
        "table_formats": serde_json::to_value(&doc.table_formats).map_err(|_| CommandError::invalid_params())?
    }))
}

fn convert_restore(doc: &mut crate::document::Document, value: &serde_json::Value) -> Result<(), CommandError> {
    let objects = serde_json::from_value(value.get("objects").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_| CommandError::invalid_params())?;
    let colour = serde_json::from_value(value.get("colour").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_| CommandError::invalid_params())?;
    if let Some(swatches) = value.get("swatches") {
        doc.swatches = serde_json::from_value(swatches.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(stories) = value.get("stories") {
        doc.stories = serde_json::from_value(stories.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(table_formats) = value.get("table_formats") {
        doc.table_formats = serde_json::from_value(table_formats.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    doc.objects = objects;
    doc.colour = colour;
    Ok(())
}

pub fn can_execute_profile_convert(session: &crate::command::Session, command: &dtool_protocol::Command) -> Option<bool> {
    match command.name.as_str() {
        PROFILE_CONVERT => Some(session.has_document && command.params.get("format").and_then(serde_json::Value::as_str).and_then(super::ColourFormat::parse).is_some()),
        PROFILE_CONVERT_RESTORE => Some(session.has_document && command.params.get("objects").is_some()),
        _ => None,
    }
}

pub fn run_profile_convert(session: &mut crate::command::Session, command: &dtool_protocol::Command) -> Option<dtool_protocol::CommandResult> {
    match command.name.as_str() {
        PROFILE_CONVERT => Some((|| {
            if !session.has_document { return Err(CommandError::no_document()); }
            let services = super::services().read().map_err(|_| CommandError::new("INTERNAL"))?;
            let (format, profile, intent, bpc) = parse_convert_params(&command.params, &services.preferences)?;
            let before = convert_snapshot(&session.document)?;
            let mut next = session.document.clone();
            let report = convert_document_full(&mut next, format, &profile, intent, bpc, &services)?;
            let (count, skipped, spot_kept, registration_kept) = (report.converted, report.skipped, report.spot_kept, report.registration_kept);
            drop(services);
            let after = convert_snapshot(&next)?;
            next.version = session.document.version + 1;
            session.document = next;
            session.undo.push(crate::undo::Change {
                name: PROFILE_CONVERT.to_string(),
                redo: dtool_protocol::Command::with_params(PROFILE_CONVERT_RESTORE, after),
                undo: dtool_protocol::Command::with_params(PROFILE_CONVERT_RESTORE, before),
            });
            session.generation += 1;
            Ok(serde_json::json!({
                "doc_version": session.generation,
                "format": format.to_string(),
                "profile": session.document.colour.profile.clone(),
                "converted": count,
                "skipped": skipped,
                "spot_kept": spot_kept,
                "registration_kept": registration_kept,
                "changes": [{ "name": PROFILE_CONVERT }]
            }))
        })()),
        PROFILE_CONVERT_RESTORE => Some((|| {
            if !session.has_document { return Err(CommandError::no_document()); }
            convert_restore(&mut session.document, &command.params)?;
            session.generation += 1;
            Ok(serde_json::json!({ "doc_version": session.generation }))
        })()),
        _ => None,
    }
}

pub fn apply_profile_convert(session: &mut crate::command::Session, command: &dtool_protocol::Command) -> Option<Result<(), CommandError>> {
    match command.name.as_str() {
        PROFILE_CONVERT_RESTORE => Some(convert_restore(&mut session.document, &command.params)),
        PROFILE_CONVERT => Some((|| {
            let services = super::services().read().map_err(|_| CommandError::new("INTERNAL"))?;
            let (format, profile, intent, bpc) = parse_convert_params(&command.params, &services.preferences)?;
            let mut next = session.document.clone();
            convert_document(&mut next, format, &profile, intent, bpc, &services)?;
            drop(services);
            session.document = next;
            Ok(())
        })()),
        _ => None,
    }
}

pub const OPEN_CONVERTED: &str = "colour.open.converted";
pub const OPEN_MISMATCH: &str = "colour.open.mismatch";

thread_local! {
    static OPEN_REPORT: RefCell<Option<serde_json::Value>> = const { RefCell::new(None) };
}

pub fn open_policy(doc: &mut crate::document::Document, services: &super::ColourServices) -> Option<serde_json::Value> {
    let prefs = &services.preferences;
    let working = doc.colour.format.default_profile(prefs);
    let working = services.profiles.canonical(&working);
    if working.is_empty() || working == services.profiles.canonical(&doc.colour.profile) {
        return None;
    }
    let from = doc.colour.profile.clone();
    let mut converted = None;
    if prefs.convert_on_open && services.profiles.find(&working).is_some() {
        let target = doc.colour.format;
        if let Ok(count) = convert_document(doc, target, &working, prefs.intent, prefs.bpc, services) {
            converted = Some(count);
        }
    }
    let key = if converted.is_some() { OPEN_CONVERTED } else { OPEN_MISMATCH };
    Some(serde_json::json!({
        "action": if converted.is_some() { "converted" } else { "kept" },
        "from": from,
        "to": working,
        "converted": converted.unwrap_or(0),
        "warn": prefs.warn_on_convert,
        "message_key": if prefs.warn_on_convert { serde_json::Value::String(key.to_string()) } else { serde_json::Value::Null }
    }))
}

pub fn set_open_report(report: Option<serde_json::Value>) {
    OPEN_REPORT.with(|slot| *slot.borrow_mut() = report);
}

pub fn take_open_report() -> Option<serde_json::Value> {
    OPEN_REPORT.with(|slot| slot.borrow_mut().take())
}