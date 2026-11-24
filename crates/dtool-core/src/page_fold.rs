use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FoldAxis {
    #[default]
    Vertical,
    Horizontal,
}

impl FoldAxis {
    pub fn index(self) -> usize {
        usize::from(self == Self::Horizontal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PageFold {
    pub axis: FoldAxis,
    fractions: [f32; 2],
}

impl PageFold {
    pub fn equal(axis: FoldAxis) -> Self {
        Self { axis, fractions: [1.0 / 3.0, 2.0 / 3.0] }
    }

    pub fn from_positions(axis: FoldAxis, positions: [f32; 2], size: [f32; 2]) -> Option<Self> {
        let extent = size[axis.index()];
        let fold = Self { axis, fractions: positions.map(|position| position / extent) };
        fold.positions(size)?;
        Some(fold)
    }

    pub fn positions(self, size: [f32; 2]) -> Option<[f32; 2]> {
        let extent = size[self.axis.index()];
        let [first, second] = self.fractions.map(|position| position * extent);
        (extent.is_finite() && 0.0 < first && first < second && second < extent).then_some([first, second])
    }

    pub fn segments(self, [x, y, w, h]: [f32; 4]) -> Option<[[[f32; 2]; 2]; 2]> {
        if w <= 0.0 || h <= 0.0 || ![x, y, x + w, y + h].iter().all(|value| value.is_finite()) {
            return None;
        }
        let positions = self.positions([w, h])?;
        Some(positions.map(|offset| match self.axis {
            FoldAxis::Vertical => [[x + offset, y], [x + offset, y + h]],
            FoldAxis::Horizontal => [[x, y + offset], [x + w, y + offset]],
        }))
    }
}

pub fn segments(doc: &crate::Document) -> impl Iterator<Item = (FoldAxis, [[[f32; 2]; 2]; 2])> + '_ {
    let rects = doc.pages.iter().any(|page| page.fold.is_some()).then(|| crate::scene::page_rects(doc));
    doc.pages.iter().zip(rects.into_iter().flatten()).filter_map(|(page, rect)| {
        let fold = page.fold?;
        Some((fold.axis, fold.segments(rect)?))
    })
}
