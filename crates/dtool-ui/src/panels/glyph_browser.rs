use dtool_core::{CommandError, Session, StoryId};
use dtool_design::space;
use dtool_strings::text;
use dtool_tools::dtool_text::glyphs::{self, GlyphInfo, GlyphPage, GlyphQuery, RecentGlyphs, Subset};
use dtool_tools::dtool_text::{story_from_core, FontId, FontLibrary};
use serde_json::{json, Value};

pub const GLYPH_BROWSER_PANEL_KEY: &str = "glyph_browser";

pub struct GlyphBrowserState {
    pub font_id: Option<FontId>,
    pub font_family: String,
    pub font_style: String,
    pub lock_font_and_style: bool,
    pub subset: Subset,
    pub search_text: String,
    pub offset: usize,
    pub limit: usize,
    pub page: Option<GlyphPage>,
    pub recent: RecentGlyphs,
}

impl Default for GlyphBrowserState {
    fn default() -> Self {
        Self::new()
    }
}

impl GlyphBrowserState {
    pub fn new() -> Self {
        Self {
            font_id: None,
            font_family: String::new(),
            font_style: String::new(),
            lock_font_and_style: false,
            subset: Subset::All,
            search_text: String::new(),
            offset: 0,
            limit: 64,
            page: None,
            recent: RecentGlyphs::new(),
        }
    }

    pub fn current_query(&self) -> Option<GlyphQuery> {
        Some(GlyphQuery {
            font: self.font_id?,
            search: Some(self.search_text.trim().to_string()).filter(|s| !s.is_empty()),
            subset: self.subset.clone(),
            offset: self.offset,
            limit: self.limit.max(1),
        })
    }

    pub fn query(&mut self, lib: &FontLibrary) {
        self.page = self.current_query().and_then(|q| glyphs::query_glyphs(lib, &q, &self.recent).ok());
    }

    pub fn set_face(&mut self, lib: &FontLibrary, id: FontId) {
        if let Some(face) = lib.face(id) {
            self.font_family = face.family.clone();
            self.font_style = face.style.clone();
        }
        self.font_id = Some(id);
        self.first_page(lib);
    }

    pub fn set_subset(&mut self, lib: &FontLibrary, subset: Subset) {
        self.subset = subset;
        self.first_page(lib);
    }

    pub fn set_search(&mut self, lib: &FontLibrary, search: &str) {
        self.search_text = search.to_string();
        self.first_page(lib);
    }

    pub fn next_page(&mut self, lib: &FontLibrary) {
        if let Some(page) = &self.page {
            if self.offset + self.limit < page.total {
                self.offset += self.limit;
                self.query(lib);
            }
        }
    }

    pub fn prev_page(&mut self, lib: &FontLibrary) {
        if self.offset > 0 {
            self.offset = self.offset.saturating_sub(self.limit);
            self.query(lib);
        }
    }

    pub fn first_page(&mut self, lib: &FontLibrary) {
        self.offset = 0;
        self.query(lib);
    }

    pub fn follow_story_face(&mut self, lib: &FontLibrary, session: &Session) -> bool {
        if self.lock_font_and_style {
            return false;
        }
        let Some(id) = story_face(lib, session) else { return false };
        if self.font_id == Some(id) {
            return false;
        }
        self.set_face(lib, id);
        true
    }

    pub fn insert_at(&mut self, session: &mut Session, index: usize) -> Result<bool, CommandError> {
        let (Some(font), Some(glyph)) = (self.font_id, self.page.as_ref().and_then(|page| page.items.get(index)).cloned()) else {
            return Ok(false);
        };
        self.insert(session, font, &glyph)
    }

    pub fn insert(&mut self, session: &mut Session, font: FontId, glyph: &GlyphInfo) -> Result<bool, CommandError> {
        let (story, at) = insertion_point(session).ok_or_else(CommandError::invalid_params)?;
        glyphs::register_encoded_glyphs(font, std::slice::from_ref(glyph));
        let mut params = json!({ "story": story.0, "at": at, "font": u64::from(font.0), "gid": glyph.gid });
        if let Some(ch) = glyph.unicode {
            params["char"] = Value::from(ch.to_string());
        }
        session.run_named("text.glyph.insert", params)?;
        if let Some(edit) = session.text_edit.as_mut().filter(|edit| edit.story == story && edit.caret == at && edit.anchor == at) {
            edit.caret = at + 1;
            edit.anchor = at + 1;
        }
        self.recent.push(font, glyph.gid);
        if self.subset == Subset::Recent {
            self.page = None;
        }
        Ok(true)
    }
}

impl GlyphBrowserState {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut dtool_core::workspace::Workspace) -> bool {
        let lib = FontLibrary::shared();
        match workspace.session_mut() {
            Some(session) => render_glyph_browser(ui, self, &lib, session).unwrap_or(false),
            None => {
                ui.label(text("glyphs.no_results"));
                false
            }
        }
    }
}

pub fn insertion_point(session: &Session) -> Option<(StoryId, usize)> {
    let story = session.active_story_id()?;
    let len = session.document.stories.iter().find(|s| s.id == story)?.text.chars().count();
    let at = match &session.text_edit {
        Some(edit) if edit.story == story => edit.caret.min(len),
        _ => len,
    };
    Some((story, at))
}

pub fn story_face(lib: &FontLibrary, session: &Session) -> Option<FontId> {
    let (story_id, at) = insertion_point(session)?;
    let story = story_from_core(session.document.stories.iter().find(|s| s.id == story_id)?);
    let probe = at.saturating_sub(1);
    let run = story
        .chars
        .iter()
        .find(|run| run.range.start <= probe && probe < run.range.end)
        .or_else(|| story.chars.last())?;
    let style = &run.style;
    lib.resolve(&style.family, &style.style).or_else(|| lib.faces(&style.family).first().map(|face| face.id))
}

fn subset_label(subset: &Subset) -> String {
    match subset {
        Subset::All => text("glyphs.subset.all").to_string(),
        Subset::Recent => text("glyphs.subset.recent").to_string(),
        Subset::Block(name) => name.clone(),
    }
}

pub fn render_glyph_browser(ui: &mut egui::Ui, state: &mut GlyphBrowserState, lib: &FontLibrary, session: &mut Session) -> Result<bool, CommandError> {
    if state.font_id.is_none() || !state.lock_font_and_style {
        state.follow_story_face(lib, session);
    }
    if state.page.is_none() && state.font_id.is_some() {
        state.query(lib);
    }
    ui.heading(text("glyphs.title"));
    ui.add_space(space("sm"));

    let mut pick: Option<FontId> = None;
    ui.horizontal(|ui| {
        ui.label(text("glyphs.font"));
        egui::ComboBox::from_id_salt("glyph_browser_font").selected_text(state.font_family.clone()).show_ui(ui, |ui| {
            for family in lib.families() {
                if ui.selectable_label(family == state.font_family, family).clicked() {
                    pick = lib.resolve(family, &state.font_style).or_else(|| lib.faces(family).first().map(|face| face.id));
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label(text("glyphs.font_style"));
        egui::ComboBox::from_id_salt("glyph_browser_font_style").selected_text(state.font_style.clone()).show_ui(ui, |ui| {
            for face in lib.faces(&state.font_family) {
                if ui.selectable_label(face.style == state.font_style, &face.style).clicked() {
                    pick = Some(face.id);
                }
            }
        });
    });
    if let Some(id) = pick {
        state.set_face(lib, id);
    }
    ui.checkbox(&mut state.lock_font_and_style, text("glyphs.lock"));

    let mut subset_pick: Option<Subset> = None;
    ui.horizontal(|ui| {
        ui.label(text("glyphs.subset"));
        egui::ComboBox::from_id_salt("glyph_browser_subset").selected_text(subset_label(&state.subset)).show_ui(ui, |ui| {
            let mut options = vec![Subset::All, Subset::Recent];
            if let Some(font) = state.font_id {
                options.extend(glyphs::get_subsets(lib, font).into_iter().map(Subset::Block));
            }
            for option in options {
                if ui.selectable_label(option == state.subset, subset_label(&option)).clicked() {
                    subset_pick = Some(option);
                }
            }
        });
    });
    if let Some(subset) = subset_pick {
        state.set_subset(lib, subset);
    }

    let response = ui.add(egui::TextEdit::singleline(&mut state.search_text).hint_text(text("glyphs.search")));
    if response.changed() {
        state.first_page(lib);
    }

    let mut inserted = false;
    let Some(page) = state.page.clone() else {
        ui.add_space(space("sm"));
        ui.label(text("glyphs.no_results"));
        return Ok(false);
    };
    if page.items.is_empty() {
        ui.add_space(space("sm"));
        ui.label(text("glyphs.no_results"));
        return Ok(false);
    }
    ui.add_space(space("sm"));
    ui.label(format!("{} - {} / {}", state.offset + 1, (state.offset + state.limit).min(page.total), page.total));
    ui.horizontal(|ui| {
        if ui.button("\u{25c0}").clicked() {
            state.prev_page(lib);
        }
        if ui.button("\u{25b6}").clicked() {
            state.next_page(lib);
        }
    });
    ui.add_space(space("sm"));
    let columns = 8;
    let mut chosen = None;
    egui::Grid::new("glyph_browser_grid").num_columns(columns).spacing([space("xs"), space("xs")]).show(ui, |ui| {
        for (index, glyph) in page.items.iter().enumerate() {
            let label = glyph.unicode.map(|ch| ch.to_string()).unwrap_or_else(|| format!("#{}", glyph.gid));
            let cell = ui.add(egui::Button::new(label).min_size(egui::vec2(28.0, 28.0)));
            let cell = match glyph.name.as_deref() {
                Some(name) => cell.on_hover_text(name),
                None => cell,
            };
            if cell.double_clicked() {
                chosen = Some(index);
            }
            if (index + 1) % columns == 0 {
                ui.end_row();
            }
        }
    });
    if let Some(index) = chosen {
        if let (Some(font), Some(glyph)) = (state.font_id, page.items.get(index)) {
            inserted = state.insert(session, font, glyph)?;
        }
    }
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dtool_core::document::{Document, Units};
    use dtool_core::{ObjectId, ObjectKind};

    fn face_with(lib: &FontLibrary, ch: char) -> Option<FontId> {
        ["Malgun Gothic", "Arial", "Segoe UI", "Noto Sans KR", "Pretendard", "Inter"]
            .iter()
            .flat_map(|family| lib.faces(family).into_iter().map(|face| face.id).collect::<Vec<_>>())
            .chain(lib.families().into_iter().flat_map(|family| lib.faces(family).into_iter().map(|face| face.id).collect::<Vec<_>>()))
            .find(|id| {
                let query = GlyphQuery { font: *id, search: Some(ch.to_string()), subset: Subset::All, offset: 0, limit: 8 };
                glyphs::query_glyphs(lib, &query, &RecentGlyphs::new()).is_ok_and(|page| page.items.iter().any(|g| g.unicode == Some(ch)))
            })
    }

    fn session_with_story() -> (Session, StoryId) {
        let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
        let id = ObjectId(session.run_named("text.art.add", json!({ "pos": [10.0, 20.0], "text": "xy" })).unwrap()["id"].as_u64().unwrap());
        session.selection.ids = vec![id];
        let story = match session.document.get_object(id).unwrap().kind {
            ObjectKind::ArtText { story } => story,
            _ => unreachable!(),
        };
        (session, story)
    }

    fn story_text(session: &Session, story: StoryId) -> String {
        session.document.stories.iter().find(|s| s.id == story).unwrap().text.clone()
    }

    #[test]
    fn real_face_lists_glyphs_and_search_and_subset_narrow() {
        let lib = FontLibrary::shared();
        let Some(font) = face_with(&lib, 'A') else { return };
        let mut state = GlyphBrowserState::new();
        state.set_face(&lib, font);
        let all = state.page.clone().unwrap();
        assert!(all.total > 50 && !all.items.is_empty());
        assert_eq!(state.font_family, lib.face(font).unwrap().family);
        state.set_subset(&lib, Subset::Block("Basic Latin".into()));
        let latin = state.page.clone().unwrap();
        assert!(latin.total < all.total);
        assert!(latin.items.iter().all(|g| g.unicode.is_some_and(|c| (c as u32) < 0x80)));
        state.set_search(&lib, "A");
        let found = state.page.clone().unwrap();
        assert!(found.total < latin.total && found.items.iter().any(|g| g.unicode == Some('A')));
        state.set_search(&lib, "");
        state.limit = 10;
        state.first_page(&lib);
        let first = state.page.clone().unwrap().items.iter().map(|g| g.gid).collect::<Vec<_>>();
        state.next_page(&lib);
        assert_eq!(state.offset, 10);
        assert_ne!(state.page.clone().unwrap().items.iter().map(|g| g.gid).collect::<Vec<_>>(), first);
    }

    #[test]
    fn insert_is_one_undo_and_updates_recent() {
        let lib = FontLibrary::shared();
        let Some(font) = face_with(&lib, 'A') else { return };
        let (mut session, story) = session_with_story();
        let mut state = GlyphBrowserState::new();
        state.set_face(&lib, font);
        state.set_search(&lib, "A");
        let index = state.page.as_ref().unwrap().items.iter().position(|g| g.unicode == Some('A')).unwrap();
        let gid = state.page.as_ref().unwrap().items[index].gid;
        let depth = session.undo.applied_steps();
        assert!(state.insert_at(&mut session, index).unwrap());
        assert_eq!(story_text(&session, story), "xyA");
        assert_eq!(session.undo.applied_steps(), depth + 1);
        assert_eq!(state.recent.items.first(), Some(&(font, gid)));
        state.set_subset(&lib, Subset::Recent);
        state.set_search(&lib, "");
        assert_eq!(state.page.as_ref().unwrap().items.iter().map(|g| g.gid).collect::<Vec<_>>(), vec![gid]);
        session.run_named("edit.undo", json!({})).unwrap();
        assert_eq!(story_text(&session, story), "xy");
    }

    #[test]
    fn missing_face_shows_no_results_and_leaves_document() {
        let lib = FontLibrary::shared();
        let (mut session, story) = session_with_story();
        let version = session.document.version;
        let depth = session.undo.applied_steps();
        let mut state = GlyphBrowserState::new();
        state.lock_font_and_style = true;
        state.font_id = Some(FontId(u32::MAX));
        state.first_page(&lib);
        assert!(state.page.is_none());
        assert!(!state.insert_at(&mut session, 0).unwrap());
        assert_eq!(session.document.version, version);
        assert_eq!(session.undo.applied_steps(), depth);
        assert_eq!(story_text(&session, story), "xy");
    }

    #[test]
    fn follows_story_face_unless_locked() {
        let lib = FontLibrary::shared();
        let (session, _) = session_with_story();
        let Some(face) = story_face(&lib, &session) else { return };
        let mut state = GlyphBrowserState::new();
        assert!(state.follow_story_face(&lib, &session));
        assert_eq!(state.font_id, Some(face));
        let mut locked = GlyphBrowserState::new();
        locked.lock_font_and_style = true;
        assert!(!locked.follow_story_face(&lib, &session));
        assert_eq!(locked.font_id, None);
    }
}
