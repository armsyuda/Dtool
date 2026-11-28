use crate::context_toolbar::{
    ADJUSTMENT_AMOUNT, ADJUSTMENT_KIND, ADJUSTMENT_RANGE, ADJUSTMENT_TOOL, ToolOptionSpec,
};
use crate::tone::{AdjustmentKind, Mode, Range, ToneLayer};
use crate::{CommandError, CommandResult, Object, Session};
use serde_json::{Value, json};

fn selected(session: &Session) -> Option<(&Object, ToneLayer)> {
    let &[id] = session.selection.ids.as_slice() else { return None };
    let object = session.document.get_object(id)?;
    let spec = object.tone_layer?;
    (matches!(spec.mode, Mode::Adjustment { .. }) && crate::tone::attached(&session.document, object))
        .then_some((object, spec))
}

pub(super) fn current(session: &Session) -> ToneLayer {
    if let Some((_, spec)) = selected(session) { return spec; }
    let options = &session.tool_options;
    let kind = options.choice(ADJUSTMENT_TOOL, ADJUSTMENT_KIND)
        .map_or(AdjustmentKind::Exposure, |value| AdjustmentKind::from_option(&value));
    let amount = (options.number(ADJUSTMENT_TOOL, ADJUSTMENT_AMOUNT).unwrap_or(1.0) * 100.0).round() as i16;
    let range = options.choice(ADJUSTMENT_TOOL, ADJUSTMENT_RANGE)
        .map_or(Range::Full, |value| Range::from_option(&value));
    ToneLayer { mode: Mode::Adjustment { kind, amount }, range }
}

pub(crate) fn is_setting(tool: &str, id: &str) -> bool {
    tool == ADJUSTMENT_TOOL && matches!(id, ADJUSTMENT_KIND | ADJUSTMENT_AMOUNT | ADJUSTMENT_RANGE)
}

pub(crate) fn shown(session: &Session, tool: &str, id: &str) -> Option<Value> {
    if !is_setting(tool, id) { return None; }
    let (_, spec) = selected(session)?;
    let Mode::Adjustment { kind, amount } = spec.mode else { return None; };
    Some(match id {
        ADJUSTMENT_KIND => json!(kind.option()),
        ADJUSTMENT_AMOUNT => json!(f64::from(amount) / 100.0),
        _ => json!(spec.range.option()),
    })
}

pub(crate) fn edit(
    session: &mut Session, tool: &str, id: &str, option: &ToolOptionSpec, value: &Value, record: bool,
) -> Option<CommandResult> {
    if !is_setting(tool, id) { return None; }
    let (object, previous) = selected(session)?;
    let Mode::Adjustment { mut kind, mut amount } = previous.mode else { return None; };
    if !super::editable(&session.document, object) {
        return Some(Err(CommandError::new("LOCKED")));
    }

    let value = option.normalize(value)?;
    let mut range = previous.range;
    match id {
        ADJUSTMENT_KIND => kind = AdjustmentKind::from_option(value.as_str()?),
        ADJUSTMENT_AMOUNT => amount = (value.as_f64()? * 100.0).round() as i16,
        _ => range = Range::from_option(value.as_str()?),
    }
    let next = ToneLayer { mode: Mode::Adjustment { kind, amount }, range };
    let reply = json!({"tool": tool, "id": id, "value": value});
    if next == previous { return Some(Ok(reply)); }

    let target = object.id;
    if record {
        let mut document = session.document.clone();
        document.find_object_mut(target.0)?.tone_layer = Some(next);
        let selection = session.selection.ids.clone();
        Some(crate::commands::document_change::commit(session, "pixel.brush.adjustment.set", document, selection, reply))
    } else {
        session.document.find_object_mut(target.0)?.tone_layer = Some(next);
        Some(Ok(reply))
    }
}
