use serde::{Deserialize, Serialize};

use crate::object::{Object, ObjectId};
use crate::selection::PixelSelection;
use crate::table::model::{TableFormatDef, TableFormatId};
use crate::paint::{Paint, StrokeStyle};

pub use crate::table::model::StoryId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakKind { Line, Column, Frame, Page, OddPage, EvenPage }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Special { RightIndentTab, IndentToHere, Break(BreakKind) }

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Story {
    pub id: StoryId,
    pub text: String,
    #[serde(deserialize_with = "crate::paint::deserialize_ranges")]
    pub ranges: Vec<serde_json::Value>,
    #[serde(default)]
    pub specials: Vec<(usize, Special)>,
    #[serde(default)]
    pub paras: Vec<serde_json::Value>,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Units {
    #[default]
    #[serde(alias = "Pixels")]
    Px,
    #[serde(alias = "Millimeters")]
    Mm,
    #[serde(alias = "Centimeters")]
    Cm,
    #[serde(alias = "Inches")]
    In,
    #[serde(alias = "Points")]
    Pt,
}

#[allow(non_upper_case_globals)]
impl Units {
    pub const Points: Self = Self::Pt;
    pub const Millimeters: Self = Self::Mm;
    pub const Inches: Self = Self::In;
    pub const Pixels: Self = Self::Px;
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SetupMargins {
    pub enabled: bool,
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub linked: bool,
}

impl Default for SetupMargins {
    fn default() -> Self {
        Self { enabled: true, left: 0.0, right: 0.0, top: 0.0, bottom: 0.0, linked: false }
    }
}

fn setup_landscape<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Option::<bool>::deserialize(deserializer).map(|value| value.unwrap_or(false))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DocumentSetup {
    #[serde(default, deserialize_with = "setup_landscape")]
    pub landscape: bool,
    pub facing_pages: bool,
    pub arrange_vertical: bool,
    pub start_on_right: bool,
    pub reflow_pages: bool,
    pub transparent_background: bool,
    pub margins: SetupMargins,
    pub bleed: SetupMargins,
    pub page_origin: [f32; 2],
}

impl Default for DocumentSetup {
    fn default() -> Self {
        Self {
            landscape: false,
            facing_pages: true,
            arrange_vertical: false,
            start_on_right: true,
            reflow_pages: false,
            transparent_background: false,
            margins: SetupMargins::default(),
            bleed: SetupMargins::default(),
            page_origin: [0.0, 0.0],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PageId(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    #[serde(default)]
    pub id: PageId,
    pub size: [f32; 2],
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_colour: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artboard_id: Option<ObjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fold: Option<crate::page_fold::PageFold>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_guides: Option<crate::guides::column_layout::ColumnLayout>,
    /// Pages with the same group form a spread when consecutive in layout order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread_group: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread_spine: Option<usize>,
}

impl Page {
    pub fn new(id: PageId, size: [f32; 2]) -> Self {
        Self { id, size, name: String::new(), tag_colour: None, artboard_id: None, fold: None, column_guides: None,
            spread_group: None, spread_spine: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidateError {
    DuplicateObjectId(u64),
    DuplicateStoryId(u64),
    DuplicateResourceId(u64),
    DuplicatePageId(u64),
    DuplicateCellId { table: u64, cell: u64 },
    MissingStory { object: u64, story: u64 },
    MissingPathObject { object: u64, path: u64 },
    MissingPage { object: u64, page: u64 },
    PlacedImageMissingObject { object: u64 },
    PlacedImageMissingResource { object: u64, resource: u64 },
}

impl std::fmt::Display for ValidateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for ValidateError {}

impl From<ValidateError> for dtool_protocol::CommandError {
    fn from(_: ValidateError) -> Self { dtool_protocol::CommandError::new("IO.INVALID_FORMAT") }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Process-local identity (not saved); clones share it, new/opened documents get a fresh one.
    #[serde(skip)]
    pub instance: DocInstance,
    #[serde(default)]
    pub setup: DocumentSetup,
    #[serde(default)]
    pub grid: crate::grid::Grid,
    #[serde(default)]
    pub rulers: crate::rulers::RulerState,
    #[serde(default)]
    pub slices: Vec<crate::slice::Slice>,
    #[serde(default)]
    pub export_defaults: serde_json::Value,
    #[serde(default)]
    pub next_slice_id: u64,
    #[serde(default)]
    pub colour: crate::colour::DocColour,
    pub version: u64,
    pub units: Units,
    pub dpi: f32,
    pub pages: Vec<Page>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub artboard_backing_pages: std::collections::BTreeSet<PageId>,
    /// Original vector positions of linked boards while their pages are arranged for layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_artboard_origins: Option<std::collections::BTreeMap<u64, [f32; 2]>>,
    /// The page sequence was authored in Layout and survives visits to Vector.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preserve_layout_page_order: bool,
    /// Fixed page positions while automatic spread reflow is disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_spreads: Option<std::collections::BTreeMap<u64, crate::commands::pages::spreads::FixedSpread>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub page_move_anchor_to_spine: bool,
    #[serde(default, skip_serializing_if = "crate::commands::masters::PageMoveMasterPolicy::is_keep")]
    pub page_move_master_policy: crate::commands::masters::PageMoveMasterPolicy,
    #[serde(default, skip_serializing_if = "crate::commands::masters::MasterPages::is_empty")]
    pub master_pages: crate::commands::masters::MasterPages,
    pub objects: Vec<Object>,
    pub drawing_scale: [f32; 2],
    #[serde(default, skip_serializing_if = "crate::units::ReadoutUnit::is_document")]
    pub measurement_unit: crate::units::ReadoutUnit,
    #[serde(default)]
    pub next_object_id: u64,
    #[serde(default)]
    pub next_page_id: u64,
    #[serde(default)]
    pub next_story_id: u64,
    #[serde(default)]
    pub next_resource_id: u64,
    #[serde(default)]
    pub table_formats: Vec<TableFormatDef>,
    #[serde(default)]
    pub default_table_format: Option<TableFormatId>,
    #[serde(default, skip_serializing_if = "crate::table::sort::TableSortOptions::is_default")]
    pub table_sort_options: crate::table::sort::TableSortOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixel_selection: Option<PixelSelection>,
    #[serde(default)]
    pub spare_channels: Vec<(String, crate::selection::PixelSelection)>,
    #[serde(default)]
    pub stories: Vec<Story>,
    #[serde(default)]
    pub pixel_layers: crate::pixel::PixelLedger,
    #[serde(default)]
    pub layer_masks: Vec<crate::raster_source::LayerMask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_raster_dpi: Option<f32>,
    #[serde(default)]
    pub text_styles: crate::text_styles::TextStyleStore,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub font_substitutions: Vec<crate::text_cmd::FontSubstitution>,
    #[serde(default)]
    pub text_wraps: crate::text_wrap::WrapTable,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spell_ignore: Vec<String>,
    #[serde(default)]
    pub resources: Vec<crate::resources::ImageResource>,
    #[serde(default)]
    pub placed_images: Vec<crate::resources::PlacedImage>,
    #[serde(default)]
    pub merge_sources: Vec<crate::data_merge::DataSource>,
    #[serde(default)]
    pub merge_bindings: Vec<crate::data_merge::MergeBinding>,
    #[serde(default)]
    pub merge_layouts: Vec<crate::data_merge::MergeLayout>,
    #[serde(default)]
    pub frame_chains: Vec<crate::frame::FrameChain>,
    #[serde(default)]
    pub text_links: Vec<crate::text_link::TextLink>,
    #[serde(default)]
    pub text_import_linked: bool,
    #[serde(default)]
    pub sections: Vec<crate::longdoc::sections::Section>,
    #[serde(default)]
    pub anchors: Vec<crate::longdoc::anchors::Anchor>,
    #[serde(default)]
    pub hyperlinks: Vec<crate::longdoc::anchors::Hyperlink>,
    #[serde(default)]
    pub cross_refs: Vec<crate::longdoc::xref::CrossRef>,
    #[serde(default)]
    pub swatches: Vec<crate::colour::swatch::Swatch>,
    #[serde(default)]
    pub next_swatch_id: u64,
    #[serde(default)]
    pub swatch_bindings: Vec<crate::colour::swatch::SwatchBinding>,
    #[serde(default)]
    pub palettes: Vec<String>,
    #[serde(default)]
    pub notes: crate::longdoc::notes::NotesData,
    #[serde(default)]
    pub toc: crate::longdoc::toc::TocData,
    #[serde(default)]
    pub index: crate::longdoc::index::IndexData,
    #[serde(default)]
    pub metadata: crate::metadata::DocMetadata,
    #[serde(default)]
    pub field_info: crate::longdoc::fields::FieldDocInfo,
    #[serde(default)]
    pub layer_states: Vec<crate::commands::layer_state::LayerState>,
    #[serde(default)]
    pub picture_frames: Vec<crate::commands::picture_frame::PictureFrame>,
    #[serde(default)]
    pub layer_meta: Vec<crate::commands::layer_meta::LayerMeta>,
    #[serde(default, skip_serializing_if = "crate::commands::symbols::Symbols::is_empty")]
    pub symbols: crate::commands::symbols::Symbols,
    #[serde(default)]
    pub geometry_rev: u32,
}

pub const GEOMETRY_REV: u32 = 1;

static NEXT_DOC_INSTANCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Identity of a live document in this process, for display-only state such as font preview.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocInstance(pub u64);

impl Default for DocInstance {
    fn default() -> Self {
        Self(NEXT_DOC_INSTANCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

impl std::fmt::Debug for DocInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DocInstance")
    }
}

impl Document {
    pub fn new(size: [f32; 2], units: Units, dpi: f32, page_count: u32) -> Self {
        let dpi = if dpi == 0.0 { 300.0 } else { dpi };
        let pages: Vec<Page> = (0..page_count).map(|index| Page::new(PageId(index as u64 + 1), size)).collect();
        let next_page_id = pages.len() as u64 + 1;
        Document {
            instance: DocInstance::default(),
            setup: DocumentSetup::default(),
            grid: crate::grid::Grid::default(),
            rulers: crate::rulers::RulerState::default(),
            slices: Vec::new(),
            export_defaults: serde_json::json!({}),
            artboard_backing_pages: Default::default(),
            layout_artboard_origins: None,
            preserve_layout_page_order: false,
            fixed_spreads: None,
            page_move_anchor_to_spine: false,
            page_move_master_policy: Default::default(),
            master_pages: Default::default(),
            next_slice_id: 1,
            colour: crate::colour::DocColour::default(),
            version: 0,
            units,
            dpi,
            pages,
            objects: Vec::new(),
            drawing_scale: [1.0, 1.0],
            measurement_unit: Default::default(),
            next_object_id: 1,
            next_page_id,
            next_story_id: 1,
            next_resource_id: 1,
            table_formats: Vec::new(),
            default_table_format: None,
            table_sort_options: Default::default(),
            pixel_selection: None,
            spare_channels: Vec::new(),
            stories: Vec::new(),
            pixel_layers: crate::pixel::PixelLedger::default(),
            layer_masks: Vec::new(),
            effect_raster_dpi: None,
            text_styles: Default::default(),
            font_substitutions: Vec::new(),
            text_wraps: Default::default(),
            spell_ignore: Vec::new(),
            geometry_rev: GEOMETRY_REV,
            resources: Vec::new(),
            placed_images: Vec::new(),
            merge_sources: Vec::new(),
            merge_bindings: Vec::new(),
            merge_layouts: Vec::new(),
            frame_chains: Vec::new(),
            text_links: Vec::new(),
            text_import_linked: false,
            sections: Vec::new(),
            anchors: Vec::new(),
            hyperlinks: Vec::new(),
            cross_refs: Vec::new(),
            swatches: Vec::new(),
            next_swatch_id: 1,
            swatch_bindings: Vec::new(),
            palettes: Vec::new(),
            notes: Default::default(),
            toc: Default::default(),
            index: Default::default(),
            metadata: Default::default(),
            field_info: Default::default(),
            layer_states: Vec::new(),
            picture_frames: Vec::new(),
            layer_meta: Vec::new(),
            symbols: Default::default(),
        }
    }

    pub(crate) fn alloc_object_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_object_id.max(1));
        self.next_object_id = id.0 + 1;
        id
    }

    pub fn create_object(&mut self, mut object: Object) -> ObjectId {
        if object.id == ObjectId(0) || self.get_object(object.id).is_some() {
            object.id = self.alloc_object_id();
        } else {
            let id_val = object.id.0;
            if id_val >= self.next_object_id {
                self.next_object_id = id_val + 1;
            }
        }
        let id = object.id;
        self.objects.push(object);
        id
    }

    pub fn try_create_object(&mut self, object: Object) -> Result<ObjectId, dtool_protocol::CommandError> {
        if object.id != ObjectId(0) && self.get_object(object.id).is_some() {
            return Err(dtool_protocol::CommandError::invalid_params());
        }
        Ok(self.create_object(object))
    }

    pub fn alloc_page_id(&mut self) -> PageId {
        let max_live = self.pages.iter().map(|p| p.id.0).max().unwrap_or(0);
        let id = self.next_page_id.max(max_live + 1).max(1);
        self.next_page_id = id + 1;
        PageId(id)
    }

    pub fn push_page(&mut self, size: [f32; 2]) -> PageId {
        let id = self.alloc_page_id();
        self.pages.push(Page::new(id, size));
        id
    }

    pub fn linked_artboard(&self, page: PageId) -> Option<&Object> {
        let id = self.pages.iter().find(|item| item.id == page)?.artboard_id?;
        self.get_object(id).filter(|board| board.page_id == Some(page)
            && board.parent_id.is_none() && crate::artboard::is_artboard(board))
    }

    pub fn peek_story_id(&self) -> StoryId {
        let max_live = self.stories.iter().map(|s| s.id.0).max().unwrap_or(0);
        StoryId(self.next_story_id.max(max_live + 1).max(1))
    }

    pub fn alloc_story_id(&mut self) -> StoryId {
        let id = self.peek_story_id();
        self.next_story_id = id.0 + 1;
        id
    }

    pub fn alloc_resource_id(&mut self) -> crate::resources::ResourceId {
        let id = self.next_resource_id();
        self.next_resource_id = id.0 + 1;
        id
    }

    pub fn raise_id_high_water(&mut self) {
        let max_object = self.objects.iter().map(|o| o.id.0).max().unwrap_or(0);
        self.next_object_id = self.next_object_id.max(max_object + 1);
        let max_page = self.pages.iter().map(|p| p.id.0).max().unwrap_or(0);
        self.next_page_id = self.next_page_id.max(max_page + 1);
        let max_story = self.stories.iter().map(|s| s.id.0).max().unwrap_or(0);
        self.next_story_id = self.next_story_id.max(max_story + 1);
        let max_resource = self.resources.iter().map(|r| r.id.0).max().unwrap_or(0);
        self.next_resource_id = self.next_resource_id.max(max_resource + 1);
        let max_slice = self.slices.iter().map(|s| s.id.0).max().unwrap_or(0);
        self.next_slice_id = self.next_slice_id.max(max_slice + 1);
        let max_swatch = self.swatches.iter().map(|s| s.id).max().unwrap_or(0);
        self.next_swatch_id = self.next_swatch_id.max(max_swatch + 1);
    }

    pub fn keep_high_water_from(&mut self, previous: &Document) {
        self.next_object_id = self.next_object_id.max(previous.next_object_id);
        self.next_page_id = self.next_page_id.max(previous.next_page_id);
        self.next_story_id = self.next_story_id.max(previous.next_story_id);
        self.next_resource_id = self.next_resource_id.max(previous.next_resource_id);
        self.next_slice_id = self.next_slice_id.max(previous.next_slice_id);
        self.next_swatch_id = self.next_swatch_id.max(previous.next_swatch_id);
        self.raise_id_high_water();
    }

    pub fn hydrate_ids(&mut self) {
        self.raise_id_high_water();
        self.retag_imported_artboards();
        for object in &mut self.objects {
            if let crate::object::ObjectKind::Table(table) = &mut object.kind {
                let _ = table.hydrate_cell_ids();
            }
        }
        for index in 0..self.pages.len() {
            if self.pages[index].id.0 == 0 {
                let id = self.alloc_page_id();
                self.pages[index].id = id;
            }
        }
        self.raise_id_high_water();
    }

    pub fn retag_imported_artboards(&mut self) {
        for object in &mut self.objects {
            let numbered = object.name.strip_prefix("Artboard ").is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
            if numbered && matches!(object.kind, crate::object::ObjectKind::Group | crate::object::ObjectKind::Shape) {
                object.kind = crate::object::ObjectKind::Artboard;
            }
        }
    }

    pub fn validate_refs(&self) -> Result<(), ValidateError> {
        use std::collections::HashSet;
        let mut objects = HashSet::new();
        for object in &self.objects {
            if !objects.insert(object.id.0) {
                return Err(ValidateError::DuplicateObjectId(object.id.0));
            }
        }
        let mut stories = HashSet::new();
        for story in &self.stories {
            if !stories.insert(story.id.0) {
                return Err(ValidateError::DuplicateStoryId(story.id.0));
            }
        }
        let mut resources = HashSet::new();
        for resource in &self.resources {
            if !resources.insert(resource.id.0) {
                return Err(ValidateError::DuplicateResourceId(resource.id.0));
            }
        }
        let mut pages = HashSet::new();
        for page in &self.pages {
            if page.id.0 != 0 && !pages.insert(page.id.0) {
                return Err(ValidateError::DuplicatePageId(page.id.0));
            }
        }
        for object in &self.objects {
            if let Some(story) = object.story_id() {
                if !stories.contains(&story.0) {
                    return Err(ValidateError::MissingStory { object: object.id.0, story: story.0 });
                }
            }
            if let Some(data) = object.path_text_data() {
                if !objects.contains(&data.path.0) {
                    return Err(ValidateError::MissingPathObject { object: object.id.0, path: data.path.0 });
                }
            }
            if let Some(page) = object.page_id {
                if !pages.contains(&page.0) {
                    return Err(ValidateError::MissingPage { object: object.id.0, page: page.0 });
                }
            }
            if let crate::object::ObjectKind::Table(table) = &object.kind {
                let mut cells = HashSet::new();
                for cell in &table.cells {
                    if !cells.insert(cell.id.0) {
                        return Err(ValidateError::DuplicateCellId { table: object.id.0, cell: cell.id.0 });
                    }
                }
            }
        }
        for placed in &self.placed_images {
            if !objects.contains(&placed.object_id.0) {
                return Err(ValidateError::PlacedImageMissingObject { object: placed.object_id.0 });
            }
            if !resources.contains(&placed.resource.0) {
                return Err(ValidateError::PlacedImageMissingResource { object: placed.object_id.0, resource: placed.resource.0 });
            }
        }
        Ok(())
    }

    pub fn validate_and_hydrate(&mut self) -> Result<(), ValidateError> {
        self.validate_refs()?;
        self.hydrate_ids();
        Ok(())
    }

    pub fn from_native_json(body: &str) -> Result<Document, dtool_protocol::CommandError> {
        let mut document: Document = serde_json::from_str(body)
            .map_err(|_| dtool_protocol::CommandError::new("IO.INVALID_FORMAT"))?;
        document.validate_and_hydrate()?;
        document.migrate_geometry();
        Ok(document)
    }

    pub fn migrate_geometry(&mut self) {
        if self.geometry_rev >= GEOMETRY_REV {
            return;
        }
        for object in &mut self.objects {
            let flipped = object.size[0] < 0.0 || object.size[1] < 0.0;
            if object.rotation_deg == 0.0 && object.shear_deg == 0.0 && !flipped {
                continue;
            }
            let [a, b, c, d] = crate::geometry::object_linear(object.rotation_deg, object.shear_deg);
            let half = [object.size[0] * 0.5, object.size[1] * 0.5];
            let turned = [a * half[0] + c * half[1], b * half[0] + d * half[1]];
            object.pos = [object.pos[0] + turned[0] - half[0].abs(), object.pos[1] + turned[1] - half[1].abs()];
        }
        self.geometry_rev = GEOMETRY_REV;
    }

    pub fn update_object(
        &mut self,
        id: ObjectId,
        patch: ObjectPatch,
    ) -> Result<(), dtool_protocol::CommandError> {
        if let Some(obj) = self.objects.iter_mut().find(|o| o.id == id) {
            if let Some(name) = patch.name {
                obj.name = name;
            }
            if let Some(pos) = patch.pos {
                obj.pos = pos;
            }
            if let Some(size) = patch.size {
                obj.size = size;
            }
            if let Some(fill) = patch.fill {
                obj.fill = fill;
            }
            if let Some(opacity) = patch.opacity {
                obj.opacity = opacity;
            }
            Ok(())
        } else {
            Err(dtool_protocol::CommandError {
                code: "OBJECT_NOT_FOUND".to_string(),
                message_key: format!("error.object_not_found.{}", id.0),
            })
        }
    }

    pub fn delete_object(&mut self, id: ObjectId) -> Result<(), dtool_protocol::CommandError> {
        let initial_len = self.objects.len();
        let doomed = self.subtree_ids(id);
        self.detach_objects(&doomed);
        if self.objects.len() == initial_len {
            Err(dtool_protocol::CommandError {
                code: "OBJECT_NOT_FOUND".to_string(),
                message_key: format!("error.object_not_found.{}", id.0),
            })
        } else {
            Ok(())
        }
    }

    pub fn detach_objects(&mut self, doomed: &std::collections::HashSet<ObjectId>) {
        self.preserve_paint_images_on_detach(doomed);
        self.objects.retain(|obj| !doomed.contains(&obj.id));
        self.pixel_layers.entries.retain(|entry| !doomed.contains(&entry.object));
        self.placed_images.retain(|image| !doomed.contains(&image.object_id));
        self.layer_masks.retain(|mask| !doomed.contains(&mask.object));
        crate::frame::prune_chains(self);
    }

    pub fn subtree_ids(&self, root: ObjectId) -> std::collections::HashSet<ObjectId> {
        let mut doomed = std::collections::HashSet::new();
        if self.get_object(root).is_none() {
            return doomed;
        }
        doomed.insert(root);
        loop {
            let before = doomed.len();
            for object in &self.objects {
                if object.parent_id.is_some_and(|parent| doomed.contains(&parent)) {
                    doomed.insert(object.id);
                }
            }
            if doomed.len() == before {
                return doomed;
            }
        }
    }

    pub fn get_object(&self, id: ObjectId) -> Option<&Object> {
        self.objects.iter().find(|o| o.id == id)
    }

    pub fn set_units(&mut self, units: Units) {
        self.units = units;
    }

    pub fn set_drawing_scale(&mut self, scale: [f32; 2]) -> Result<(), dtool_protocol::CommandError> {
        Self::validate_drawing_scale(scale)?;
        self.drawing_scale = scale;
        Ok(())
    }

    pub(crate) fn validate_drawing_scale(scale: [f32; 2]) -> Result<(), dtool_protocol::CommandError> {
        if scale.iter().any(|value| !value.is_finite() || *value <= 0.0) {
            return Err(dtool_protocol::CommandError::invalid_params());
        }
        Ok(())
    }

    pub fn clone_object(&mut self, id: ObjectId) -> Result<ObjectId, dtool_protocol::CommandError> {
        let source = self.get_object(id).ok_or_else(|| dtool_protocol::CommandError {
            code: "OBJECT_NOT_FOUND".to_string(),
            message_key: format!("error.object_not_found.{}", id.0),
        })?;

        let mut cloned = source.clone();
        cloned.id = ObjectId(0);
        let mut copied: Vec<(StoryId, StoryId)> = Vec::new();
        let mut old_ids: Vec<StoryId> = Vec::new();
        match &cloned.kind {
            crate::object::ObjectKind::ArtText { story } | crate::object::ObjectKind::FrameText { story, .. } => old_ids.push(*story),
            crate::object::ObjectKind::PathText(data) => old_ids.push(data.story),
            crate::object::ObjectKind::Table(table) => {
                for cell in &table.cells {
                    if !old_ids.contains(&cell.story) {
                        old_ids.push(cell.story);
                    }
                }
            }
            _ => {}
        }
        for old in old_ids {
            let mut story = self.stories.iter().find(|s| s.id == old).cloned().ok_or_else(dtool_protocol::CommandError::not_found)?;
            story.id = self.alloc_story_id();
            copied.push((old, story.id));
            self.stories.push(story);
        }
        let remap = |old: StoryId| copied.iter().find(|(from, _)| *from == old).map(|(_, to)| *to).unwrap_or(old);
        match &mut cloned.kind {
            crate::object::ObjectKind::ArtText { story } | crate::object::ObjectKind::FrameText { story, .. } => *story = remap(*story),
            crate::object::ObjectKind::PathText(data) => data.story = remap(data.story),
            crate::object::ObjectKind::Table(table) => {
                for cell in &mut table.cells {
                    cell.story = remap(cell.story);
                }
            }
            _ => {}
        }

        let new_id = self.create_object(cloned);
        Ok(new_id)
    }

    pub fn copy_paint_from_object(&self, id: ObjectId) -> Option<Paint> {
        self.get_object(id).map(|obj| obj.fill.clone())
    }

    pub fn copy_stroke_from_object(&self, id: ObjectId) -> Option<StrokeStyle> {
        self.get_object(id).map(|obj| obj.stroke.clone())
    }

    pub fn apply_transform_to_object(
        &mut self,
        id: ObjectId,
        scale_x: f64,
        scale_y: f64,
        rotate_rad: f64,
    ) -> Result<(), dtool_protocol::CommandError> {
        let obj = self.find_object_mut(id.0).ok_or_else(|| dtool_protocol::CommandError {
            code: "OBJECT_NOT_FOUND".to_string(),
            message_key: format!("error.object_not_found.{}", id.0),
        })?;

        crate::paint::apply_transform_to_paint(&mut obj.fill, scale_x, scale_y, rotate_rad);
        crate::paint::apply_transform_to_stroke(&mut obj.stroke, scale_x, scale_y, rotate_rad);
        let transform = kurbo::Affine::rotate(rotate_rad) * kurbo::Affine::scale_non_uniform(scale_x, scale_y);
        match &mut obj.transparency {
            crate::paint::Transparency::Gradient { geom, .. } => *geom = geom.transform(transform.as_coeffs()),
            crate::paint::Transparency::InverseBitmap(bitmap) => {
                bitmap.matrix = (transform * kurbo::Affine::new(bitmap.matrix)).as_coeffs();
            }
            crate::paint::Transparency::None => {}
        }

        Ok(())
    }

    pub fn import_object_with_paint(&mut self, source: &Object) -> ObjectId {
        let mut imported = source.clone();
        imported.id = ObjectId(0);
        self.create_object(imported)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ObjectPatch {
    pub name: Option<String>,
    pub pos: Option<[f32; 2]>,
    pub size: Option<[f32; 2]>,
    pub fill: Option<Paint>,
    pub opacity: Option<f32>,
}

impl Default for Document {
    fn default() -> Self {
        Document::new([800.0, 600.0], Units::Px, 96.0, 1)
    }
}

impl Document {
    pub fn objects(&self) -> &[Object] { &self.objects }
    pub fn all_objects(&self) -> &[Object] { &self.objects }
    pub fn find_object_mut(&mut self, id: u64) -> Option<&mut Object> {
        self.objects.iter_mut().find(|o| o.id.0 == id)
    }
    pub fn selected_objects_mut(&mut self) -> impl Iterator<Item = &mut Object> {
        self.objects.iter_mut()
    }
}

/// Named text styles of another native .dtool document (`text.style.import`).
pub fn read_text_styles(path: &std::path::Path) -> Result<crate::text_styles::TextStyleStore, dtool_protocol::CommandError> {
    crate::native_io::read_file(path).map(|(doc, _)| doc.text_styles).map_err(|err| err.read_error())
}
