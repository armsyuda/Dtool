use crate::command::Session;
use crate::selection::{parse_length, OutlineAlign, PixelSelection};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const MODIFY: &str = "selection.modify";
pub const MAX_RADIUS: f32 = 1000.0;
const RESTORE: &str = "selection.grow.restore";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation { Grow, Shrink, Feather, Smooth, Outline }

impl Operation {
    pub fn key(self) -> &'static str {
        match self {
            Self::Grow => "grow",
            Self::Shrink => "shrink",
            Self::Feather => "feather",
            Self::Smooth => "smooth",
            Self::Outline => "outline",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adjustment {
    pub operation: Operation,
    pub radius: f32,
    pub circular: bool,
    pub alignment: OutlineAlign,
}

impl Adjustment {
    fn parse(params: &Value, dpi: f32) -> Result<Self, CommandError> {
        let operation = match params["operation"].as_str() {
            Some("grow") => Operation::Grow,
            Some("shrink") => Operation::Shrink,
            Some("feather") => Operation::Feather,
            Some("smooth") => Operation::Smooth,
            Some("outline") => Operation::Outline,
            _ => return Err(CommandError::invalid_params()),
        };
        if operation == Operation::Grow
            && params.as_object().is_none_or(|params| params.keys().any(|key| !matches!(key.as_str(), "operation" | "radius" | "circular")))
        {
            return Err(CommandError::invalid_params());
        }
        let radius = match params.get("radius") {
            Some(value) => parse_length(value, dpi)?,
            None if operation != Operation::Grow => 0.0,
            None => return Err(CommandError::invalid_params()),
        };
        if !(0.0..=MAX_RADIUS).contains(&radius) { return Err(CommandError::invalid_params()); }
        let circular = match params.get("circular") {
            Some(value) => value.as_bool().ok_or_else(CommandError::invalid_params)?,
            None => operation != Operation::Grow,
        };
        let alignment = match params["alignment"].as_str() {
            Some("outside") => OutlineAlign::Outside,
            Some("inside") => OutlineAlign::Inside,
            _ => OutlineAlign::Centre,
        };
        Ok(Self { operation, radius, circular, alignment })
    }

    pub fn edit(self, selection: &mut PixelSelection) {
        match self.operation {
            Operation::Grow => selection.grow(self.radius, self.circular),
            Operation::Shrink => selection.shrink(self.radius, self.circular),
            Operation::Feather => selection.feather(self.radius),
            Operation::Smooth => selection.smooth(self.radius),
            Operation::Outline => selection.outline(self.radius, self.alignment),
        }
    }

    pub fn command(self) -> Command {
        let mut params = json!({"operation": self.operation.key(), "radius": self.radius});
        match self.operation {
            Operation::Grow | Operation::Shrink => params["circular"] = json!(self.circular),
            Operation::Outline => {
                params["alignment"] = json!(match self.alignment {
                    OutlineAlign::Outside => "outside",
                    OutlineAlign::Centre => "centre",
                    OutlineAlign::Inside => "inside",
                });
            }
            _ => {}
        }
        Command::with_params(MODIFY, params)
    }
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    (command.name == MODIFY).then(|| session.has_document
        && session.document.pixel_selection.as_ref().is_some_and(|mask| !mask.is_empty()))
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    match command.name.as_str() {
        MODIFY => Some(edit_selection(session, command, false).map(|_| ())),
        RESTORE => Some((|| {
            let selection: Option<PixelSelection> = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
            if selection.as_ref().is_some_and(|mask| !valid(mask)) { return Err(CommandError::invalid_params()); }
            session.document.pixel_selection = selection;
            Ok(())
        })()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    (command.name == MODIFY).then(|| edit_selection(session, command, true))
}

fn valid(mask: &PixelSelection) -> bool {
    (mask.width as usize).checked_mul(mask.height as usize) == Some(mask.coverage.len())
}

fn edit_selection(session: &mut Session, command: &Command, record: bool) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let adjustment = Adjustment::parse(&command.params, session.document.dpi)?;
    let before = session.document.pixel_selection.as_ref().filter(|mask| !mask.is_empty())
        .ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
    if !valid(before) { return Err(CommandError::invalid_params()); }
    let mut after = before.clone();
    adjustment.edit(&mut after);
    let result = json!({"operation": adjustment.operation.key(), "radius_px": adjustment.radius});
    if &after == before {
        return Ok(json!({"doc_version": session.generation, "changes": [], "result": result}));
    }
    let before = session.document.pixel_selection.replace(after);
    let mut summary = if record { super::pixel_selection::record_mask(session, command, before) }
        else { crate::command::pixel_selection_summary(&session.document) };
    summary["result"] = result;
    Ok(summary)
}
