use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Swatch {
    pub id: u64,
    pub name: String,
    pub colour: crate::paint::Colour,
    #[serde(default)]
    pub global: bool,
    #[serde(default)]
    pub spot: bool,
    #[serde(default)]
    pub overprint: bool,
    #[serde(default)]
    pub registration: bool,
    #[serde(default)]
    pub palette: String,
    #[serde(default)]
    pub paint: Option<crate::paint::Paint>,
}

impl Swatch {
    pub fn new(id: u64, name: &str, colour: crate::paint::Colour) -> Self {
        Self { id, name: name.to_string(), colour, global: false, spot: false, overprint: false, registration: false, palette: String::new(), paint: None }
    }
}

pub fn next_id(doc: &crate::document::Document) -> u64 {
    let used = doc.swatches.iter().map(|swatch| swatch.id).max().unwrap_or(0);
    doc.next_swatch_id.max(used + 1).max(1)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SwatchTarget {
    Fill,
    Stroke,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwatchBinding {
    pub object: crate::object::ObjectId,
    pub target: SwatchTarget,
    pub swatch: u64,
    #[serde(default)]
    pub stop: Option<usize>,
    #[serde(default)]
    pub story: Option<crate::document::StoryId>,
    #[serde(default)]
    pub range: Option<usize>,
    #[serde(default)]
    pub range_id: Option<u64>,
}

impl SwatchBinding {
    pub fn paint(object: crate::object::ObjectId, target: SwatchTarget, swatch: u64) -> Self {
        Self { object, target, swatch, stop: None, story: None, range: None, range_id: None }
    }
    pub fn same_use(&self, other: &SwatchBinding) -> bool {
        let same_range = if self.range_id.is_some() || other.range_id.is_some() { self.range_id == other.range_id } else { self.range == other.range };
        self.object == other.object && self.target == other.target && self.stop == other.stop && self.story == other.story && same_range
    }
    pub fn hits_range(&self, index: usize, range: &serde_json::Value) -> bool {
        match self.range_id {
            Some(id) => range_id(range) == Some(id),
            None => self.range.is_none() || self.range == Some(index),
        }
    }
}

pub const RANGE_ID_KEY: &str = "swatch_range_id";

pub fn range_id(range: &serde_json::Value) -> Option<u64> {
    range.get(RANGE_ID_KEY).and_then(serde_json::Value::as_u64)
}

pub fn ensure_range_id(doc: &mut crate::document::Document, story: crate::document::StoryId, index: usize) -> Option<u64> {
    let next = doc.stories.iter().flat_map(|item| item.ranges.iter()).filter_map(range_id).max().unwrap_or(0) + 1;
    let range = doc.stories.iter_mut().find(|item| item.id == story)?.ranges.get_mut(index)?;
    if let Some(id) = range_id(range) {
        return Some(id);
    }
    range.as_object_mut()?.insert(RANGE_ID_KEY.to_string(), serde_json::json!(next));
    Some(next)
}

pub fn target_paint(object: &crate::object::Object, target: SwatchTarget) -> &crate::paint::Paint {
    match target {
        SwatchTarget::Fill | SwatchTarget::Text => &object.fill,
        SwatchTarget::Stroke => &object.stroke.paint,
    }
}

pub fn set_target_paint(object: &mut crate::object::Object, target: SwatchTarget, paint: crate::paint::Paint) {
    match target {
        SwatchTarget::Fill | SwatchTarget::Text => object.fill = paint,
        SwatchTarget::Stroke => object.stroke.paint = paint,
    }
}

pub fn bind(doc: &mut crate::document::Document, object: crate::object::ObjectId, target: SwatchTarget, swatch: Option<u64>) {
    bind_use(doc, SwatchBinding::paint(object, target, 0), swatch);
}

pub fn bind_use(doc: &mut crate::document::Document, key: SwatchBinding, swatch: Option<u64>) {
    doc.swatch_bindings.retain(|binding| !binding.same_use(&key));
    if let Some(swatch) = swatch {
        doc.swatch_bindings.push(SwatchBinding { swatch, ..key });
    }
}

fn stop_colour(paint: &mut crate::paint::Paint, index: usize) -> Option<&mut crate::paint::Colour> {
    match paint {
        crate::paint::Paint::Gradient { stops, .. } => stops.get_mut(index).map(|stop| &mut stop.colour),
        _ => None,
    }
}

pub fn set_stop_colour(object: &mut crate::object::Object, target: SwatchTarget, index: usize, colour: crate::paint::Colour) -> bool {
    let paint = match target {
        SwatchTarget::Stroke => &mut object.stroke.paint,
        _ => &mut object.fill,
    };
    match stop_colour(paint, index) {
        Some(slot) => {
            *slot = colour;
            true
        }
        None => false,
    }
}

pub fn set_story_colour(doc: &mut crate::document::Document, story: crate::document::StoryId, range: Option<usize>, colour: crate::paint::Colour) -> Option<usize> {
    let story = doc.stories.iter_mut().find(|item| item.id == story)?;
    let paint = crate::paint::Paint::Solid { colour };
    let mut count = 0;
    for (index, item) in story.ranges.iter_mut().enumerate() {
        if range.is_none() || range == Some(index) {
            crate::paint::set_range_fill(item, Some(&paint));
            count += 1;
        }
    }
    (count > 0).then_some(count)
}

pub fn propagate_all(doc: &mut crate::document::Document, swatch: u64, old: crate::paint::Colour, new: crate::paint::Colour) -> (Vec<u64>, Vec<u64>) {
    let old_paint = crate::paint::Paint::Solid { colour: old };
    let new_paint = crate::paint::Paint::Solid { colour: new };
    let bindings: Vec<SwatchBinding> = doc.swatch_bindings.iter().copied().filter(|binding| binding.swatch == swatch).collect();
    let mut objects = Vec::new();
    let mut stories = Vec::new();
    for binding in bindings {
        if binding.target == SwatchTarget::Text {
            let Some(story_id) = binding.story else { continue };
            let Some(story) = doc.stories.iter_mut().find(|item| item.id == story_id) else { continue };
            let mut touched = false;
            for (index, item) in story.ranges.iter_mut().enumerate() {
                if binding.hits_range(index, item) && crate::paint::range_fill(item).as_ref() == Some(&old_paint) {
                    crate::paint::set_range_fill(item, Some(&new_paint));
                    touched = true;
                }
            }
            if touched && !stories.contains(&story_id.0) {
                stories.push(story_id.0);
            }
            continue;
        }
        let Some(object) = doc.objects.iter_mut().find(|object| object.id == binding.object) else { continue };
        let changed = match binding.stop {
            Some(index) => {
                let paint = if binding.target == SwatchTarget::Stroke { &mut object.stroke.paint } else { &mut object.fill };
                match stop_colour(paint, index) {
                    Some(slot) if *slot == old => {
                        *slot = new;
                        true
                    }
                    _ => false,
                }
            }
            None => {
                if *target_paint(object, binding.target) == old_paint {
                    set_target_paint(object, binding.target, new_paint.clone());
                    true
                } else {
                    false
                }
            }
        };
        if changed && !objects.contains(&binding.object.0) {
            objects.push(binding.object.0);
        }
    }
    (objects, stories)
}

pub fn detach(doc: &mut crate::document::Document, swatch: u64) -> usize {
    let before = doc.swatch_bindings.len();
    doc.swatch_bindings.retain(|binding| binding.swatch != swatch);
    before - doc.swatch_bindings.len()
}

pub fn propagate(doc: &mut crate::document::Document, swatch: u64, old: crate::paint::Colour, new: crate::paint::Colour) -> Vec<u64> {
    propagate_all(doc, swatch, old, new).0
}