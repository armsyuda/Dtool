use dtool_core::{Command, CommandError, Session};

pub const STYLE_PICK: &str = "style.pick";
pub const STYLE_APPLY: &str = "style.apply";
pub const STYLE_PARTS: [&str; 6] = dtool_core::commands::style_pick::STORED_PARTS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleParts {
    pub fill: bool,
    pub stroke: bool,
    pub paint_order: bool,
    pub opacity: bool,
    pub effects: bool,
    pub blend_mode: bool,
}

impl Default for StyleParts {
    fn default() -> Self {
        Self { fill: true, stroke: true, paint_order: true, opacity: true, effects: false, blend_mode: false }
    }
}

impl StyleParts {
    pub fn get(&self, part: &str) -> bool {
        match part {
            "fill" => self.fill,
            "stroke" => self.stroke,
            "paint_order" => self.paint_order,
            "opacity" => self.opacity,
            "effects" => self.effects,
            "blend_mode" => self.blend_mode,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct StylePickerTool {
    picked: Option<Command>,
    pub source: Option<u64>,
    pub parts: StyleParts,
}

impl StylePickerTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_style(&self) -> bool {
        self.picked.is_some()
    }

    pub fn pick(&mut self, session: &mut Session) -> Result<u64, CommandError> {
        let id = session.selection.ids.first().ok_or_else(CommandError::not_found)?.0;
        self.pick_object(session, id)
    }

    pub fn pick_object(&mut self, session: &mut Session, id: u64) -> Result<u64, CommandError> {
        let mut pick = Command::new(STYLE_PICK);
        pick.params["id"] = id.into();
        let reply = session.run(&pick)?;
        let id = reply["id"].as_u64().ok_or_else(CommandError::not_found)?;
        if !reply["style"].is_object() {
            return Err(CommandError::invalid_params());
        }
        let mut apply = Command::new(STYLE_APPLY);
        apply.params["style"] = reply["style"].clone();
        self.picked = Some(apply);
        self.source = Some(id);
        Ok(id)
    }

    pub fn apply_command(&self) -> Option<Command> {
        let mut command = self.picked.clone()?;
        for part in STYLE_PARTS {
            command.params["parts"][part] = self.parts.get(part).into();
        }
        Some(command)
    }

    pub fn apply(&self, session: &mut Session) -> Result<u64, CommandError> {
        let command = self.apply_command().ok_or_else(CommandError::not_found)?;
        let reply = session.run(&command)?;
        Ok(reply["applied_count"].as_u64().unwrap_or(0))
    }

    pub fn clear(&mut self) {
        self.picked = None;
        self.source = None;
    }
}

#[cfg(test)]
mod contracts {
    use super::*;
    use dtool_core::document::Units;
    use dtool_core::{Document, Object};

    #[test]
    fn pick_then_apply_copies_opacity_in_one_undo() {
        let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
        let a = session.document.create_object(Object { opacity: 0.5, size: [10.0, 10.0], ..Object::default() });
        let b = session.document.create_object(Object { opacity: 1.0, size: [10.0, 10.0], ..Object::default() });
        let mut tool = StylePickerTool::new();
        assert!(tool.apply(&mut session).is_err());
        session.selection.ids = vec![a];
        assert_eq!(tool.pick(&mut session).unwrap(), a.0);
        session.selection.ids = vec![b];
        let generation = session.generation;
        assert_eq!(tool.apply(&mut session).unwrap(), 1);
        assert_eq!(session.generation, generation + 1);
        assert_eq!(session.document.get_object(b).unwrap().opacity, 0.5);
        tool.parts.opacity = false;
        assert_eq!(tool.apply_command().unwrap().params["parts"]["opacity"], false);
    }
}
