use dtool_protocol::{CommandError, CommandResult};
use serde::{Deserialize, Serialize};

use crate::command::{Session, TransformSelectionBoxArgs, execute_transform_selection_box};
use crate::{Document, Object, ObjectId};

pub const MOVE_DUPLICATE: &str = "edit.move_duplicate";
pub const MAX_COPIES: u16 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MoveDuplicate {
    pub translate: [f32; 2],
    pub rotation_deg: f32,
    pub scale: [f32; 2],
    pub copies: u16,
    pub behind: bool,
    /// Document coordinates of the pivot edited for this operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f32; 2]>,
}

impl Default for MoveDuplicate {
    fn default() -> Self {
        Self { translate: [0.0; 2], rotation_deg: 0.0, scale: [1.0; 2], copies: 0, behind: false, origin: None }
    }
}

pub fn available(session: &Session) -> bool {
    session.has_document && !session.selection.ids.is_empty()
        && session.selection.ids.iter().all(|id| {
            session.document.get_object(*id).is_some_and(|object| !object.locked)
        })
}

pub fn default_origin(session: &Session) -> Option<[f32; 2]> {
    session.custom_transform_origin(&session.selection.ids).or_else(|| {
        let [x, y, width, height] = session.selection_bounds()?;
        Some([x + width * 0.5, y + height * 0.5])
    })
}

impl MoveDuplicate {
    pub fn changes_document(self) -> bool {
        self.copies != 0 || self.translate != [0.0; 2] || self.rotation_deg != 0.0 || self.scale != [1.0; 2]
    }

    pub fn preview(self, session: &Session) -> Result<Document, CommandError> {
        let (mut draft, _) = self.apply(session)?;
        if self.copies != 0 && !session.document.symbols.is_empty() {
            crate::commands::symbols::structure::sync(
                &mut draft, &session.document.objects, session.symbol_sync,
            );
        }
        Ok(draft)
    }

    fn apply(self, session: &Session) -> Result<(Document, Vec<ObjectId>), CommandError> {
        if !available(session) || self.copies > MAX_COPIES || !self.translate.iter().all(|v| v.is_finite())
            || !self.rotation_deg.is_finite() || !self.scale.iter().all(|v| v.is_finite() && *v > 0.0)
            || self.origin.is_some_and(|point| !point.iter().all(|v| v.is_finite()))
        {
            return Err(CommandError::invalid_params());
        }
        let mut draft = session.document.clone();
        let roots = &session.selection.ids;
        let origin = self.origin.or_else(|| default_origin(session)).ok_or_else(CommandError::invalid_params)?;
        if let (Some(point), [id]) = (self.origin, roots.as_slice()) {
            let object = draft.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
            object.transform_origin = Some(crate::command::local_transform_origin(object, point)?);
        }
        let mut step = TransformSelectionBoxArgs {
            origin,
            translate: self.translate,
            rotation_deg: self.rotation_deg,
            scale: self.scale,
            ..Default::default()
        };
        if self.copies == 0 {
            if self.changes_document() {
                execute_transform_selection_box(&mut draft, roots, &step)?;
            }
            return Ok((draft, roots.clone()));
        }
        let mut previous = roots.clone();
        let mut created = Vec::new();
        for _ in 0..self.copies {
            let ids = super::copy_duplicate_roots(&mut draft, &previous, [0.0; 2]);
            if ids.is_empty() { return Err(CommandError::invalid_params()); }
            execute_transform_selection_box(&mut draft, &ids, &step)?;
            created.extend(ids.iter().copied());
            previous = ids;
            step.origin[0] += self.translate[0];
            step.origin[1] += self.translate[1];
        }
        place_adjacent(&mut draft.objects, roots, &created, self.behind);
        Ok((draft, created))
    }
}

pub(super) fn run(session: &mut Session, params: &serde_json::Value) -> CommandResult {
    let settings = serde_json::from_value::<MoveDuplicate>(params.clone())
        .map_err(|_| CommandError::invalid_params())?;
    let (draft, ids) = settings.apply(session)?;
    let changes_origin = settings.origin.is_some() && match session.selection.ids.as_slice() {
        [id] => session.document.get_object(*id).and_then(|o| o.transform_origin)
            != draft.get_object(*id).and_then(|o| o.transform_origin),
        _ => false,
    };
    if !settings.changes_document() && !changes_origin {
        return Ok(super::ids_json(&ids));
    }
    let before = crate::commands::arrange::snapshot(session);
    session.document = draft;
    super::commit_paste(session, MOVE_DUPLICATE, before, ids)
}

fn place_adjacent(objects: &mut Vec<Object>, sources: &[ObjectId], copies: &[ObjectId], behind: bool) {
    use std::collections::{HashMap, HashSet};

    let ids: HashSet<_> = copies.iter().copied().collect();
    let sources: HashSet<_> = sources.iter().copied().collect();
    let roots = |object: &Object| (object.parent_id, object.page_id);
    let mut anchors = HashMap::new();
    for object in objects.iter().filter(|object| sources.contains(&object.id)) {
        if behind {
            anchors.entry(roots(object)).or_insert(object.id);
        } else {
            anchors.insert(roots(object), object.id);
        }
    }
    let mut blocks: HashMap<ObjectId, Vec<Object>> = HashMap::new();
    let mut remaining = Vec::with_capacity(objects.len());
    for object in std::mem::take(objects) {
        if let Some(anchor) = anchors.get(&roots(&object)).filter(|_| ids.contains(&object.id)) {
            blocks.entry(*anchor).or_default().push(object);
        } else {
            remaining.push(object);
        }
    }
    objects.reserve(remaining.len() + copies.len());
    for object in remaining {
        let block = blocks.remove(&object.id).unwrap_or_default();
        if behind {
            objects.extend(block);
            objects.push(object);
        } else {
            objects.push(object);
            objects.extend(block);
        }
    }
}
