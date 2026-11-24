use crate::colour::{ColourFormat, ColourModel, DocColour};
use crate::command::{DocNewArgs, Session, execute_doc_new};
use crate::workspace::{ContextId, Workspace};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{Value, json};

pub fn parse_colour_format(label: &str) -> Option<ColourFormat> {
    let label = label.trim();
    ColourFormat::colour_format_list().iter().copied().find(|format| format.to_string().eq_ignore_ascii_case(label))
}

pub fn default_profile(format: ColourFormat, services: &crate::colour::ColourServices) -> String {
    let profile = format.default_profile(&services.preferences);
    if profile.is_empty() && format.model == ColourModel::Grey && services.profiles.find(crate::colour::ProfileStore::GREY_D50).is_some() {
        return crate::colour::ProfileStore::GREY_D50.to_string();
    }
    profile
}

pub fn parse_colour(params: &Value) -> Result<DocColour, CommandError> {
    let format = match params.get("colour_format") {
        None | Some(Value::Null) => ColourFormat::RGBA_8,
        Some(Value::String(label)) => parse_colour_format(label).ok_or_else(CommandError::invalid_params)?,
        _ => return Err(CommandError::invalid_params()),
    };
    let services = crate::colour::services().read().map_err(|_| CommandError::new("INTERNAL"))?;
    let profile = match params.get("colour_profile") {
        None | Some(Value::Null) => default_profile(format, &services),
        Some(Value::String(profile)) => profile.clone(),
        _ => return Err(CommandError::invalid_params()),
    };
    let found = services.profiles.find(&profile).ok_or_else(|| CommandError::new("PROFILE_NOT_FOUND"))?;
    if found.model != format.model { return Err(CommandError::new("PROFILE_MODEL_MISMATCH")); }
    Ok(DocColour { format, profile })
}

pub fn run_workspace(workspace: &mut Workspace, command: &Command) -> Option<(Option<ContextId>, CommandResult)> {
    if command.name != "doc.new" { return None; }
    let mut target = None;
    let result = (|| {
        let document = prepare_new_document(&command.params)?;
        let title = command.params.get("title").and_then(Value::as_str).or_else(|| command.params.get("preset").and_then(Value::as_str)).unwrap_or_default();
        let id = workspace.insert_context_with_generation(title, None, document, 0, 0);
        let context = workspace.get_mut(id).ok_or_else(CommandError::context_not_found)?;
        context.initial_view_pending = true;
        let persona = match command.params["document_kind"].as_str() {
            Some("layout_pages") => Some("layout"),
            Some("vector_artboard") => Some("vector"),
            _ => None,
        };
        if let Some(persona) = persona
            && let Some(mode) = super::persona::select_document_persona(&mut context.session, persona)
        {
            super::persona::apply_default_view_mode(&mut context.session.view, mode);
            context.view_state = context.session.view.clone();
        }
        target = Some(id);
        Ok(json!({"context_id": id.0, "doc_version": context.session.generation, "colour": query_colour(&context.session.document.colour)}))
    })();
    Some((target, result))
}

pub fn prepare_new_document(params: &Value) -> Result<crate::Document, CommandError> {
    let kind = match params.get("document_kind") {
        None | Some(Value::Null) => None,
        Some(Value::String(kind)) if matches!(kind.as_str(), "vector_artboard" | "layout_pages") => Some(kind.as_str()),
        _ => return Err(CommandError::invalid_params()),
    };
    let mut args: DocNewArgs = serde_json::from_value(params.clone()).map_err(|_| CommandError::invalid_params())?;
    if kind == Some("vector_artboard") {
        args.pages = 1;
    }
    let mut document = execute_doc_new(args)?;
    document.colour = parse_colour(params)?;
    crate::commands::doc_setup::apply_new_setup(&mut document, params)?;
    match kind {
        Some("vector_artboard") => {
            document.setup.facing_pages = false;
            super::page_artboards::initialize_artboard(&mut document)
                .ok_or_else(CommandError::invalid_params)?;
        }
        kind if kind == Some("layout_pages") || (kind.is_none() && super::persona::layout_active()) => {
            document.setup.arrange_vertical = true;
            document.layout_artboard_origins = Some(Default::default());
            document.preserve_layout_page_order = true;
        }
        _ => {}
    }
    Ok(document)
}

pub fn query_colour(colour: &DocColour) -> Value { json!({"format": colour.format.to_string(), "profile": colour.profile}) }

