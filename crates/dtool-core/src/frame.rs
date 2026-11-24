use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::object::{ObjectId, ObjectKind};
use crate::table::model::{CellId, StoryId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum VAlign {
    #[default]
    Top,
    Center,
    Bottom,
    Justify,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameProps {
    pub size: [f32; 2],
    #[serde(default = "default_columns")]
    pub columns: u32,
    #[serde(default)]
    pub gutter: f32,
    #[serde(default)]
    pub column_widths: Option<Vec<f32>>,
    #[serde(default)]
    pub link_gutters: bool,
    #[serde(default)]
    pub insets: [f32; 4],
    #[serde(default)]
    pub valign: VAlign,
    #[serde(default)]
    pub max_para_space: f32,
    #[serde(default)]
    pub auto_height: bool,
    #[serde(default)]
    pub ignore_baseline_grid: bool,
    #[serde(default)]
    pub shape: Option<ObjectId>,
    #[serde(default)]
    pub shape_path: Option<Vec<[f32; 2]>>,
}

fn default_columns() -> u32 {
    1
}

impl Default for FrameProps {
    fn default() -> Self {
        Self {
            size: [0.0, 0.0],
            columns: default_columns(),
            gutter: 0.0,
            column_widths: None,
            link_gutters: false,
            insets: [0.0; 4],
            valign: VAlign::Top,
            max_para_space: 0.0,
            auto_height: false,
            ignore_baseline_grid: false,
            shape: None,
            shape_path: None,
        }
    }
}

impl FrameProps {
    pub fn width(&self) -> f32 {
        self.size[0]
    }

    pub fn height(&self) -> f32 {
        self.size[1]
    }

    pub fn from_rect(size: [f32; 2]) -> Self {
        Self {
            size,
            ..Self::default()
        }
    }

    pub fn following_shape(size: [f32; 2], shape: ObjectId, path: Option<Vec<[f32; 2]>>) -> Self {
        Self {
            size,
            shape: Some(shape),
            shape_path: path,
            ..Self::default()
        }
    }

    pub fn content_size(&self) -> [f32; 2] {
        let w = (self.size[0] - self.insets[0] - self.insets[2]).max(0.0);
        let h = (self.size[1] - self.insets[1] - self.insets[3]).max(0.0);
        [w, h]
    }

    pub fn wrap_width(&self) -> f32 {
        self.content_size()[0]
    }
}

pub fn rect_path(pos: [f32; 2], size: [f32; 2]) -> Vec<[f32; 2]> {
    let [x, y] = pos;
    let [w, h] = size;
    vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
}

pub fn ellipse_path(pos: [f32; 2], size: [f32; 2], steps: usize) -> Vec<[f32; 2]> {
    let steps = steps.max(8);
    let cx = pos[0] + size[0] * 0.5;
    let cy = pos[1] + size[1] * 0.5;
    let rx = size[0] * 0.5;
    let ry = size[1] * 0.5;
    (0..steps)
        .map(|i| {
            let a = (i as f32) * std::f32::consts::TAU / (steps as f32);
            [cx + rx * a.cos(), cy + ry * a.sin()]
        })
        .collect()
}

pub fn to_local_path(path: &[[f32; 2]], origin: [f32; 2]) -> Vec<[f32; 2]> {
    path.iter()
        .map(|p| [p[0] - origin[0], p[1] - origin[1]])
        .collect()
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FrameChain {
    pub story: StoryId,
    pub frames: Vec<ObjectId>,
}

impl FrameChain {
    pub fn position(&self, id: ObjectId) -> Option<usize> {
        self.frames.iter().position(|frame| *frame == id)
    }
}

pub fn frame_story(doc: &Document, id: ObjectId) -> Option<StoryId> {
    match doc.get_object(id)?.kind {
        ObjectKind::FrameText { story } => Some(story),
        _ => None,
    }
}

pub fn chain_of(doc: &Document, id: ObjectId) -> Option<&FrameChain> {
    doc.frame_chains.iter().find(|chain| chain.frames.contains(&id))
}

pub fn flow_frames(doc: &Document, story: StoryId) -> Vec<ObjectId> {
    if let Some(chain) = doc.frame_chains.iter().find(|chain| chain.story == story) {
        return chain.frames.iter().copied().filter(|id| frame_story(doc, *id) == Some(story)).collect();
    }
    doc.objects
        .iter()
        .filter(|object| matches!(object.kind, ObjectKind::FrameText { story: s } if s == story))
        .map(|object| object.id)
        .take(1)
        .collect()
}

pub fn prune_chains(doc: &mut Document) {
    let alive: Vec<(ObjectId, StoryId)> = doc
        .objects
        .iter()
        .filter_map(|object| match object.kind {
            ObjectKind::FrameText { story } => Some((object.id, story)),
            _ => None,
        })
        .collect();
    for chain in &mut doc.frame_chains {
        let story = chain.story;
        chain.frames.retain(|id| alive.contains(&(*id, story)));
    }
    doc.frame_chains.retain(|chain| chain.frames.len() > 1);
}

pub fn flow_links(doc: &Document) -> Vec<(ObjectId, ObjectId)> {
    doc.frame_chains
        .iter()
        .flat_map(|chain| chain.frames.windows(2).map(|pair| (pair[0], pair[1])))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoryOwner {
    Object(ObjectId),
    TableCell(ObjectId, CellId),
}

pub fn story_owners(doc: &Document, story: StoryId) -> Vec<StoryOwner> {
    let mut owners = Vec::new();
    for object in &doc.objects {
        if object.story_id() == Some(story) {
            owners.push(StoryOwner::Object(object.id));
        }
        if let ObjectKind::Table(table) = &object.kind {
            owners.extend(table.cells.iter().filter(|cell| cell.story == story).map(|cell| StoryOwner::TableCell(object.id, cell.id)));
        }
    }
    owners
}

pub fn drop_story_if_unowned(doc: &mut Document, story: StoryId) -> bool {
    if !story_owners(doc, story).is_empty() {
        return false;
    }
    let before = doc.stories.len();
    doc.stories.retain(|s| s.id != story);
    doc.stories.len() != before
}
