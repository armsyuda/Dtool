use crate::spelling::ko_grammar::{self, GrammarIssue};
use crate::spelling::{replace, without_issue_overlap, Misspelling, Speller};
use crate::fonts::FontLibrary;
use crate::story::Line;
use dtool_core::{CommandError, Document, ObjectId, Session, StoryId};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const SETTLE: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Underline {
    pub start: usize,
    pub end: usize,
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
}

pub struct LiveSpell {
    speller: Option<Speller>,
    marks: HashMap<StoryId, Vec<Misspelling>>,
    issues: HashMap<StoryId, Vec<GrammarIssue>>,
    grammar: bool,
    checked: HashMap<StoryId, String>,
    pending: HashMap<StoryId, Instant>,
    settle: Duration,
}

impl LiveSpell {
    pub fn new(speller: Option<Speller>) -> Self {
        Self { speller, marks: HashMap::new(), issues: HashMap::new(), grammar: true, checked: HashMap::new(), pending: HashMap::new(), settle: SETTLE }
    }

    pub fn en_us() -> Self {
        Self::new(Speller::en_us().ok())
    }

    pub fn with_settle(mut self, settle: Duration) -> Self {
        self.settle = settle;
        self
    }

    pub fn with_grammar(mut self, grammar: bool) -> Self {
        self.grammar = grammar;
        self
    }

    pub fn set_grammar(&mut self, enabled: bool) {
        if self.grammar != enabled { self.grammar = enabled; self.checked.clear(); self.issues.clear(); self.pending.clear(); }
    }

    pub fn pending(&self, story: StoryId) -> bool { self.pending.contains_key(&story) }

    pub fn available(&self) -> bool {
        self.speller.is_some()
    }

    pub fn note_edit(&mut self, story: StoryId, now: Instant) {
        self.pending.insert(story, now);
    }

    pub fn tick(&mut self, session: &Session, now: Instant) -> bool {
        let due: Vec<StoryId> = self.pending.iter().filter(|(_, at)| now.saturating_duration_since(**at) >= self.settle).map(|(id, _)| *id).collect();
        let mut changed = false;
        for story in due {
            self.pending.remove(&story);
            changed |= self.refresh(session, story);
        }
        changed
    }

    pub fn refresh(&mut self, session: &Session, story: StoryId) -> bool {
        let text = session.document.stories.iter().find(|s| s.id == story).map(|s| s.text.clone());
        let Some(text) = text else {
            self.checked.remove(&story);
            let issues = self.issues.remove(&story).is_some();
            return self.marks.remove(&story).is_some() || issues;
        };
        let issues: Vec<GrammarIssue> = if !self.grammar {
            Vec::new()
        } else {
            match ko_grammar::check_if_ready(&text) {
                Some(list) => list.into_iter().filter(|i| !crate::spelling::doc_ignores(&session.document, &i.text)).collect(),
                None => {
                    self.pending.entry(story).or_insert_with(Instant::now);
                    self.issues.get(&story).cloned().unwrap_or_default()
                }
            }
        };
        let found: Vec<Misspelling> = match &self.speller {
            Some(speller) => without_issue_overlap(speller.check_text(&text).into_iter().filter(|m| !crate::spelling::doc_ignores(&session.document, &m.word)).collect(), &issues),
            None => Vec::new(),
        };
        self.checked.insert(story, text);
        let before = self.marks.get(&story).cloned().unwrap_or_default();
        let before_issues = self.issues.get(&story).cloned().unwrap_or_default();
        let changed = before != found || before_issues != issues;
        if found.is_empty() {
            self.marks.remove(&story);
        } else {
            self.marks.insert(story, found);
        }
        if issues.is_empty() {
            self.issues.remove(&story);
        } else {
            self.issues.insert(story, issues);
        }
        changed
    }

    pub fn marks(&self, story: StoryId) -> &[Misspelling] {
        self.marks.get(&story).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn issues(&self, story: StoryId) -> &[GrammarIssue] {
        self.issues.get(&story).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn issue_marks(&self, story: StoryId) -> Vec<Misspelling> {
        self.issues(story).iter().map(GrammarIssue::as_mark).collect()
    }

    pub fn issue_at(&self, story: StoryId, index: usize) -> Option<&GrammarIssue> {
        self.issues(story).iter().find(|i| i.contains(index))
    }

    pub fn replace_issue_at(&mut self, session: &mut Session, story: StoryId, index: usize) -> Result<(), CommandError> {
        let issue = self.issue_at(story, index).cloned().ok_or_else(CommandError::not_found)?;
        replace(session, story, &issue.as_mark(), &issue.suggestion)?;
        self.refresh(session, story);
        Ok(())
    }

    pub fn is_stale(&self, session: &Session, story: StoryId) -> bool {
        let current = session.document.stories.iter().find(|s| s.id == story).map(|s| s.text.as_str());
        current != self.checked.get(&story).map(|s| s.as_str())
    }

    pub fn ignore(&mut self, session: &Session, word: &str) {
        if let Some(speller) = self.speller.as_mut() {
            speller.ignore(word);
        }
        let mut stories: Vec<StoryId> = self.marks.keys().copied().collect();
        stories.extend(self.issues.keys().copied().filter(|id| !self.marks.contains_key(id)));
        for story in stories {
            self.refresh(session, story);
        }
    }

    pub fn mark_at(&self, story: StoryId, index: usize) -> Option<&Misspelling> {
        self.marks(story).iter().find(|m| m.start <= index && index < m.end)
    }

    pub fn suggestions_at(&self, story: StoryId, index: usize) -> Option<(Misspelling, Vec<String>)> {
        if let Some(issue) = self.issue_at(story, index) {
            return Some((issue.as_mark(), vec![issue.suggestion.clone()]));
        }
        let item = self.mark_at(story, index)?.clone();
        let list = self.speller.as_ref()?.suggest(&item.word);
        Some((item, list))
    }

    pub fn replace_at(&mut self, session: &mut Session, story: StoryId, index: usize, with: &str) -> Result<(), CommandError> {
        let item = self.issue_at(story, index).map(GrammarIssue::as_mark).or_else(|| self.mark_at(story, index).cloned()).ok_or_else(CommandError::not_found)?;
        replace(session, story, &item, with)?;
        self.refresh(session, story);
        Ok(())
    }
}

pub fn underlines(marks: &[Misspelling], carets: &[[f32; 2]], line_height: f32) -> Vec<Underline> {
    let mut out = Vec::new();
    for item in marks {
        if item.end >= carets.len() || item.start >= item.end {
            continue;
        }
        let mut seg_start = item.start;
        for i in item.start..item.end {
            let next = i + 1;
            let wraps = (carets[next][1] - carets[seg_start][1]).abs() > line_height * 0.5;
            if next == item.end || wraps {
                let x1 = if wraps {
                    let prev = carets[i.saturating_sub(1).max(seg_start)][0];
                    carets[i][0] + (carets[i][0] - prev).abs()
                } else {
                    carets[next][0]
                };
                out.push(Underline { start: seg_start, end: next, x0: carets[seg_start][0], x1, y: carets[seg_start][1] });
                seg_start = next;
            }
        }
    }
    out
}

pub fn squiggle(line: &Underline, amplitude: f32, period: f32) -> Vec<[f32; 2]> {
    let width = (line.x1 - line.x0).abs();
    let step = (period / 2.0).max(0.5);
    let count = (width / step).ceil().max(1.0) as usize;
    let dir = if line.x1 >= line.x0 { 1.0 } else { -1.0 };
    (0..=count)
        .map(|i| {
            let x = line.x0 + dir * (i as f32 * step).min(width);
            let y = line.y + if i % 2 == 0 { 0.0 } else { amplitude };
            [x, y]
        })
        .collect()
}

pub fn pen_edges(line: &Line) -> impl Iterator<Item = (usize, f32, f32)> + '_ {
    let base = line.glyphs.first().map(|g| g.offset[0]).unwrap_or(0.0);
    line.glyphs.iter().scan(base, |pen, g| {
        let x0 = *pen;
        *pen += g.advance;
        Some((g.cluster, x0, *pen))
    })
}

pub fn underlines_in_lines(marks: &[Misspelling], lines: &[Line]) -> Vec<Underline> {
    let mut out = Vec::new();
    for line in lines {
        for item in marks {
            let low = item.start.max(line.range.start);
            let high = item.end.min(line.range.end);
            if low >= high {
                continue;
            }
            let mut edges = pen_edges(line).filter(|(cluster, _, _)| *cluster >= low && *cluster < high).map(|(_, a, b)| (a, b));
            let Some(first) = edges.next() else { continue };
            let (x0, x1) = edges.fold(first, |(a, b), (c, d)| (a.min(c), b.max(d)));
            out.push(Underline { start: low, end: high, x0, x1, y: line.baseline });
        }
    }
    out
}

pub fn story_underlines(lib: &FontLibrary, doc: &Document, story: StoryId, marks: &[Misspelling]) -> Vec<(ObjectId, Underline)> {
    if marks.is_empty() {
        return Vec::new();
    }
    let Some(flow) = crate::frame::layout_doc_flow(lib, doc, story) else { return Vec::new() };
    let mut out = Vec::new();
    for (frame, lines) in flow.frames.iter().zip(flow.layout.per_frame.iter()) {
        let Some(object) = doc.get_object(*frame) else { continue };
        for line in underlines_in_lines(marks, lines) {
            out.push((*frame, Underline { x0: line.x0 + object.pos[0], x1: line.x1 + object.pos[0], y: line.y + object.pos[1], ..line }));
        }
    }
    out
}
