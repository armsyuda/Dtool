use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Default for Modifiers {
    fn default() -> Self {
        Self { ctrl: false, shift: false, alt: false }
    }
}

impl Modifiers {
    pub fn parse(value: &str) -> Modifiers {
        let mut out = Modifiers::default();
        for part in value.split([',', '+']) {
            match part.trim().to_lowercase().as_str() {
                "control" | "ctrl" => out.ctrl = true,
                "shift" => out.shift = true,
                "alt" => out.alt = true,
                _ => {}
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Canvas,
    TextField,
    Dialog,
    Menu,
    Other,
}
