use std::collections::HashMap;
use std::ops::Range;

use dtool_core::longdoc::anchors::LinkTarget;
use dtool_core::longdoc::toc::{TocEntry, TocRecord, build_toc, toc_text};
use dtool_core::{Document, Object, StoryId};
use dtool_text::{FontLibrary, canvas_text::{text_anchor_point, text_range_quads}};
use krilla::geom::{Point, Quadrilateral, Rect};
use krilla::{
    action::LinkAction,
    annotation::{Annotation, LinkAnnotation, Target},
    destination::XyzDestination,
};

#[derive(Clone, Copy)]
pub(super) struct Placement {
    pub trim: [f32; 4],
    pub offset: [f32; 2],
    pub scale: f32,
}

impl Placement {
    fn point(self, [x, y]: [f32; 2]) -> Point {
        Point::from_xy(
            (x - self.trim[0] + self.offset[0]) * self.scale,
            (y - self.trim[1] + self.offset[1]) * self.scale,
        )
    }

    fn contains(self, [x, y]: [f32; 2]) -> bool {
        let [left, top, w, h] = self.trim;
        x >= left && y >= top && x < left + w && y < top + h
    }

    fn annotation(self, quad: [[f32; 2]; 4], target: Target) -> Option<Annotation> {
        if quad.iter().flatten().any(|value| !value.is_finite()) {
            return None;
        }
        let [x, y, w, h] = self.trim;
        let bounds = dtool_core::scene::contours_bounds(&[quad.to_vec()])?;
        let [left, top, right, bottom] = [
            bounds[0].max(x), bounds[1].max(y), bounds[2].min(x + w), bounds[3].min(y + h),
        ];
        if right <= left || bottom <= top {
            return None;
        }
        let link = if bounds == [left, top, right, bottom] {
            let [a, b, c, d] = quad;
            let points = [d, c, b, a].map(|point| self.point(point));
            LinkAnnotation::new_with_quad_points(vec![Quadrilateral(points)], target)
        } else {
            let first = self.point([left, top]);
            let last = self.point([right, bottom]);
            LinkAnnotation::new(Rect::from_ltrb(first.x, first.y, last.x, last.y)?, target)
        };
        Some(Annotation::new_link(link, None))
    }
}

enum Destination {
    Uri(String),
    Page(XyzDestination),
}

impl Destination {
    fn target(&self) -> Target {
        match self {
            Self::Uri(uri) => Target::Action(LinkAction::new(uri.clone()).into()),
            Self::Page(destination) => Target::Destination(destination.clone().into()),
        }
    }
}

pub(super) struct Links {
    stories: HashMap<StoryId, Vec<(Range<usize>, Destination)>>,
}

struct Destinations<'a> {
    doc: &'a Document,
    placements: &'a [Placement],
    batches: &'a [Vec<usize>],
    members: Option<std::collections::HashSet<dtool_core::ObjectId>>,
}

impl<'a> Destinations<'a> {
    fn new(doc: &'a Document, placements: &'a [Placement], batches: &'a [Vec<usize>], area: &crate::ExportArea) -> Self {
        Self { doc, placements, batches, members: crate::artboard_members(doc, area) }
    }

    fn locate(&self, page: usize, location: [f32; 2]) -> Option<XyzDestination> {
        let output = if self.batches.is_empty() {
            self.placements.iter().position(|placement| placement.contains(location))?
        } else {
            self.batches.iter().position(|indices| indices.contains(&page))?
        };
        let placement = self.placements.get(output)?;
        placement.contains(location).then(|| XyzDestination::new(output, placement.point(location)))
    }

    fn text(&self, fonts: &FontLibrary, story: StoryId, at: usize, heading: bool) -> Option<XyzDestination> {
        let doc = self.doc;
        doc.objects.iter().filter(|object| object.story_id() == Some(story)
            && self.members.as_ref().is_none_or(|ids| ids.contains(&object.id))
            && crate::export_visible(doc, object)
            && !dtool_core::clip_mask::is_mask(doc, object)
            && !dtool_core::commands::masters::owns_source(doc, object.id))
            .find_map(|object| {
                let point = if heading {
                    text_range_quads(fonts, doc, object, at..at.saturating_add(1)).first()?[0]
                } else {
                    text_anchor_point(fonts, doc, object, at)?
                };
                self.locate(dtool_core::data_merge::layout::object_page_index(doc, object.id), point)
            })
    }
}

pub(super) fn bookmarks(
    doc: &Document, placements: &[Placement], batches: &[Vec<usize>], area: &crate::ExportArea,
) -> krilla::outline::Outline {
    use krilla::outline::{Outline, OutlineNode};
    let fonts = FontLibrary::shared();
    let destinations = Destinations::new(doc, placements, batches, area);
    let mut outline = Outline::new();
    let mut pending: Vec<(u8, Option<OutlineNode>)> = Vec::new();
    let attach = |pending: &mut Vec<(u8, Option<OutlineNode>)>, outline: &mut Outline| {
        if let Some((_, Some(node))) = pending.pop() {
            if let Some(parent) = pending.iter_mut().rev().find_map(|(_, node)| node.as_mut()) {
                parent.push_child(node);
            } else {
                outline.push_child(node);
            }
        }
    };
    dtool_text::frame::install_flow_ranges();
    dtool_text::frame::with_doc_layout_reuse(doc, &fonts, || {
        for entry in dtool_core::longdoc::toc::build_toc(doc, &doc.toc.spec) {
            while pending.last().is_some_and(|(level, _)| *level >= entry.level) {
                attach(&mut pending, &mut outline);
            }
            let node = doc.story_at_pos(entry.story_pos)
                .and_then(|(story, at)| destinations.text(&fonts, story, at, true))
                .map(|destination| OutlineNode::new(entry.text, destination).with_open(true));
            pending.push((entry.level, node));
        }
    });
    while !pending.is_empty() {
        attach(&mut pending, &mut outline);
    }
    outline
}

impl Links {
    pub(super) fn new(
        doc: &Document, placements: &[Placement], batches: &[Vec<usize>], area: &crate::ExportArea,
    ) -> Self {
        let mut stories = HashMap::<_, Vec<_>>::new();
        let fonts = FontLibrary::shared();
        let destinations = Destinations::new(doc, placements, batches, area);
        if !doc.toc.tocs.is_empty() {
            dtool_text::frame::install_flow_ranges();
        }
        dtool_text::frame::with_doc_layout_reuse(doc, &fonts, || {
            for link in doc.hyperlinks() {
                let Some((story, from)) = doc.story_at_pos(link.range.0) else { continue };
                let Some((end_story, to)) = doc.story_at_pos(link.range.1) else { continue };
                if story != end_story || from >= to { continue; }
                let destination = match &link.target {
                    LinkTarget::Url(uri) => safe_uri(uri, false).map(Destination::Uri),
                    LinkTarget::Email(email) => safe_uri(email, true).map(Destination::Uri),
                    LinkTarget::Page(number) => number.checked_sub(1).filter(|&page| page < doc.pages.len())
                        .and_then(|page| {
                            let [x, y, _, _] = dtool_core::data_merge::layout::page_rect(doc, page);
                            destinations.locate(page, [x, y])
                        }).map(Destination::Page),
                    LinkTarget::Anchor(name) => doc.find_anchor(name).and_then(|anchor| {
                        let (story, at) = doc.story_at_pos(anchor.story_pos)?;
                        destinations.text(&fonts, story, at, false)
                    }).map(Destination::Page),
                };
                if let Some(destination) = destination {
                    stories.entry(story).or_default().push((from..to, destination));
                }
            }
            for record in &doc.toc.tocs {
                let Some(story) = doc.stories.iter().find(|story| story.id.0 == record.story) else { continue; };
                let links = stories.entry(story.id).or_default();
                let authored = links.len();
                for (range, at) in toc_rows(doc, story, record) {
                    if links[..authored].iter().any(|(existing, _)| existing.start < range.end && range.start < existing.end) {
                        continue;
                    }
                    if let Some(destination) = doc.story_at_pos(at)
                        .and_then(|(id, at)| destinations.text(&fonts, id, at, true))
                    {
                        links.push((range, Destination::Page(destination)));
                    }
                }
            }
        });
        Self { stories }
    }

    pub(super) fn annotations(&self, doc: &Document, placement: Placement, object: &Object) -> Vec<Annotation> {
        let Some(links) = object.story_id().and_then(|story| self.stories.get(&story)) else { return Vec::new() };
        let fonts = FontLibrary::shared();
        dtool_text::frame::with_doc_layout_reuse(doc, &fonts, || {
            links.iter().flat_map(|(range, destination)| {
                text_range_quads(&fonts, doc, object, range.clone()).into_iter()
                    .filter_map(|quad| placement.annotation(quad, destination.target()))
            }).collect()
        })
    }
}

fn toc_row(run: &dtool_core::serde_json::Value, length: usize) -> Option<(Range<usize>, u64)> {
    let start = usize::try_from(run.get("start")?.as_u64()?).ok()?;
    let end = usize::try_from(run.get("end")?.as_u64()?).ok()?;
    let level = run.get("toc_level")?.as_u64()?;
    (start < end && end <= length).then_some((start..end, level))
}

fn toc_rows(doc: &Document, story: &dtool_core::Story, record: &TocRecord) -> Vec<(Range<usize>, usize)> {
    let entries = build_toc(doc, &record.spec);
    let (generated, runs) = toc_text(&entries, &record.spec);
    let actual: Vec<char> = story.text.chars().collect();
    if story.text == generated {
        return runs.iter().zip(&entries).filter_map(|(run, entry)| {
            Some((toc_row(run, actual.len())?.0, entry.story_pos))
        }).collect();
    }

    // An older or edited TOC can have a different order or page labels. Only
    // keep rows whose displayed text and level identify a single current heading.
    let generated: Vec<char> = generated.chars().collect();
    let mut unique = HashMap::<(String, u64), Option<&TocEntry>>::new();
    for (run, entry) in runs.iter().zip(&entries) {
        if let Some((range, level)) = toc_row(run, generated.len()) {
            let text = generated[range].iter().collect();
            unique.entry((text, level)).and_modify(|entry| *entry = None).or_insert(Some(entry));
        }
    }
    story.paras.iter().filter_map(|run| {
        let (range, level) = toc_row(run, actual.len())?;
        let text = actual[range.clone()].iter().collect();
        let entry = unique.get(&(text, level))?.as_ref()?;
        Some((range, entry.story_pos))
    }).collect()
}

fn safe_uri(value: &str, email: bool) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    if email && !value.contains(':') {
        return Some(format!("mailto:{value}"));
    }
    let (scheme, _) = value.split_once(':')?;
    ["http", "https", "ftp", "mailto"].iter().any(|allowed| scheme.eq_ignore_ascii_case(allowed))
        .then(|| value.to_owned())
}
