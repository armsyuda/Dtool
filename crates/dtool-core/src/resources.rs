use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use crate::document::Document;
use crate::object::ObjectId;
use crate::object::ObjectId as NodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetId(pub u64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub id: AssetId,
    pub name: String,
    pub nodes: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetGroup {
    pub name: String,
    pub items: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetCategory {
    pub name: String,
    pub groups: Vec<AssetGroup>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssetStore {
    pub categories: Vec<AssetCategory>,
    next_id: u64,
}

impl AssetStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_category(&mut self, name: String) {
        if !self.categories.iter().any(|c| c.name == name) {
            self.categories.push(AssetCategory {
                name,
                groups: Vec::new(),
            });
        }
    }

    pub fn add_group(&mut self, category_name: &str, group_name: String) -> Result<(), String> {
        let cat = self
            .categories
            .iter_mut()
            .find(|c| c.name == category_name)
            .ok_or_else(|| format!("Category not found: {}", category_name))?;

        if !cat.groups.iter().any(|g| g.name == group_name) {
            cat.groups.push(AssetGroup {
                name: group_name,
                items: Vec::new(),
            });
        }
        Ok(())
    }

    pub fn add_asset(&mut self, category_name: &str, group_name: &str, asset: Asset) -> Result<AssetId, String> {
        let cat = self
            .categories
            .iter_mut()
            .find(|c| c.name == category_name)
            .ok_or_else(|| format!("Category not found: {}", category_name))?;

        let group = cat
            .groups
            .iter_mut()
            .find(|g| g.name == group_name)
            .ok_or_else(|| format!("Group not found: {}", group_name))?;

        let id = AssetId(self.next_id);
        self.next_id += 1;
        let mut new_asset = asset;
        new_asset.id = id;
        group.items.push(new_asset);
        Ok(id)
    }

    pub fn list(&self) -> &Vec<AssetCategory> {
        &self.categories
    }

    pub fn find_asset(&self, id: AssetId) -> Option<&Asset> {
        for cat in &self.categories {
            for group in &cat.groups {
                if let Some(asset) = group.items.iter().find(|a| a.id == id) {
                    return Some(asset);
                }
            }
        }
        None
    }

    pub fn remove_asset(&mut self, id: AssetId) -> Result<(), String> {
        for cat in &mut self.categories {
            for group in &mut cat.groups {
                if let Some(pos) = group.items.iter().position(|a| a.id == id) {
                    group.items.remove(pos);
                    return Ok(());
                }
            }
        }
        Err(format!("Asset not found: {:?}", id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StyleId(pub u64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectStyle {
    pub id: StyleId,
    pub name: String,
    pub appearance: Appearance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    pub fills: Vec<AppearancePaint>,
    pub strokes: Vec<StrokeLayer>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearancePaint {
    pub color: [f32; 4],
    pub opacity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrokeLayer {
    pub paint: AppearancePaint,
    pub width: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StyleStore {
    pub styles: Vec<ObjectStyle>,
    next_id: u64,
    #[serde(default)]
    pub applied: Vec<(NodeId, StyleId)>,
}

impl StyleStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&mut self, name: String, appearance: Appearance) -> StyleId {
        let id = StyleId(self.next_id);
        self.next_id += 1;
        self.styles.push(ObjectStyle {
            id,
            name,
            appearance,
        });
        id
    }

    pub fn list(&self) -> &Vec<ObjectStyle> {
        &self.styles
    }

    pub fn find(&self, id: StyleId) -> Option<&ObjectStyle> {
        self.styles.iter().find(|s| s.id == id)
    }

    pub fn delete(&mut self, id: StyleId) -> Result<(), String> {
        if let Some(pos) = self.styles.iter().position(|s| s.id == id) {
            self.styles.remove(pos);
            self.applied.retain(|(_, style)| *style != id);
            Ok(())
        } else {
            Err(format!("Style not found: {:?}", id))
        }
    }

    pub fn rename(&mut self, id: StyleId, new_name: String) -> Result<(), String> {
        if let Some(style) = self.styles.iter_mut().find(|s| s.id == id) {
            style.name = new_name;
            Ok(())
        } else {
            Err(format!("Style not found: {:?}", id))
        }
    }

    pub fn apply(&mut self, id: StyleId, node_id: NodeId) -> Result<(), String> {
        if self.find(id).is_none() {
            return Err(format!("Style not found: {:?}", id));
        }
        if let Some(entry) = self.applied.iter_mut().find(|(node, _)| *node == node_id) {
            entry.1 = id;
        } else {
            self.applied.push((node_id, id));
        }
        Ok(())
    }

    pub fn style_for(&self, node_id: NodeId) -> Option<&ObjectStyle> {
        self.applied
            .iter()
            .find(|(node, _)| *node == node_id)
            .and_then(|(_, style)| self.find(*style))
    }

    pub fn appearance_for(&self, node_id: NodeId) -> Option<&Appearance> {
        self.style_for(node_id).map(|style| &style.appearance)
    }
}

pub const DEFAULT_RESOURCE_DPI: f32 = 72.0;

fn default_resource_dpi() -> f32 {
    DEFAULT_RESOURCE_DPI
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct ResourceId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ImageFormat {
    #[default]
    Png,
    Jpeg,
    Gif,
    Tiff,
    Bmp,
    Tga,
    Webp,
    Heif,
    JpegXl,
    Exr,
    Hdr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ResourceStatus {
    #[default]
    Embedded,
    Linked,
    Modified,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PlacementPolicy {
    #[default]
    Embedded,
    Linked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageResource {
    pub id: ResourceId,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub embedded: bool,
    #[serde(default)]
    pub format: ImageFormat,
    #[serde(default)]
    pub pixel_size: [u32; 2],
    #[serde(default = "default_resource_dpi")]
    pub dpi: f32,
    #[serde(default)]
    pub colour_space: String,
    #[serde(default)]
    pub icc: Option<String>,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub mtime: Option<u64>,
    #[serde(default)]
    pub status: ResourceStatus,
    #[serde(default)]
    pub data: Option<Vec<u8>>,
}

impl Default for ImageResource {
    fn default() -> Self {
        Self {
            id: ResourceId(0),
            name: String::new(),
            path: None,
            embedded: true,
            format: ImageFormat::default(),
            pixel_size: [0, 0],
            dpi: DEFAULT_RESOURCE_DPI,
            colour_space: String::new(),
            icc: None,
            hash: None,
            mtime: None,
            status: ResourceStatus::Embedded,
            data: None,
        }
    }
}

pub fn file_mtime(path: &Path) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    modified.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl ImageResource {
    pub fn new_embedded(format: ImageFormat, pixel_size: [u32; 2], dpi: f32, data: Vec<u8>) -> Self {
        Self {
            format,
            pixel_size,
            dpi: if dpi > 0.0 { dpi } else { DEFAULT_RESOURCE_DPI },
            embedded: true,
            data: Some(data),
            status: ResourceStatus::Embedded,
            ..Self::default()
        }
    }

    pub fn new_linked(path: PathBuf, format: ImageFormat, pixel_size: [u32; 2], dpi: f32) -> Self {
        let mtime = file_mtime(&path);
        let status = if mtime.is_some() { ResourceStatus::Linked } else { ResourceStatus::Missing };
        Self {
            name: file_name_of(&path),
            path: Some(path),
            format,
            pixel_size,
            dpi: if dpi > 0.0 { dpi } else { DEFAULT_RESOURCE_DPI },
            embedded: false,
            mtime,
            status,
            ..Self::default()
        }
    }

    pub fn is_linked(&self) -> bool {
        !self.embedded && self.path.is_some()
    }

    pub fn display_name(&self) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        match &self.path {
            Some(path) => file_name_of(path),
            None => format!("{:?}", self.format),
        }
    }

    pub fn effective_dpi(&self) -> f32 {
        if self.dpi > 0.0 { self.dpi } else { DEFAULT_RESOURCE_DPI }
    }

    pub fn native_size_pt(&self) -> [f32; 2] {
        let dpi = self.effective_dpi();
        [
            self.pixel_size[0] as f32 * 72.0 / dpi,
            self.pixel_size[1] as f32 * 72.0 / dpi,
        ]
    }

    pub fn placed_dpi(&self, placed_size_pt: [f32; 2]) -> f32 {
        if placed_size_pt[0] <= 0.0 || self.pixel_size[0] == 0 {
            return self.effective_dpi();
        }
        self.pixel_size[0] as f32 * 72.0 / placed_size_pt[0]
    }

    pub fn resolved_path(&self, doc_dir: Option<&Path>) -> Option<PathBuf> {
        let path = self.path.as_ref()?;
        match doc_dir {
            Some(dir) if path.is_relative() => Some(dir.join(path)),
            _ => Some(path.clone()),
        }
    }

    pub fn check_link(&self, doc_dir: Option<&Path>) -> ResourceStatus {
        self.status_and_file_size(doc_dir).0
    }

    fn status_and_file_size(&self, doc_dir: Option<&Path>) -> (ResourceStatus, Option<u64>) {
        if self.embedded {
            return (ResourceStatus::Embedded, self.data.as_ref().map(|data| data.len() as u64));
        }
        let Some(path) = self.resolved_path(doc_dir) else {
            return (ResourceStatus::Missing, None);
        };

        let Ok(data) = std::fs::read(&path) else {
            return (ResourceStatus::Missing, None);
        };
        let size = Some(data.len() as u64);
        if self.hash.as_ref().is_some_and(|hash| *hash != compute_sha256(&data)) {
            return (ResourceStatus::Modified, size);
        }
        match file_mtime(&path) {
            None => (ResourceStatus::Missing, None),
            Some(now) if self.mtime.is_some_and(|stored| stored != now) => (ResourceStatus::Modified, size),
            Some(_) => (ResourceStatus::Linked, size),
        }
    }

    pub fn refresh_link(&mut self, doc_dir: Option<&Path>) -> ResourceStatus {
        self.status = self.check_link(doc_dir);
        self.status
    }

    pub fn mark_updated(&mut self, doc_dir: Option<&Path>, hash: Option<String>) -> ResourceStatus {
        if let Some(path) = self.resolved_path(doc_dir) {
            self.mtime = file_mtime(&path);
        }
        if hash.is_some() {
            self.hash = hash;
        }
        self.refresh_link(doc_dir)
    }

    pub fn embed(&mut self, data: Vec<u8>) {
        self.embedded = true;
        self.data = Some(data);
        self.status = ResourceStatus::Embedded;
    }

    pub fn make_linked(&mut self, path: PathBuf, hash: Option<String>) {
        self.mtime = file_mtime(&path);
        if self.name.is_empty() {
            self.name = file_name_of(&path);
        }
        self.path = Some(path);
        self.embedded = false;
        self.data = None;
        if hash.is_some() {
            self.hash = hash;
        }
        self.status = if self.mtime.is_some() { ResourceStatus::Linked } else { ResourceStatus::Missing };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlacedImage {
    pub object_id: ObjectId,
    pub resource: ResourceId,
}

fn resource_not_found(id: ResourceId) -> dtool_protocol::CommandError {
    dtool_protocol::CommandError {
        code: "NOT_FOUND".to_string(),
        message_key: format!("error.resource_not_found.{}", id.0),
    }
}

impl Document {
    pub fn next_resource_id(&self) -> ResourceId {
        let max = self.resources.iter().map(|r| r.id.0).max().unwrap_or(0);
        ResourceId(self.next_resource_id.max(max + 1).max(1))
    }

    pub fn add_image_resource(&mut self, mut resource: ImageResource) -> ResourceId {
        if resource.id.0 == 0 || self.image_resource(resource.id).is_some() {
            resource.id = self.alloc_resource_id();
        }
        let id = resource.id;
        self.next_resource_id = self.next_resource_id.max(id.0 + 1);
        self.resources.push(resource);
        id
    }

    pub fn image_resource(&self, id: ResourceId) -> Option<&ImageResource> {
        self.resources.iter().find(|r| r.id == id)
    }

    pub fn image_resource_mut(&mut self, id: ResourceId) -> Option<&mut ImageResource> {
        self.resources.iter_mut().find(|r| r.id == id)
    }

    pub fn remove_image_resource(&mut self, id: ResourceId) -> Option<ImageResource> {
        let pos = self.resources.iter().position(|r| r.id == id)?;
        self.placed_images.retain(|p| p.resource != id);
        Some(self.resources.remove(pos))
    }

    pub fn place_image(&mut self, object_id: ObjectId, resource: ResourceId) -> Result<(), dtool_protocol::CommandError> {
        if self.image_resource(resource).is_none() {
            return Err(resource_not_found(resource));
        }
        if let Some(existing) = self.placed_images.iter_mut().find(|p| p.object_id == object_id) {
            existing.resource = resource;
        } else {
            self.placed_images.push(PlacedImage { object_id, resource });
        }
        Ok(())
    }

    pub fn unplace_image(&mut self, object_id: ObjectId) -> Option<PlacedImage> {
        let pos = self.placed_images.iter().position(|p| p.object_id == object_id)?;
        Some(self.placed_images.remove(pos))
    }

    pub fn placed_resource(&self, object_id: ObjectId) -> Option<ResourceId> {
        self.placed_images
            .iter()
            .find(|p| p.object_id == object_id)
            .map(|p| p.resource)
    }

    pub fn resource_uses(&self, id: ResourceId) -> Vec<ObjectId> {
        self.placed_images
            .iter()
            .filter(|p| p.resource == id)
            .map(|p| p.object_id)
            .collect()
    }

    pub fn unused_resources(&self) -> Vec<ResourceId> {
        self.resources
            .iter()
            .filter(|r| !self.placed_images.iter().any(|p| p.resource == r.id))
            .map(|r| r.id)
            .collect()
    }

    pub fn refresh_resource_links(&mut self, doc_dir: Option<&Path>) -> Vec<(ResourceId, ResourceStatus)> {
        self.resources
            .iter_mut()
            .map(|r| (r.id, r.refresh_link(doc_dir)))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceRow {
    pub id: ResourceId,
    pub name: String,
    pub status: ResourceStatus,
    pub original_size: [u32; 2],
    pub original_dpi: f32,
    pub placed_dpi: f32,
    pub colour_space: String,
    pub icc: Option<String>,
    pub uses: Vec<ObjectId>,
    #[serde(default)]
    pub format: ImageFormat,
    #[serde(default)]
    pub placed_size: Option<[f32; 2]>,
    #[serde(default)]
    pub file_size_bytes: Option<u64>,
    #[serde(default)]
    pub locations: Vec<ResourceLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceLocation {
    Page { number: usize, id: crate::document::PageId, label: String },
    Artboard { id: ObjectId, name: String },
    Master { id: ObjectId, name: String },
    Pasteboard,
}

fn resource_location(doc: &Document, id: ObjectId) -> Option<ResourceLocation> {
    let object = doc.get_object(id)?;
    let board = crate::artboard::owner(doc, id);
    if let Some(board) = board.filter(|board| doc.master_pages.roots.contains(&board.id)) {
        return Some(ResourceLocation::Master {
            id: board.id,
            name: crate::commands::masters::master_name(board).to_owned(),
        });
    }
    if doc.layout_artboard_origins.is_none() && let Some(board) = board {
        return Some(ResourceLocation::Artboard { id: board.id, name: board.name.clone() });
    }
    let page = board.and_then(|board| board.page_id).or(object.page_id)
        .and_then(|id| doc.pages.iter().position(|page| page.id == id))
        .or_else(|| {
            let [x, y, _, _] = crate::geometry::object_world_bounds(object);
            crate::artboard::edit::page_at_point(doc, [x, y])
        });
    Some(page.map_or(ResourceLocation::Pasteboard, |index| ResourceLocation::Page {
        number: index + 1,
        id: doc.pages[index].id,
        label: crate::longdoc::sections::section_page_label(doc, index),
    }))
}

pub fn resource_rows(doc: &Document, doc_dir: Option<&Path>) -> Vec<ResourceRow> {
    doc.resources.iter().map(|r| {
        let (status, file_size_bytes) = r.status_and_file_size(doc_dir);
        let uses = doc.resource_uses(r.id);
        let placed_size = uses.first().and_then(|id| doc.get_object(*id))
            .map(|object| object.size.map(|length| crate::units::px_to_pt(length.abs(), doc.dpi)));
        let placed_dpi = placed_size.map_or_else(|| r.effective_dpi(), |size| r.placed_dpi(size));
        let mut locations: Vec<_> = uses.iter().filter_map(|&id| resource_location(doc, id)).collect();
        locations.sort_by_key(|location| match location {
            ResourceLocation::Page { number, .. } => (0, *number as u64),
            ResourceLocation::Artboard { id, .. } => (1, id.0),
            ResourceLocation::Master { id, .. } => (2, id.0),
            ResourceLocation::Pasteboard => (3, 0),
        });
        locations.dedup();

        ResourceRow {
            id: r.id,
            name: r.display_name(),
            status,
            original_size: r.pixel_size,
            original_dpi: r.effective_dpi(),
            placed_dpi,
            colour_space: r.colour_space.clone(),
            icc: r.icc.clone(),
            uses,
            format: r.format,
            placed_size,
            file_size_bytes,
            locations,
        }
    }).collect()
}

pub fn embed_resource(doc: &mut Document, id: ResourceId, doc_dir: Option<&Path>) -> Result<(), dtool_protocol::CommandError> {
    let resource = doc.image_resource_mut(id).ok_or_else(|| resource_not_found(id))?;
    
    if resource.embedded && resource.data.is_some() {
        return Ok(());
    }

    let data = if let Some(path) = resource.resolved_path(doc_dir) {
        std::fs::read(&path).map_err(|_| dtool_protocol::CommandError::not_found())?
    } else if let Some(data) = &resource.data {
        data.clone()
    } else {
        return Err(dtool_protocol::CommandError::not_found());
    };

    resource.embed(data);
    Ok(())
}

pub(crate) fn collect_linked_resources(doc: &mut Document, ids: &[ResourceId], folder: &Path, doc_dir: Option<&Path>) -> Result<(), dtool_protocol::CommandError> {
    use dtool_protocol::CommandError;
    use std::collections::HashMap;
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, ErrorKind};

    let io_error = |_| CommandError::validation("error.io_error");
    let folder = std::path::absolute(folder).map_err(io_error)?;
    let mut files = Vec::new();
    let mut sources = HashMap::new();
    let mut placements = Vec::new();
    for &id in ids {
        let resource = doc.image_resource(id).ok_or_else(|| resource_not_found(id))?;
        if !resource.is_linked() {
            return Err(CommandError::invalid_params());
        }
        let path = resource.resolved_path(doc_dir).ok_or_else(CommandError::not_found)?;
        let source = fs::canonicalize(&path).map_err(io_error)?;
        let index = match sources.get(&source) {
            Some(&index) => index,
            None => {
                let input = File::open(&source).map_err(io_error)?;
                let metadata = input.metadata().map_err(io_error)?;
                if !metadata.is_file() {
                    return Err(CommandError::invalid_params());
                }
                let name = path.file_name().ok_or_else(CommandError::invalid_params)?;
                let index = files.len();
                files.push((input, folder.join(name), metadata.modified().map_err(io_error)?));
                sources.insert(source, index);
                index
            }
        };
        placements.push((id, index));
    }

    fs::create_dir_all(&folder).map_err(io_error)?;
    let mut written = Vec::new();
    for (input, destination, modified) in &mut files {
        let result = (|| -> io::Result<()> {
            let mut output = OpenOptions::new().write(true).create_new(true).open(&*destination)?;
            written.push(destination.clone());
            io::copy(input, &mut output)?;
            output.set_modified(*modified)?;
            output.sync_all()
        })();
        if let Err(error) = result {
            for path in written {
                let _ = fs::remove_file(path);
            }
            return Err(if error.kind() == ErrorKind::AlreadyExists {
                CommandError::validation("rm.collect_exists")
            } else {
                io_error(error)
            });
        }
    }

    for (id, index) in placements {
        let resource = doc.image_resource_mut(id).expect("resource validated before copying");
        resource.path = Some(files[index].1.clone());
        resource.refresh_link(doc_dir);
    }
    Ok(())
}

pub fn make_linked_resource(doc: &mut Document, id: ResourceId, folder: &Path) -> Result<(), dtool_protocol::CommandError> {
    let resource = doc.image_resource_mut(id).ok_or_else(|| resource_not_found(id))?;

    if !resource.embedded && resource.path.is_some() {
        return Ok(());
    }

    let data = resource.data.clone().ok_or_else(|| dtool_protocol::CommandError::not_found())?;
    
    let name = if resource.name.is_empty() {
        format!("resource_{}", id.0)
    } else {
        resource.name.clone()
    };

    let target_path = folder.join(&name);

    if target_path.exists() {
        return Err(dtool_protocol::CommandError::validation("rm.replace_existing"));
    }

    std::fs::create_dir_all(folder).map_err(|_| dtool_protocol::CommandError::not_found())?;
    std::fs::write(&target_path, &data).map_err(|_| dtool_protocol::CommandError::not_found())?;

    let hash = compute_sha256(&data);
    
    resource.make_linked(target_path, Some(hash));
    Ok(())
}

pub fn update_resource(doc: &mut Document, id: ResourceId, doc_dir: Option<&Path>) -> Result<(), dtool_protocol::CommandError> {
    let resource = doc.image_resource(id).ok_or_else(|| resource_not_found(id))?;
    if !resource.is_linked() {
        return Ok(());
    }

    let path = resource.resolved_path(doc_dir).ok_or_else(dtool_protocol::CommandError::not_found)?;
    let stored_path = resource.path.clone().ok_or_else(dtool_protocol::CommandError::not_found)?;
    reload_link(doc, id, &path, stored_path)
}

pub fn relink_resource(doc: &mut Document, id: ResourceId, path_or_folder: &Path) -> Result<(), dtool_protocol::CommandError> {
    let resource = doc.image_resource(id).ok_or_else(|| resource_not_found(id))?;
    if !resource.is_linked() {
        return Ok(());
    }

    let target = if path_or_folder.is_dir() {
        let file_name = resource.path.as_deref().and_then(Path::file_name)
            .ok_or_else(|| dtool_protocol::CommandError::validation("rm.select_folder"))?;
        path_or_folder.join(file_name)
    } else {
        path_or_folder.to_path_buf()
    };
    reload_link(doc, id, &target, target.clone())
}

fn reload_link(doc: &mut Document, id: ResourceId, path: &Path, stored_path: PathBuf) -> Result<(), dtool_protocol::CommandError> {
    let decoded = crate::commands::image_place::decode_path(path)?;
    let mut resource = as_linked_resource(decoded.resource, path.to_path_buf());
    resource.path = Some(stored_path);
    replace_shared_image(doc, id, resource, &decoded.rgba)
}

pub(crate) fn replace_shared_image(
    doc: &mut Document,
    id: ResourceId,
    mut resource: ImageResource,
    rgba: &[u8],
) -> Result<(), dtool_protocol::CommandError> {
    let [width, height] = resource.pixel_size;
    let expected = (width as usize).checked_mul(height as usize).and_then(|area| area.checked_mul(4));
    if width == 0 || height == 0 || expected != Some(rgba.len()) {
        return Err(dtool_protocol::CommandError::invalid_params());
    }

    resource.id = id;
    *doc.image_resource_mut(id).ok_or_else(|| resource_not_found(id))? = resource;
    let raster = crate::pixel::PixelLayer::from_rgba(width, height, rgba);
    for object in doc.resource_uses(id) {
        if let Some(item) = doc.find_object_mut(object.0) {
            item.raster_data = Some(rgba.to_vec());
            item.raster_size = Some((width, height));
            doc.pixel_layers.insert(object, raster.clone());
        }
    }
    Ok(())
}

pub fn as_linked_resource(resource: ImageResource, path: std::path::PathBuf) -> ImageResource {
    let mut linked = ImageResource::new_linked(path, resource.format, resource.pixel_size, resource.dpi);
    if !resource.name.is_empty() {
        linked.name = resource.name;
    }
    if let Some(data) = resource.data.as_ref() {
        linked.hash = Some(resource_content_hash(data));
    }
    linked.colour_space = resource.colour_space;
    linked.icc = resource.icc;
    linked
}

pub fn replace_placement(
    document: &mut Document,
    object: crate::object::ObjectId,
    mut resource: ImageResource,
    rgba: Vec<u8>,
    link: bool,
    path: Option<std::path::PathBuf>,
) -> Result<ResourceId, dtool_protocol::CommandError> {
    let object_ref = document.get_object(object).ok_or_else(dtool_protocol::CommandError::not_found)?;
    let [width, height] = resource.pixel_size;
    let length = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4));
    if width == 0 || height == 0 || length != Some(rgba.len()) {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    let _ = object_ref;
    if link {
        let path = path.or(resource.path.clone()).ok_or_else(dtool_protocol::CommandError::invalid_params)?;
        resource = as_linked_resource(resource, path);
    } else if let Some(path) = path {
        if resource.path.is_none() {
            resource.path = Some(path);
        }
    }
    resource.id = ResourceId(0);
    let resource_id = document.add_image_resource(resource);
    document.place_image(object, resource_id)?;
    if let Some(item) = document.objects.iter_mut().find(|item| item.id == object) {
        item.raster_data = Some(rgba.clone());
        item.raster_size = Some((width, height));
    }
    document.pixel_layers.insert(object, crate::pixel::PixelLayer::from_rgba(width, height, &rgba));
    Ok(resource_id)
}

pub fn resource_content_hash(data: &[u8]) -> String {
        compute_sha256(data)
    }

fn compute_sha256(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}
