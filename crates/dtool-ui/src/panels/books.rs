use crate::Workspace;
use dtool_core::longdoc::books::{Book, ChapterStatus};
use dtool_core::workspace::Workspace as CoreWorkspace;
use dtool_strings::{format, text};
use std::path::PathBuf;

#[derive(Default)]
pub struct BooksPanel {
    pub book_path: Option<PathBuf>,
    pub book: Book,
    book_path_input: String,
    chapter_path_input: String,
}

impl BooksPanel {
    fn set_book(&mut self, path: PathBuf, book: Book) {
        self.book_path_input = path.to_string_lossy().into_owned();
        self.book_path = Some(path);
        self.book = book;
    }

    fn reload(&mut self) {
        if let Some(path) = self.book_path.clone() {
            if let Ok(book) = Book::load(&path) {
                self.set_book(path, book);
            }
        }
    }

    fn persist(&mut self) {
        if let Some(path) = self.book_path.as_ref() {
            let _ = self.book.save(path);
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) {
        ui.heading(text("panel.books.title"));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.book_path_input).desired_width(220.0));
            if dtool_design::button(ui, text("book.new")).clicked() {
                let path = PathBuf::from(self.book_path_input.trim());
                if !path.as_os_str().is_empty() {
                    if let Ok(book) = Book::create(&path) {
                        self.set_book(path, book);
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.chapter_path_input).desired_width(220.0));
            if dtool_design::button(ui, text("book.add_chapter")).clicked() {
                let chapter = PathBuf::from(self.chapter_path_input.trim());
                if !chapter.as_os_str().is_empty() && self.book_path.is_some() {
                    self.book.add_chapter(chapter);
                    self.persist();
                    self.reload();
                    self.chapter_path_input.clear();
                }
            }
        });

        let mut open: Option<PathBuf> = None;
        let mut remove: Option<usize> = None;
        let mut move_to: Option<(usize, usize)> = None;
        let last = self.book.chapters.len().saturating_sub(1);
        for index in 0..self.book.chapters.len() {
            let path = self.book.chapters[index].clone();
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("?");
            let label = match self.book.chapter_status(index, Some(workspace)) {
                Some(ChapterStatus::Modified) => format("book.modified", &[("name", name)]),
                Some(ChapterStatus::Missing) => format("book.missing", &[("name", name)]),
                _ => name.to_string(),
            };
            ui.horizontal(|ui| {
                let response = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
                if response.double_clicked() && path.exists() {
                    open = Some(path);
                }
                if index > 0 && ui.small_button("↑").clicked() {
                    move_to = Some((index, index - 1));
                }
                if index < last && ui.small_button("↓").clicked() {
                    move_to = Some((index, index + 1));
                }
                if ui.small_button("×").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(path) = open {
            let _ = workspace.open_path(&path);
        }
        if let Some((from, to)) = move_to {
            if self.book.move_chapter(from, to).is_ok() {
                self.persist();
            }
        }
        if let Some(index) = remove {
            if self.book.remove_chapter(index).is_ok() {
                self.persist();
            }
        }
    }
}

pub fn show(ui: &mut egui::Ui, workspace: &mut Workspace) {
    workspace.books_panel.show(ui, &mut workspace.core);
}
