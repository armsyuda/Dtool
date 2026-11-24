use dtool_core::Command;

pub const CROP_VECTOR_SET: &str = "crop.vector.set";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VectorCropTool {
    pub active_object: Option<u64>,
    pub crop_rect: Option<[f32; 4]>,
}

impl VectorCropTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_crop(&mut self, object: u64, rect: [f32; 4]) {
        self.active_object = Some(object);
        self.crop_rect = Some(rect);
    }

    pub fn clear_crop(&mut self) {
        self.crop_rect = None;
    }

    pub fn command(&self) -> Option<Command> {
        let id = self.active_object?;
        let mut command = Command::new(CROP_VECTOR_SET);
        command.params["id"] = id.into();
        command.params["rect"] = self.crop_rect.map(|rect| rect.to_vec()).into();
        Some(command)
    }
}

#[cfg(test)]
mod contracts {
    use super::*;

    #[test]
    fn builds_set_and_clear_commands() {
        let mut tool = VectorCropTool::new();
        assert!(tool.command().is_none());
        tool.set_crop(7, [1.0, 2.0, 3.0, 4.0]);
        let set = tool.command().unwrap();
        assert_eq!(set.name, CROP_VECTOR_SET);
        assert_eq!(set.params["id"], 7_u64);
        assert_eq!(set.params["rect"][2], 3.0_f64);
        tool.clear_crop();
        assert!(tool.command().unwrap().params["rect"].is_null());
    }
}
