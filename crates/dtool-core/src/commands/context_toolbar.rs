use crate::command::Session;
use crate::context_toolbar::{canonical_tool, find_option, OptionKind, OPTIONS_SET, OPTION_SET, PEN_STROKE_WIDTH, QUERY, SHAPE_CORNER_RADIUS, SHAPE_STROKE_WIDTH};
use crate::object::{Object, ObjectKind};
use crate::paint::{Paint, StrokeStyle};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

mod shape_recipe;
pub mod gradient;

fn ours(command: &Command) -> bool {
    matches!(command.name.as_str(), QUERY | OPTION_SET | OPTIONS_SET)
}

fn tool_of(session: &Session, command: &Command) -> Result<&'static str, CommandError> {
    let raw = command.params.get("tool").and_then(|v| v.as_str()).unwrap_or(session.active_tool.as_str());
    canonical_tool(raw).ok_or_else(CommandError::invalid_params)
}

fn option_id(command: &Command) -> Result<&str, CommandError> {
    command.params.get("id").and_then(|v| v.as_str()).ok_or_else(CommandError::invalid_params)
}

fn check_option(session: &Session, command: &Command) -> Result<(), CommandError> {
    let tool = tool_of(session, command)?;
    let id = option_id(command)?;
    if shape_recipe::matches(id) {
        return shape_recipe::plan(session, tool, id, &command.params["value"]).map(|_| ());
    }
    let spec = find_option(tool, id).ok_or_else(CommandError::invalid_params)?;
    if spec.is_action() {
        return Ok(());
    }
    let value = command.params.get("value").ok_or_else(CommandError::invalid_params)?;
    if id == gradient::TYPE && value == "bitmap"
        && (gradient::Target::selected_bitmap_target(session, tool).is_none()
            || command.params["params"]["path"].as_str().is_none_or(|path| path.trim().is_empty()))
    {
        return Err(CommandError::invalid_params());
    }
    spec.normalize(value).map(|_| ()).ok_or_else(CommandError::invalid_params)
}

fn check_bulk(session: &Session, command: &Command) -> Result<(), CommandError> {
    tool_of(session, command)?;
    command.params.get("options").and_then(|v| v.as_object()).map(|_| ()).ok_or_else(CommandError::invalid_params)
}

fn is_stroke_width(id: &str) -> bool {
    matches!(id, PEN_STROKE_WIDTH | SHAPE_STROKE_WIDTH)
}

fn selected_strokes(session: &Session) -> impl Iterator<Item = &Object> {
    session.selection.ids.iter().filter_map(|id| session.document.get_object(*id)).filter(|object| {
        !object.locked && matches!(object.kind, ObjectKind::Shape | ObjectKind::Curve(_) | ObjectKind::Compound(_))
    })
}

fn selected_rounded_rectangles(session: &Session) -> impl Iterator<Item = &Object> {
    session.selection.ids.iter().filter_map(|id| session.document.get_object(*id))
        .filter(|object| !object.locked && super::shape::rounded_corner(object).is_some())
}

fn units_per_point(session: &Session) -> f32 {
    crate::units::units_per_point(session.document.units, session.document.dpi)
}

pub fn drawing_stroke(session: &Session, tool: &str, mut stroke: StrokeStyle) -> StrokeStyle {
    let width_id = if tool.starts_with("tool.pen") { PEN_STROKE_WIDTH } else { SHAPE_STROKE_WIDTH };
    if let Some(width) = canonical_tool(tool).and_then(|tool| session.tool_options.explicit_number(tool, width_id)) {
        stroke.width = width as f32 * units_per_point(session);
        if stroke.width > 0.0 && stroke.paint == Paint::None {
            stroke.paint = super::curve_edit::default_stroke().paint;
        }
    }
    stroke
}

fn set_option(session: &mut Session, command: &Command, record: bool) -> CommandResult {
    let tool = tool_of(session, command)?;
    let id = option_id(command)?;
    if shape_recipe::matches(id) {
        let edit = shape_recipe::plan(session, tool, id, &command.params["value"])?;
        return edit.apply(session, tool, id, record);
    }
    check_option(session, command)?;
    let spec = find_option(tool, id).ok_or_else(CommandError::invalid_params)?;
    if let OptionKind::Action { command: target } = spec.kind {
        let params = command.params.get("params").cloned().unwrap_or_else(|| json!({}));
        let result = session.run(&Command { name: target.to_string(), params })?;
        return Ok(json!({ "tool": tool, "id": spec.id, "command": target, "result": result }));
    }
    let value = command.params.get("value").cloned().unwrap_or(Value::Null);
    if let Some(result) = super::pixel_brush::adjustment::edit(session, tool, id, spec, &value, record) {
        return result;
    }
    if spec.id == "zoom.level" {
        let value = spec.normalize(&value).and_then(|value| value.as_f64()).ok_or_else(CommandError::invalid_params)?;
        let result = set_zoom_percentage(session, value, command.params.get("params"))?;
        return Ok(json!({ "tool": tool, "id": spec.id, "value": zoom_percentage(session), "result": result }));
    }
    if is_stroke_width(spec.id) {
        let ids: Vec<_> = selected_strokes(session).map(|object| object.id.0).collect();
        if !ids.is_empty() {
            let width = value.as_f64().ok_or_else(CommandError::invalid_params)? * f64::from(units_per_point(session));
            session.run(&Command::with_params("stroke.set", json!({ "ids": ids, "width": width })))?;
        }
    }
    if spec.id == SHAPE_CORNER_RADIUS {
        let ids: Vec<_> = selected_rounded_rectangles(session).map(|object| object.id.0).collect();
        if !ids.is_empty() {
            let radius = Command::with_params(super::shape::CORNER_RADIUS_SET, json!({ "ids": ids, "percent": value }));
            if record {
                session.run(&radius)?;
            } else if let Some(result) = super::shape::apply(session, &radius) {
                result?;
            }
        }
    }
    if spec.id == gradient::TYPE && value == "bitmap" {
        let path = command.params["params"]["path"].as_str().ok_or_else(CommandError::invalid_params)?;
        return gradient::apply_bitmap(session, tool, std::path::Path::new(path), record);
    }
    if spec.id == gradient::TYPE
        && let Some(kind) = value.as_str().and_then(gradient::parse_kind)
        && let Some((object, target)) = gradient::Target::selected(session, tool)
    {
        let change = match target.gradient(object) {
            Some((current, stops, geom)) if kind != current => Some(target.command(object, kind, stops.to_vec(), *geom)),
            _ if matches!(target.paint(object), Some(Paint::Bitmap(_))) => Some(target.from_bitmap(object, kind)),
            _ => None,
        };
        if let Some(change) = change {
            if record { session.run(&change)?; } else { session.apply_command(&change)?; }
        }
    }
    let (spec, value) = session.tool_options.set(tool, spec.id, &value).ok_or_else(CommandError::invalid_params)?;
    if spec.id == "gradient.target" {
        session.appearance_focus = None;
    }
    Ok(json!({ "tool": tool, "id": spec.id, "value": value }))
}

fn set_bulk(session: &mut Session, command: &Command, record: bool) -> CommandResult {
    check_bulk(session, command)?;
    let tool = tool_of(session, command)?;
    let entries = command.params.get("options").and_then(|v| v.as_object()).cloned().unwrap_or_default();
    let mut applied = serde_json::Map::new();
    let mut ignored = Vec::new();
    for (key, value) in entries.iter() {
        if key == "zoom.level" && let Some(spec) = find_option(tool, key) {
            let zoom = spec.normalize(value).and_then(|value| value.as_f64());
            if zoom.is_some_and(|zoom| set_zoom_percentage(session, zoom, None).is_ok()) {
                applied.insert(spec.id.to_string(), json!(zoom_percentage(session)));
            } else {
                ignored.push(key.clone());
            }
            continue;
        }
        if let Some(spec) = find_option(tool, key).filter(|spec| is_stroke_width(spec.id) || matches!(spec.id, SHAPE_CORNER_RADIUS | gradient::TYPE)
            || super::pixel_brush::adjustment::is_setting(tool, spec.id)) {
            let command = Command::with_params(OPTION_SET, json!({ "tool": tool, "id": spec.id, "value": value }));
            match set_option(session, &command, record) {
                Ok(result) => { applied.insert(spec.id.to_string(), result["value"].clone()); }
                Err(_) => ignored.push(key.clone()),
            }
            continue;
        }
        match session.tool_options.set(tool, key, value) {
            Some((spec, value)) => {
                if spec.id == "gradient.target" {
                    session.appearance_focus = None;
                }
                applied.insert(spec.id.to_string(), value);
            }
            None => ignored.push(key.clone()),
        }
    }
    Ok(json!({ "tool": tool, "applied": applied, "ignored": ignored }))
}

fn actual_zoom_factor(session: &Session) -> f64 {
    f64::from(session.actual_zoom())
}

fn zoom_percentage(session: &Session) -> f64 {
    (f64::from(session.view.zoom) / actual_zoom_factor(session) * 10_000.0).round() / 100.0
}

fn set_zoom_percentage(session: &mut Session, percent: f64, params: Option<&Value>) -> CommandResult {
    let mut params = params.cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        return Err(CommandError::invalid_params());
    }
    params["zoom"] = json!(actual_zoom_factor(session) * percent / 100.0);
    session.run(&Command::with_params("view.zoom.set", params))
}

pub fn query_with_state(session: &Session, tool: &str) -> Value {
    let mut query = session.tool_options.query(tool);
    let brush_values = (query["tool"] == super::brush::VECTOR_TOOL).then(|| super::brush::brush_control_values(session));
    let board_settings = (query["tool"] == crate::context_toolbar::ARTBOARD_TOOL).then(|| {
        use crate::artboard::edit::{ToolSettings, page_index};
        ToolSettings::read(session, page_index(session))
    });
    if let Some(options) = query.get_mut("options").and_then(Value::as_array_mut) {
        use crate::context_toolbar::{FILTER_BRUSH_KIND, FILTER_BRUSH_TOOL, VECTOR_FLOOD, VECTOR_FLOOD_BITMAP, VECTOR_FLOOD_BITMAP_FIT, VECTOR_FLOOD_FIT, VECTOR_FLOOD_GRADIENT_FIT, VECTOR_FLOOD_VISIBLE};
        if tool == FILTER_BRUSH_TOOL && session.tool_options.choice(tool, FILTER_BRUSH_KIND).as_deref() != Some("smudge") {
            options.retain(|option| option["id"] != "brush.strength");
        }
        if super::pixel_brush::placed_photo(session).is_none() {
            options.retain(|option| option["id"] != super::pixel_brush::PREPARE_PHOTO);
        }
        let area_fill = (tool == super::fill::TOOL).then(|| {
            let tool = super::fill::TOOL;
            (
                session.tool_options.flag(tool, "fill.context_fill") == Some(true),
                session.tool_options.choice(tool, "fill.fill_type").as_deref() == Some("transparent"),
                super::pixel_brush::settings_for(session, tool, crate::brush::BrushOp::Paint).colour,
            )
        });
        let resolved_boundary = tool == VECTOR_FLOOD
            && session.tool_options.flag(tool, VECTOR_FLOOD_VISIBLE) == Some(true);
        let bitmap_selected = tool == VECTOR_FLOOD
            && session.tool_options.choice(tool, VECTOR_FLOOD_BITMAP).is_some_and(|path| !path.is_empty());
        let gradient_selected = tool == VECTOR_FLOOD && !bitmap_selected
            && matches!(super::defaults::for_document(&session.document).fill, Paint::Gradient { .. });
        if tool == VECTOR_FLOOD {
            options.retain(|option| match option["id"].as_str() {
                Some(VECTOR_FLOOD_BITMAP_FIT) => bitmap_selected,
                Some(VECTOR_FLOOD_GRADIENT_FIT) => gradient_selected,
                _ => true,
            });
        }
        for option in options.iter_mut() {
            let mut available = true;
            if tool == VECTOR_FLOOD {
                available = match option["id"].as_str() {
                    Some(VECTOR_FLOOD_FIT) => !resolved_boundary,
                    _ => true,
                };
            }
            if let Some((context, transparent, [r, g, b, a])) = area_fill {
                let id = option["id"].as_str().unwrap_or_default();
                available = !match id {
                    "fill.context_fill" => session.quick_mask.active,
                    "fill.fill_type" => context || session.quick_mask.active,
                    "brush.foreground" | "fill.pigment" => context || transparent || session.quick_mask.active,
                    _ => false,
                };
                if id == "brush.foreground" {
                    option["value"] = json!(format!("#{r:02X}{g:02X}{b:02X}{a:02X}"));
                    option["key"] = json!("context.fill.colour");
                }
            }
            if let Some(value) = option["id"].as_str().and_then(|id| super::pixel_brush::adjustment::shown(session, tool, id)) {
                option["value"] = value;
            }
            if let Some(id) = option["id"].as_str().filter(|id| is_stroke_width(id)) {
                let mut widths = selected_strokes(session).map(|object| object.stroke.width);
                if let Some(width) = widths.next() {
                    option["value"] = json!(width / units_per_point(session));
                    option["state"] = json!(if widths.all(|other| other == width) { "enabled" } else { "mixed" });
                    continue;
                }
                if session.tool_options.explicit_number(tool, id).is_none() {
                    let defaults = super::defaults::for_document(&session.document);
                    let stroke = defaults.stroke_for_document(&session.document);
                    let width = if stroke.paint == Paint::None { 0.0 } else { stroke.width / units_per_point(session) };
                    option["value"] = json!(if tool == "tool.shape.spiral" && width <= 0.0 { super::curve_edit::DEFAULT_STROKE_WIDTH } else { width });
                }
            } else if option["id"] == SHAPE_CORNER_RADIUS {
                let mut radii = selected_rounded_rectangles(session).filter_map(|object| {
                    let corner = super::shape::rounded_corner(object)?;
                    let short = super::shape::short_side(object);
                    Some(corner.radii.map(|value| {
                        let percent = if corner.absolute && short > 0.0 { value / short * 100.0 } else { value };
                        percent.clamp(0.0, 50.0)
                    }))
                }).flatten();
                if let Some(radius) = radii.next() {
                    option["value"] = json!(radius);
                    option["state"] = json!(if radii.all(|other| (other - radius).abs() < 0.0001) { "enabled" } else { "mixed" });
                    continue;
                }
            } else if let Some(settings) = &board_settings {
                use crate::context_toolbar::{ARTBOARD_WIDTH, ARTBOARD_HEIGHT, ARTBOARD_SPACING};
                match option["id"].as_str() {
                    Some(ARTBOARD_WIDTH) => option["value"] = json!(settings.size[0]),
                    Some(ARTBOARD_HEIGHT) => option["value"] = json!(settings.size[1]),
                    Some(ARTBOARD_SPACING) => option["value"] = json!(settings.spacing),
                    _ => {}
                }
            } else if option["id"] == gradient::TYPE {
                if option["value"] == "bitmap" {
                    option["value"] = json!("linear");
                }
                if gradient::Target::selected_bitmap_target(session, tool).is_none()
                    && let Some(values) = option["values"].as_array_mut()
                {
                    values.retain(|value| value != "bitmap");
                }
                if let Some((object, target)) = gradient::Target::selected(session, tool) {
                    if let Some((kind, _, _)) = target.gradient(object) {
                        option["value"] = json!(gradient::kind_value(kind));
                    } else if matches!(target.paint(object), Some(Paint::Bitmap(_))) {
                        option["value"] = json!("bitmap");
                    }
                }
            } else if option["id"] == "zoom.level" {
                option["value"] = json!(zoom_percentage(session));
            } else if let Some(values) = brush_values
                && let Some(index) = super::brush::BRUSH_NUMBERS.iter().position(|id| option["id"] == *id)
            {
                option["value"] = json!(values[index]);
            }
            let enabled = available && match option.get("kind").and_then(Value::as_str) {
                Some("action") => option
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|target| session.can_execute(&Command::with_params(target, json!({})))),
                _ => true,
            };
            option["state"] = json!(if enabled { "enabled" } else { "disabled" });
        }
        options.splice(..0, shape_recipe::options(session, tool));
    }
    query
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if !ours(command) {
        return None;
    }
    Some(match command.name.as_str() {
        QUERY => command.params.get("tool").is_none() || tool_of(session, command).is_ok(),
        OPTION_SET => check_option(session, command).is_ok(),
        _ => check_bulk(session, command).is_ok(),
    })
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !ours(command) {
        return None;
    }
    Some(match command.name.as_str() {
        QUERY => {
            if command.params.get("tool").is_some() {
                tool_of(session, command).map(|tool| query_with_state(session, tool))
            } else {
                Ok(query_with_state(session, session.active_tool.as_str()))
            }
        }
        OPTION_SET => set_option(session, command, true),
        _ => set_bulk(session, command, true),
    })
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if !ours(command) {
        return None;
    }
    Some(match command.name.as_str() {
        QUERY => Ok(()),
        OPTION_SET => {
            let action = tool_of(session, command)
                .ok()
                .and_then(|tool| option_id(command).ok().and_then(|id| find_option(tool, id)))
                .map(|spec| spec.is_action())
                .unwrap_or(false);
            if action { check_option(session, command) } else { set_option(session, command, false).map(|_| ()) }
        }
        _ => set_bulk(session, command, false).map(|_| ()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_toolbar::TOOL_IDS;

    fn session() -> Session {
        Session::new(crate::document::Document::default())
    }

    fn ids(value: &Value) -> Vec<String> {
        value["ids"].as_array().cloned().unwrap_or_default().iter().filter_map(|v| v.as_str().map(String::from)).collect()
    }

    #[test]
    fn full_tool_list_includes_text_knife_shape_builder() {
        for id in ["tool.text", "tool.text.frame", "tool.knife", "tool.shape_builder", "tool.point_transform"] {
            assert!(TOOL_IDS.contains(&id));
        }
        let mut s = session();
        assert!(s.run_named("ui.tool.set", json!({ "id": "tool.knife" })).is_ok());
        let listed = s.run_named("ui.tools.list", json!({})).expect("list");
        assert!(listed["tools"].as_array().expect("tools").iter().any(|v| v == "tool.shape_builder"));
    }

    #[test]
    fn move_and_zoom_have_different_option_sets() {
        let mut s = session();
        s.run_named("ui.tool.set", json!({ "id": "tool.move" })).expect("set move");
        let move_ids = ids(&s.run_named(QUERY, json!({})).expect("query move"));
        assert!(move_ids.contains(&"move.align".to_string()));
        s.run_named("ui.tool.set", json!({ "id": "tool.zoom" })).expect("set zoom");
        let zoom_ids = ids(&s.run_named(QUERY, json!({})).expect("query zoom"));
        assert!(zoom_ids.contains(&"zoom.mode".to_string()));
        assert_ne!(move_ids, zoom_ids);
    }

    #[test]
    fn legacy_short_active_tool_resolves() {
        let mut s = session();
        assert_eq!(s.active_tool, "tool.move");
        s.active_tool = "move".to_string();
        let out = s.run_named(QUERY, json!({})).expect("query");
        assert_eq!(out["tool"], "tool.move");
    }

    #[test]
    fn option_set_validates_and_persists() {
        let mut s = session();
        s.run_named(OPTION_SET, json!({ "tool": "tool.shape.polygon", "id": "shape.sides", "value": 7 })).expect("sides");
        let out = s.run_named(QUERY, json!({ "tool": "tool.shape.polygon" })).expect("query");
        let sides = out["options"].as_array().expect("options").iter().find(|o| o["id"] == "shape.sides").cloned().expect("sides");
        assert_eq!(sides["value"].as_f64(), Some(7.0));
        assert!(s.run_named(OPTION_SET, json!({ "tool": "tool.shape.polygon", "id": "shape.sides", "value": 1 })).is_err());
        assert!(s.run_named(OPTION_SET, json!({ "tool": "tool.text", "id": "text.align", "value": "diagonal" })).is_err());
        assert!(s.run_named(OPTION_SET, json!({ "tool": "tool.knife", "id": "knife.mode", "value": "freehand" })).is_ok());
        assert!(s.run_named(OPTION_SET, json!({ "tool": "tool.nope", "id": "x", "value": 1 })).is_err());
    }

    #[test]
    fn tool_set_stores_canonical_id() {
        let mut s = session();
        s.run_named("ui.tool.set", json!({ "id": "zoom" })).expect("zoom");
        assert_eq!(s.active_tool, "tool.zoom");
        s.run_named("ui.tool.set", json!({ "id": "smudge_brush" })).expect("smudge");
        assert_eq!(s.active_tool, "tool.paint.smudge");
    }

    #[test]
    fn typed_store_access() {
        let mut s = session();
        assert_eq!(s.tool_options.number("tool.paint.brush", "brush.width"), Some(64.0));
        assert!(s.tool_options.set_number("tool.paint.brush", "brush.width", 12.0));
        assert_eq!(s.tool_options.number("tool.paint.brush", "width"), Some(12.0));
        assert!(!s.tool_options.set_number("tool.paint.brush", "brush.width", 0.0));
        assert_eq!(s.tool_options.choice("tool.marquee.freehand", "lasso.mode").as_deref(), Some("freehand"));
        assert_eq!(s.tool_options.flag("tool.move", "move.align"), None);
    }

    #[test]
    fn bulk_options_accept_short_keys() {
        let mut s = session();
        let out = s
            .run_named(OPTIONS_SET, json!({ "tool": "fill", "options": { "context_fill": true, "fill_type": "Context", "colour": [0, 0, 0, 255] } }))
            .expect("bulk");
        assert_eq!(out["tool"], "tool.flood_fill");
        assert_eq!(out["applied"]["fill.context_fill"], true);
        assert_eq!(out["applied"]["fill.fill_type"], "context");
        assert_eq!(out["ignored"][0], "colour");
    }

    #[test]
    fn query_action_state_follows_can_execute() {
        let mut s = session();
        s.run_named("ui.tool.set", json!({ "id": "tool.move" })).expect("set move");
        let state = |s: &mut Session, id: &str| {
            let query = s.run_named(QUERY, json!({})).expect("query");
            query["options"].as_array().expect("options").iter().find(|option| option["id"] == id).expect("option")["state"].clone()
        };
        assert_eq!(state(&mut s, "move.align.left"), json!("disabled"));
        let a = s.document.create_object(crate::Object { name: "a".into(), size: [10.0, 10.0], ..crate::Object::default() });
        s.selection.set(vec![a], crate::selection::SelectOp::Replace);
        assert_eq!(state(&mut s, "move.align.left"), json!("disabled"));
        let b = s.document.create_object(crate::Object { name: "b".into(), pos: [30.0, 5.0], size: [10.0, 10.0], ..crate::Object::default() });
        s.selection.set(vec![a, b], crate::selection::SelectOp::Replace);
        assert_eq!(state(&mut s, "move.align.left"), json!("enabled"));
        s.run_named("ui.tool.set", json!({ "id": "tool.zoom" })).expect("set zoom");
        let zoom = s.run_named(QUERY, json!({})).expect("query zoom");
        for option in zoom["options"].as_array().expect("options") {
            let expected = match option["command"].as_str() {
                Some(target) => s.can_execute(&Command::with_params(target, json!({}))),
                None => true,
            };
            assert_eq!(option["state"], json!(if expected { "enabled" } else { "disabled" }), "{option}");
            if option["kind"] != "action" {
                assert_eq!(option["state"], "enabled");
            }
        }
        assert_eq!(state(&mut s, "zoom.in"), json!("enabled"));
        assert_eq!(state(&mut s, "zoom.out"), json!("enabled"));
        assert_eq!(state(&mut s, "zoom.actual"), json!("enabled"));
        assert_eq!(state(&mut s, "zoom.fit"), json!("disabled"));
    }
}
