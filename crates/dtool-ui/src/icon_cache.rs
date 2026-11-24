use std::collections::HashMap;

use egui::Color32;

#[derive(Debug, Clone, PartialEq)]
pub struct IconRaster {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

impl IconRaster {
    pub fn covered_pixels(&self) -> usize {
        self.rgba.chunks_exact(4).filter(|pixel| pixel[3] > 0).count()
    }

    pub fn color_image(&self) -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied(self.size, &self.rgba)
    }
}

pub fn rasterize_svg(svg: &str, px: u32) -> Option<IconRaster> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).ok()?;
    let mut image = resvg::tiny_skia::Pixmap::new(px, px)?;
    let original = tree.size();
    let scale = px as f32 / original.width().max(original.height());
    let tx = (px as f32 - original.width() * scale) * 0.5;
    let ty = (px as f32 - original.height() * scale) * 0.5;
    let transform = resvg::tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, tx, ty);
    resvg::render(&tree, transform, &mut image.as_mut());
    let rgba = image.pixels().iter().flat_map(|pixel| {
        let pixel = pixel.demultiply();
        [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
    }).collect();
    Some(IconRaster { size: [px as usize; 2], rgba })
}

pub fn rasterize_icon(id: &str, colour: Color32, px: u32) -> Option<IconRaster> {
    rasterize_svg(&dtool_design::icons::themed_svg(id, colour)?, px)
}

type IconKey = (String, [u8; 4], u32);

fn icon_key(id: &str, colour: Color32, px: u32) -> IconKey {
    (id.to_string(), [colour.r(), colour.g(), colour.b(), colour.a()], px)
}

#[derive(Clone, Default)]
pub struct IconCache {
    rasters: HashMap<IconKey, Option<IconRaster>>,
    textures: HashMap<IconKey, egui::TextureHandle>,
    renders: usize,
}

impl std::fmt::Debug for IconCache {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("IconCache")
            .field("rasters", &self.rasters.len())
            .field("textures", &self.textures.len())
            .field("renders", &self.renders)
            .finish()
    }
}

impl IconCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn renders(&self) -> usize {
        self.renders
    }

    pub fn len(&self) -> usize {
        self.rasters.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rasters.is_empty()
    }

    pub fn clear(&mut self) {
        self.rasters.clear();
        self.textures.clear();
    }

    pub fn raster(&mut self, id: &str, colour: Color32, px: u32) -> Option<&IconRaster> {
        let key = icon_key(id, colour, px);
        if !self.rasters.contains_key(&key) {
            self.renders += 1;
            self.rasters.insert(key.clone(), rasterize_icon(id, colour, px));
        }
        self.rasters.get(&key).and_then(Option::as_ref)
    }

    pub fn tool_raster(&mut self, tool_id: &str, colour: Color32, px: u32) -> Option<&IconRaster> {
        let id = dtool_design::icons::tool_icon_for(tool_id)?;
        self.raster(id, colour, px)
    }

    pub fn texture(&mut self, ctx: &egui::Context, id: &str, colour: Color32, px: u32) -> Option<egui::TextureHandle> {
        let key = icon_key(id, colour, px);
        if let Some(handle) = self.textures.get(&key) {
            return Some(handle.clone());
        }
        let image = self.raster(id, colour, px)?.color_image();
        let handle = ctx.load_texture(format!("icon.{}.{}", id, px), image, egui::TextureOptions::LINEAR);
        self.textures.insert(key, handle.clone());
        Some(handle)
    }

    pub fn tool_texture(&mut self, ctx: &egui::Context, tool_id: &str, colour: Color32, px: u32) -> Option<egui::TextureHandle> {
        let id = dtool_design::icons::tool_icon_for(tool_id)?;
        self.texture(ctx, id, colour, px)
    }

    pub fn themed_texture(&mut self, ctx: &egui::Context, id: &str, px: u32) -> Option<egui::TextureHandle> {
        self.texture(ctx, id, dtool_design::icons::current_icon_colour(), px)
    }

    pub(crate) fn studio_button(&mut self, ui: &mut egui::Ui, id: &str, label: &str, selected: bool) -> egui::Response {
        let colour = if selected { ui.visuals().selection.stroke.color } else { dtool_design::icons::current_icon_colour() };
        let texture = self.texture(ui.ctx(), id, colour, dtool_design::icon_texture_px(ui));
        let button = match &texture {
            Some(texture) => egui::Button::image(egui::Image::new(texture).fit_to_exact_size(egui::Vec2::splat(18.0))),
            None => egui::Button::new(label),
        };
        let response = ui.add_sized([24.0, 23.0], button.selected(selected)).on_hover_text(label);
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
        response
    }
}
