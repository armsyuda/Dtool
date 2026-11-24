use crate::{Command, CommandError, CommandResult, Document, ObjectId, ObjectKind, Session};
use crate::table::model::{TableFormatDef, TableFormatId, TablePattern};
use crate::table::{resolve_cell, resolve_frame};
use serde_json::{json, Value};

mod defaults;
pub(crate) use defaults::seed_new_document;
pub use defaults::{DEFAULT_RESET, DEFAULT_SAVE};

fn conflict() -> CommandError { CommandError{code:"CONFLICT".into(),message_key:"error.table_format_name".into()} }

pub fn validate_definition(definition:&TableFormatDef)->Result<(),CommandError> {
    let pattern=&definition.pattern;
    if pattern.rows==0 || pattern.cols==0 || pattern.slots.len()!=usize::from(pattern.rows)*usize::from(pattern.cols)
        || u32::from(pattern.top_band_rows)+u32::from(pattern.bottom_band_rows)>u32::from(pattern.rows)
        || u32::from(pattern.left_band_cols)+u32::from(pattern.right_band_cols)>u32::from(pattern.cols) {return Err(CommandError::invalid_params());}
    let mut ids=std::collections::HashSet::new();
    for (id,_,_) in &definition.cell_formats {if *id>u16::MAX as u64 || !ids.insert(*id){return Err(CommandError::invalid_params());}}
    if pattern.slots.iter().any(|slot|!ids.contains(&u64::from(*slot))){return Err(CommandError::invalid_params());}
    Ok(())
}

pub fn run(session:&mut Session,command:&Command)->Option<CommandResult> {
    match command.name.as_str() {
        "style.table.apply" => return Some(apply(session, command)),
        "style.table.from_selection" => return Some(capture(session, command)),
        "style.table.import" | "style.table.import.query" => return Some(import(session, command)),
        DEFAULT_SAVE | DEFAULT_RESET => return Some(defaults::run(session, command)),
        _ => {}
    }
    if matches!(command.name.as_str(), "style.table.duplicate" | "style.table.delete") { return Some(copy_or_delete(session, command)); }
    if !matches!(command.name.as_str(),"style.table.define"|"style.table.list"|"style.table.query"){return None;}
    Some(execute(session,command))
}

fn execute(session:&mut Session,command:&Command)->CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    if command.name=="style.table.list" {
        return Ok(serde_json::Value::Array(session.document.table_formats.iter().map(|def|{
            let in_use=session.document.default_table_format==Some(def.id) || session.document.objects.iter().any(|o|matches!(&o.kind,ObjectKind::Table(t) if t.format==Some(def.id)));
            serde_json::json!({"id":def.id,"name":def.name,"in_use":in_use})
        }).collect()));
    }
    if command.name=="style.table.query" {
        let id=command.params.get("id").and_then(|v|v.as_u64()).ok_or_else(CommandError::invalid_params)?;
        let def=session.document.table_formats.iter().find(|d|d.id.0==id).ok_or_else(CommandError::not_found)?;
        return serde_json::to_value(def).map_err(|_|CommandError::invalid_params());
    }
    let name = format_name(&command.params)?;
    let definition: TableFormatDef = serde_json::from_value(command.params.get("definition").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_|CommandError::invalid_params())?;
    let update = match command.params["op"].as_str() {
        Some("update") => true,
        Some("create") => false,
        _ => return Err(CommandError::invalid_params()),
    };
    store_definition(session, &command.name, definition, update, name)
}

fn format_name(params: &Value) -> Result<&str, CommandError> {
    params["name"].as_str().map(str::trim).filter(|name| !name.is_empty()).ok_or_else(CommandError::invalid_params)
}

fn store_definition(session: &mut Session, command: &str, definition: TableFormatDef, update: bool, name: &str) -> CommandResult {
    let mut after = session.document.clone();
    let id = insert_definition(&mut after, definition, update, name)?;
    crate::commands::document_change::commit(session, command, after, session.selection.ids.clone(), json!({"id":id}))
}

fn insert_definition(doc: &mut Document, mut definition: TableFormatDef, update: bool, name: &str) -> Result<TableFormatId, CommandError> {
    validate_definition(&definition)?;
    let existing=if update {Some(doc.table_formats.iter().position(|d|d.id==definition.id).ok_or_else(CommandError::not_found)?)}else{None};
    if doc.table_formats.iter().enumerate().any(|(i,d)|d.name==name && Some(i)!=existing){return Err(conflict());}
    if let Some(index)=existing { definition.id=doc.table_formats[index].id; }
    else {
        let high=doc.table_formats.iter().map(|d|d.id.0).max().unwrap_or(0);
        definition.id=TableFormatId(high.checked_add(1).ok_or_else(CommandError::invalid_params)?);
    }
    let mut high=doc.table_formats.iter().flat_map(|d|d.cell_formats.iter().map(|c|c.0)).max().unwrap_or(0);
    let mut mapping=std::collections::HashMap::new();
    for (id,_,_) in &mut definition.cell_formats {
        high=high.checked_add(1).filter(|v|*v<=u16::MAX as u64).ok_or_else(CommandError::invalid_params)?;
        mapping.insert(*id,high as u16);*id=high;
    }
    for slot in &mut definition.pattern.slots {*slot=mapping[&u64::from(*slot)];}
    definition.name=name.into();let id=definition.id;
    if let Some(index)=existing {doc.table_formats[index]=definition;}else{doc.table_formats.push(definition);}
    Ok(id)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportPick {
    id: TableFormatId,
    rename_to: Option<String>,
}

fn import(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let path = command.params["path"].as_str().filter(|value| !value.trim().is_empty()).ok_or_else(CommandError::invalid_params)?;
    let (source, _) = crate::native_io::read_file(std::path::Path::new(path)).map_err(|error| error.read_error())?;
    if command.name == "style.table.import.query" {
        return Ok(json!({"formats":source.table_formats}));
    }
    let picks: Vec<ImportPick> = serde_json::from_value(command.params["picks"].clone()).map_err(|_| CommandError::invalid_params())?;
    if picks.is_empty() { return Err(CommandError::invalid_params()); }
    let mut after = session.document.clone();
    let mut report = Vec::with_capacity(picks.len());
    let mut dependencies = FormatDependencies::default();
    let mut selected = std::collections::HashSet::new();
    for pick in picks {
        if !selected.insert(pick.id) { return Err(CommandError::invalid_params()); }
        let definition = source.table_formats.iter().find(|item| item.id == pick.id).ok_or_else(CommandError::not_found)?;
        let name = pick.rename_to.as_deref().unwrap_or(&definition.name).trim().to_string();
        if name.is_empty() { return Err(CommandError::invalid_params()); }
        if after.table_formats.iter().any(|format| format.name == name) { return Err(conflict()); }
        let definition = dependencies.copy(&source, &mut after, definition)?;
        let id = insert_definition(&mut after, definition, false, &name)?;
        report.push(json!({"source_id":pick.id,"id":id,"name":name}));
    }
    crate::commands::document_change::commit(session, &command.name, after, session.selection.ids.clone(), json!({"imported":report}))
}

#[derive(Default)]
struct FormatDependencies {
    resources: std::collections::HashMap<String, String>,
    paragraphs: std::collections::HashMap<String, String>,
    reuse_images: bool,
}

impl FormatDependencies {
    fn copy(&mut self, source: &Document, after: &mut Document, definition: &TableFormatDef) -> Result<TableFormatDef, CommandError> {
        let mut definition = definition.clone();
        let scale = crate::units::units_per_point(after.units, after.dpi) / crate::units::units_per_point(source.units, source.dpi);
        remap_appearance(source, after, self, definition.frame.fill.iter_mut(), &mut definition.frame.edges)?;
        scale_insets(&mut definition.frame.insets, scale);
        for (_, _, cell) in &mut definition.cell_formats {
            remap_appearance(source, after, self, cell.fill.iter_mut(), &mut cell.edges)?;
            if let Some(name) = &mut cell.paragraph_style {
                match self.paragraphs.get(name) {
                    Some(renamed) => *name = String::clone(renamed),
                    None => {
                        let original = name.clone();
                        *name = import_paragraph(source, after, &original)?;
                        self.paragraphs.insert(original, name.clone());
                    }
                }
            }
            scale_insets(&mut cell.insets, scale);
        }
        Ok(definition)
    }
}

fn scale_insets(insets: &mut super::model::Insets, scale: f32) {
    insets.left *= scale;
    insets.right *= scale;
    insets.top *= scale;
    insets.bottom *= scale;
}

fn remap_appearance<'a>(
    source: &Document, after: &mut Document, dependencies: &mut FormatDependencies,
    fills: impl Iterator<Item = &'a mut crate::paint::Paint>, edges: &'a mut [Option<super::model::EdgeStyle>; 4],
) -> Result<(), CommandError> {
    use crate::paint::Paint;
    for paint in fills.chain(edges.iter_mut().flatten().map(|edge| &mut edge.paint)) {
        let resource = match paint {
            Paint::Bitmap(paint) => &mut paint.resource_id,
            Paint::Pattern(paint) => &mut paint.resource_id,
            _ => continue,
        };
        if let Some(id) = dependencies.resources.get(resource) {
            *resource = id.clone();
            continue;
        }
        let ([width, height], data) = crate::scene::bitmap_source(source, resource).ok_or_else(CommandError::not_found)?;
        let size = [u32::try_from(width).map_err(|_| CommandError::out_of_range())?, u32::try_from(height).map_err(|_| CommandError::out_of_range())?];
        let existing = dependencies.reuse_images.then(|| {
            after.resources.iter().find(|image| image.embedded && image.pixel_size == size && image.data.as_ref() == Some(&data))
                .map(|image| image.id)
        }).flatten();
        let id = existing.unwrap_or_else(|| after.add_image_resource(crate::resources::ImageResource {
            name: resource.clone(), pixel_size: size, data: Some(data), embedded: true, ..Default::default()
        })).0.to_string();
        dependencies.resources.insert(resource.clone(), id.clone());
        *resource = id;
    }
    Ok(())
}

fn import_paragraph(source: &Document, after: &mut Document, name: &str) -> Result<String, CommandError> {
    use crate::text_styles::{NamedTextStyle, StyleKind};
    let kind = StyleKind::Paragraph;
    let props = source.text_styles.resolve(kind, name)?;
    let mut candidate = name.to_string();
    let mut suffix = 2;
    while after.text_styles.get(kind, &candidate).is_some() {
        if after.text_styles.resolve(kind, &candidate)? == props { return Ok(candidate); }
        candidate = format!("{name} {suffix}");
        suffix += 1;
    }
    after.text_styles.define(NamedTextStyle { name: candidate.clone(), kind, props, based_on: None, next_style: None })?;
    Ok(candidate)
}

fn target(session: &Session, params: &Value) -> Result<ObjectId, CommandError> {
    if !session.has_document { return Err(CommandError::no_document()); }
    if let Some(id) = params.get("object_id") {
        return id.as_u64().map(ObjectId).ok_or_else(CommandError::invalid_params);
    }
    match session.selection.ids.as_slice() {
        [id] => Ok(*id),
        _ => Err(CommandError::invalid_params()),
    }
}

fn apply(session: &mut Session, command: &Command) -> CommandResult {
    let id = target(session, &command.params)?;
    let format = TableFormatId(command.params["id"].as_u64().ok_or_else(CommandError::invalid_params)?);
    let override_local = command.params.get("override_local")
        .map(|value| value.as_bool().ok_or_else(CommandError::invalid_params)).transpose()?.unwrap_or(false);
    if !session.document.table_formats.iter().any(|definition| definition.id == format) {
        return Err(CommandError::not_found());
    }
    let mut after = session.document.clone();
    let object = after.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
    if object.locked { return Err(CommandError::new("LOCKED")); }
    let ObjectKind::Table(table) = &mut object.kind else { return Err(CommandError::invalid_params()); };
    if table.format == Some(format) && (!override_local || (table.frame_mask.0 == 0 && table.cells.iter().all(|cell| cell.mask.0 == 0))) {
        return Ok(json!({"id":format,"object_id":id.0,"doc_version":session.generation}));
    }
    table.format = Some(format);
    if override_local {
        table.frame_mask = Default::default();
        for cell in &mut table.cells { cell.mask = Default::default(); }
    }
    crate::commands::document_change::commit(session, &command.name, after, session.selection.ids.clone(), json!({"id":format,"object_id":id.0}))
}

fn capture(session: &mut Session, command: &Command) -> CommandResult {
    let id = target(session, &command.params)?;
    let doc = &session.document;
    let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
    let ObjectKind::Table(table) = &object.kind else { return Err(CommandError::invalid_params()); };
    let update = command.params.get("update")
        .map(|value| value.as_bool().ok_or_else(CommandError::invalid_params)).transpose()?.unwrap_or(false);
    let (format_id, name) = if update {
        let id = TableFormatId(command.params["id"].as_u64().ok_or_else(CommandError::invalid_params)?);
        let existing = doc.table_formats.iter().find(|def| def.id == id).ok_or_else(CommandError::not_found)?;
        (id, existing.name.clone())
    } else {
        (TableFormatId::default(), format_name(&command.params)?.to_string())
    };
    let rows = u16::try_from(table.row_heights_pt.len()).map_err(|_| CommandError::out_of_range())?;
    let cols = u16::try_from(table.col_widths_pt.len()).map_err(|_| CommandError::out_of_range())?;
    let area = usize::from(rows) * usize::from(cols);
    if area != table.cells.len() { return Err(CommandError::invalid_params()); }
    let mut slots = vec![None; area];
    let mut formats = Vec::new();
    let mut by_format = std::collections::HashMap::new();
    for cell in &table.cells {
        if cell.row >= u32::from(rows) || cell.col >= u32::from(cols) { return Err(CommandError::invalid_params()); }
        let format = resolve_cell(doc, table, cell);
        let key = serde_json::to_vec(&format).map_err(|_| CommandError::invalid_params())?;
        let slot = match by_format.get(&key) {
            Some(id) => *id,
            None => {
                let id = u16::try_from(formats.len() + 1).map_err(|_| CommandError::out_of_range())?;
                by_format.insert(key, id);
                formats.push((u64::from(id), format!("{} {id}", dtool_strings::text("table.formats.cell")), format));
                id
            }
        };
        let index = cell.row as usize * usize::from(cols) + cell.col as usize;
        if slots[index].replace(slot).is_some() { return Err(CommandError::invalid_params()); }
    }
    let top = table.header_rows.min(rows);
    let definition = TableFormatDef {
        id: format_id,
        name: name.clone(),
        frame: resolve_frame(doc, table),
        cell_formats: formats,
        pattern: TablePattern {
            rows, cols, top_band_rows: top, bottom_band_rows: table.footer_rows.min(rows - top),
            left_band_cols: 0, right_band_cols: 0,
            slots: slots.into_iter().collect::<Option<Vec<_>>>().ok_or_else(CommandError::invalid_params)?,
        },
    };
    store_definition(session, &command.name, definition, update, &name)
}

fn copy_or_delete(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let id = command.params.get("id").and_then(|value| value.as_u64()).ok_or_else(CommandError::invalid_params)?;
    let index = session.document.table_formats.iter().position(|format| format.id.0 == id).ok_or_else(CommandError::not_found)?;
    let before = session.document.clone();
    let mut after = before.clone();
    let result_id;
    if command.name == "style.table.duplicate" {
        let mut definition = after.table_formats[index].clone();
        validate_definition(&definition)?;
        let name = match command.params.get("name") {
            Some(serde_json::Value::String(name)) if !name.trim().is_empty() => name.trim().to_string(),
            Some(_) => return Err(CommandError::invalid_params()),
            None => {
                let base = format!("{} {}", definition.name, dtool_strings::text("table.format.copy_suffix"));
                let mut name = base.clone();
                let mut suffix = 2u64;
                while after.table_formats.iter().any(|format| format.name == name) {
                    name = format!("{} {}", base, suffix);
                    suffix = suffix.checked_add(1).ok_or_else(CommandError::invalid_params)?;
                }
                name
            }
        };
        if after.table_formats.iter().any(|format| format.name == name) { return Err(conflict()); }
        let high = after.table_formats.iter().map(|format| format.id.0).max().unwrap_or(0);
        let fresh = high.checked_add(1).ok_or_else(CommandError::invalid_params)?;
        let mut cell_high = after.table_formats.iter().flat_map(|format| format.cell_formats.iter().map(|cell| cell.0)).max().unwrap_or(0);
        let mut remap = std::collections::HashMap::new();
        for (id, _, _) in &mut definition.cell_formats {
            cell_high = cell_high.checked_add(1).filter(|id| *id <= u16::MAX as u64).ok_or_else(CommandError::invalid_params)?;
            remap.insert(*id, cell_high as u16);
            *id = cell_high;
        }
        for slot in &mut definition.pattern.slots { *slot = *remap.get(&u64::from(*slot)).ok_or_else(CommandError::invalid_params)?; }
        definition.id = TableFormatId(fresh);
        definition.name = name;
        after.table_formats.push(definition);
        result_id = fresh;
    } else {
        if after.default_table_format == Some(TableFormatId(id)) { return Err(CommandError::new("CONFLICT")); }
        let replacement = match command.params.get("replacement") {
            None | Some(serde_json::Value::Null) => None,
            Some(value) => Some(value.as_u64().filter(|replacement| *replacement != id).ok_or_else(CommandError::invalid_params)?),
        };
        if replacement.is_some_and(|replacement| !after.table_formats.iter().any(|format| format.id.0 == replacement)) { return Err(CommandError::not_found()); }
        let used = after.objects.iter().any(|object| matches!(&object.kind, ObjectKind::Table(table) if table.format == Some(TableFormatId(id))));
        if used && replacement.is_none() { return Err(CommandError::new("CONFLICT")); }
        for object in &mut after.objects {
            if let ObjectKind::Table(table) = &mut object.kind {
                if table.format == Some(TableFormatId(id)) { table.format = replacement.map(TableFormatId); }
            }
        }
        after.table_formats.remove(index);
        result_id = id;
    }
    session.undo.push(crate::undo::Change {
        name: command.name.clone(),
        undo: Command::with_params("core.feature.restore", serde_json::json!({"document": before})),
        redo: Command::with_params("core.feature.restore", serde_json::json!({"document": after})),
    });
    session.document = after;
    session.document.version += 1;
    session.generation += 1;
    Ok(serde_json::json!({"id": result_id, "doc_version": session.generation, "changes": [{"name": command.name}]}))
}
