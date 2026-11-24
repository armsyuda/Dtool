use dtool_protocol::CommandError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportedRun {
    pub start: usize,
    pub end: usize,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub size_pt: Option<f32>,
    #[serde(default)]
    pub style: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportedText {
    pub text: String,
    pub runs: Vec<ImportedRun>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    Txt,
    Rtf,
    Docx,
}

pub fn import_format(path: &Path) -> ImportFormat {
    match path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.to_ascii_lowercase()).as_deref() {
        Some("docx") => ImportFormat::Docx,
        Some("rtf") => ImportFormat::Rtf,
        _ => ImportFormat::Txt,
    }
}

pub fn import_bytes(format: ImportFormat, bytes: &[u8]) -> Result<ImportedText, CommandError> {
    match format {
        ImportFormat::Txt => Ok(ImportedText { text: crate::text_link::decode_text(bytes), runs: Vec::new() }),
        ImportFormat::Rtf => parse_rtf(bytes),
        ImportFormat::Docx => parse_docx(bytes),
    }
}

pub fn import_file(path: &Path) -> Result<ImportedText, CommandError> {
    let bytes = std::fs::read(path).map_err(|_| CommandError::not_found())?;
    import_bytes(import_format(path), &bytes)
}

pub fn styled_ranges(base: &Value, imported: &ImportedText) -> Vec<Value> {
    let len = imported.text.chars().count();
    let mut cuts = vec![0, len];
    for run in &imported.runs {
        cuts.push(run.start.min(len));
        cuts.push(run.end.min(len));
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut out = Vec::new();
    for pair in cuts.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if end <= start {
            continue;
        }
        let mut range = base.clone();
        range["start"] = json!(start);
        range["end"] = json!(end);
        if let Some(run) = imported.runs.iter().find(|run| run.start <= start && run.end >= end) {
            if run.bold {
                range["weight"] = json!(700);
            }
            match (run.bold, run.italic) {
                (true, true) => range["style"] = json!("Bold Italic"),
                (false, true) => range["style"] = json!("Italic"),
                (true, false) => range["style"] = json!("Bold"),
                (false, false) => {}
            }
            if let Some(size) = run.size_pt {
                range["size_pt"] = json!(size);
            }
            if let Some(name) = &run.style {
                range["text_style"] = json!(name);
            }
        }
        out.push(range);
    }
    if out.is_empty() {
        let mut range = base.clone();
        range["start"] = json!(0);
        range["end"] = json!(0);
        out.push(range);
    }
    out
}

fn bad_file() -> CommandError {
    CommandError::invalid_params()
}

#[derive(Debug, Clone, Default, PartialEq)]
struct RunStyle {
    bold: bool,
    italic: bool,
    size_pt: Option<f32>,
    style: Option<String>,
}

#[derive(Default)]
struct Builder {
    text: String,
    len: usize,
    runs: Vec<ImportedRun>,
}

impl Builder {
    fn push(&mut self, chunk: &str, style: &RunStyle) {
        if chunk.is_empty() {
            return;
        }
        let start = self.len;
        self.text.push_str(chunk);
        self.len += chunk.chars().count();
        if *style == RunStyle::default() {
            return;
        }
        if let Some(last) = self.runs.last_mut() {
            if last.end == start && last.bold == style.bold && last.italic == style.italic && last.size_pt == style.size_pt && last.style == style.style {
                last.end = self.len;
                return;
            }
        }
        self.runs.push(ImportedRun { start, end: self.len, bold: style.bold, italic: style.italic, size_pt: style.size_pt, style: style.style.clone() });
    }

    fn finish(mut self) -> ImportedText {
        while self.text.ends_with('\n') {
            self.text.pop();
            self.len -= 1;
        }
        let len = self.len;
        for run in &mut self.runs {
            run.end = run.end.min(len);
        }
        self.runs.retain(|run| run.end > run.start);
        ImportedText { text: self.text, runs: self.runs }
    }
}

const RTF_DESTINATIONS: &[&str] = &[
    "fonttbl", "colortbl", "stylesheet", "info", "pict", "header", "headerl", "headerr", "headerf", "footer", "footerl", "footerr", "footerf",
    "footnote", "object", "listtable", "listoverridetable", "rsidtbl", "generator", "xmlnstbl", "themedata", "colorschememapping",
    "datastore", "latentstyles", "filetbl", "revtbl", "fldinst", "bkmkstart", "bkmkend",
];

fn parse_rtf(bytes: &[u8]) -> Result<ImportedText, CommandError> {
    let src = String::from_utf8_lossy(bytes);
    if !src.trim_start().starts_with("{\\rtf") {
        return Err(bad_file());
    }
    let chars: Vec<char> = src.chars().collect();
    let mut out = Builder::default();
    let mut stack: Vec<(RunStyle, bool, usize)> = Vec::new();
    let mut style = RunStyle::default();
    let mut skip = false;
    let mut uc = 1usize;
    let mut pending = 0usize;
    let mut group_start = false;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' => {
                stack.push((style.clone(), skip, uc));
                group_start = true;
                i += 1;
                continue;
            }
            '}' => {
                let (saved, saved_skip, saved_uc) = stack.pop().ok_or_else(bad_file)?;
                style = saved;
                skip = saved_skip;
                uc = saved_uc;
                pending = 0;
                i += 1;
            }
            '\\' => {
                i += 1;
                let next = *chars.get(i).ok_or_else(bad_file)?;
                if next.is_ascii_alphabetic() {
                    let word_start = i;
                    while i < chars.len() && chars[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let word: String = chars[word_start..i].iter().collect();
                    let num_start = i;
                    if i < chars.len() && chars[i] == '-' {
                        i += 1;
                    }
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                    let num: Option<i32> = if i > num_start { chars[num_start..i].iter().collect::<String>().parse().ok() } else { None };
                    if i < chars.len() && chars[i] == ' ' {
                        i += 1;
                    }
                    if group_start && RTF_DESTINATIONS.contains(&word.as_str()) {
                        skip = true;
                    }
                    if !skip {
                        match word.as_str() {
                            "b" => style.bold = num != Some(0),
                            "i" => style.italic = num != Some(0),
                            "fs" => style.size_pt = num.filter(|n| *n > 0).map(|n| n as f32 / 2.0),
                            "plain" => style = RunStyle::default(),
                            "par" | "line" => out.push("\n", &RunStyle::default()),
                            "tab" => out.push("\t", &style),
                            "uc" => uc = num.unwrap_or(1).max(0) as usize,
                            "u" => {
                                if let Some(n) = num {
                                    let code = if n < 0 { (n + 65536) as u32 } else { n as u32 };
                                    if let Some(ch) = char::from_u32(code) {
                                        out.push(&ch.to_string(), &style);
                                    }
                                }
                                pending = uc;
                            }
                            _ => {}
                        }
                    }
                } else if next == '*' {
                    skip = true;
                    i += 1;
                } else if next == '\'' {
                    let hex: String = chars.get(i + 1..i + 3).ok_or_else(bad_file)?.iter().collect();
                    let byte = u8::from_str_radix(&hex, 16).map_err(|_| bad_file())?;
                    i += 3;
                    if pending > 0 {
                        pending -= 1;
                    } else if !skip {
                        out.push(&char::from(byte).to_string(), &style);
                    }
                } else {
                    i += 1;
                    if !skip {
                        match next {
                            '\\' | '{' | '}' => out.push(&next.to_string(), &style),
                            '~' => out.push("\u{a0}", &style),
                            '\n' | '\r' => out.push("\n", &RunStyle::default()),
                            _ => {}
                        }
                    }
                }
                group_start = false;
                continue;
            }
            '\r' | '\n' => {
                i += 1;
            }
            _ => {
                if pending > 0 {
                    pending -= 1;
                } else if !skip {
                    out.push(&c.to_string(), &style);
                }
                i += 1;
            }
        }
        group_start = false;
    }
    if !stack.is_empty() {
        return Err(bad_file());
    }
    Ok(out.finish())
}

fn parse_docx(bytes: &[u8]) -> Result<ImportedText, CommandError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| bad_file())?;
    let document = zip_entry(&mut archive, "word/document.xml")?.ok_or_else(bad_file)?;
    let styles = zip_entry(&mut archive, "word/styles.xml")?.map(|xml| style_names(&xml)).transpose()?.unwrap_or_default();
    docx_body(&document, &styles)
}

fn zip_entry(archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Result<Option<String>, CommandError> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };
    let mut buf = String::new();
    entry.read_to_string(&mut buf).map_err(|_| bad_file())?;
    Ok(Some(buf))
}

#[derive(Clone, Copy)]
struct Wml<'a> {
    ns: Option<&'a str>,
}

impl<'a> Wml<'a> {
    fn of(doc: &'a roxmltree::Document) -> Self {
        Wml { ns: doc.root_element().tag_name().namespace() }
    }

    fn is(self, node: roxmltree::Node, name: &str) -> bool {
        node.is_element() && node.tag_name().namespace() == self.ns && node.tag_name().name() == name
    }

    fn element(self, node: roxmltree::Node) -> bool {
        node.is_element() && node.tag_name().namespace() == self.ns
    }

    fn child<'d, 'i>(self, node: roxmltree::Node<'d, 'i>, name: &str) -> Option<roxmltree::Node<'d, 'i>> {
        node.children().find(|child| self.is(*child, name))
    }

    fn attr(self, node: roxmltree::Node, name: &str) -> Option<String> {
        match self.ns {
            Some(ns) => node.attribute((ns, name)),
            None => node.attribute(name),
        }
        .map(str::to_string)
    }

    fn val(self, node: roxmltree::Node) -> Option<String> {
        self.attr(node, "val")
    }

    fn flag_on(self, node: roxmltree::Node) -> bool {
        !matches!(self.val(node).as_deref(), Some("0") | Some("false") | Some("off") | Some("none"))
    }
}

fn parse_xml(xml: &str) -> Result<roxmltree::Document<'_>, CommandError> {
    roxmltree::Document::parse(xml).map_err(|_| bad_file())
}

fn style_names(xml: &str) -> Result<HashMap<String, String>, CommandError> {
    let doc = parse_xml(xml)?;
    let w = Wml::of(&doc);
    let mut names = HashMap::new();
    for style in doc.descendants().filter(|node| w.is(*node, "style")) {
        if let (Some(id), Some(name)) = (w.attr(style, "styleId"), w.child(style, "name").and_then(|node| w.val(node))) {
            names.insert(id, name);
        }
    }
    Ok(names)
}

fn docx_body(xml: &str, styles: &HashMap<String, String>) -> Result<ImportedText, CommandError> {
    let doc = parse_xml(xml)?;
    let w = Wml::of(&doc);
    let body = doc.descendants().find(|node| w.is(*node, "body")).ok_or_else(bad_file)?;
    let style_name = |id: String| styles.get(&id).cloned().unwrap_or(id);
    let mut out = Builder::default();
    let mut paras = 0usize;
    for para in body.descendants().filter(|node| w.is(*node, "p")) {
        if paras > 0 {
            out.push("\n", &RunStyle::default());
        }
        paras += 1;
        let para_style = w.child(para, "pPr").and_then(|ppr| w.child(ppr, "pStyle")).and_then(|node| w.val(node)).map(style_name);
        let runs = para
            .descendants()
            .filter(|node| w.is(*node, "r") && node.ancestors().find(|up| w.is(*up, "p")) == Some(para));
        for r in runs {
            let mut run = RunStyle { style: para_style.clone(), ..RunStyle::default() };
            if let Some(rpr) = w.child(r, "rPr") {
                for prop in rpr.children().filter(|node| w.element(*node)) {
                    match prop.tag_name().name() {
                        "b" => run.bold = w.flag_on(prop),
                        "i" => run.italic = w.flag_on(prop),
                        "sz" => run.size_pt = w.val(prop).and_then(|v| v.parse::<f32>().ok()).map(|v| v / 2.0),
                        "rStyle" => run.style = w.val(prop).map(style_name),
                        _ => {}
                    }
                }
            }
            for item in r.children().filter(|node| w.element(*node)) {
                match item.tag_name().name() {
                    "t" => {
                        let text: String = item.children().filter_map(|child| child.text()).collect();
                        out.push(&text, &run);
                    }
                    "tab" => out.push("\t", &run),
                    "br" | "cr" => out.push("\n", &RunStyle::default()),
                    _ => {}
                }
            }
        }
    }
    Ok(out.finish())
}
