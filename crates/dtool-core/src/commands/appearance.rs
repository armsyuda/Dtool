use crate::command::Session;
use crate::object::{Object, ObjectKind};
use crate::paint::{Paint, PaintOrder, StrokeStyle, validate_paint, validate_stroke};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::blend::BlendMode;
use std::collections::{BTreeMap, BTreeSet};

pub fn supports_multiple(object: &Object) -> bool {
    matches!(object.kind, ObjectKind::Shape | ObjectKind::Curve(_) | ObjectKind::Compound(_))
}

pub fn has_layers(object: &Object) -> bool {
    !object.additional_fills.is_empty() || !object.additional_strokes.is_empty()
        || !object.hidden_appearance.is_empty() || !object.appearance_stack.is_empty()
        || !object.appearance_blends.is_empty()
}

pub fn visible_fills<'a>(object: &'a Object, primary: &'a Paint) -> impl Iterator<Item = &'a Paint> {
    std::iter::once(primary).chain(&object.additional_fills).enumerate()
        .filter_map(|(index, paint)| (!object.hidden_appearance.fills.contains(&index)).then_some(paint))
}

pub fn visible_strokes(object: &Object) -> impl Iterator<Item = &StrokeStyle> {
    std::iter::once(&object.stroke).chain(&object.additional_strokes).enumerate()
        .filter_map(|(index, stroke)| (!object.hidden_appearance.strokes.contains(&index)).then_some(stroke))
}

pub struct AppearancePath {
    pub path: kurbo::BezPath,
    pub slot: PaintSlot,
    pub paint: Paint,
    pub stroke: Option<StrokeStyle>,
    pub even_odd: bool,
    pub blend: RowBlend,
}

pub fn vector_paths(doc: &crate::Document, object: &Object, primary_fill: &Paint, native_strokes: bool) -> Option<Vec<AppearancePath>> {
    vector_paths_with_tolerance(doc, object, primary_fill, native_strokes, crate::outline::OUTLINE_TOLERANCE)
}

pub fn vector_paths_with_tolerance(doc: &crate::Document, object: &Object, primary_fill: &Paint,
    native_strokes: bool, tolerance: f64,
) -> Option<Vec<AppearancePath>> {
    let world = crate::outline::object_outline_world_with_tolerance(object, tolerance)?;
    let [x, y] = object.pos.map(f64::from);
    let matrix = kurbo::Affine::new(crate::geometry::object_matrix(object).map(f64::from)) * kurbo::Affine::translate((x, y));
    let world = crate::commands::warp::live_warp_outline_with_tolerance(doc, object.id, &world, tolerance).unwrap_or(world);
    let stroke_tolerance = tolerance.min(crate::boolean::DEFAULT_TOLERANCE);
    let mut closed = kurbo::BezPath::new();
    let mut start = 0;
    for (index, element) in world.elements().iter().enumerate() {
        match element {
            kurbo::PathEl::MoveTo(_) => start = index,
            kurbo::PathEl::ClosePath => closed.extend(world.elements()[start..=index].iter().copied()),
            _ => {}
        }
    }
    let mapped = |paint: &Paint| match paint {
        Paint::Gradient { kind, stops, geom } => Paint::Gradient { kind: *kind, stops: stops.clone(), geom: geom.transform(matrix.as_coeffs()) },
        Paint::Bitmap(bitmap) => Paint::Bitmap(crate::paint::BitmapPaint {
            matrix: (matrix * kurbo::Affine::new(bitmap.matrix)).as_coeffs(),
            ..bitmap.clone()
        }),
        paint => paint.clone(),
    };
    let mut paths = Vec::new();
    let add_stroke = |paths: &mut Vec<AppearancePath>, path: &kurbo::BezPath, style: &StrokeStyle, slot: PaintSlot, blend: RowBlend| {
        let native = native_strokes && !blend.erases() && style.pressure.is_empty()
            && style.cap != crate::paint::Cap::RoundInset && style.align == crate::paint::StrokeAlign::Centre;
        let paint = mapped(&style.paint);
        if native {
            paths.push(AppearancePath { path: path.clone(), slot, stroke: Some(StrokeStyle { paint: paint.clone(), ..style.clone() }), paint, even_odd: false, blend });
        } else if let Some((outline, even_odd)) = crate::outline::styled_stroke_path_with_tolerance(path, style, stroke_tolerance)
            .or_else(|| {
                (stroke_tolerance < crate::boolean::DEFAULT_TOLERANCE)
                    .then(|| crate::outline::styled_stroke_path(path, style)).flatten()
            })
        {
            paths.push(AppearancePath { path: outline, slot, paint, stroke: None, even_odd, blend });
        }
    };
    let fill_count = object.additional_fills.len() + usize::from(
        PaintSlot { kind: Kind::Fill, index: 0 }.paint(object).is_some() || *primary_fill != Paint::None,
    );
    let stroke_count = object.additional_strokes.len() + usize::from(
        PaintSlot { kind: Kind::Stroke, index: 0 }.stroke(object).is_some(),
    );
    for (kind, index) in ordered_slots(fill_count, stroke_count, &object.appearance_stack, object.paint_order) {
        let slot = PaintSlot { kind, index };
        if !object.hidden_appearance.is_visible(slot) {
            continue;
        }
        let blend = object.appearance_blends.get(slot);
        if kind == Kind::Fill {
            let paint = if index == 0 { primary_fill } else { &object.additional_fills[index - 1] };
            if *paint != Paint::None && !closed.elements().is_empty() {
                paths.push(AppearancePath { path: closed.clone(), slot, paint: mapped(paint), stroke: None, even_odd: object.fill_rule.is_even_odd(), blend });
            }
            continue;
        }
        let Some(stroke) = slot.stroke(object).filter(|stroke| stroke.width > 0.0 && stroke.paint != Paint::None) else { continue; };
        add_stroke(&mut paths, &world, stroke, slot, blend);
        for (path, arrow_stroke) in crate::commands::stroke::arrow_paths(&world, stroke) {
            if let Some(style) = arrow_stroke {
                add_stroke(&mut paths, &path, &style, slot, blend);
            } else {
                paths.push(AppearancePath { path, slot, paint: mapped(&stroke.paint), stroke: None, even_odd: false, blend });
            }
        }
    }
    Some(paths)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Fill,
    Stroke,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RowOperator {
    Erase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RowBlend {
    Standard(BlendMode),
    Operator(RowOperator),
}

impl Default for RowBlend {
    fn default() -> Self {
        Self::Standard(BlendMode::Normal)
    }
}

impl RowBlend {
    pub const ERASE: Self = Self::Operator(RowOperator::Erase);

    pub fn is_normal(self) -> bool {
        matches!(self, Self::Standard(mode) if mode.is_normal())
    }

    pub fn erases(self) -> bool {
        self == Self::ERASE
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RowBlends {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    fills: BTreeMap<usize, RowBlend>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    strokes: BTreeMap<usize, RowBlend>,
}

impl RowBlends {
    pub fn is_empty(&self) -> bool {
        self.fills.is_empty() && self.strokes.is_empty()
    }

    pub fn get(&self, slot: PaintSlot) -> RowBlend {
        let rows = match slot.kind { Kind::Fill => &self.fills, Kind::Stroke => &self.strokes };
        rows.get(&slot.index).copied().unwrap_or_default()
    }

    pub(crate) fn set(&mut self, slot: PaintSlot, mode: RowBlend) -> bool {
        let rows = self.rows_mut(slot.kind);
        if mode == RowBlend::default() {
            rows.remove(&slot.index).is_some()
        } else {
            rows.insert(slot.index, mode) != Some(mode)
        }
    }

    fn rows_mut(&mut self, kind: Kind) -> &mut BTreeMap<usize, RowBlend> {
        match kind { Kind::Fill => &mut self.fills, Kind::Stroke => &mut self.strokes }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HiddenRows {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    fills: BTreeSet<usize>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    strokes: BTreeSet<usize>,
}

impl HiddenRows {
    pub fn is_empty(&self) -> bool {
        self.fills.is_empty() && self.strokes.is_empty()
    }

    pub fn is_visible(&self, slot: PaintSlot) -> bool {
        !match slot.kind {
            Kind::Fill => &self.fills,
            Kind::Stroke => &self.strokes,
        }.contains(&slot.index)
    }

    fn rows_mut(&mut self, kind: Kind) -> &mut BTreeSet<usize> {
        match kind {
            Kind::Fill => &mut self.fills,
            Kind::Stroke => &mut self.strokes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaintSlot {
    pub kind: Kind,
    pub index: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct Focus {
    pub node: crate::ObjectId,
    pub slot: PaintSlot,
    generation: u64,
}

impl PaintSlot {
    pub fn stroke(self, object: &Object) -> Option<&StrokeStyle> {
        if self.kind != Kind::Stroke { return None; }
        if self.index == 0 {
            (object.stroke != StrokeStyle::default() || !object.additional_strokes.is_empty()
                || object.hidden_appearance.strokes.contains(&0)
                || object.appearance_blends.strokes.contains_key(&0)
                || object.appearance_stack.contains(&Kind::Stroke)).then_some(&object.stroke)
        } else {
            object.additional_strokes.get(self.index - 1)
        }
    }

    pub fn paint(self, object: &Object) -> Option<&Paint> {
        match self.kind {
            Kind::Stroke => self.stroke(object).map(|stroke| &stroke.paint),
            Kind::Fill if self.index == 0 => (object.fill != Paint::None || !object.additional_fills.is_empty()
                || object.hidden_appearance.fills.contains(&0)
                || object.appearance_blends.fills.contains_key(&0)
                || object.appearance_stack.contains(&Kind::Fill)).then_some(&object.fill),
            Kind::Fill => object.additional_fills.get(self.index - 1),
        }
    }

    pub fn paint_command(self, node: crate::ObjectId, paint: Paint) -> Command {
        Command::with_params("appearance.update", json!({
            "node": node, "kind": kind_name(self.kind), "index": self.index, "paint": paint,
        }))
    }

    pub fn stroke_command(self, node: crate::ObjectId, patch: Value) -> Command {
        Command::with_params("appearance.update", json!({
            "node": node, "kind": kind_name(self.kind), "index": self.index, "stroke_patch": patch,
        }))
    }

    pub fn focus_command(self, node: crate::ObjectId) -> Command {
        Command::with_params("appearance.focus", json!({
            "node": node, "kind": kind_name(self.kind), "index": self.index,
        }))
    }
}

impl Focus {
    pub fn current(session: &Session) -> Option<Self> {
        session.appearance_focus.filter(|focus| focus.active(session))
    }

    pub fn active(self, session: &Session) -> bool {
        session.selection.ids.as_slice() == [self.node]
            && self.generation == session.generation
            && session.document.get_object(self.node).is_some_and(|object| !object.locked && self.slot.paint(object).is_some())
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AppearanceList {
    pub fills: Vec<Paint>,
    pub strokes: Vec<StrokeStyle>,
    #[serde(default, skip_serializing_if = "HiddenRows::is_empty")]
    pub hidden: HiddenRows,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stack: Vec<Kind>,
    #[serde(default, skip_serializing_if = "RowBlends::is_empty")]
    pub blends: RowBlends,
}

pub trait AppearanceSlots {
    fn appearance(&self) -> AppearanceList;
    fn set_appearance(&mut self, list: &AppearanceList) -> Result<(), CommandError>;
    fn paint_order(&self) -> PaintOrder;
}

impl AppearanceSlots for Object {
    fn appearance(&self) -> AppearanceList {
        AppearanceList {
            fills: PaintSlot { kind: Kind::Fill, index: 0 }.paint(self).cloned()
                .into_iter().chain(self.additional_fills.iter().cloned()).collect(),
            strokes: PaintSlot { kind: Kind::Stroke, index: 0 }.stroke(self).cloned()
                .into_iter().chain(self.additional_strokes.iter().cloned()).collect(),
            hidden: self.hidden_appearance.clone(),
            stack: self.appearance_stack.clone(),
            blends: self.appearance_blends.clone(),
        }
    }

    fn set_appearance(&mut self, list: &AppearanceList) -> Result<(), CommandError> {
        if (!supports_multiple(self) && (list.fills.len() > 1 || list.strokes.len() > 1 || !list.hidden.is_empty() || !list.stack.is_empty() || !list.blends.is_empty()))
            || list.hidden.fills.last().is_some_and(|&index| index >= list.fills.len())
            || list.hidden.strokes.last().is_some_and(|&index| index >= list.strokes.len())
            || list.blends.fills.last_key_value().is_some_and(|(&index, _)| index >= list.fills.len())
            || list.blends.strokes.last_key_value().is_some_and(|(&index, _)| index >= list.strokes.len())
            || !list.fills.iter().all(validate_paint) || !list.strokes.iter().all(validate_stroke)
        {
            return Err(CommandError::invalid_params());
        }
        self.fill = list.fills.first().cloned().unwrap_or(Paint::None);
        self.stroke = list.strokes.first().cloned().unwrap_or_default();
        self.additional_fills = list.fills.iter().skip(1).cloned().collect();
        self.additional_strokes = list.strokes.iter().skip(1).cloned().collect();
        self.hidden_appearance = list.hidden.clone();
        self.appearance_stack.clone_from(&list.stack);
        self.appearance_blends.clone_from(&list.blends);
        Ok(())
    }

    fn paint_order(&self) -> PaintOrder {
        self.paint_order
    }
}

impl AppearanceList {
    pub fn len(&self, kind: Kind) -> usize {
        match kind {
            Kind::Fill => self.fills.len(),
            Kind::Stroke => self.strokes.len(),
        }
    }

    pub fn add_fill(&mut self, paint: Paint) -> usize {
        self.fills.push(paint);
        self.append_slot(Kind::Fill);
        self.fills.len() - 1
    }

    pub fn add_stroke(&mut self, stroke: StrokeStyle) -> usize {
        self.strokes.push(stroke);
        self.append_slot(Kind::Stroke);
        self.strokes.len() - 1
    }

    fn append_slot(&mut self, kind: Kind) {
        if !self.stack.is_empty() {
            let index = self.stack.iter().rposition(|&slot| slot == kind)
                .map_or(self.stack.len(), |index| index + 1);
            self.stack.insert(index, kind);
        }
    }

    pub fn remove(&mut self, kind: Kind, index: usize) -> Result<(), CommandError> {
        if index >= self.len(kind) {
            return Err(CommandError::invalid_params());
        }
        if let Some((position, _)) = self.stack.iter().enumerate().filter(|(_, slot)| **slot == kind).nth(index) {
            self.stack.remove(position);
        }
        match kind {
            Kind::Fill => {
                self.fills.remove(index);
            }
            Kind::Stroke => {
                self.strokes.remove(index);
            }
        }
        let hidden = self.hidden.rows_mut(kind);
        *hidden = hidden.iter().copied().filter_map(|row| {
            (row != index).then(|| if row > index { row - 1 } else { row })
        }).collect();
        let blends = self.blends.rows_mut(kind);
        *blends = blends.iter().filter_map(|(&row, &mode)| {
            (row != index).then(|| (if row > index { row - 1 } else { row }, mode))
        }).collect();
        Ok(())
    }

    pub fn reorder(&mut self, kind: Kind, from: usize, to: usize) -> Result<bool, CommandError> {
        let len = self.len(kind);
        if from >= len || to >= len {
            return Err(CommandError::invalid_params());
        }
        if from == to {
            return Ok(false);
        }
        match kind {
            Kind::Fill => {
                let item = self.fills.remove(from);
                self.fills.insert(to, item);
            }
            Kind::Stroke => {
                let item = self.strokes.remove(from);
                self.strokes.insert(to, item);
            }
        }
        let hidden = self.hidden.rows_mut(kind);
        *hidden = hidden.iter().map(|&row| moved_index(row, from, to)).collect();
        let blends = self.blends.rows_mut(kind);
        *blends = blends.iter().map(|(&row, &mode)| (moved_index(row, from, to), mode)).collect();
        Ok(true)
    }

    pub fn render_order(&self, order: PaintOrder) -> Vec<(Kind, usize)> {
        ordered_slots(self.fills.len(), self.strokes.len(), &self.stack, order)
    }

    fn move_slot(&mut self, order: PaintOrder, from: usize, to: usize) -> Result<Option<(Kind, usize, usize)>, CommandError> {
        let mut slots = self.render_order(order);
        if from >= slots.len() || to >= slots.len() {
            return Err(CommandError::invalid_params());
        }
        if from == to {
            return Ok(None);
        }
        let (kind, index) = slots.remove(from);
        slots.insert(to, (kind, index));
        let new_index = slots[..to].iter().filter(|(other, _)| *other == kind).count();
        self.reorder(kind, index, new_index)?;
        let default = ordered_slots(self.fills.len(), self.strokes.len(), &[], order);
        self.stack = if slots.iter().map(|(kind, _)| kind).eq(default.iter().map(|(kind, _)| kind)) {
            Vec::new()
        } else {
            slots.into_iter().map(|(kind, _)| kind).collect()
        };
        Ok(Some((kind, index, new_index)))
    }
}

fn moved_index(row: usize, from: usize, to: usize) -> usize {
    if row == from { to }
    else if from < row && row <= to { row - 1 }
    else if to <= row && row < from { row + 1 }
    else { row }
}

fn ordered_slots(fills: usize, strokes: usize, stack: &[Kind], order: PaintOrder) -> Vec<(Kind, usize)> {
    if !stack.is_empty() && stack.len() == fills + strokes
        && stack.iter().filter(|&&kind| kind == Kind::Fill).count() == fills
    {
        let (mut fill, mut stroke) = (0, 0);
        return stack.iter().map(|&kind| {
            let index = match kind { Kind::Fill => &mut fill, Kind::Stroke => &mut stroke };
            let slot = (kind, *index);
            *index += 1;
            slot
        }).collect();
    }
    let fills = (0..fills).map(|i| (Kind::Fill, i));
    let strokes = (0..strokes).map(|i| (Kind::Stroke, i));
    match order {
        PaintOrder::StrokeOver => fills.chain(strokes).collect(),
        PaintOrder::StrokeUnder => strokes.chain(fills).collect(),
    }
}

pub fn query_json(node: u64, list: &AppearanceList, order: PaintOrder) -> Value {
    let render: Vec<Value> = list.render_order(order).into_iter().map(|(kind, index)| json!({ "kind": kind_name(kind), "index": index })).collect();
    json!({
        "node": node,
        "fills": list.fills,
        "strokes": list.strokes,
        "hidden": list.hidden,
        "blends": list.blends,
        "paint_order": order,
        "render_order": render
    })
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Fill => "fill",
        Kind::Stroke => "stroke",
    }
}

fn kind_param(cmd: &Command) -> Result<Kind, CommandError> {
    match cmd.params.get("kind").and_then(Value::as_str).unwrap_or("fill") {
        "fill" => Ok(Kind::Fill),
        "stroke" => Ok(Kind::Stroke),
        _ => Err(CommandError::invalid_params()),
    }
}

pub fn edit_list(cmd: &Command, list: &mut AppearanceList) -> Result<bool, CommandError> {
    match cmd.name.as_str() {
        "appearance.add_fill" => {
            let paint: Paint = serde_json::from_value(cmd.params.get("paint").cloned().unwrap_or(Value::Null)).map_err(|_| CommandError::invalid_params())?;
            list.add_fill(paint);
            Ok(true)
        }
        "appearance.add_stroke" => {
            let stroke: StrokeStyle = serde_json::from_value(cmd.params.get("stroke").cloned().unwrap_or(Value::Null)).map_err(|_| CommandError::invalid_params())?;
            list.add_stroke(stroke);
            Ok(true)
        }
        "appearance.remove" => {
            list.remove(kind_param(cmd)?, index_param(cmd, "index")?)?;
            Ok(true)
        }
        "appearance.update" => {
            let kind = kind_param(cmd)?;
            let index = index_param(cmd, "index")?;
            let visible = cmd.params.get("visible").map(|value| value.as_bool().ok_or_else(CommandError::invalid_params)).transpose()?;
            let paint = cmd.params.get("paint").map(crate::paint::paint_from_param).transpose()?;
            let blend: Option<RowBlend> = cmd.params.get("blend").cloned().map(serde_json::from_value)
                .transpose().map_err(|_| CommandError::invalid_params())?;
            if blend == Some(RowBlend::Standard(BlendMode::PassThrough)) {
                return Err(CommandError::invalid_params());
            }
            let paint_changed = match kind {
                Kind::Fill => {
                    let target = list.fills.get_mut(index).ok_or_else(CommandError::invalid_params)?;
                    match paint {
                        Some(paint) if *target != paint => { *target = paint; true }
                        None if visible.is_none() && blend.is_none() => return Err(CommandError::invalid_params()),
                        _ => false,
                    }
                }
                Kind::Stroke => {
                    let target = list.strokes.get_mut(index).ok_or_else(CommandError::invalid_params)?;
                    let before = target.clone();
                    if let Some(value) = cmd.params.get("stroke_patch") {
                        let mut patch = super::stroke::StrokePatch::from_params(value)?;
                        // Editing stroke geometry must preserve an explicitly cleared paint.
                        patch.paint.get_or_insert_with(|| target.paint.clone());
                        *target = patch.applied(target)?;
                    }
                    if let Some(paint) = paint {
                        target.paint = paint;
                    }
                    if let Some(value) = cmd.params.get("width") {
                        target.width = value.as_f64().ok_or_else(CommandError::invalid_params)? as f32;
                    }
                    *target != before
                }
            };
            let hidden = list.hidden.rows_mut(kind);
            let visibility_changed = match visible {
                Some(true) => hidden.remove(&index),
                Some(false) => hidden.insert(index),
                None => false,
            };
            let blend_changed = blend.is_some_and(|mode| list.blends.set(PaintSlot { kind, index }, mode));
            Ok(paint_changed || visibility_changed || blend_changed)
        }
        "appearance.reorder" => list.reorder(kind_param(cmd)?, index_param(cmd, "from")?, index_param(cmd, "to")?),
        _ => Err(CommandError::not_implemented()),
    }
}

fn ours(cmd: &Command) -> bool {
    matches!(cmd.name.as_str(), "appearance.add_fill" | "appearance.add_stroke" | "appearance.remove" | "appearance.reorder" | "appearance.move" | "appearance.update" | "appearance.query" | "appearance.focus")
}

pub fn can_execute(session: &Session, cmd: &Command) -> Option<bool> {
    if !ours(cmd) {
        return None;
    }
    Some(session.has_document && (cmd.name == "appearance.focus" && cmd.params["clear"] == true
        || node_param(cmd).is_ok_and(|id| session.document.objects.iter().any(|object| object.id.0 == id))))
}

pub fn run(session: &mut Session, cmd: &Command) -> Option<CommandResult> {
    if !ours(cmd) {
        return None;
    }
    Some((|| {
        if cmd.name == "appearance.focus" && cmd.params["clear"] == true {
            session.appearance_focus = None;
            return Ok(json!({"focused": false}));
        }
        let node_id = node_param(cmd)?;
        let old = session.document.objects.iter().find(|o| o.id.0 == node_id).ok_or_else(CommandError::not_found)?.clone();
        if cmd.name == "appearance.query" {
            return Ok(query_json(node_id, &old.appearance(), old.paint_order));
        }
        if old.locked { return Err(CommandError::invalid_params()); }
        if cmd.name == "appearance.focus" {
            let slot = PaintSlot { kind: kind_param(cmd)?, index: index_param(cmd, "index")? };
            if !supports_multiple(&old) || slot.paint(&old).is_none()
                || session.selection.ids.as_slice() != [old.id]
            {
                return Err(CommandError::invalid_params());
            }
            session.appearance_focus = Some(Focus { node: old.id, slot, generation: session.generation });
            return Ok(json!({"focused": true, "node": old.id, "kind": kind_name(slot.kind), "index": slot.index}));
        }
        let mut list = old.appearance();
        let movement = if cmd.name == "appearance.move" {
            list.move_slot(old.paint_order, index_param(cmd, "from")?, index_param(cmd, "to")?)?
        } else {
            None
        };
        let changed = if cmd.name == "appearance.move" { movement.is_some() } else { edit_list(cmd, &mut list)? };
        let mut next = old.clone();
        next.set_appearance(&list)?;
        if changed {
            let focus = Focus::current(session).filter(|focus| focus.node == old.id).and_then(|mut focus| {
                if let Some((kind, from, to)) = movement {
                    if focus.slot.kind == kind {
                        focus.slot.index = moved_index(focus.slot.index, from, to);
                    }
                } else if cmd.name != "appearance.update" || kind_param(cmd).ok() != Some(focus.slot.kind)
                    || index_param(cmd, "index").ok() != Some(focus.slot.index)
                {
                    return None;
                }
                Some(focus)
            });
            if let Some(obj) = session.document.objects.iter_mut().find(|o| o.id.0 == node_id) {
                *obj = next.clone();
            }
            session.push_restore_undo(&cmd.name, cmd.clone(), vec![old]);
            if let Some(mut focus) = focus {
                focus.generation = session.generation;
                session.appearance_focus = Some(focus);
            }
        }
        Ok(json!({
            "doc_version": session.generation,
            "changes": if changed { json!([{ "name": cmd.name, "id": node_id }]) } else { json!([]) },
            "appearance": query_json(node_id, &list, next.paint_order)
        }))
    })())
}

fn node_param(cmd: &Command) -> Result<u64, CommandError> {
    cmd.params.get("node").and_then(|v| v.as_u64()).ok_or_else(CommandError::invalid_params)
}

fn index_param(cmd: &Command, key: &str) -> Result<usize, CommandError> {
    cmd.params.get(key).and_then(|v| v.as_u64()).map(|v| v as usize).ok_or_else(CommandError::invalid_params)
}

pub fn apply(session: &mut Session, cmd: &Command) -> Option<Result<(), CommandError>> {
    if !ours(cmd) {
        return None;
    }
    if matches!(cmd.name.as_str(), "appearance.query" | "appearance.focus") {
        return Some(Ok(()));
    }
    Some((|| {
        let node_id = node_param(cmd)?;
        let obj = session.document.objects.iter_mut().find(|o| o.id.0 == node_id).ok_or_else(CommandError::not_found)?;
        let mut list = obj.appearance();
        if cmd.name == "appearance.move" {
            list.move_slot(obj.paint_order, index_param(cmd, "from")?, index_param(cmd, "to")?)?;
        } else {
            edit_list(cmd, &mut list)?;
        }
        obj.set_appearance(&list)
    })())
}
