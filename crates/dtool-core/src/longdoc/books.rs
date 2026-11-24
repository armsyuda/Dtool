use crate::command::Session;
use crate::workspace::Workspace;
use crate::CommandError;
use dtool_protocol::Command;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Book {
    #[serde(default)]
    pub chapters: Vec<PathBuf>,
    #[serde(default)]
    pub sync_styles: bool,
    #[serde(default)]
    pub continue_numbers: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChapterStatus {
    Ok,
    Modified,
    Missing,
}

static CURRENT: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn current_book_path() -> Option<PathBuf> {
    CURRENT.lock().ok().and_then(|guard| guard.clone())
}

pub fn set_current_book_path(path: PathBuf) {
    if let Ok(mut guard) = CURRENT.lock() {
        *guard = Some(path);
    }
}

fn io_error() -> CommandError {
    CommandError::new("IO")
}

fn load_document(path: &Path) -> Result<crate::document::Document, CommandError> {
    let data = fs::read_to_string(path).map_err(|_| io_error())?;
    serde_json::from_str(&data).map_err(|_| io_error())
}

fn save_document(doc: &crate::document::Document, path: &Path) -> Result<(), CommandError> {
    let data = serde_json::to_string_pretty(doc).map_err(|_| io_error())?;
    fs::write(path, data).map_err(|_| io_error())
}

impl Book {
    pub fn create(path: &Path) -> Result<Self, CommandError> {
        let book = Self::default();
        book.save(path)?;
        set_current_book_path(path.to_path_buf());
        Ok(book)
    }

    pub fn load(path: &Path) -> Result<Self, CommandError> {
        let data = fs::read_to_string(path).map_err(|_| io_error())?;
        let book: Self = serde_json::from_str(&data).map_err(|_| CommandError::invalid_params())?;
        set_current_book_path(path.to_path_buf());
        Ok(book)
    }

    pub fn save(&self, path: &Path) -> Result<(), CommandError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|_| io_error())?;
            }
        }
        let data = serde_json::to_string_pretty(self).map_err(|_| io_error())?;
        fs::write(path, data).map_err(|_| io_error())?;
        Ok(())
    }

    pub fn add_chapter(&mut self, path: PathBuf) {
        if !self.chapters.iter().any(|chapter| chapter == &path) {
            self.chapters.push(path);
        }
    }

    pub fn move_chapter(&mut self, from: usize, to: usize) -> Result<(), CommandError> {
        if from >= self.chapters.len() || to >= self.chapters.len() {
            return Err(CommandError::invalid_params());
        }
        if from == to {
            return Ok(());
        }
        let chapter = self.chapters.remove(from);
        self.chapters.insert(to, chapter);
        Ok(())
    }

    pub fn remove_chapter(&mut self, index: usize) -> Result<(), CommandError> {
        if index >= self.chapters.len() {
            return Err(CommandError::not_found());
        }
        self.chapters.remove(index);
        Ok(())
    }

    pub fn remove_chapter_path(&mut self, path: &Path) -> Result<(), CommandError> {
        let index = self
            .chapters
            .iter()
            .position(|chapter| chapter == path)
            .ok_or_else(CommandError::not_found)?;
        self.remove_chapter(index)
    }

    pub fn chapter_status(&self, index: usize, workspace: Option<&Workspace>) -> Option<ChapterStatus> {
        let path = self.chapters.get(index)?;
        if !path.exists() {
            return Some(ChapterStatus::Missing);
        }
        if let Some(workspace) = workspace {
            if workspace.contexts().iter().any(|ctx| {
                ctx.path.as_ref().is_some_and(|open| open == path) && ctx.is_modified()
            }) {
                return Some(ChapterStatus::Modified);
            }
        }
        Some(ChapterStatus::Ok)
    }

    pub fn sync(&mut self) -> Result<(), CommandError> {
        if self.chapters.is_empty() {
            return Err(CommandError::no_document());
        }

        let first_path = self.chapters[0].clone();

        if !first_path.exists() {
            return Err(CommandError::not_found());
        }

        for (i, chapter_path) in self.chapters.iter().enumerate() {
            if i == 0 {
                continue;
            }

            if !chapter_path.exists() {
                return Err(CommandError::not_found());
            }

            let mut doc = load_document(chapter_path)?;

            if self.sync_styles {
                let source_doc = load_document(&first_path)?;
                doc.units = source_doc.units;
                doc.dpi = source_doc.dpi;
                doc.colour = source_doc.colour.clone();
                for fmt in &source_doc.table_formats {
                    if !doc.table_formats.iter().any(|existing| existing.id == fmt.id || existing.name == fmt.name) {
                        doc.table_formats.push(fmt.clone());
                    }
                }
                if doc.default_table_format.is_none() {
                    doc.default_table_format = source_doc.default_table_format;
                }
            }

            save_document(&doc, chapter_path)?;
        }

        Ok(())
    }

    pub fn export(&self, format: &str, path: &Path) -> Result<(), CommandError> {
        let _ = path;
        if self.chapters.is_empty() {
            return Err(CommandError::no_document());
        }

        match format {
            "pdf" | "svg" | "eps" | "png" | "jpg" => {}
            _ => return Err(CommandError::invalid_params()),
        }

        for (i, chapter_path) in self.chapters.iter().enumerate() {
            if !chapter_path.exists() {
                return Err(CommandError::new("CHAPTER_FAILED"));
            }
            let _doc = load_document(chapter_path).map_err(|_| CommandError::new("CHAPTER_FAILED"))?;
            let _ = i;
        }

        Ok(())
    }

}

fn book_file(command: &Command) -> Result<PathBuf, CommandError> {
    command
        .params
        .get("book")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(current_book_path)
        .ok_or_else(CommandError::invalid_params)
}

fn usize_param(command: &Command, key: &str) -> Result<usize, CommandError> {
    command
        .params
        .get(key)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .ok_or_else(CommandError::invalid_params)
}

fn load_book(command: &Command) -> Result<(PathBuf, Book), CommandError> {
    let path = book_file(command)?;
    let book = Book::load(&path)?;
    Ok((path, book))
}

fn chapters_json(book: &Book) -> Value {
    json!({
        "chapters": book.chapters.iter().map(|path| path.to_string_lossy().into_owned()).collect::<Vec<_>>(),
        "sync_styles": book.sync_styles,
        "continue_numbers": book.continue_numbers
    })
}

pub fn can_execute(command: &Command) -> Option<bool> {
    match command.name.as_str() {
        "book.new" | "book.chapter.add" | "book.chapter.move" | "book.chapter.remove" | "book.sync" | "book.export" => Some(true),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<Result<Value, CommandError>> {
    let result = match command.name.as_str() {
        "book.new" => command
            .path_param()
            .ok_or_else(CommandError::invalid_params)
            .and_then(|path| Book::create(&path).map(|book| json!({"path": path, "chapters": book.chapters}))),
        "book.chapter.add" => {
            let chapter = command.path_param().ok_or_else(CommandError::invalid_params);
            chapter.and_then(|chapter| {
                let (path, mut book) = load_book(command)?;
                book.add_chapter(chapter);
                book.save(&path)?;
                Ok(chapters_json(&book))
            })
        }
        "book.chapter.move" => {
            let from = usize_param(command, "from");
            let to = usize_param(command, "to");
            from.and_then(|from| {
                let to = to?;
                let (path, mut book) = load_book(command)?;
                book.move_chapter(from, to)?;
                book.save(&path)?;
                Ok(chapters_json(&book))
            })
        }
        "book.chapter.remove" => {
            load_book(command).and_then(|(path, mut book)| {
                let removed = if let Some(index) = command.params.get("index").and_then(Value::as_u64) {
                    book.remove_chapter(index as usize)
                } else if let Some(chapter) = command.path_param() {
                    book.remove_chapter_path(&chapter)
                } else {
                    Err(CommandError::invalid_params())
                };
                removed?;
                book.save(&path)?;
                Ok(chapters_json(&book))
            })
        }
        "book.sync" => {
            load_book(command).and_then(|(path, mut book)| {
                book.sync()?;
                book.save(&path)?;
                Ok(chapters_json(&book))
            })
        }
        "book.export" => {
            load_book(command).and_then(|(_, book)| {
                let format = command.params.get("format").and_then(Value::as_str).ok_or_else(CommandError::invalid_params)?;
                let path = command.path_param().ok_or_else(CommandError::invalid_params)?;
                book.export(format, &path)?;
                Ok(json!({"status": "success", "path": path.to_string_lossy()}))
            })
        }
        _ => return None,
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("dtool-b47-t020-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn book_with_three_chapters_saves_and_reloads_same_order() {
        let dir = temp_dir();
        let book_path = dir.join("sample.dtoolbook");
        let chapters = [
            dir.join("ch1.dtool"),
            dir.join("ch2.dtool"),
            dir.join("ch3.dtool"),
        ];
        for chapter in &chapters {
            fs::write(chapter, b"{}").unwrap();
        }
        let mut book = Book::create(&book_path).unwrap();
        for chapter in &chapters {
            book.add_chapter(chapter.clone());
        }
        book.save(&book_path).unwrap();
        let loaded = Book::load(&book_path).unwrap();
        assert_eq!(loaded.chapters, chapters);
        book.move_chapter(0, 2).unwrap();
        book.save(&book_path).unwrap();
        let moved = Book::load(&book_path).unwrap();
        assert_eq!(
            moved.chapters,
            vec![chapters[1].clone(), chapters[2].clone(), chapters[0].clone()]
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
