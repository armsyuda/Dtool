use crate::document::Document;

pub mod edit;
use crate::{CommandError, Object, ObjectId, ObjectKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtboardSource {
    DragRect,
    Selection,
    Document,
    Custom,
}

pub fn is_imported_artboard_name(name: &str) -> bool {
    match name.strip_prefix("Artboard ") {
        Some(rest) if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) => true,
        _ => false,
    }
}

pub fn is_artboard(object: &Object) -> bool {
    object.kind == ObjectKind::Artboard || (object.kind == ObjectKind::Group && is_imported_artboard_name(&object.name))
}

pub fn owner(doc: &Document, id: ObjectId) -> Option<&Object> {
    let mut current = doc.get_object(id)?;
    for _ in 0..doc.objects.len() {
        current = doc.get_object(current.parent_id?)?;
        if is_artboard(current) {
            return Some(current);
        }
    }
    None
}

pub(crate) fn owner_or_self(doc: &Document, id: ObjectId) -> Option<&Object> {
    doc.get_object(id).filter(|object| is_artboard(object)).or_else(|| owner(doc, id))
}

pub fn contains(object: &Object, point: [f32; 2]) -> bool {
    crate::geometry::doc_to_object_local(object, point).is_some_and(|[x, y]| {
        let [left, top] = object.pos;
        x >= left && y >= top && x <= left + object.size[0].abs() && y <= top + object.size[1].abs()
    })
}

pub fn imported_artboard_candidates(doc: &Document) -> Vec<ObjectId> {
    doc.objects
        .iter()
        .filter(|object| is_imported_artboard_name(&object.name))
        .map(|object| object.id)
        .collect()
}

pub fn object_not_found(id: ObjectId) -> CommandError {
    CommandError {
        code: "OBJECT_NOT_FOUND".to_string(),
        message_key: format!("error.object_not_found.{}", id.0),
    }
}

pub fn artboard_bounds(doc: &Document, id: ObjectId) -> Result<[f32; 4], CommandError> {
    let object = match doc.get_object(id) {
        Some(object) => object,
        None => return Err(object_not_found(id)),
    };
    if !is_artboard(object) {
        return Err(CommandError::invalid_params());
    }
    Ok([object.pos[0], object.pos[1], object.size[0], object.size[1]])
}

fn validate_rect(rect: [f32; 4]) -> Result<[f32; 4], CommandError> {
    if rect.iter().any(|value| !value.is_finite()) || rect[2] <= 0.0 || rect[3] <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    Ok(rect)
}

pub fn plan_create_rect(
    source: ArtboardSource,
    doc: &Document,
    selection_bounds: Option<[f32; 4]>,
    rect: Option<[f32; 4]>,
    size: Option<[f32; 2]>,
) -> Result<[f32; 4], CommandError> {
    match source {
        ArtboardSource::Document => {
            let page = doc.pages.first().ok_or_else(CommandError::invalid_params)?;
            let (width, height) = match size {
                Some(size) if size[0] > 0.0 && size[1] > 0.0 && size[0].is_finite() && size[1].is_finite() => {
                    (size[0], size[1])
                }
                None => (page.size[0], page.size[1]),
                Some(_) => return Err(CommandError::invalid_params()),
            };
            if width <= 0.0 || height <= 0.0 || !width.is_finite() || !height.is_finite() {
                return Err(CommandError::invalid_params());
            }
            Ok([0.0, 0.0, width, height])
        }
        ArtboardSource::Selection => {
            let bounds = selection_bounds.ok_or_else(CommandError::invalid_params)?;
            validate_rect(bounds)
        }
        ArtboardSource::DragRect | ArtboardSource::Custom => {
            let rect = rect.ok_or_else(CommandError::invalid_params)?;
            validate_rect(rect)
        }
    }
}

pub fn next_artboard_name(doc: &Document) -> String {
    next_available_name(doc, |index| format!("아트보드_{index:02}"))
}

// Older imports identify artboards through the names of their group objects.
fn next_imported_artboard_name(doc: &Document) -> String {
    next_available_name(doc, |index| format!("Artboard {index}"))
}

fn next_available_name(doc: &Document, format_name: impl Fn(usize) -> String) -> String {
    let mut index = 1;
    loop {
        let name = format_name(index);
        if doc.objects.iter().all(|object| object.name != name) {
            return name;
        }
        index += 1;
    }
}

pub fn create(
    doc: &mut Document,
    source: ArtboardSource,
    selection_bounds: Option<[f32; 4]>,
    rect: Option<[f32; 4]>,
    size: Option<[f32; 2]>,
    name: Option<String>,
) -> Result<ObjectId, CommandError> {
    let rect = plan_create_rect(source, doc, selection_bounds, rect, size)?;
    let name = match name {
        Some(name) if is_imported_artboard_name(&name) => name,
        _ => next_imported_artboard_name(doc),
    };
    let mut object = Object::default();
    object.name = name;
    object.pos = [rect[0], rect[1]];
    object.size = [rect[2], rect[3]];
    object.kind = ObjectKind::Group;
    object.page_id = doc.pages.first().map(|page| page.id);
    Ok(doc.create_object(object))
}

pub fn contained_ids(doc: &Document, artboard_id: ObjectId) -> Result<Vec<ObjectId>, CommandError> {
    let [x, y, width, height] = artboard_bounds(doc, artboard_id)?;
    Ok(doc
        .objects
        .iter()
        .filter_map(|object| {
            if object.id == artboard_id {
                return None;
            }
            if object.parent_id == Some(artboard_id) {
                return Some(object.id);
            }
            let inside = object.pos[0] >= x
                && object.pos[1] >= y
                && object.pos[0] <= x + width
                && object.pos[1] <= y + height;
            if inside {
                Some(object.id)
            } else {
                None
            }
        })
        .collect())
}

pub fn update(
    doc: &mut Document,
    id: ObjectId,
    rect: [f32; 4],
    rename: Option<String>,
) -> Result<(), CommandError> {
    let rect = validate_rect(rect)?;
    let old = artboard_bounds(doc, id)?;
    let dx = rect[0] - old[0];
    let dy = rect[1] - old[1];
    let contained = contained_ids(doc, id)?;
    for child_id in contained {
        if let Some(object) = doc.find_object_mut(child_id.0) {
            object.pos[0] += dx;
            object.pos[1] += dy;
        }
    }
    let object = doc.find_object_mut(id.0).ok_or_else(|| object_not_found(id))?;
    if !is_artboard(object) {
        return Err(CommandError::invalid_params());
    }
    object.pos = [rect[0], rect[1]];
    object.size = [rect[2], rect[3]];
    if let Some(name) = rename {
        if !is_imported_artboard_name(&name) {
            return Err(CommandError::invalid_params());
        }
        object.name = name;
    }
    Ok(())
}

pub fn duplicate(doc: &mut Document, id: ObjectId) -> Result<ObjectId, CommandError> {
    let _ = artboard_bounds(doc, id)?;
    let new_id = doc.clone_object(id)?;
    let name = if doc.get_object(id).is_some_and(|object| object.kind == ObjectKind::Artboard) {
        next_artboard_name(doc)
    } else {
        next_imported_artboard_name(doc)
    };
    if let Some(object) = doc.find_object_mut(new_id.0) {
        object.name = name;
        object.pos[0] += object.size[0] + 32.0;
    }
    Ok(new_id)
}

pub fn delete(doc: &mut Document, id: ObjectId) -> Result<(), CommandError> {
    let _ = artboard_bounds(doc, id)?;
    let children: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter(|object| object.parent_id == Some(id))
        .map(|object| object.id)
        .collect();
    for child_id in children {
        if let Some(object) = doc.find_object_mut(child_id.0) {
            object.parent_id = None;
        }
    }
    doc.delete_object(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Units;

    fn doc() -> Document {
        Document::new([800.0, 600.0], Units::Px, 72.0, 1)
    }

    fn shape(doc: &mut Document, pos: [f32; 2], size: [f32; 2]) -> ObjectId {
        let mut object = Object::default();
        object.name = "Shape".into();
        object.pos = pos;
        object.size = size;
        object.kind = ObjectKind::Shape;
        doc.create_object(object)
    }

    #[test]
    fn imported_name_matches_pdf_spelling() {
        assert!(is_imported_artboard_name("Artboard 1"));
        assert!(is_imported_artboard_name("Artboard 12"));
        assert!(!is_imported_artboard_name("Artboard"));
        assert!(!is_imported_artboard_name("Artboard "));
        assert!(!is_imported_artboard_name("Artboard N"));
        assert!(!is_imported_artboard_name("artboard 1"));
        assert!(!is_imported_artboard_name("Page 1"));
        assert!(!is_imported_artboard_name("Artboard 1a"));
        assert!(!is_imported_artboard_name("Artboard  1"));
    }

    #[test]
    fn create_from_document_size_returns_named_object() {
        let mut doc = doc();
        let id = create(&mut doc, ArtboardSource::Document, None, None, None, None).unwrap();
        let object = doc.get_object(id).unwrap();
        assert_eq!(object.name, "Artboard 1");
        assert_eq!(object.pos, [0.0, 0.0]);
        assert_eq!(object.size, [800.0, 600.0]);
        assert_eq!(object.kind, ObjectKind::Group);
        assert_eq!(artboard_bounds(&doc, id).unwrap(), [0.0, 0.0, 800.0, 600.0]);
        let second = create(&mut doc, ArtboardSource::Document, None, None, None, None).unwrap();
        assert_eq!(doc.get_object(second).unwrap().name, "Artboard 2");
        assert_ne!(id, second);
    }

    #[test]
    fn bounds_reject_non_artboard_and_unknown_id() {
        let mut doc = doc();
        let artboard = create(&mut doc, ArtboardSource::Document, None, None, None, None).unwrap();
        let live = shape(&mut doc, [10.0, 10.0], [20.0, 20.0]);
        assert_eq!(
            artboard_bounds(&doc, live).unwrap_err().code,
            "INVALID_PARAMS"
        );
        assert_eq!(
            artboard_bounds(&doc, ObjectId(999)).unwrap_err().code,
            "OBJECT_NOT_FOUND"
        );
        assert!(artboard_bounds(&doc, artboard).is_ok());
    }

    #[test]
    fn update_moves_contained_objects_by_rect_delta() {
        let mut doc = doc();
        let artboard = create(
            &mut doc,
            ArtboardSource::Custom,
            None,
            Some([0.0, 0.0, 200.0, 200.0]),
            None,
            None,
        )
        .unwrap();
        let inside = shape(&mut doc, [10.0, 20.0], [30.0, 30.0]);
        let outside = shape(&mut doc, [400.0, 400.0], [10.0, 10.0]);
        update(&mut doc, artboard, [50.0, 80.0, 200.0, 200.0], None).unwrap();
        assert_eq!(artboard_bounds(&doc, artboard).unwrap(), [50.0, 80.0, 200.0, 200.0]);
        assert_eq!(doc.get_object(inside).unwrap().pos, [60.0, 100.0]);
        assert_eq!(doc.get_object(outside).unwrap().pos, [400.0, 400.0]);
    }

    #[test]
    fn update_rejects_live_non_artboard() {
        let mut doc = doc();
        let live = shape(&mut doc, [0.0, 0.0], [10.0, 10.0]);
        assert_eq!(
            update(&mut doc, live, [1.0, 1.0, 10.0, 10.0], None)
                .unwrap_err()
                .code,
            "INVALID_PARAMS"
        );
    }

    #[test]
    fn duplicate_keeps_source_and_assigns_next_name() {
        let mut doc = doc();
        let source = create(&mut doc, ArtboardSource::Document, None, None, None, None).unwrap();
        let copy = duplicate(&mut doc, source).unwrap();
        assert_ne!(copy, source);
        assert_eq!(doc.get_object(source).unwrap().name, "Artboard 1");
        assert_eq!(doc.get_object(copy).unwrap().name, "Artboard 2");
        assert_eq!(doc.get_object(copy).unwrap().pos, [832.0, 0.0]);
        assert_eq!(artboard_bounds(&doc, copy).unwrap()[2], 800.0);
    }

    #[test]
    fn delete_removes_artboard_and_keeps_unparented_content() {
        let mut doc = doc();
        let artboard = create(
            &mut doc,
            ArtboardSource::Custom,
            None,
            Some([0.0, 0.0, 100.0, 100.0]),
            None,
            None,
        )
        .unwrap();
        let child = shape(&mut doc, [5.0, 5.0], [10.0, 10.0]);
        doc.find_object_mut(child.0).unwrap().parent_id = Some(artboard);
        delete(&mut doc, artboard).unwrap();
        assert!(doc.get_object(artboard).is_none());
        let kept = doc.get_object(child).unwrap();
        assert!(kept.parent_id.is_none());
        assert_eq!(kept.pos, [5.0, 5.0]);
    }

    #[test]
    fn candidates_list_import_named_groups() {
        let mut doc = doc();
        let mut group = Object::default();
        group.name = "Artboard 1".into();
        group.kind = ObjectKind::Group;
        group.pos = [0.0, 0.0];
        group.size = [10.0, 10.0];
        let id = doc.create_object(group);
        let other = shape(&mut doc, [0.0, 0.0], [1.0, 1.0]);
        let found = imported_artboard_candidates(&doc);
        assert_eq!(found, vec![id]);
        assert!(!found.contains(&other));
    }

    #[test]
    fn plan_create_rejects_non_positive_rect() {
        let doc = doc();
        assert_eq!(
            plan_create_rect(ArtboardSource::Custom, &doc, None, Some([0.0, 0.0, 0.0, 10.0]), None)
                .unwrap_err()
                .code,
            "INVALID_PARAMS"
        );
    }
}
