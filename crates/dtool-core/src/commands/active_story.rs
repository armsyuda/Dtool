use crate::command::Session;
use crate::document::StoryId;

pub(crate) fn is_para_break(ch: char) -> bool {
    ch == '\n' || ch == '\u{2029}'
}

impl Session {
    pub fn selected_table_stories(&self) -> Vec<StoryId> {
        if self.text_edit.is_some() {
            return Vec::new();
        }
        let selection = crate::table::selection::for_format(self);
        let Some(object) = selection.table.and_then(|id| self.document.get_object(id)).filter(|object| !object.locked) else {
            return Vec::new();
        };
        let crate::ObjectKind::Table(table) = &object.kind else {
            return Vec::new();
        };
        let cells = selection.cells.into_iter().collect::<std::collections::HashSet<_>>();
        let mut seen = std::collections::HashSet::new();
        table.cells.iter().filter(|cell| cells.contains(&cell.id) && seen.insert(cell.story)).map(|cell| cell.story).collect()
    }

    pub fn selected_text_stories(&self) -> Vec<StoryId> {
        let table = self.selected_table_stories();
        if !table.is_empty() {
            return table;
        }
        let mut seen = std::collections::HashSet::new();
        self.selection.ids.iter()
            .filter_map(|id| self.document.get_object(*id)?.story_id())
            .filter(|story| seen.insert(*story))
            .collect()
    }

    pub fn active_story_id(&self) -> Option<StoryId> {
        if let Some(edit) = &self.text_edit {
            return Some(edit.story);
        }
        if self.active_tool == "tool.table" {
            let selection = crate::table::selection::current(self);
            if let ([cell], Some(crate::ObjectKind::Table(table))) = (
                selection.cells.as_slice(),
                selection.table.and_then(|id| self.document.get_object(id)).map(|object| &object.kind),
            ) {
                return table.cells.iter().find(|entry| entry.id == *cell).map(|entry| entry.story);
            }
        }
        self.selection.ids.iter().find_map(|id| self.document.get_object(*id).and_then(|object| object.story_id()))
    }

    pub fn active_para_range(&self) -> Option<(StoryId, [usize; 2])> {
        let story = self.active_story_id()?;
        let chars: Vec<char> = self.document.stories.iter().find(|s| s.id == story)?.text.chars().collect();
        let len = chars.len();
        let (low, high) = match &self.text_edit {
            Some(edit) if edit.story == story => (edit.anchor.min(edit.caret).min(len), edit.anchor.max(edit.caret).min(len)),
            _ => return Some((story, [0, len])),
        };
        let start = chars[..low].iter().rposition(|ch| is_para_break(*ch)).map(|i| i + 1).unwrap_or(0);
        // A nonempty selection ends before `high`, including when it ends just after a break.
        let last = if low == high { high } else { high - 1 };
        let end = chars[last..].iter().position(|ch| is_para_break(*ch)).map(|i| last + i + 1).unwrap_or(len);
        Some((story, [start, end.max(start)]))
    }
}
