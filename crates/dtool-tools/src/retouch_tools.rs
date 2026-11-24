use dtool_core::command::Session;
use dtool_core::object::ObjectId;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetouchTool {
    Inpainting,
    Clone,
    UndoBrush,
    RedEye,
}

#[derive(Debug, Clone)]
pub struct RedEyeJob {
    pub generation: u64,
    pub rect: [f32; 4],
    pub target_node: ObjectId,
}

impl RetouchTool {
    pub fn id(&self) -> &'static str {
        match self {
            RetouchTool::Inpainting => "inpaint",
            RetouchTool::Clone => "clone",
            RetouchTool::UndoBrush => "undo_brush",
            RetouchTool::RedEye => "red_eye",
        }
    }

    pub fn label_key(&self) -> &'static str {
        match self {
            RetouchTool::Inpainting => "tool.inpaint",
            RetouchTool::Clone => "tool.clone",
            RetouchTool::UndoBrush => "tool.undo_brush",
            RetouchTool::RedEye => "tool.red_eye",
        }
    }

    pub fn execute_red_eye(&self, session: &mut Session, rect: [f32; 4], target_node: ObjectId) -> Result<CommandResult, CommandError> {
        let params = serde_json::json!({
            "rect": rect,
            "target": format!("node:{}", target_node.0)
        });

        let cmd = Command {
            name: "pixel.red_eye".to_string(),
            params,
        };

        session.execute_command(cmd)
    }
}
