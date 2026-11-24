use dtool_core::deform::BoneChain;
use dtool_protocol::{Command, CommandError, CommandResult};

pub struct DeformTool {
    pub chains: Vec<BoneChain>,
    pub active_chain: Option<usize>,
    pub dragging_point: Option<(usize, usize)>,
    pub preview_active: bool,
}

impl DeformTool {
    pub fn new() -> Self {
        Self {
            chains: Vec::new(),
            active_chain: None,
            dragging_point: None,
            preview_active: false,
        }
    }

    pub fn start_chain(&mut self) {
        self.chains.push(BoneChain::new());
        self.active_chain = Some(self.chains.len() - 1);
        self.preview_active = true;
    }

    pub fn add_point(&mut self, from: [f32; 2], to: [f32; 2]) {
        if let Some(idx) = self.active_chain {
            if idx < self.chains.len() {
                self.chains[idx].add_point(from, to);
            }
        }
    }

    pub fn end_chain(&mut self) {
        self.active_chain = None;
    }

    pub fn begin_drag(&mut self, chain_idx: usize, point_idx: usize) {
        if chain_idx < self.chains.len() && point_idx < self.chains[chain_idx].len() {
            self.dragging_point = Some((chain_idx, point_idx));
        }
    }

    pub fn update_drag(&mut self, to: [f32; 2]) {
        if let Some((ci, pi)) = self.dragging_point {
            if ci < self.chains.len() && pi < self.chains[ci].to.len() {
                self.chains[ci].to[pi] = to;
            }
        }
    }

    pub fn end_drag(&mut self) {
        self.dragging_point = None;
    }

    pub fn cancel(&mut self) {
        self.chains.clear();
        self.active_chain = None;
        self.dragging_point = None;
        self.preview_active = false;
    }

    pub fn build_command(&self, node: dtool_core::ObjectId) -> Option<Command> {
        if self.chains.is_empty() {
            return None;
        }

        let chains_json: Vec<serde_json::Value> = self.chains.iter().map(|c| {
            serde_json::json!({
                "from": c.from,
                "to": c.to
            })
        }).collect();

        Some(Command {
            name: "raster.deform".to_string(),
            params: serde_json::json!({
                "node": node.0,
                "chains": chains_json
            }),
        })
    }

    pub fn is_active(&self) -> bool {
        self.preview_active || !self.chains.is_empty()
    }
}
