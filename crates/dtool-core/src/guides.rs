use serde::{Deserialize, Serialize};

pub mod column_layout;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct GuideId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuideKind {
    Horizontal,
    Vertical,
    Column,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    pub id: GuideId,
    pub kind: GuideKind,
    pub position: f32,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub locked: bool,
    /// When set, position is relative to the page origin along the guide axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_id: Option<crate::document::PageId>,
}

impl GuideKind {
    pub fn axis(self) -> usize { usize::from(self == Self::Horizontal) }
}

pub fn page_bounds(doc: &crate::Document, page: crate::document::PageId) -> Option<[f32; 4]> {
    doc.pages.iter().position(|item| item.id == page)
        .map(|index| crate::data_merge::layout::page_rect(doc, index))
        .filter(|rect| rect.iter().all(|value| value.is_finite()) && rect[2] > 0.0 && rect[3] > 0.0)
}

#[derive(Clone, Copy)]
pub struct PercentAxis {
    start: f64,
    length: f64,
}

impl PercentAxis {
    pub fn new(doc: &crate::Document, page: Option<crate::document::PageId>, kind: GuideKind) -> Option<Self> {
        let axis = kind.axis();
        if let Some(page) = page {
            let bounds = page_bounds(doc, page)?;
            return Some(Self { start: 0.0, length: f64::from(bounds[axis + 2]) });
        }
        let (start, end) = crate::scene::page_rects(doc).into_iter()
            .filter_map(|rect| {
                let start = f64::from(rect[axis]);
                let length = f64::from(rect[axis + 2]);
                (start.is_finite() && length.is_finite() && length > 0.0).then_some((start, start + length))
            })
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(start, end), (low, high)| (start.min(low), end.max(high)));
        let length = end - start;
        (length.is_finite() && length > 0.0).then_some(Self { start, length })
    }

    pub fn to_percent(self, position: f64) -> f64 {
        (position - self.start) / self.length * 100.0
    }

    pub fn from_percent(self, percent: f64) -> Option<f64> {
        let position = self.start + (percent / 100.0) * self.length;
        ((position as f32).is_finite()).then_some(position)
    }
}

impl Guide {
    pub fn page_segment(&self, doc: &crate::Document) -> Option<[[f32; 2]; 2]> {
        let [x, y, w, h] = page_bounds(doc, self.page_id?)?;
        let axis = self.kind.axis();
        if self.position < 0.0 || self.position > [w, h][axis] {
            return None;
        }
        let mut segment = [[x, y], [x + w, y + h]];
        segment[0][axis] += self.position;
        segment[1][axis] = segment[0][axis];
        Some(segment)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Guides {
    pub items: Vec<Guide>,
    pub show: bool,
    pub show_columns: bool,
    pub locked: bool,
    pub colour: Option<String>,
    #[serde(default)]
    pub next_guide_id: u64,
}

impl Default for Guides {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            show: true,
            show_columns: true,
            locked: false,
            colour: None,
            next_guide_id: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideError {
    InvalidParams,
    NotFound,
    Locked,
}

impl Guides {
    pub fn default_name(id: GuideId) -> String {
        format!("{:02}", id.0)
    }

    pub fn next_id(&self) -> GuideId {
        GuideId(self.next_guide_id.max(self.items.iter().map(|g| g.id.0).max().unwrap_or(0) + 1))
    }

    pub fn replace_with(&mut self, next: Guides) {
        let high = self.next_id().0.max(next.next_id().0);
        *self = next;
        self.next_guide_id = high;
    }

    pub fn find(&self, id: Option<u64>, name: Option<&str>) -> Vec<&Guide> {
        self.find_in_scope(id, name, None)
    }

    pub(crate) fn find_in_scope(
        &self, id: Option<u64>, name: Option<&str>, page_id: Option<crate::document::PageId>,
    ) -> Vec<&Guide> {
        let candidates: Vec<&Guide> = self.items.iter().filter(|guide| {
            id.is_none_or(|id| guide.id.0 == id)
                && page_id.is_none_or(|page| guide.page_id == Some(page))
        }).collect();
        let Some(name) = name.map(|n| n.trim().to_lowercase()) else { return candidates; };
        let named = |g: &&Guide| g.name.as_deref().map(str::trim).filter(|n| !n.is_empty())
            .map(str::to_lowercase).unwrap_or_else(|| Self::default_name(g.id));
        let exact: Vec<&Guide> = candidates.iter().copied().filter(|g| named(g) == name).collect();
        if !exact.is_empty() { return exact; }
        let number = name.strip_prefix("가이드").unwrap_or(&name).trim();
        let number = number.strip_suffix("가이드").unwrap_or(number).trim();
        let number = number.strip_suffix('번').unwrap_or(number).trim();
        if let Ok(number) = number.parse::<u64>() {
            return candidates.into_iter().filter(|guide| guide.id.0 == number).collect();
        }
        candidates.into_iter().filter(|g| named(g).contains(&name)).collect()
    }

    pub fn list(&self) -> &[Guide] {
        &self.items
    }

    pub fn get(&self, id: GuideId) -> Option<&Guide> {
        self.items.iter().find(|g| g.id == id)
    }

    pub fn add(&mut self, kind: GuideKind, position: f32, name: Option<String>) -> Result<GuideId, GuideError> {
        self.add_on_page(kind, position, name, None)
    }

    pub fn add_on_page(&mut self, kind: GuideKind, position: f32, name: Option<String>, page_id: Option<crate::document::PageId>) -> Result<GuideId, GuideError> {
        if !position.is_finite() {
            return Err(GuideError::InvalidParams);
        }
        let id = self.next_id();
        self.items.push(Guide {
            id,
            kind,
            position,
            name: Some(name.filter(|value| !value.trim().is_empty()).unwrap_or_else(|| Self::default_name(id))),
            locked: false,
            page_id,
        });
        self.next_guide_id = id.0 + 1;
        Ok(id)
    }

    pub fn insert(&mut self, guide: Guide) -> Result<(), GuideError> {
        if self.get(guide.id).is_some() || !guide.position.is_finite() {
            return Err(GuideError::InvalidParams);
        }
        self.next_guide_id = self.next_guide_id.max(guide.id.0 + 1);
        self.items.push(guide);
        Ok(())
    }

    pub fn can_move(&self, id: GuideId) -> Result<(), GuideError> {
        let guide = self.get(id).ok_or(GuideError::NotFound)?;
        if self.locked || guide.locked {
            return Err(GuideError::Locked);
        }
        Ok(())
    }

    pub fn update(
        &mut self,
        id: GuideId,
        kind: Option<GuideKind>,
        position: Option<f32>,
        name: Option<Option<String>>,
        locked: Option<bool>,
    ) -> Result<Guide, GuideError> {
        if position.map_or(false, |p| !p.is_finite()) {
            return Err(GuideError::InvalidParams);
        }
        if kind.is_some() || position.is_some() {
            self.can_move(id)?;
        }
        let guide = self.items.iter_mut().find(|g| g.id == id).ok_or(GuideError::NotFound)?;
        let old = guide.clone();
        if let Some(k) = kind {
            guide.kind = k;
        }
        if let Some(p) = position {
            guide.position = p;
        }
        if let Some(n) = name {
            guide.name = Some(n.filter(|value| !value.trim().is_empty()).unwrap_or_else(|| Self::default_name(id)));
        }
        if let Some(l) = locked {
            guide.locked = l;
        }
        Ok(old)
    }

    pub fn restore(&mut self, guide: Guide) -> Result<(), GuideError> {
        let slot = self.items.iter_mut().find(|g| g.id == guide.id).ok_or(GuideError::NotFound)?;
        let id = guide.id.0;
        *slot = guide;
        self.next_guide_id = self.next_guide_id.max(id + 1);
        Ok(())
    }

    pub fn delete(&mut self, id: GuideId) -> Result<Guide, GuideError> {
        self.can_move(id)?;
        self.remove(id)
    }

    pub fn remove(&mut self, id: GuideId) -> Result<Guide, GuideError> {
        let index = self.items.iter().position(|g| g.id == id).ok_or(GuideError::NotFound)?;
        Ok(self.items.remove(index))
    }

    pub fn set_show(&mut self, show: Option<bool>, show_columns: Option<bool>, locked: Option<bool>) {
        if let Some(v) = show {
            self.show = v;
        }
        if let Some(v) = show_columns {
            self.show_columns = v;
        }
        if let Some(v) = locked {
            self.locked = v;
        }
    }
}
