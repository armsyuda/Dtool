use crate::document::{Document, DocumentSetup, PageId, SetupMargins};
use crate::colour::ColourFormat;
use serde::{Deserialize, Serialize};
use dtool_protocol::CommandError;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SetupScope {
    WholeDocument,
    SelectedSpreads,
}

impl Default for SetupScope {
    fn default() -> Self {
        Self::WholeDocument
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ResizeMode {
    Rescale,
    AnchorToPage,
}

impl Default for ResizeMode {
    fn default() -> Self {
        Self::AnchorToPage
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ProfileConversion {
    Assign,
    Convert,
}

impl Default for ProfileConversion {
    fn default() -> Self {
        Self::Convert
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Margins {
    pub enabled: bool,
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub linked: bool,
}

impl Default for Margins {
    fn default() -> Self {
        Self {
            enabled: true,
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
            linked: false,
        }
    }
}

pub use crate::document::Units;

pub fn unit_factor(from: Units, from_dpi: f32, to: Units, to_dpi: f32) -> f32 {
    if from == to && from_dpi == to_dpi {
        1.0
    } else {
        crate::units::per_inch(to, to_dpi) / crate::units::per_inch(from, from_dpi)
    }
}
impl From<SetupMargins> for Margins {
    fn from(value: SetupMargins) -> Self {
        Self { enabled: value.enabled, left: value.left, right: value.right, top: value.top, bottom: value.bottom, linked: value.linked }
    }
}

impl From<Margins> for SetupMargins {
    fn from(value: Margins) -> Self {
        Self { enabled: value.enabled, left: value.left, right: value.right, top: value.top, bottom: value.bottom, linked: value.linked }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Anchor9 {
    pub x: f32,
    pub y: f32,
}

impl Default for Anchor9 {
    fn default() -> Self {
        Self { x: 0.5, y: 0.5 }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct DocSetup {
    pub units: Units,
    pub dpi: f32,
    pub page_size: [f32; 2],
    pub landscape: bool,
    pub facing_pages: bool,
    pub arrange_vertical: bool,
    pub start_on_right: bool,
    pub reflow_pages: bool,
    pub colour_format: String,
    pub colour_profile: String,
    pub transparent_background: bool,
    pub margins: Margins,
    pub bleed: Margins,
    pub page_origin: [f32; 2],
    pub drawing_scale: [f32; 2],
    pub measurement_unit: crate::units::ReadoutUnit,
    pub profile_conversion: ProfileConversion,
    pub resize: ResizeMode,
    pub anchor: Anchor9,
    pub scope: SetupScope,
    pub page_ids: Vec<PageId>,
}

impl Default for DocSetup {
    fn default() -> Self {
        Self {
            units: Units::Points,
            dpi: 72.0,
            page_size: [612.0, 792.0],
            landscape: false,
            facing_pages: true,
            arrange_vertical: false,
            start_on_right: true,
            reflow_pages: false,
            colour_format: ColourFormat::RGBA_8.to_string(),
            colour_profile: crate::colour::DocColour::default().profile,
            transparent_background: false,
            margins: Margins::default(),
            bleed: Margins::default(),
            page_origin: [0.0, 0.0],
            drawing_scale: [1.0, 1.0],
            measurement_unit: Default::default(),
            profile_conversion: ProfileConversion::Convert,
            resize: ResizeMode::AnchorToPage,
            anchor: Anchor9::default(),
            scope: SetupScope::WholeDocument,
            page_ids: Vec::new(),
        }
    }
}

impl DocSetup {
    pub fn from_document(doc: &Document) -> Self {
        let (units, length_scale) = match doc.units {
            crate::document::Units::Pt => (Units::Points, 1.0),
            crate::document::Units::Mm => (Units::Millimeters, 1.0),
            crate::document::Units::Cm => (Units::Millimeters, 10.0),
            crate::document::Units::In => (Units::Inches, 1.0),
            crate::document::Units::Px => (Units::Pixels, 1.0),
        };
        let page_size = doc.pages.first().map(|page| page.size.map(|value| value * length_scale)).unwrap_or_default();
        let convert = |value: SetupMargins| Margins {
            enabled: value.enabled, left: value.left * length_scale, right: value.right * length_scale,
            top: value.top * length_scale, bottom: value.bottom * length_scale, linked: value.linked,
        };
        Self {
            units,
            dpi: doc.dpi,
            page_size,
            landscape: doc.setup.landscape,
            facing_pages: doc.setup.facing_pages,
            arrange_vertical: doc.setup.arrange_vertical,
            start_on_right: doc.setup.start_on_right,
            reflow_pages: doc.setup.reflow_pages,
            colour_format: doc.colour.format.to_string(),
            colour_profile: doc.colour.profile.clone(),
            transparent_background: doc.setup.transparent_background,
            margins: convert(doc.setup.margins),
            bleed: convert(doc.setup.bleed),
            page_origin: doc.setup.page_origin.map(|value| value * length_scale),
            drawing_scale: doc.drawing_scale,
            measurement_unit: doc.measurement_unit,
            profile_conversion: ProfileConversion::Convert,
            resize: ResizeMode::AnchorToPage,
            anchor: Anchor9::default(),
            scope: SetupScope::WholeDocument,
            page_ids: Vec::new(),
        }
    }

    pub fn colour_format(&self) -> Result<ColourFormat, CommandError> {
        self.format()
    }

    fn format(&self) -> Result<ColourFormat, CommandError> {
        ColourFormat::parse(&self.colour_format).or_else(|| match self.colour_format.as_str() {
            "RGB" | "RGBA" => Some(ColourFormat::RGBA_8),
            "CMYK" => Some(ColourFormat::CMYK_8),
            "Lab" | "LAB" => Some(ColourFormat::LAB_16),
            "Grey" | "Gray" => Some(ColourFormat::GREY_8),
            _ => None,
        }).ok_or_else(CommandError::invalid_params)
    }

    pub fn validate(&self) -> Result<(), CommandError> {
        if self.page_size.iter().any(|v| !v.is_finite() || *v <= 0.0)
            || !self.dpi.is_finite() || self.dpi <= 0.0
            || [self.anchor.x, self.anchor.y].iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || self.colour_profile.trim().is_empty()
            || self.page_origin.iter().any(|v| !v.is_finite())
        {
            return Err(CommandError::invalid_params());
        }
        Document::validate_drawing_scale(self.drawing_scale)?;
        self.format()?;
        for margins in [self.margins, self.bleed] {
            if [margins.left, margins.right, margins.top, margins.bottom].iter().any(|v| !v.is_finite() || *v < 0.0) {
                return Err(CommandError::invalid_params());
            }
        }
        let margin = if self.margins.enabled { self.margins } else { Margins::default() };
        let bleed = if self.bleed.enabled { self.bleed } else { Margins::default() };
        let width = margin.left + margin.right + bleed.left + bleed.right;
        let height = margin.top + margin.bottom + bleed.top + bleed.bottom;
        if width >= self.page_size[0] || height >= self.page_size[1] {
            return Err(CommandError::invalid_params());
        }
        Ok(())
    }

    pub fn unit_factor_for(&self, doc: &Document) -> f32 {
        unit_factor(doc.units, doc.dpi, self.units, self.dpi)
    }

    pub fn convert_units(&mut self, units: Units) {
        if units == self.units {
            return;
        }
        let k = crate::units::per_inch(units, self.dpi) / crate::units::per_inch(self.units, self.dpi);
        if !k.is_finite() || k <= 0.0 {
            return;
        }
        self.page_size = self.page_size.map(|value| value * k);
        self.page_origin = self.page_origin.map(|value| value * k);
        for margins in [&mut self.margins, &mut self.bleed] {
            margins.left *= k;
            margins.right *= k;
            margins.top *= k;
            margins.bottom *= k;
        }
        self.units = units;
    }

    fn apply_selected_spreads(&self, doc: &mut Document) -> Result<(), CommandError> {
        use crate::commands::{masters, pages};
        use crate::data_merge::layout::{object_page_index, spread_ranges};
        use std::collections::HashSet;

        if doc.layout_artboard_origins.is_none() || self.page_ids.is_empty()
            || [self.anchor.x, self.anchor.y].iter().any(|value| !(0.0..=1.0).contains(value))
        {
            return Err(CommandError::invalid_params());
        }
        for id in &self.page_ids {
            if !doc.pages.iter().any(|page| page.id == *id) {
                return Err(CommandError::not_found());
            }
        }
        let factor = unit_factor(self.units, self.dpi, doc.units, doc.dpi);
        let size = self.page_size.map(|value| value * factor);
        if size.iter().any(|value| !(0.01..=1_000_000.0).contains(value)) {
            return Err(CommandError::invalid_params());
        }
        let selected: HashSet<_> = self.page_ids.iter().copied().collect();
        let mut indices = HashSet::new();
        for spread in spread_ranges(doc) {
            if doc.pages[spread.clone()].iter().any(|page| selected.contains(&page.id)) {
                indices.extend(spread);
            }
        }

        let mut next = doc.clone();
        let old_rects = crate::scene::page_rects(doc);
        let mut changed = HashSet::new();
        let mut boards = HashSet::new();
        for index in indices {
            let [_, _, old_width, old_height] = old_rects[index];
            if [old_width, old_height] == size { continue; }
            if old_width <= 0.0 || old_height <= 0.0 || !pages::can_resize(doc, doc.pages[index].id) {
                return Err(CommandError::invalid_params());
            }
            next.pages[index].size = size;
            if let Some(board) = doc.linked_artboard(doc.pages[index].id) {
                next.find_object_mut(board.id.0).ok_or_else(CommandError::not_found)?.size = size;
                boards.insert(board.id);
            }
            changed.insert(index);
        }
        if changed.is_empty() { return Ok(()); }

        pages::preserve_content(doc, &mut next);
        let new_rects = crate::scene::page_rects(&next);
        let inherited: HashSet<_> = doc.master_pages.inherited_content()
            .flat_map(|item| item.group.into_iter().chain(item.links.iter().map(|&(_, id)| id)))
            .collect();
        for object in &mut next.objects {
            let page = object_page_index(doc, object.id);
            if !changed.contains(&page) || boards.contains(&object.id) || inherited.contains(&object.id)
                || masters::owns_source(doc, object.id)
            {
                continue;
            }
            let [_, _, width, height] = old_rects[page];
            let origin = [new_rects[page][0], new_rects[page][1]];
            let old_size = [width, height];
            let (scale, shift) = if self.resize == ResizeMode::Rescale {
                ([size[0] / width, size[1] / height], [0.0, 0.0])
            } else {
                ([1.0, 1.0], [(size[0] - old_size[0]) * self.anchor.x, (size[1] - old_size[1]) * self.anchor.y])
            };
            transform_object_dimensions(std::slice::from_mut(object), 1.0, scale, |position, length, axis| {
                (origin[axis] + (position - origin[axis]) * scale[axis] + shift[axis], length * scale[axis])
            })?;
        }
        masters::lock_positions(&mut next);
        *doc = next;
        Ok(())
    }

    pub fn try_apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let followers = crate::commands::layer_kinds::FillLayerFollowers::collect(doc);
        self.apply_dimensions_and_options(doc)?;
        followers.update(doc);
        Ok(())
    }

    fn apply_dimensions_and_options(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.scope == SetupScope::SelectedSpreads {
            return self.apply_selected_spreads(doc);
        }
        self.validate()?;
        let measure_changed = doc.units != self.units || doc.dpi != self.dpi
            || doc.drawing_scale != self.drawing_scale || doc.measurement_unit != self.measurement_unit
            || self.resize == ResizeMode::Rescale;
        let factor = unit_factor(doc.units, doc.dpi, self.units, self.dpi);
        if !factor.is_finite() || factor <= 0.0 {
            return Err(CommandError::invalid_params());
        }
        let old_size = doc.pages.first().map(|page| page.size.map(|value| value * factor)).unwrap_or(self.page_size);
        if old_size.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(CommandError::invalid_params());
        }
        let format = self.format()?;
        let scale = [self.page_size[0] / old_size[0], self.page_size[1] / old_size[1]];
        let shift = [(self.page_size[0] - old_size[0]) * self.anchor.x, (self.page_size[1] - old_size[1]) * self.anchor.y];
        if scale.iter().chain(shift.iter()).any(|v| !v.is_finite()) {
            return Err(CommandError::invalid_params());
        }
        let rescale = self.resize == ResizeMode::Rescale;
        let place = |pos: f32, size: f32, axis: usize| -> (f32, f32) {
            if rescale {
                (pos * factor * scale[axis], size * factor * scale[axis])
            } else {
                (pos * factor + shift[axis], size * factor)
            }
        };
        let previous_layout = doc.layout_artboard_origins.is_some().then(|| doc.clone());
        let curve_scale = if rescale { [factor * scale[0], factor * scale[1]] } else { [factor, factor] };
        transform_object_dimensions(&mut doc.objects, factor, curve_scale, place)?;
        if factor != 1.0 {
            doc.symbols.convert_units(factor)?;
            for setup in doc.master_pages.page_setup.values_mut() {
                let margins = &mut setup.margins;
                for distance in [&mut margins.left, &mut margins.right, &mut margins.top, &mut margins.bottom] {
                    *distance *= factor;
                }
            }
            doc.rulers.origin = doc.rulers.origin.map(|value| value * factor);
            if let Some(settings) = &mut doc.grid.baseline {
                settings.offset *= factor;
                doc.grid.baseline_spacing *= factor;
            }
            for slice in &mut doc.slices {
                for value in &mut slice.bounds {
                    *value *= factor;
                }
            }
        }
        doc.units = self.units;
        doc.dpi = self.dpi;
        doc.drawing_scale = self.drawing_scale;
        doc.measurement_unit = self.measurement_unit;
        if doc.pages.is_empty() { doc.push_page(self.page_size); }
        for page in &mut doc.pages {
            if let Some(layout) = &mut page.column_guides {
                layout.gutter *= factor;
            }
            page.size = if previous_layout.is_some() && old_size == self.page_size {
                page.size.map(|value| value * factor)
            } else {
                self.page_size
            };
        }
        doc.colour.format = format;
        doc.colour.profile = self.colour_profile.clone();
        doc.setup = DocumentSetup {
            landscape: self.landscape,
            facing_pages: self.facing_pages,
            arrange_vertical: self.arrange_vertical,
            start_on_right: self.start_on_right,
            reflow_pages: self.reflow_pages,
            transparent_background: self.transparent_background,
            margins: self.margins.into(),
            bleed: self.bleed.into(),
            page_origin: self.page_origin,
        };
        if let Some(before) = previous_layout {
            if let Some(origins) = &mut doc.layout_artboard_origins {
                for origin in origins.values_mut() {
                    *origin = std::array::from_fn(|axis| place(origin[axis], 0.0, axis).0);
                }
            }
            let flow = crate::data_merge::layout::flow_origins(doc);
            crate::commands::pages::move_page_content(&before, doc, |_, index| {
                let old = crate::data_merge::layout::page_origin(&before, index);
                let linked = before.pages.get(index).is_some_and(|page| before.linked_artboard(page.id).is_some());
                std::array::from_fn(|axis| {
                    let mut transformed = place(old[axis], 0.0, axis).0;
                    if !linked && !rescale {
                        transformed -= shift[axis];
                    }
                    flow[index][axis] + old[axis] - transformed
                })
            });
            crate::commands::masters::resize_layer_references(&before, doc, curve_scale);
            crate::commands::masters::lock_positions(doc);
        }
        if measure_changed {
            crate::commands::measure::refresh_saved_dimensions(doc);
        }
        Ok(())
    }

    pub fn apply(&self, doc: &mut Document) {
        let _ = self.try_apply(doc);
    }
}

pub(crate) fn transform_object_dimensions(
    objects: &mut [crate::Object],
    factor: f32,
    curve_scale: [f32; 2],
    place: impl Fn(f32, f32, usize) -> (f32, f32),
) -> Result<(), CommandError> {
    for object in objects.iter() {
        for axis in 0..2 {
            let (position, size) = place(object.pos[axis], object.size[axis], axis);
            if !position.is_finite() || !size.is_finite() { return Err(CommandError::invalid_params()); }
        }
    }
    for object in objects.iter_mut() {
        for axis in 0..2 {
            let (position, size) = place(object.pos[axis], object.size[axis], axis);
            object.pos[axis] = position;
            object.size[axis] = size;
        }
        if factor != 1.0 {
            convert_object_lengths(object, factor)?;
        }
        if curve_scale != [1.0, 1.0] {
            if let crate::object::ObjectKind::Curve(curve) = &mut object.kind {
                crate::canvas_size::scale_curve(curve, curve_scale[0] as f64, curve_scale[1] as f64);
            }
        }
        if let Some(frame) = &mut object.frame {
            if factor != 1.0 {
                frame.gutter *= factor;
                frame.max_para_space *= factor;
                for inset in &mut frame.insets {
                    *inset *= factor;
                }
                if let Some(widths) = &mut frame.column_widths {
                    for width in widths {
                        *width *= factor;
                    }
                }
            }
            for axis in 0..2 {
                frame.size[axis] *= curve_scale[axis];
            }
            if let Some(path) = &mut frame.shape_path {
                for point in path {
                    point[0] *= curve_scale[0];
                    point[1] *= curve_scale[1];
                }
            }
        }
    }
    Ok(())
}

fn convert_object_lengths(object: &mut crate::Object, factor: f32) -> Result<(), CommandError> {
    use crate::object::ObjectKind;
    use crate::paint::{Transparency, transform_paint_matrix};

    let scale = f64::from(factor);
    let transform = kurbo::Affine::scale(scale);
    let paint = |value: &mut crate::paint::Paint| transform_paint_matrix(value, transform.as_coeffs());
    for fill in std::iter::once(&mut object.fill).chain(&mut object.additional_fills) {
        paint(fill);
    }
    for stroke in std::iter::once(&mut object.stroke).chain(&mut object.additional_strokes) {
        paint(&mut stroke.paint);
        stroke.width *= factor;
        stroke.dash_offset *= factor;
        for dash in &mut stroke.dash {
            *dash *= factor;
        }
    }
    match &mut object.transparency {
        Transparency::Gradient { kind, geom, .. } => *geom = geom.transform_for_kind(*kind, transform.as_coeffs()),
        Transparency::InverseBitmap(bitmap) => {
            bitmap.matrix = (transform * kurbo::Affine::new(bitmap.matrix)).as_coeffs();
        }
        Transparency::None => {}
    }
    if let Some(shape) = &mut object.shape {
        shape.size = shape.size.map(|length| length * factor);
        if let Some(corner) = shape.corner.as_mut().filter(|corner| corner.absolute) {
            corner.radii = corner.radii.map(|length| length * factor);
        }
    }
    if let Some(crop) = &mut object.vector_crop {
        crop.rect = crop.rect.map(|length| length * factor);
    }
    match &mut object.kind {
        ObjectKind::Curve(curve) => {
            for contour in std::iter::once(&mut curve.contour).chain(&mut curve.holes) {
                for corner in contour.points.iter_mut().filter_map(|point| point.corner.as_mut()) {
                    corner.radius *= scale;
                }
            }
        }
        ObjectKind::Compound(compound) => {
            compound.origin = compound.origin.map(|length| length * factor);
            transform_object_dimensions(&mut compound.operands, factor, [factor; 2], |pos, size, _| {
                (pos * factor, size * factor)
            })?;
        }
        ObjectKind::Dimension(dimension) => {
            dimension.start = dimension.start.map(|length| length * scale);
            dimension.end = dimension.end.map(|length| length * scale);
            dimension.offset *= scale;
        }
        _ => {}
    }
    for effect in &mut object.effects {
        effect.scale_lengths(factor);
    }
    Ok(())
}

#[derive(Clone)]
pub struct PreviewStore {
    pub token: u64,
    pub original: Document,
}

impl PreviewStore {
    pub fn new(token: u64, doc: &Document) -> Self {
        Self {
            token,
            original: doc.clone(),
        }
    }
}
