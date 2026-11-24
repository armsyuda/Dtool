use crate::{CommandError, ObjectId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SET: &str = "object.text_wrap.set";
pub const CLEAR: &str = "object.text_wrap.clear";
pub const IGNORE: &str = "object.text_wrap.ignore";
pub const QUERY: &str = "object.text_wrap.query";
pub const MAX_DISTANCE: f32 = 10000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WrapMode {
    #[default]
    None,
    Square,
    Tight,
    JumpObject,
    Through,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WrapSides {
    #[default]
    Both,
    Left,
    Right,
    Largest,
    Smallest,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct TextWrap {
    #[serde(default)]
    pub mode: WrapMode,
    #[serde(default)]
    pub sides: WrapSides,
    #[serde(default)]
    pub distance: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WrapEntry {
    pub object: ObjectId,
    #[serde(default)]
    pub wrap: Option<TextWrap>,
    #[serde(default)]
    pub ignore: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WrapTable {
    #[serde(default)]
    pub entries: Vec<WrapEntry>,
}

fn distance(value: &Value) -> Result<f32, CommandError> {
    let d = value.as_f64().ok_or_else(CommandError::invalid_params)? as f32;
    if !d.is_finite() || d < 0.0 || d > MAX_DISTANCE {
        return Err(CommandError::new("OUT_OF_RANGE"));
    }
    Ok(d)
}

fn distances(value: &Value, current: [f32; 4]) -> Result<[f32; 4], CommandError> {
    match value {
        Value::Number(_) => {
            let d = distance(value)?;
            Ok([d; 4])
        }
        Value::Array(items) if items.len() == 4 => {
            let mut out = [0.0; 4];
            for (slot, item) in out.iter_mut().zip(items) {
                *slot = distance(item)?;
            }
            Ok(out)
        }
        Value::Object(map) => {
            let mut out = current;
            for (slot, key) in ["top", "right", "bottom", "left"].iter().enumerate() {
                if let Some(item) = map.get(*key) {
                    out[slot] = distance(item)?;
                }
            }
            Ok(out)
        }
        _ => Err(CommandError::invalid_params()),
    }
}

impl TextWrap {
    pub fn patched(current: Option<&TextWrap>, params: &Value) -> Result<TextWrap, CommandError> {
        let mut wrap = current.copied().unwrap_or(TextWrap { mode: WrapMode::Square, ..TextWrap::default() });
        if let Some(value) = params.get("mode") {
            wrap.mode = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
        }
        if let Some(value) = params.get("sides") {
            wrap.sides = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
        }
        if let Some(value) = params.get("distance") {
            wrap.distance = distances(value, wrap.distance)?;
        }
        Ok(wrap)
    }

    pub fn exclusion(&self, bounds: [f32; 4]) -> Option<[f32; 4]> {
        match self.mode {
            WrapMode::None | WrapMode::Through => None,
            WrapMode::Square | WrapMode::Tight | WrapMode::JumpObject => {
                let [top, right, bottom, left] = self.distance;
                Some([bounds[0] - left, bounds[1] - top, bounds[2] + left + right, bounds[3] + top + bottom])
            }
        }
    }
}

pub fn line_spans(frame: [f32; 2], line: [f32; 2], wrappers: &[(TextWrap, [f32; 4])]) -> Vec<[f32; 2]> {
    let mut spans = vec![frame];
    let mut single = false;
    for (wrap, bounds) in wrappers {
        let Some(rect) = wrap.exclusion(*bounds) else { continue };
        if line[1] <= rect[1] || line[0] >= rect[1] + rect[3] {
            continue;
        }
        let (x0, x1) = (rect[0], rect[0] + rect[2]);
        let mut next = Vec::new();
        for span in spans {
            if x1 <= span[0] || x0 >= span[1] {
                next.push(span);
                continue;
            }
            if wrap.mode == WrapMode::JumpObject {
                continue;
            }
            single |= wrap.sides != WrapSides::Both;
            let left = [span[0], x0.max(span[0])];
            let right = [x1.min(span[1]), span[1]];
            let width = |s: &[f32; 2]| s[1] - s[0];
            let keep: Vec<[f32; 2]> = match wrap.sides {
                WrapSides::Both => vec![left, right],
                WrapSides::Left => vec![left],
                WrapSides::Right => vec![right],
                WrapSides::Largest => vec![if width(&left) >= width(&right) { left } else { right }],
                WrapSides::Smallest => vec![if width(&left) <= width(&right) { left } else { right }],
            };
            next.extend(keep.into_iter().filter(|s| width(s) > 0.0));
        }
        spans = next;
    }
    if single && spans.len() > 1 {
        let widest = spans.iter().copied().fold(spans[0], |best, s| if s[1] - s[0] > best[1] - best[0] { s } else { best });
        return vec![widest];
    }
    spans
}

impl WrapTable {
    pub fn get(&self, id: ObjectId) -> Option<&WrapEntry> {
        self.entries.iter().find(|entry| entry.object == id)
    }

    fn entry_mut(&mut self, id: ObjectId) -> &mut WrapEntry {
        if let Some(index) = self.entries.iter().position(|entry| entry.object == id) {
            return &mut self.entries[index];
        }
        self.entries.push(WrapEntry { object: id, wrap: None, ignore: false });
        self.entries.last_mut().expect("entry just pushed")
    }

    fn tidy(&mut self) {
        self.entries.retain(|entry| entry.wrap.is_some() || entry.ignore);
    }

    pub fn query(&self, id: ObjectId) -> Value {
        match self.get(id) {
            Some(entry) => json!({"id": id.0, "wrap": entry.wrap, "ignore": entry.ignore}),
            None => json!({"id": id.0, "wrap": Value::Null, "ignore": false}),
        }
    }

    pub fn edited(&self, name: &str, params: &Value) -> Result<Option<WrapTable>, CommandError> {
        let id = ObjectId(params.get("id").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?);
        let mut next = self.clone();
        match name {
            SET => {
                let current = self.get(id).and_then(|entry| entry.wrap.as_ref());
                let wrap = TextWrap::patched(current, params)?;
                next.entry_mut(id).wrap = Some(wrap);
            }
            CLEAR => next.entry_mut(id).wrap = None,
            IGNORE => next.entry_mut(id).ignore = params.get("on").and_then(Value::as_bool).ok_or_else(CommandError::invalid_params)?,
            _ => return Err(CommandError::not_implemented()),
        }
        next.tidy();
        Ok((next != *self).then_some(next))
    }

    pub fn wrappers_for(&self, frame: ObjectId, candidates: &[(ObjectId, [f32; 4])]) -> Vec<(TextWrap, [f32; 4])> {
        if self.get(frame).is_some_and(|entry| entry.ignore) {
            return Vec::new();
        }
        candidates
            .iter()
            .filter(|(id, _)| *id != frame)
            .filter_map(|(id, bounds)| self.get(*id).and_then(|entry| entry.wrap).map(|wrap| (wrap, *bounds)))
            .collect()
    }
}