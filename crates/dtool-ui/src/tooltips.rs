use dtool_strings::text;

pub fn icon_tooltip(icon_id: &str) -> String {
    text(format!("tooltip.{}", icon_id).as_str()).to_string()
}

pub fn tool_tooltip(tool_id: &str) -> String {
    let short = tool_id.strip_prefix("tool.").unwrap_or(tool_id);
    let key = format!("tooltip.tool.{}", short);
    let label = text(&key);
    if tool_id == "tool.shapes" || (short.starts_with("shape.")
        && matches!(crate::canvas_input::tool_kind(tool_id), crate::canvas_input::ToolKind::Shape(_))) {
        format!("{label}\n{}", text("tool.shape_grid.hint"))
    } else {
        label.to_string()
    }
}

pub fn control_tooltip(control_id: &str) -> String {
    text(format!("tooltip.ctrl.{}", control_id).as_str()).to_string()
}
