use crate::object::ObjectId as NodeId;

#[derive(Clone, Debug)]
pub enum NodeKind {
    Path,
    Text,
    Shape,
    Dimension,
    WarpGroup,
}

#[derive(Clone, Debug)]
pub struct ModelNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub data: serde_json::Value,
}

impl ModelNode {
    pub fn new(id: NodeId, kind: NodeKind, data: serde_json::Value) -> Self {
        Self { id, kind, data }
    }
}
