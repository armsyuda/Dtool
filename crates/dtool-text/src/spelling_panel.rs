use crate::spelling::{replace, Misspelling, Speller};
use dtool_core::{CommandError, Session, StoryId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellIssue {
    pub story: StoryId,
    pub item: Misspelling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellScope {
    Story(StoryId),
    Document,
}

pub struct SpellPanel {
    speller: Result<Speller, CommandError>,
    pub scope: SpellScope,
    pub issues: Vec<SpellIssue>,
    pub selected: Option<usize>,
    pub suggestions: Vec<String>,
    pub error: Option<String>,
}

impl SpellPanel {
    pub fn new(speller: Result<Speller, CommandError>, scope: SpellScope) -> Self {
        let error = speller.as_ref().err().map(|e| e.code.clone());
        Self { speller, scope, issues: Vec::new(), selected: None, suggestions: Vec::new(), error }
    }

    pub fn en_us(scope: SpellScope) -> Self {
        Self::new(Speller::en_us(), scope)
    }

    pub fn available(&self) -> bool {
        self.speller.is_ok()
    }

    pub fn check(&mut self, session: &Session) {
        self.issues.clear();
        self.suggestions.clear();
        let speller = match &self.speller {
            Ok(speller) => speller,
            Err(e) => {
                self.error = Some(e.code.clone());
                self.selected = None;
                return;
            }
        };
        for story in &session.document.stories {
            let wanted = match self.scope {
                SpellScope::Story(id) => story.id == id,
                SpellScope::Document => true,
            };
            if wanted {
                self.issues.extend(speller.check_text(&story.text).into_iter().map(|item| SpellIssue { story: story.id, item }));
            }
        }
        self.error = None;
        let keep = self.selected.unwrap_or(0);
        self.select(if self.issues.is_empty() { None } else { Some(keep.min(self.issues.len() - 1)) });
    }

    pub fn select(&mut self, index: Option<usize>) {
        self.selected = index.filter(|i| *i < self.issues.len());
        self.suggestions = match (self.selected, &self.speller) {
            (Some(i), Ok(speller)) => speller.suggest(&self.issues[i].item.word),
            _ => Vec::new(),
        };
    }

    pub fn current(&self) -> Option<&SpellIssue> {
        self.selected.and_then(|i| self.issues.get(i))
    }

    pub fn replace(&mut self, session: &mut Session, with: &str) -> Result<(), CommandError> {
        if let Err(e) = &self.speller {
            return Err(e.clone());
        }
        let issue = self.current().cloned().ok_or_else(CommandError::not_found)?;
        let result = replace(session, issue.story, &issue.item, with);
        if let Err(e) = &result {
            self.error = Some(e.code.clone());
        }
        let error = self.error.clone();
        self.check(session);
        if result.is_err() {
            self.error = error;
        }
        result
    }

    pub fn ignore(&mut self, session: &Session) -> Result<(), CommandError> {
        let word = self.current().map(|issue| issue.item.word.clone()).ok_or_else(CommandError::not_found)?;
        match self.speller.as_mut() {
            Ok(speller) => speller.ignore(&word),
            Err(e) => return Err(e.clone()),
        }
        self.check(session);
        Ok(())
    }

    pub fn rows(&self) -> Vec<String> {
        self.issues.iter().map(|issue| issue.item.word.clone()).collect()
    }
}
