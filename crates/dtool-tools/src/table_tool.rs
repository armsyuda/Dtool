use dtool_core::table::commands::run as table_run;
use dtool_protocol::{Command, CommandResult};

pub struct TableTool {
    pub start_pt: Option<[f32; 2]>,
    pub end_pt: Option<[f32; 2]>,
}

impl TableTool {
    pub fn new() -> Self {
        Self { start_pt: None, end_pt: None }
    }

    pub fn on_drag_start(&mut self, pt: [f32; 2]) {
        self.start_pt = Some(pt);
        self.end_pt = Some(pt);
    }

    pub fn on_drag_update(&mut self, pt: [f32; 2]) {
        if self.start_pt.is_some() {
            self.end_pt = Some(pt);
        }
    }

    pub fn on_drag_end(&mut self) -> Option<Command> {
        let start = self.start_pt.take()?;
        let end = self.end_pt.take()?;

        let width = (end[0] - start[0]).abs();
        let height = (end[1] - start[1]).abs();

        if width < 1.0 && height < 1.0 {
            return Some(Command::with_params("table.create", serde_json::json!({
                "rows": 2,
                "cols": 2,
                "bounds": [160.0, 80.0],
                "pos": [start[0], start[1]]
            })));
        }

        let rows = (height / 40.0).max(1.0) as u32;
        let cols = (width / 40.0).max(1.0) as u32;

        Some(Command::with_params("table.create", serde_json::json!({
            "rows": rows,
            "cols": cols,
            "bounds": [width, height],
            "pos": [start[0].min(end[0]), start[1].min(end[1])]
        })))
    }

    pub fn run_command(&mut self, session: &mut dtool_core::Session, command: Command) -> Option<CommandResult> {
        table_run(session, &command)
    }
}
