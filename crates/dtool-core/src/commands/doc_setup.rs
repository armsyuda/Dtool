use crate::command::Session;
use crate::doc_setup::{Anchor9, DocSetup, Margins, PreviewStore, ProfileConversion, ResizeMode, SetupScope, Units, unit_factor};
use crate::document::Document;
use crate::undo::Change;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;

pub const DOC_QUERY: &str = "doc.query";
pub const DOC_SETUP_QUERY: &str = "doc.setup.query";
pub const DOC_SETUP_PREVIEW: &str = "doc.setup.preview";
pub const DOC_SETUP_APPLY: &str = "doc.setup.apply";
pub const DOC_SETUP_CANCEL: &str = "doc.setup.cancel";
pub const DOC_SETUP_RESTORE: &str = "doc.setup.restore";
pub const DOC_SETUP_TRANSPARENT: &str = "doc.setup.transparent_background";
pub const MENU_SETUP_TRANSPARENT: &str = "document.setup.transparent_background";
pub const STALE_PREVIEW: &str = "STALE_PREVIEW";

thread_local! {
    static PREVIEWS: RefCell<(u64, HashMap<u64, PreviewStore>)> = RefCell::new((1, HashMap::new()));
}

fn handles(command: &Command) -> bool {
    match command.name.as_str() {
        DOC_SETUP_QUERY | DOC_SETUP_PREVIEW | DOC_SETUP_APPLY | DOC_SETUP_CANCEL | DOC_SETUP_RESTORE
        | DOC_SETUP_TRANSPARENT | MENU_SETUP_TRANSPARENT => true,
        DOC_QUERY => true,
        _ => false,
    }
}

pub fn units_name(units: Units) -> &'static str {
    match units {
        Units::Pt => "points",
        Units::Mm => "millimeters",
        Units::Cm => "centimeters",
        Units::In => "inches",
        Units::Px => "pixels",
    }
}

pub fn parse_units(value: &str) -> Option<Units> {
    match crate::units::parse_unit(value) {
        Some(crate::units::UnitToken::Unit(units)) => Some(units),
        _ => None,
    }
}

fn stale() -> CommandError {
    CommandError::new(STALE_PREVIEW)
}

fn read_f32(value: &Value) -> Result<f32, CommandError> {
    value.as_f64().map(|v| v as f32).filter(|v| v.is_finite()).ok_or_else(CommandError::invalid_params)
}

fn read_bool(value: &Value) -> Result<bool, CommandError> {
    value.as_bool().ok_or_else(CommandError::invalid_params)
}

fn read_token(params: &Value) -> Result<Option<u64>, CommandError> {
    match params.get("token") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_u64().map(Some).ok_or_else(CommandError::invalid_params),
    }
}

pub fn margins_value(margins: &Margins) -> Value {
    json!({
        "enabled": margins.enabled,
        "left": margins.left,
        "right": margins.right,
        "top": margins.top,
        "bottom": margins.bottom,
        "linked": margins.linked
    })
}

pub fn merge_margins(target: &mut Margins, value: &Value) -> Result<(), CommandError> {
    let map = value.as_object().ok_or_else(CommandError::invalid_params)?;
    if let Some(v) = map.get("enabled") {
        target.enabled = read_bool(v)?;
    }
    if let Some(v) = map.get("linked") {
        target.linked = read_bool(v)?;
    }
    for (key, slot) in [("left", &mut target.left), ("right", &mut target.right), ("top", &mut target.top), ("bottom", &mut target.bottom)] {
        if let Some(v) = map.get(key) {
            *slot = read_f32(v)?;
        }
    }
    Ok(())
}

pub fn setup_value(doc: &Document) -> Value {
    let setup = DocSetup::from_document(doc);
    json!({
        "units": units_name(setup.units),
        "dpi": setup.dpi,
        "page_size": setup.page_size,
        "page_width": setup.page_size[0],
        "page_height": setup.page_size[1],
        "landscape": setup.landscape,
        "facing_pages": setup.facing_pages,
        "arrange_vertical": setup.arrange_vertical,
        "start_on_right": setup.start_on_right,
        "reflow_pages": setup.reflow_pages,
        "colour_format": setup.colour_format,
        "colour_profile": setup.colour_profile,
        "transparent_background": setup.transparent_background,
        "margins": margins_value(&setup.margins),
        "bleed": margins_value(&setup.bleed),
        "page_origin": setup.page_origin,
        "drawing_scale": setup.drawing_scale,
        "measurement_unit": setup.measurement_unit,
        "profile_conversion": conversion_name(setup.profile_conversion)
    })
}

pub fn conversion_name(conversion: ProfileConversion) -> &'static str {
    match conversion {
        ProfileConversion::Assign => "assign",
        ProfileConversion::Convert => "convert",
    }
}

pub fn colour_changed(before: &Document, after: &Document) -> bool {
    before.colour.format.to_string() != after.colour.format.to_string() || before.colour.profile != after.colour.profile
}

fn read_pair(value: &Value, base: [f32; 2]) -> Result<[f32; 2], CommandError> {
    match value {
        Value::Array(pair) if pair.len() == 2 => Ok([read_f32(&pair[0])?, read_f32(&pair[1])?]),
        Value::Object(map) => Ok([
            map.get("x").map(read_f32).transpose()?.unwrap_or(base[0]),
            map.get("y").map(read_f32).transpose()?.unwrap_or(base[1]),
        ]),
        _ => Err(CommandError::invalid_params()),
    }
}

pub fn apply_new_setup(doc: &mut Document, params: &Value) -> Result<(), CommandError> {
    let source = match params.get("setup") {
        Some(Value::Object(_)) => params.get("setup").cloned().unwrap_or(Value::Null),
        Some(Value::Null) | None => params.clone(),
        Some(_) => return Err(CommandError::invalid_params()),
    };
    let mut setup = DocSetup::from_document(doc);
    for (key, slot) in [
        ("landscape", &mut setup.landscape),
        ("facing_pages", &mut setup.facing_pages),
        ("arrange_vertical", &mut setup.arrange_vertical),
        ("start_on_right", &mut setup.start_on_right),
        ("reflow_pages", &mut setup.reflow_pages),
        ("transparent_background", &mut setup.transparent_background),
    ] {
        if let Some(v) = source.get(key) {
            *slot = read_bool(v)?;
        }
    }
    if let Some(v) = source.get("margins") {
        merge_margins(&mut setup.margins, v)?;
    }
    if let Some(v) = source.get("bleed") {
        merge_margins(&mut setup.bleed, v)?;
    }
    if let Some(v) = source.get("page_origin") {
        setup.page_origin = read_pair(v, setup.page_origin)?;
    }
    if let Some(v) = source.get("drawing_scale") {
        setup.drawing_scale = read_pair(v, setup.drawing_scale)?;
    }
    if let Some(v) = source.get("measurement_unit") {
        setup.measurement_unit = serde_json::from_value(v.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    setup.validate()?;
    let mut next = doc.clone();
    setup.try_apply(&mut next)?;
    *doc = next;
    Ok(())
}

pub fn setup_from_params(doc: &Document, params: &Value) -> Result<DocSetup, CommandError> {
    if !params.is_object() && !params.is_null() {
        return Err(CommandError::invalid_params());
    }
    let mut setup = DocSetup::from_document(doc);
    if let Some(v) = params.get("units") {
        let units = v.as_str().and_then(parse_units).ok_or_else(CommandError::invalid_params)?;
        setup.convert_units(units);
    }
    if let Some(v) = params.get("dpi") {
        setup.dpi = read_f32(v)?;
    }
    if let Some(v) = params.get("page_size") {
        let pair = v.as_array().filter(|a| a.len() == 2).ok_or_else(CommandError::invalid_params)?;
        setup.page_size = [read_f32(&pair[0])?, read_f32(&pair[1])?];
    }
    if let Some(v) = params.get("page_width") {
        setup.page_size[0] = read_f32(v)?;
    }
    if let Some(v) = params.get("page_height") {
        setup.page_size[1] = read_f32(v)?;
    }
    for (key, slot) in [
        ("landscape", &mut setup.landscape),
        ("facing_pages", &mut setup.facing_pages),
        ("arrange_vertical", &mut setup.arrange_vertical),
        ("start_on_right", &mut setup.start_on_right),
        ("reflow_pages", &mut setup.reflow_pages),
        ("transparent_background", &mut setup.transparent_background),
    ] {
        if let Some(v) = params.get(key) {
            *slot = read_bool(v)?;
        }
    }
    if let Some(v) = params.get("colour_format") {
        setup.colour_format = v.as_str().ok_or_else(CommandError::invalid_params)?.to_string();
    }
    if let Some(v) = params.get("colour_profile") {
        setup.colour_profile = v.as_str().ok_or_else(CommandError::invalid_params)?.to_string();
    }
    if let Some(v) = params.get("margins") {
        merge_margins(&mut setup.margins, v)?;
    }
    if let Some(v) = params.get("bleed") {
        merge_margins(&mut setup.bleed, v)?;
    }
    if let Some(v) = params.get("profile_conversion") {
        setup.profile_conversion = match v.as_str() {
            Some("assign") | Some("Assign") => ProfileConversion::Assign,
            Some("convert") | Some("Convert") => ProfileConversion::Convert,
            _ => return Err(CommandError::invalid_params()),
        };
    }
    if let Some(v) = params.get("resize") {
        setup.resize = match v.as_str() {
            Some("rescale") => ResizeMode::Rescale,
            Some("anchor_to_page") | Some("anchor") => ResizeMode::AnchorToPage,
            _ => return Err(CommandError::invalid_params()),
        };
    }
    if let Some(v) = params.get("page_origin") {
        setup.page_origin = read_pair(v, setup.page_origin)?;
    }
    if let Some(v) = params.get("drawing_scale") {
        setup.drawing_scale = read_pair(v, setup.drawing_scale)?;
    }
    if let Some(v) = params.get("measurement_unit") {
        setup.measurement_unit = serde_json::from_value(v.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(v) = params.get("anchor") {
        setup.anchor = match v {
            Value::Array(pair) if pair.len() == 2 => Anchor9 { x: read_f32(&pair[0])?, y: read_f32(&pair[1])? },
            Value::Object(map) => Anchor9 {
                x: map.get("x").map(read_f32).transpose()?.unwrap_or(setup.anchor.x),
                y: map.get("y").map(read_f32).transpose()?.unwrap_or(setup.anchor.y),
            },
            _ => return Err(CommandError::invalid_params()),
        };
    }
    if let Some(v) = params.get("scope") {
        setup.scope = match v.as_str() {
            Some("whole_document") => SetupScope::WholeDocument,
            Some("selected_spreads") => SetupScope::SelectedSpreads,
            _ => return Err(CommandError::invalid_params()),
        };
    }
    if setup.scope == SetupScope::SelectedSpreads {
        if params.get("page_size").is_none()
            && (params.get("page_width").is_none() || params.get("page_height").is_none())
        {
            return Err(CommandError::invalid_params());
        }
        setup.page_ids = serde_json::from_value(params["page_ids"].clone())
            .map_err(|_| CommandError::invalid_params())?;
    }
    if doc.layout_artboard_origins.is_some() {
        setup.arrange_vertical = true;
    }
    Ok(setup)
}

pub fn snapshot(doc: &Document) -> Result<Value, CommandError> {
    serde_json::to_value(doc).map_err(|_| CommandError::new("INTERNAL"))
}

fn restore_guides(session: &mut Session, params: &Value) -> Result<(), CommandError> {
    if let Some(value) = params.get("guides") {
        session.guides.replace_with(serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?);
    }
    Ok(())
}

fn preview_original(token: u64) -> Option<Document> {
    PREVIEWS.with(|previews| previews.borrow().1.get(&token).map(|store| store.original.clone()))
}

fn store_preview(original: Document) -> u64 {
    PREVIEWS.with(|previews| {
        let mut previews = previews.borrow_mut();
        let token = previews.0;
        previews.0 += 1;
        previews.1.clear();
        previews.1.insert(token, PreviewStore { token, original });
        token
    })
}

fn take_preview(token: u64) -> Option<Document> {
    PREVIEWS.with(|previews| previews.borrow_mut().1.remove(&token).map(|store| store.original))
}

pub fn active_preview() -> Option<u64> {
    PREVIEWS.with(|previews| previews.borrow().1.keys().next().copied())
}

pub fn has_preview(token: u64) -> bool {
    PREVIEWS.with(|previews| previews.borrow().1.contains_key(&token))
}

fn built(base: &Document, params: &Value) -> Result<Document, CommandError> {
    let setup = setup_from_params(base, params)?;
    let mut next = base.clone();
    if setup.scope == SetupScope::SelectedSpreads {
        setup.try_apply(&mut next)?;
        return Ok(next);
    }
    let format = setup.colour_format()?;
    let target_changed = format.to_string() != base.colour.format.to_string() || setup.colour_profile != base.colour.profile;
    if setup.profile_conversion == ProfileConversion::Convert && target_changed {
        let services = crate::colour::services().read().map_err(|_| CommandError::new("INTERNAL"))?;
        let intent = services.preferences.intent;
        let bpc = services.preferences.bpc;
        crate::colour::convert::convert_document(&mut next, format, &setup.colour_profile, intent, bpc, &services)?;
    }
    setup.try_apply(&mut next)?;
    Ok(next)
}

fn guides_for_document(session: &Session, target: &Document) -> Option<crate::guides::Guides> {
    let current = &session.document;
    let factor = unit_factor(current.units, current.dpi, target.units, target.dpi);
    if factor == 1.0 {
        return None;
    }
    let mut guides = session.guides.clone();
    for guide in &mut guides.items {
        guide.position *= factor;
    }
    Some(guides)
}

fn commit(session: &mut Session, name: &str, base: &Document, mut next: Document, token: Option<u64>) -> CommandResult {
    next.pixel_selection = session.document.pixel_selection.clone();
    next.version = base.version + 1;
    let mut undo = json!({ "document": snapshot(base)? });
    let mut redo = json!({ "document": snapshot(&next)? });
    let original_guides = guides_for_document(session, base);
    let updated_guides = guides_for_document(session, &next);
    if original_guides.is_some() || updated_guides.is_some() {
        undo["guides"] = serde_json::to_value(original_guides.as_ref().unwrap_or(&session.guides))
            .map_err(|_| CommandError::new("INTERNAL"))?;
        redo["guides"] = serde_json::to_value(updated_guides.as_ref().unwrap_or(&session.guides))
            .map_err(|_| CommandError::new("INTERNAL"))?;
        if let Some(guides) = updated_guides {
            session.guides.replace_with(guides);
        }
    }
    if let Some(token) = token {
        take_preview(token);
    }
    replace_document_preserving_view(session, next);
    session.undo.push(Change {
        name: name.to_string(),
        redo: Command::with_params(DOC_SETUP_RESTORE, redo),
        undo: Command::with_params(DOC_SETUP_RESTORE, undo),
    });
    session.generation += 1;
    Ok(json!({
        "doc_version": session.generation,
        "changes": [{ "name": name }],
        "setup": setup_value(&session.document)
    }))
}

fn run_preview(session: &mut Session, command: &Command) -> CommandResult {
    let token = read_token(&command.params)?;
    let original = match token {
        Some(token) => preview_original(token).ok_or_else(stale)?,
        None => session.document.clone(),
    };
    let mut next = built(&original, &command.params)?;
    next.pixel_selection = session.document.pixel_selection.clone();
    let token = match token {
        Some(token) => token,
        None => store_preview(original),
    };
    replace_preview_document(session, next);
    Ok(json!({ "token": token, "setup": setup_value(&session.document) }))
}

fn run_cancel(session: &mut Session, command: &Command) -> CommandResult {
    let token = read_token(&command.params)?.ok_or_else(CommandError::invalid_params)?;
    let mut original = take_preview(token).ok_or_else(stale)?;
    original.pixel_selection = session.document.pixel_selection.take();
    replace_preview_document(session, original);
    Ok(json!({ "token": token, "doc_version": session.generation, "setup": setup_value(&session.document) }))
}

fn run_apply(session: &mut Session, command: &Command) -> CommandResult {
    let token = read_token(&command.params)?;
    let base = match token {
        Some(token) => preview_original(token).ok_or_else(stale)?,
        None => session.document.clone(),
    };
    let next = built(&base, &command.params)?;
    commit(session, DOC_SETUP_APPLY, &base, next, token)
}

fn replace_preview_document(session: &mut Session, next: Document) {
    if let Some(guides) = guides_for_document(session, &next) {
        session.guides.replace_with(guides);
    }
    replace_document_preserving_view(session, next);
}

/// Keep the camera in screen space when the document's coordinate unit changes.
pub fn replace_document_preserving_view(session: &mut Session, next: Document) {
    let current = &session.document;
    let factor = unit_factor(current.units, current.dpi, next.units, next.dpi);
    if factor != 1.0 {
        let zoom = session.view.zoom / factor;
        if zoom.is_finite() && zoom > 0.0 {
            session.view.zoom = zoom;
        }
    }
    session.document = next;
}

fn transparent_value(session: &Session, params: &Value) -> Result<bool, CommandError> {
    match params.get("value").or_else(|| params.get("transparent_background")) {
        Some(v) => read_bool(v),
        None => Ok(!session.document.setup.transparent_background),
    }
}

fn run_transparent(session: &mut Session, command: &Command) -> CommandResult {
    let value = transparent_value(session, &command.params)?;
    let base = session.document.clone();
    let mut next = base.clone();
    next.setup.transparent_background = value;
    commit(session, DOC_SETUP_TRANSPARENT, &base, next, None)
}

fn restore(session: &mut Session, params: &Value) -> Result<(), CommandError> {
    let value = params.get("document").ok_or_else(CommandError::invalid_params)?;
    let mut next: Document = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
    restore_guides(session, params)?;
    next.pixel_selection = session.document.pixel_selection.take();
    replace_document_preserving_view(session, next);
    Ok(())
}

fn run_restore(session: &mut Session, command: &Command) -> CommandResult {
    restore(session, &command.params)?;
    session.generation += 1;
    Ok(json!({ "doc_version": session.generation, "setup": setup_value(&session.document) }))
}

fn doc_query_since(command: &Command) -> Result<Option<u64>, CommandError> {
    let Some(params) = command.params.as_object() else {
        return if command.params.is_null() { Ok(None) } else { Err(CommandError::invalid_params()) };
    };
    if params.keys().any(|key| key != "since") {
        return Err(CommandError::invalid_params());
    }
    match params.get("since") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_u64().map(Some).ok_or_else(CommandError::invalid_params),
    }
}

fn run_doc_query(session: &Session, command: &Command) -> CommandResult {
    if let Some(since) = doc_query_since(command)? {
        return session.query(Some(since));
    }
    let colour = json!({ "format": session.document.colour.format.to_string(), "profile": session.document.colour.profile });
    Ok(json!({
        "doc_version": session.generation,
        "snapshot": snapshot(&session.document)?,
        "colour": colour,
        "setup": setup_value(&session.document)
    }))
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if !handles(command) {
        return None;
    }
    if !session.has_document {
        return Some(false);
    }
    Some(match command.name.as_str() {
        DOC_SETUP_RESTORE => command.params.get("document").is_some(),
        DOC_QUERY => doc_query_since(command).is_ok_and(|since| since.is_none_or(|since| session.change_log.is_kept(since, session.generation))),
        DOC_SETUP_CANCEL => read_token(&command.params).ok().flatten().is_some_and(has_preview),
        DOC_SETUP_PREVIEW | DOC_SETUP_APPLY => match read_token(&command.params) {
            Ok(Some(token)) => has_preview(token),
            Ok(None) => true,
            Err(_) => false,
        },
        _ => true,
    })
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !handles(command) {
        return None;
    }
    if !session.has_document {
        return Some(Err(CommandError::no_document()));
    }
    Some(match command.name.as_str() {
        DOC_QUERY => run_doc_query(session, command),
        DOC_SETUP_QUERY => Ok(setup_value(&session.document)),
        DOC_SETUP_PREVIEW => run_preview(session, command),
        DOC_SETUP_APPLY => run_apply(session, command),
        DOC_SETUP_CANCEL => run_cancel(session, command),
        DOC_SETUP_RESTORE => run_restore(session, command),
        _ => run_transparent(session, command),
    })
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    match command.name.as_str() {
        DOC_SETUP_RESTORE => Some(restore(session, &command.params)),
        DOC_SETUP_APPLY => Some(built(&session.document, &command.params).map(|mut next| {
            next.pixel_selection = session.document.pixel_selection.take();
            replace_preview_document(session, next);
        })),
        DOC_SETUP_TRANSPARENT | MENU_SETUP_TRANSPARENT => Some(transparent_value(session, &command.params).map(|value| {
            session.document.setup.transparent_background = value;
        })),
        _ => None,
    }
}
