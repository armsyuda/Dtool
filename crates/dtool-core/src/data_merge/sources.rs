use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read};
use crate::CommandError;
use super::{DataSource, FileFormat, RowWarning, SourceKind};

#[derive(Clone, Debug)]
pub enum SourceBody {
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug)]
pub struct SourceRefresh {
    pub hash: String,
    pub hash_changed: bool,
    pub applied: bool,
    pub warnings: Vec<RowWarning>,
    pub body: Option<SourceBody>,
}

#[derive(Clone, Debug)]
pub struct PendingSourceUpdate {
    pub index: usize,
    pub name: String,
    pub hash: String,
    pub body: SourceBody,
}

pub fn load_source(src: &mut DataSource) -> Result<Vec<RowWarning>, CommandError> {
    if is_xlsx(src) {
        let bytes = read_source_bytes(src)?;
        apply_source_bytes(src, &bytes)
    } else {
        let content = read_source_content(src)?;
        apply_source_content(src, &content)
    }
}

pub fn content_hash(content: &str) -> String {
    format!("{:x}", hash_bytes(content.as_bytes()))
}

pub fn content_hash_bytes(data: &[u8]) -> String {
    format!("{:x}", hash_bytes(data))
}

pub fn hash_changed(src: &DataSource, content: &str) -> bool {
    let hash = content_hash(content);
    !src.fetched_hash.is_empty() && src.fetched_hash != hash
}

pub fn apply_source_content(src: &mut DataSource, content: &str) -> Result<Vec<RowWarning>, CommandError> {
    if is_xlsx(src) {
        return apply_source_bytes(src, content.as_bytes());
    }
    let warnings = parse_content(src, content)?;
    src.fetched_hash = content_hash(content);
    Ok(warnings)
}

pub fn apply_source_bytes(src: &mut DataSource, bytes: &[u8]) -> Result<Vec<RowWarning>, CommandError> {
    if is_xlsx(src) {
        let warnings = parse_xlsx_bytes(src, bytes)?;
        src.fetched_hash = content_hash_bytes(bytes);
        Ok(warnings)
    } else {
        let content = decode_text(bytes)?;
        apply_source_content(src, &content)
    }
}

pub fn confirm_source_update(src: &mut DataSource, content: &str) -> Result<Vec<RowWarning>, CommandError> {
    apply_source_content(src, content)
}

pub fn confirm_source_body(src: &mut DataSource, body: &SourceBody) -> Result<Vec<RowWarning>, CommandError> {
    match body {
        SourceBody::Text(content) => apply_source_content(src, content),
        SourceBody::Bytes(bytes) => apply_source_bytes(src, bytes),
    }
}

pub fn refresh_source(src: &mut DataSource) -> Result<SourceRefresh, CommandError> {
    if is_xlsx(src) {
        let bytes = read_source_bytes(src)?;
        refresh_source_from_bytes(src, &bytes)
    } else {
        let content = read_source_content(src)?;
        refresh_source_from_content(src, &content)
    }
}

pub fn refresh_source_from_content(src: &mut DataSource, content: &str) -> Result<SourceRefresh, CommandError> {
    let hash = content_hash(content);
    let changed = !src.fetched_hash.is_empty() && src.fetched_hash != hash;
    if changed {
        Ok(SourceRefresh {
            hash,
            hash_changed: true,
            applied: false,
            warnings: Vec::new(),
            body: Some(SourceBody::Text(content.to_string())),
        })
    } else {
        let warnings = apply_source_content(src, content)?;
        Ok(SourceRefresh {
            hash,
            hash_changed: false,
            applied: true,
            warnings,
            body: None,
        })
    }
}

pub fn refresh_source_from_bytes(src: &mut DataSource, bytes: &[u8]) -> Result<SourceRefresh, CommandError> {
    let hash = if is_xlsx(src) {
        content_hash_bytes(bytes)
    } else {
        content_hash(&decode_text(bytes)?)
    };
    let changed = !src.fetched_hash.is_empty() && src.fetched_hash != hash;
    if changed {
        let body = if is_xlsx(src) {
            SourceBody::Bytes(bytes.to_vec())
        } else {
            SourceBody::Text(decode_text(bytes)?)
        };
        Ok(SourceRefresh {
            hash,
            hash_changed: true,
            applied: false,
            warnings: Vec::new(),
            body: Some(body),
        })
    } else {
        let warnings = apply_source_bytes(src, bytes)?;
        Ok(SourceRefresh {
            hash,
            hash_changed: false,
            applied: true,
            warnings,
            body: None,
        })
    }
}

pub fn refresh_source_by_name(sources: &mut [DataSource], name: &str) -> Result<SourceRefresh, CommandError> {
    let src = sources
        .iter_mut()
        .find(|src| src.name == name)
        .ok_or_else(|| CommandError::invalid_params())?;
    refresh_source(src)
}

pub fn sources_needing_update(sources: &[DataSource]) -> Result<Vec<PendingSourceUpdate>, CommandError> {
    let mut pending = Vec::new();
    for (index, src) in sources.iter().enumerate() {
        if !src.enabled {
            continue;
        }
        if is_xlsx(src) {
            match read_source_bytes(src) {
                Ok(bytes) => {
                    let hash = content_hash_bytes(&bytes);
                    if !src.fetched_hash.is_empty() && src.fetched_hash != hash {
                        pending.push(PendingSourceUpdate {
                            index,
                            name: src.name.clone(),
                            hash,
                            body: SourceBody::Bytes(bytes),
                        });
                    }
                }
                Err(err) if err.code == "NOT_IMPLEMENTED" => {}
                Err(err) => return Err(err),
            }
        } else {
            match read_source_content(src) {
                Ok(content) => {
                    if hash_changed(src, &content) {
                        pending.push(PendingSourceUpdate {
                            index,
                            name: src.name.clone(),
                            hash: content_hash(&content),
                            body: SourceBody::Text(content),
                        });
                    }
                }
                Err(err) if err.code == "NOT_IMPLEMENTED" => {}
                Err(err) => return Err(err),
            }
        }
    }
    Ok(pending)
}

pub fn set_xlsx_sheet(src: &mut DataSource, sheet: &str) -> Result<(), CommandError> {
    match &mut src.format {
        FileFormat::Xlsx { sheet: current } => {
            *current = sheet.to_string();
            Ok(())
        }
        _ => Err(CommandError::invalid_params()),
    }
}

fn is_xlsx(src: &DataSource) -> bool {
    matches!(src.format, FileFormat::Xlsx { .. })
}

fn hash_bytes(data: &[u8]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

fn read_source_content(src: &DataSource) -> Result<String, CommandError> {
    let bytes = read_source_bytes(src)?;
    decode_text(&bytes)
}

fn read_source_bytes(src: &DataSource) -> Result<Vec<u8>, CommandError> {
    match &src.kind {
        SourceKind::File { path } => fs::read(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                file_error("NOT_FOUND", "file_missing", path)
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                file_error("MERGE_FETCH_FAILED", "permission", path)
            } else {
                file_error("MERGE_FETCH_FAILED", "read", path)
            }
        }),
        SourceKind::Url { url } => fetch_url_bytes(url).map(|(bytes, _)| bytes),
    }
}

fn file_error(code: &str, reason: &str, path: &std::path::Path) -> CommandError {
    CommandError {
        code: code.to_string(),
        message_key: format!("error.merge.{}.{}", reason, path.display()),
    }
}

fn decode_text(bytes: &[u8]) -> Result<String, CommandError> {
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(bytes);
    String::from_utf8(body.to_vec()).map_err(|_| CommandError::validation("INVALID_PARAMS"))
}

fn merge_error(code: &str, reason: &str) -> CommandError {
    CommandError {
        code: code.to_string(),
        message_key: format!("error.merge.{}", reason),
    }
}

pub fn url_format(url: &str, content_type: &str) -> Option<FileFormat> {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let extension = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if content_type.contains("application/json") || extension == "json" {
        Some(FileFormat::Json)
    } else if content_type.contains("spreadsheet") || extension == "xlsx" || extension == "xls" {
        Some(FileFormat::Xlsx { sheet: String::new() })
    } else if content_type.contains("text/csv") || extension == "csv" {
        Some(FileFormat::Csv { delimiter: ',', quote: Some('"') })
    } else if content_type.contains("tab-separated") || extension == "tsv" {
        Some(FileFormat::Csv { delimiter: '\t', quote: Some('"') })
    } else {
        None
    }
}

fn fetch_url_bytes(url: &str) -> Result<(Vec<u8>, String), CommandError> {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("https://") && !lower.starts_with("http://") {
        return Err(merge_error("INVALID_PARAMS", "url_scheme"));
    }
    match ureq::get(url).call() {
        Ok(resp) => {
            let content_type = resp.content_type().to_string();
            let mut bytes = Vec::new();
            resp.into_reader()
                .read_to_end(&mut bytes)
                .map_err(|_| merge_error("MERGE_FETCH_FAILED", "read"))?;
            if url_format(url, &content_type).is_none() && url_format(url, "").is_none() {
                return Err(merge_error("MERGE_FETCH_FAILED", "content_type"));
            }
            Ok((bytes, content_type))
        }
        Err(ureq::Error::Status(_, _)) => Err(CommandError {
            code: "MERGE_FETCH_FAILED".to_string(),
            message_key: "merge.err_url".to_string(),
        }),
        Err(_) => Err(CommandError {
            code: "MERGE_FETCH_FAILED".to_string(),
            message_key: "merge.err_url".to_string(),
        }),
    }
}

fn parse_content(src: &mut DataSource, content: &str) -> Result<Vec<RowWarning>, CommandError> {
    let mut warnings = Vec::new();
    let format = src.format.clone();
    match &format {
        FileFormat::Csv { delimiter, quote } => {
            let delimiter = if *delimiter == '\0' { detect_delimiter(content, *quote) } else { *delimiter };
            parse_csv(src, content, delimiter, *quote, &mut warnings)?;
        }
        FileFormat::Json => {
            parse_json(src, content, &mut warnings)?;
        }
        FileFormat::Xlsx { .. } => {
            return Err(merge_error("INVALID_PARAMS", "xlsx_text"));
        }
    }
    Ok(warnings)
}

fn parse_csv(
    src: &mut DataSource,
    content: &str,
    delimiter: char,
    quote: Option<char>,
    warnings: &mut Vec<RowWarning>,
) -> Result<(), CommandError> {
    let mut rows = Vec::new();
    let mut current_row = Vec::new();
    let mut current_field = String::new();
    let mut in_quotes = false;
    let mut chars = content.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if Some(ch) == quote {
                if chars.peek() == Some(&ch) {
                    current_field.push(ch);
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                current_field.push(ch);
            }
        } else {
            match ch {
                c if Some(c) == quote => {
                    in_quotes = true;
                }
                c if c == delimiter => {
                    current_row.push(current_field.clone());
                    current_field.clear();
                }
                '\n' | '\r' => {
                    if !current_field.is_empty() || !current_row.is_empty() {
                        current_row.push(current_field.clone());
                        current_field.clear();
                        rows.push(current_row.clone());
                        current_row.clear();
                    }
                }
                _ => {
                    current_field.push(ch);
                }
            }
        }
    }

    if !current_field.is_empty() || !current_row.is_empty() {
        current_row.push(current_field.clone());
        rows.push(current_row);
    }

    if rows.is_empty() {
        return Err(merge_error("MERGE_NO_DATA", "empty_file"));
    }

    let field_count;
    src.fields.clear();
    let start_index = if src.include_headers {
        src.fields = rows[0].clone();
        field_count = src.fields.len();
        1
    } else {
        field_count = rows[0].len();
        for i in 0..field_count {
            src.fields.push(format!("Field {}", i + 1));
        }
        0
    };

    src.records.clear();
    for (i, row) in rows.iter().enumerate().skip(start_index) {
        if row.len() != field_count {
            warnings.push(RowWarning {
                row_index: i,
                message: format!("Expected {} fields, got {}", field_count, row.len()),
            });
        }
        src.records.push(row.clone());
    }

    Ok(())
}

fn parse_json(
    src: &mut DataSource,
    content: &str,
    warnings: &mut Vec<RowWarning>,
) -> Result<(), CommandError> {
    let value: serde_json::Value = serde_json::from_str(content).map_err(|e| CommandError {
        code: "INVALID_PARAMS".to_string(),
        message_key: format!("error.merge.json_parse.{}.{}", e.line(), e.column()),
    })?;

    let array = match value {
        serde_json::Value::Array(arr) => arr,
        _ => return Err(merge_error("INVALID_PARAMS", "json_root")),
    };

    if array.is_empty() {
        return Err(merge_error("MERGE_NO_DATA", "empty_file"));
    }

    let mut all_fields = std::collections::HashSet::new();

    for (row_index, item) in array.iter().enumerate() {
        if let serde_json::Value::Object(map) = item {
            for key in map.keys() {
                all_fields.insert(key.clone());
            }
        } else {
            warnings.push(RowWarning {
                row_index,
                message: "JSON array contains non-object items".to_string(),
            });
        }
    }

    src.fields = all_fields.into_iter().collect();
    src.fields.sort();

    src.records.clear();
    for item in array.iter() {
        let mut record = Vec::new();
        if let serde_json::Value::Object(map) = item {
            for field in &src.fields {
                let val = map.get(field).unwrap_or(&serde_json::Value::Null);
                let text = match val {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Null => String::new(),
                    _ => val.to_string(),
                };
                record.push(text);
            }
        } else {
            for _ in &src.fields {
                record.push(String::new());
            }
        }
        src.records.push(record);
    }

    Ok(())
}

fn parse_xlsx_bytes(src: &mut DataSource, bytes: &[u8]) -> Result<Vec<RowWarning>, CommandError> {
    let entries = read_zip_entries(bytes)?;
    let workbook = zip_text(&entries, "xl/workbook.xml")
        .ok_or_else(|| merge_error("INVALID_PARAMS", "xlsx_workbook"))?;
    let rels = zip_text(&entries, "xl/_rels/workbook.xml.rels")
        .ok_or_else(|| merge_error("INVALID_PARAMS", "xlsx_rels"))?;
    let sheets = workbook_sheets(&workbook, &rels)?;
    if sheets.is_empty() {
        return Err(merge_error("MERGE_NO_DATA", "empty_file"));
    }
    let wanted = match &src.format {
        FileFormat::Xlsx { sheet } => sheet.clone(),
        _ => String::new(),
    };
    let target = select_sheet(&sheets, &wanted)
        .ok_or_else(|| merge_error("INVALID_PARAMS", "xlsx_sheet"))?;
    let sheet_xml = zip_text(&entries, &target)
        .ok_or_else(|| merge_error("INVALID_PARAMS", "xlsx_sheet"))?;
    let shared = zip_text(&entries, "xl/sharedStrings.xml")
        .map(|xml| parse_shared_strings(&xml))
        .unwrap_or_default();
    let mut warnings = Vec::new();
    fill_xlsx_records(src, &sheet_xml, &shared, &mut warnings)?;
    Ok(warnings)
}

fn read_zip_entries(bytes: &[u8]) -> Result<HashMap<String, Vec<u8>>, CommandError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| merge_error("INVALID_PARAMS", "xlsx_zip"))?;
    let mut entries = HashMap::new();
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|_| merge_error("INVALID_PARAMS", "xlsx_zip"))?;
        let name = match entry.name() {
            Ok(n) => n.replace('\\', "/"),
            Err(_) => continue,
        };
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|_| merge_error("INVALID_PARAMS", "xlsx_zip"))?;
        entries.insert(name, buf);
    }
    Ok(entries)
}

fn zip_text(entries: &HashMap<String, Vec<u8>>, path: &str) -> Option<String> {
    let wanted = path.replace('\\', "/");
    entries.iter().find_map(|(name, data)| {
        let normalized = name.replace('\\', "/");
        if normalized == wanted || normalized.eq_ignore_ascii_case(&wanted) {
            Some(String::from_utf8_lossy(data).into_owned())
        } else {
            None
        }
    })
}

fn workbook_sheets(workbook: &str, rels: &str) -> Result<Vec<(String, String)>, CommandError> {
    let mut targets = HashMap::new();
    for rel in xml_elems(rels, "Relationship") {
        if let (Some(id), Some(target)) = (xml_attr(&rel, "Id"), xml_attr(&rel, "Target")) {
            targets.insert(id, resolve_xl_target(&target));
        }
    }
    let mut sheets = Vec::new();
    for sheet in xml_elems(workbook, "sheet") {
        let name = xml_attr(&sheet, "name").unwrap_or_default();
        let rid = xml_attr(&sheet, "r:id")
            .or_else(|| xml_attr(&sheet, "id"))
            .unwrap_or_default();
        if let Some(target) = targets.get(&rid) {
            sheets.push((name, target.clone()));
        }
    }
    Ok(sheets)
}

fn resolve_xl_target(target: &str) -> String {
    let trimmed = target.trim().replace('\\', "/");
    if trimmed.starts_with("/xl/") {
        trimmed.trim_start_matches('/').to_string()
    } else if trimmed.starts_with('/') {
        format!("xl{}", trimmed)
    } else if trimmed.starts_with("xl/") {
        trimmed
    } else {
        format!("xl/{}", trimmed.trim_start_matches("./"))
    }
}

fn select_sheet(sheets: &[(String, String)], wanted: &str) -> Option<String> {
    let wanted = wanted.trim();
    if wanted.is_empty() {
        return sheets.first().map(|(_, path)| path.clone());
    }
    if let Some((_, path)) = sheets.iter().find(|(name, _)| name == wanted) {
        return Some(path.clone());
    }
    if let Some((_, path)) = sheets
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
    {
        return Some(path.clone());
    }
    if let Ok(index) = wanted.parse::<usize>() {
        if index >= 1 {
            return sheets.get(index - 1).map(|(_, path)| path.clone());
        }
    }
    None
}

fn parse_shared_strings(xml: &str) -> Vec<String> {
    xml_elems(xml, "si")
        .into_iter()
        .map(|si| {
            let parts = xml_elems(&si.inner, "t");
            if parts.is_empty() {
                xml_text(&si.inner)
            } else {
                parts.into_iter().map(|t| xml_text(&t.inner)).collect::<String>()
            }
        })
        .collect()
}

fn fill_xlsx_records(
    src: &mut DataSource,
    sheet_xml: &str,
    shared: &[String],
    warnings: &mut Vec<RowWarning>,
) -> Result<(), CommandError> {
    let mut numbered: Vec<(usize, Vec<(usize, String)>)> = Vec::new();
    for row in xml_elems(sheet_xml, "row") {
        let row_number = xml_attr(&row, "r")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(numbered.len() + 1);
        let mut cells = Vec::new();
        for cell in xml_elems(&row.inner, "c") {
            let refer = xml_attr(&cell, "r").unwrap_or_default();
            let col = column_index(&refer);
            let kind = xml_attr(&cell, "t").unwrap_or_default();
            let value = cell_value(&cell, &kind, shared);
            cells.push((col, value));
        }
        numbered.push((row_number, cells));
    }
    numbered.sort_by_key(|(n, _)| *n);
    if numbered.is_empty() {
        return Err(merge_error("MERGE_NO_DATA", "empty_file"));
    }
    let width = numbered
        .iter()
        .flat_map(|(_, cells)| cells.iter().map(|(col, _)| *col + 1))
        .max()
        .unwrap_or(0);
    if width == 0 {
        return Err(merge_error("MERGE_NO_DATA", "empty_file"));
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (_, cells) in numbered {
        let mut row = vec![String::new(); width];
        for (col, value) in cells {
            if col < width {
                row[col] = value;
            }
        }
        rows.push(row);
    }
    src.fields.clear();
    src.records.clear();
    let start_index = if src.include_headers {
        src.fields = rows[0].clone();
        1
    } else {
        for i in 0..width {
            src.fields.push(format!("Field {}", i + 1));
        }
        0
    };
    for (i, row) in rows.iter().enumerate().skip(start_index) {
        if row.len() != src.fields.len() {
            warnings.push(RowWarning {
                row_index: i,
                message: format!("Expected {} fields, got {}", src.fields.len(), row.len()),
            });
        }
        src.records.push(row.clone());
    }
    Ok(())
}

fn cell_value(cell: &XmlElem, kind: &str, shared: &[String]) -> String {
    if kind == "s" {
        let index = xml_elems(&cell.inner, "v")
            .into_iter()
            .next()
            .map(|v| xml_text(&v.inner))
            .unwrap_or_else(|| xml_text(&cell.inner));
        return index
            .parse::<usize>()
            .ok()
            .and_then(|i| shared.get(i).cloned())
            .unwrap_or_default();
    }
    if kind == "inlineStr" || kind == "str" {
        let texts = xml_elems(&cell.inner, "t");
        if texts.is_empty() {
            if let Some(v) = xml_elems(&cell.inner, "v").into_iter().next() {
                return xml_text(&v.inner);
            }
            return xml_text(&cell.inner);
        }
        return texts.into_iter().map(|t| xml_text(&t.inner)).collect();
    }
    if let Some(v) = xml_elems(&cell.inner, "v").into_iter().next() {
        return xml_text(&v.inner);
    }
    xml_text(&cell.inner)
}

fn column_index(refer: &str) -> usize {
    let mut n = 0usize;
    for ch in refer.chars() {
        if ch.is_ascii_alphabetic() {
            n = n * 26 + (ch.to_ascii_uppercase() as usize - ('A' as usize) + 1);
        } else {
            break;
        }
    }
    n.saturating_sub(1)
}

struct XmlElem {
    attrs: Vec<(String, String)>,
    inner: String,
}

fn xml_local(name: &str) -> &str {
    name.rsplit_once(':').map(|(_, local)| local).unwrap_or(name)
}

fn xml_attr(elem: &XmlElem, name: &str) -> Option<String> {
    elem.attrs.iter().find_map(|(key, value)| {
        if key == name || xml_local(key) == name || key.eq_ignore_ascii_case(name) || xml_local(key).eq_ignore_ascii_case(name)
        {
            Some(value.clone())
        } else {
            None
        }
    })
}

fn xml_unescape(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' {
            let rest: String = chars[i + 1..].iter().collect();
            if rest.starts_with("amp;") {
                out.push('&');
                i += 5;
                continue;
            }
            if rest.starts_with("lt;") {
                out.push('<');
                i += 4;
                continue;
            }
            if rest.starts_with("gt;") {
                out.push('>');
                i += 4;
                continue;
            }
            if rest.starts_with("quot;") {
                out.push('"');
                i += 6;
                continue;
            }
            if rest.starts_with("apos;") {
                out.push('\'');
                i += 6;
                continue;
            }
            if rest.starts_with('#') {
                if let Some(end) = rest.find(';') {
                    let body = &rest[1..end];
                    let cp = if let Some(hex) = body.strip_prefix(['x', 'X']) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        body.parse::<u32>().ok()
                    };
                    if let Some(ch) = cp.and_then(char::from_u32) {
                        out.push(ch);
                        i += end + 2;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn xml_text(xml: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    let bytes = xml.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if let Some(rel) = xml[i..].find('>') {
                i += rel + 1;
                continue;
            }
            break;
        }
        let next = xml[i..].find('<').unwrap_or(xml.len() - i);
        out.push_str(&xml[i..i + next]);
        i += next;
    }
    xml_unescape(&out)
}

fn xml_elems(xml: &str, want: &str) -> Vec<XmlElem> {
    let mut out = Vec::new();
    let bytes = xml.as_bytes();
    let mut i = 0;
    while i < xml.len() {
        let Some(rel) = xml[i..].find('<') else {
            break;
        };
        i += rel;
        if i + 1 >= xml.len() {
            break;
        }
        let next = xml.as_bytes()[i + 1];
        if next == b'/' || next == b'!' || next == b'?' {
            if let Some(end) = xml[i..].find('>') {
                i += end + 1;
                continue;
            }
            break;
        }
        let name_start = i + 1;
        let mut name_end = name_start;
        while name_end < xml.len() {
            let ch = xml.as_bytes()[name_end];
            if ch.is_ascii_whitespace() || ch == b'>' || ch == b'/' {
                break;
            }
            name_end += 1;
        }
        let qname = &xml[name_start..name_end];
        if xml_local(qname) != want {
            i = name_end;
            continue;
        }
        let Some(tag_end_rel) = xml[name_end..].find('>') else {
            break;
        };
        let tag_end = name_end + tag_end_rel;
        let attr_src = &xml[name_end..tag_end];
        let self_close = xml[..tag_end].trim_end().ends_with('/') || bytes[tag_end.saturating_sub(1)] == b'/';
        let attrs = parse_attrs(attr_src);
        if self_close {
            out.push(XmlElem {
                attrs,
                inner: String::new(),
            });
            i = tag_end + 1;
            continue;
        }
        let close = format!("</{}>", qname);
        let close_local = format!("</{}>", want);
        let rest = &xml[tag_end + 1..];
        let close_at = rest.find(&close).or_else(|| rest.find(&close_local));
        if let Some(pos) = close_at {
            let inner = rest[..pos].to_string();
            let close_len = if rest[pos..].starts_with(&close) {
                close.len()
            } else {
                close_local.len()
            };
            out.push(XmlElem { attrs, inner });
            i = tag_end + 1 + pos + close_len;
        } else {
            i = tag_end + 1;
        }
    }
    out
}

fn parse_attrs(src: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == '/') {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let start = i;
        while i < chars.len() && chars[i] != '=' && !chars[i].is_whitespace() {
            i += 1;
        }
        if start == i {
            i += 1;
            continue;
        }
        let name: String = chars[start..i].iter().collect();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '=' {
            continue;
        }
        i += 1;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let quote = chars[i];
        if quote != '"' && quote != '\'' {
            continue;
        }
        i += 1;
        let value_start = i;
        while i < chars.len() && chars[i] != quote {
            i += 1;
        }
        let raw: String = chars[value_start..i].iter().collect();
        if i < chars.len() {
            i += 1;
        }
        attrs.push((name, xml_unescape(&raw)));
    }
    attrs
}

fn detect_delimiter(content: &str, quote: Option<char>) -> char {
    let line = first_line(content, quote);
    let mut best = ',';
    let mut best_count = 0;
    for candidate in [',', ';', '\t', ' '] {
        let count = line.chars().filter(|c| *c == candidate).count();
        if count > best_count {
            best = candidate;
            best_count = count;
        }
    }
    best
}

fn first_line(content: &str, quote: Option<char>) -> String {
    let mut line = String::new();
    let mut in_quotes = false;
    for ch in content.chars() {
        if Some(ch) == quote {
            in_quotes = !in_quotes;
            continue;
        }
        if !in_quotes && (ch == '\n' || ch == '\r') {
            if line.is_empty() {
                continue;
            }
            break;
        }
        if !in_quotes {
            line.push(ch);
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_merge::{PageRepeat, RecordRange};
    use std::io::{Cursor, Write};
    use std::path::PathBuf;
    use zip::write::{FileOptions, SimpleFileOptions};
    use zip::CompressionMethod;
    use zip::ZipWriter;

    fn blank_source(kind: SourceKind, format: FileFormat) -> DataSource {
        DataSource {
            name: "src".to_string(),
            kind,
            format,
            fields: Vec::new(),
            records: Vec::new(),
            enabled: true,
            include_headers: true,
            numbers_as_text: false,
            case_sensitive: false,
            records_range: RecordRange::All,
            pages: PageRepeat::All,
            rules: Vec::new(),
            scripts: Vec::new(),
            fetched_hash: String::new(),
        }
    }

    fn write_xlsx(entries: &[(&str, &str)]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let options: SimpleFileOptions = FileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, body) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn two_sheet_book() -> Vec<u8> {
        let workbook = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets>
<sheet name="Sheet1" sheetId="1" r:id="rId1"/>
<sheet name="Sheet2" sheetId="2" r:id="rId2"/>
</sheets>
</workbook>"#;
        let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>
</Relationships>"#;
        let shared = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="5" uniqueCount="5">
<si><t>skip</t></si>
<si><t>name</t></si>
<si><t>age</t></si>
<si><t>Ada</t></si>
<si><t>Bob</t></si>
</sst>"#;
        let sheet1 = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c></row>
</sheetData>
</worksheet>"#;
        let sheet2 = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1"><c r="A1" t="s"><v>1</v></c><c r="B1" t="s"><v>2</v></c></row>
<row r="2"><c r="A2" t="s"><v>3</v></c><c r="B2"><v>30</v></c></row>
<row r="3"><c r="A3" t="s"><v>4</v></c><c r="B3"><v>41</v></c></row>
</sheetData>
</worksheet>"#;
        write_xlsx(&[
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", rels),
            ("xl/sharedStrings.xml", shared),
            ("xl/worksheets/sheet1.xml", sheet1),
            ("xl/worksheets/sheet2.xml", sheet2),
        ])
    }

    #[test]
    fn json_array_collects_union_fields_a_and_b() {
        let mut src = blank_source(
            SourceKind::Url {
                url: "https://example.com/data.json".to_string(),
            },
            FileFormat::Json,
        );
        let warnings = apply_source_content(&mut src, r#"[{"a":1},{"a":2,"b":"x"}]"#).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(src.fields, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(src.records.len(), 2);
        assert_eq!(src.records[0], vec!["1".to_string(), String::new()]);
        assert_eq!(src.records[1], vec!["2".to_string(), "x".to_string()]);
    }

    #[test]
    fn changed_url_body_sets_hash_changed_without_applying() {
        let mut src = blank_source(
            SourceKind::Url {
                url: "https://example.com/data.json".to_string(),
            },
            FileFormat::Json,
        );
        apply_source_content(&mut src, r#"[{"a":1}]"#).unwrap();
        let first = src.fetched_hash.clone();
        assert!(!first.is_empty());
        let result = refresh_source_from_content(&mut src, r#"[{"a":2}]"#).unwrap();
        assert!(result.hash_changed);
        assert!(!result.applied);
        assert!(result.body.is_some());
        assert_eq!(src.fetched_hash, first);
        assert_eq!(src.records[0][0], "1");
        confirm_source_update(&mut src, r#"[{"a":2}]"#).unwrap();
        assert_ne!(src.fetched_hash, first);
        assert_eq!(src.records[0][0], "2");
    }

    #[test]
    fn xlsx_sheet_2_loads_its_rows() {
        let bytes = two_sheet_book();
        let mut src = blank_source(
            SourceKind::File {
                path: PathBuf::from("book.xlsx"),
            },
            FileFormat::Xlsx {
                sheet: "2".to_string(),
            },
        );
        let warnings = apply_source_bytes(&mut src, &bytes).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(src.fields, vec!["name".to_string(), "age".to_string()]);
        assert_eq!(src.records.len(), 2);
        assert_eq!(src.records[0], vec!["Ada".to_string(), "30".to_string()]);
        assert_eq!(src.records[1], vec!["Bob".to_string(), "41".to_string()]);
    }

    #[test]
    fn set_xlsx_sheet_stores_sheet_name() {
        let mut src = blank_source(
            SourceKind::File {
                path: PathBuf::from("book.xlsx"),
            },
            FileFormat::Xlsx {
                sheet: String::new(),
            },
        );
        set_xlsx_sheet(&mut src, "Sheet2").unwrap();
        match src.format {
            FileFormat::Xlsx { sheet } => assert_eq!(sheet, "Sheet2"),
            _ => panic!("expected xlsx"),
        }
    }
}
