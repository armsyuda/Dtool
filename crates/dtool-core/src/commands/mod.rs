pub mod frame_text;
pub mod resource_cmd;
pub mod document_change;
pub mod image_place;
pub mod layer_mask;
pub mod layer_style;
#[path = "../cmd/arrange.rs"]
pub mod arrange;
#[path = "../cmd/layer_ops.rs"]
pub mod layer_ops;
#[path = "../cmd/isolate.rs"]
pub mod isolate;
#[path = "../cmd/layer_state.rs"]
pub mod layer_state;
#[path = "../cmd/picture_frame.rs"]
pub mod picture_frame;
pub mod style_pick;
pub mod export_presets;
pub mod curves;
pub mod vector_crop;
pub mod curve_edit;
pub mod curve_nodes;
pub mod measure;
pub mod selection_curves;
pub mod warp;
pub mod text_case;
pub mod text_styles_cmd;
pub mod image_trace;
pub mod symbols;
pub fn register() {}

pub mod view_actual;
pub mod view_flags;
pub mod text_host;
pub mod macro_host;
pub mod align;
pub mod toggles;
pub mod active_story;
pub mod selection_key;
pub mod nudge;
pub mod duplicate;
pub mod stroke;
pub mod defaults;
pub mod assets;
pub mod caption;
pub mod node_icc;
pub mod appearance;
pub mod recent;
pub mod colour_pick;
pub mod tools;
pub mod context_toolbar;
pub mod context_menu;
pub mod lasso;
pub mod shape;
pub mod qr;
pub mod paint_order;
pub mod feature_restore;
pub mod recovery;
pub mod ai_connection;
pub mod ui;
pub mod history_menu;
pub mod longdoc;
pub mod pages;
pub mod masters;
pub mod page_artboards;
pub mod selection;
pub mod selection_edit;
pub mod path_nodes;
pub mod path_points;
pub mod shortcuts;
pub mod layer_kinds;
pub mod pixel_brush;
pub mod tone_blend;
pub mod pixel_presets;
pub mod layer_meta;
pub mod brush;
pub mod red_eye;
pub mod selection_magic_wand;
pub mod selection_refine;
pub mod pixel_selection;
pub mod fill;
pub mod document_files;
pub mod export_batch;
pub mod table_styles;
pub mod grid;
pub mod rulers;
pub mod ruler_units;
pub mod snap;

use crate::command::Session;
pub use dtool_protocol::{Command, CommandError, CommandResult};

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if let Some(value) = crate::text_lines::can_execute(session, command) { return Some(value); }
    if let Some(value) = crate::table::selection::can_execute(session, command) { return Some(value); }
    if let Some(value) = image_place::can_execute(session, command) { return Some(value); }
    if let Some(value) = layer_mask::can_execute(session, command) { return Some(value); }
    if let Some(value) = crate::clip_mask::can_execute(session, command) { return Some(value); }
    if let Some(value) = crate::effect_raster::can_execute(session, command) { return Some(value); }
    if let Some(value) = layer_style::can_execute(session, command) { return Some(value); }
    if let Some(value)=grid::can_execute(session,command) {return Some(value);}
    if let Some(value)=rulers::can_execute(session,command) {return Some(value);}
    if let Some(value)=snap::can_execute(session,command) {return Some(value);}
    if let Some(value)=frame_text::can_execute(session,command) {return Some(value);}
    if let Some(value)=resource_cmd::can_execute(session,command) {return Some(value);}
    if command.name == "text.toggle_unicode" { return Some(crate::text_cmd::can_toggle_unicode(session)); }
    if crate::text_cmd::can_execute(command) { return Some(session.has_document); }
    if let Some(value)=selection_curves::can_execute(session,command) {return Some(value);}
    if let Some(value)=warp::can_execute(session,command) {return Some(value);}
    if let Some(value)=text_case::can_execute(session,command) {return Some(value);}
    if let Some(value)=text_styles_cmd::can_execute(session,command) {return Some(value);}
    if let Some(value) = image_trace::can_execute(session, command) { return Some(value); }
    if let Some(value) = symbols::can_execute(session, command) { return Some(value); }
    if let Some(v) = selection_modify::can_execute(session, command) { return Some(v); }
    if let Some(v) = slice::can_execute(session, command) { return Some(v); }
    if let Some(v) = layer_ops::can_execute(session, command) { return Some(v); }
    if let Some(v) = layer_kinds::can_execute(session, command) { return Some(v); }
    if let Some(v) = pixel_presets::can_execute(session, command) { return Some(v); }
    if let Some(v) = tone_blend::can_execute(session, command) { return Some(v); }
    if let Some(v) = pixel_brush::can_execute(session, command) { return Some(v); }
    if let Some(v) = layer_meta::can_execute(session, command) { return Some(v); }
    if let Some(v) = isolate::can_execute(session, command) { return Some(v); }
    if let Some(v) = layer_state::can_execute(session, command) { return Some(v); }
    if let Some(v) = picture_frame::can_execute(session, command) { return Some(v); }
    if let Some(v) = arrange::can_execute(session, command) { return Some(v); }
    if let Some(v) = style_pick::can_execute(session, command) { return Some(v); }
    if let Some(v) = export_presets::can_execute(session, command) { return Some(v); }
    if let Some(v) = curves::can_execute(session, command) { return Some(v); }
    if let Some(v) = crate::boolean::can_execute(session, command) { return Some(v); }
    if let Some(v) = vector_crop::can_execute(session, command) { return Some(v); }
    if let Some(v) = curve_edit::can_execute(session, command) { return Some(v); }
    if let Some(v) = curve_nodes::can_execute(session, command) { return Some(v); }
    if let Some(v) = measure::can_execute(session, command) { return Some(v); }
    if let Some(v) = view_actual::can_execute(session, command) { return Some(v); }
    if let Some(v) = view_flags::can_execute(session, command) { return Some(v); }
    if let Some(v) = text_host::can_execute(session, command) { return Some(v); }
    if let Some(v) = macro_host::can_execute(session, command) { return Some(v); }
    if let Some(v) = align::can_execute(session, command) { return Some(v); }
    if let Some(v) = selection_key::can_execute(session, command) { return Some(v); }
    if let Some(v) = toggles::can_execute(session, command) { return Some(v); }
    if let Some(v) = duplicate::can_execute(session, command) { return Some(v); }
    if let Some(v) = fill::can_execute(session, command) { return Some(v); }
    if let Some(v) = stroke::can_execute(session, command) { return Some(v); }
    if let Some(v) = appearance::can_execute(session, command) { return Some(v); }
    if let Some(v) = caption::can_execute(session, command) { return Some(v); }
    if let Some(v) = node_icc::can_execute(session, command) { return Some(v); }
    if let Some(v) = assets::can_execute(session, command) { return Some(v); }
    if let Some(v) = defaults::can_execute(session, command) { return Some(v); }
    if let Some(v) = recent::can_execute(session, command) { return Some(v); }
    if let Some(v) = colour_pick::can_execute(session, command) { return Some(v); }
    if let Some(v) = context_toolbar::can_execute(session, command) { return Some(v); }
    if let Some(v) = lasso::can_execute(session, command) { return Some(v); }
    if let Some(v) = shape::can_execute(session, command) { return Some(v); }
    if let Some(v) = crate::artboard::edit::can_execute(session, command) { return Some(v); }
    if let Some(v) = qr::can_execute(session, command) { return Some(v); }
    if let Some(v) = crate::data_merge::commands::can_execute(session, command) { return Some(v); }
    if let Some(v) = crate::constraints::can_execute(session, command) { return Some(v); }
    if let Some(v) = paint_order::can_execute(session, command) { return Some(v); }
    if let Some(v) = feature_restore::can_execute(session, command) { return Some(v); }
    if let Some(v) = context_menu::can_execute(session, command) { return Some(v); }
    if let Some(v) = tools::can_execute(session, command) { return Some(v); }
    if let Some(v) = crate::colour::proof::can_execute(session, command) { return Some(v); }
    if let Some(v) = ui::can_execute(session, command) { return Some(v); }
    if let Some(v) = pixel_selection::can_execute(session, command) { return Some(v); }
    if let Some(v) = selection_magic_wand::can_execute(session, command) { return Some(v); }
    if let Some(v) = red_eye::can_execute(session, command) { return Some(v); }
    if let Some(v) = history_menu::can_execute(session, command) { return Some(v); }
    if let Some(v) = crate::pixel_perspective::can_execute(session, command) { return Some(v); }
    if matches!(command.name.as_str(), "perspective.preview" | "perspective.apply" | "perspective.cancel") { return Some(session.has_document); }
    if let Some(v) = persona::can_execute(session, command) { return Some(v); }
    if let Some(v) = prefs_cmd::can_execute(session, command) { return Some(v); }
    if let Some(v) = text_wrap_cmd::can_execute(session, command) { return Some(v); }
    if let Some(v) = page_artboards::can_execute(session, command) { return Some(v); }
    if let Some(v) = masters::can_execute(session, command) { return Some(v); }
    if let Some(v) = pages::can_execute(session, command) { return Some(v); }
    if let Some(v) = longdoc::can_execute(session, command) { return Some(v); }
    if let Some(v) = selection::can_execute(session, command) { return Some(v); }
    if let Some(v) = selection_edit::can_execute(session, command) { return Some(v); }
    if let Some(v) = path_nodes::can_execute(session, command) { return Some(v); }
    if let Some(v) = path_points::can_execute(session, command) { return Some(v); }
    if let Some(v) = shortcuts::can_execute(session, command) { return Some(v); }
    if let Some(v) = brush::can_execute(session, command) { return Some(v); }
    if let Some(v) = quickmask::can_execute(session, command) {
        return Some(v);
    }
    if package::can_execute(command) {return Some(session.has_document || command.name=="doc.package.open");}
    if trace_presets::can_execute(&command.name) {return Some(true);}
    if let Some(value) = selection_from_layer::can_execute(session, command) { return Some(value); }
    if let Some(value)=selection_ranges::can_execute(session,command) {return Some(value);}
    if let Some(value)=doc_setup::can_execute(session,command) {return Some(value);}
    if let Some(value)=crate::canvas_size::can_execute(session,command) {return Some(value);}
    if let Some(value)=colour::can_execute(session,command) {return Some(value);}
    if let Some(value)=colour_assign::can_execute(session,command) {return Some(value);}
    if let Some(value)=swatches::can_execute(session,command) {return Some(value);}
    if let Some(value)=hatch::can_execute(session,command) {return Some(value);}
    if let Some(value)=palettes::can_execute(session,command) {return Some(value);}
    if let Some(value)=menus::can_execute(session,command) {return Some(value);}
    if let Some(value)=menu_invoke::can_execute(session,command) {return Some(value);}
    if let Some(value)=table_styles::can_execute(session,command) {return Some(value);}
    None

}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if let Some(value) = crate::text_lines::run(session, command) { return Some(value); }
    if let Some(value) = crate::table::selection::run(session, command) { return Some(value); }
    if let Some(value) = image_place::run(session, command) { return Some(value); }
    if let Some(value) = layer_mask::run(session, command) { return Some(value); }
    if let Some(value) = crate::clip_mask::run(session, command) { return Some(value); }
    if let Some(value) = crate::effect_raster::run(session, command) { return Some(value); }
    if let Some(value) = layer_style::run(session, command) { return Some(value); }
    if let Some(value)=grid::run(session,command) {return Some(value);}
    if let Some(value)=rulers::run(session,command) {return Some(value);}
    if let Some(value)=snap::run(session,command) {return Some(value);}
    if let Some(value)=frame_text::run(session,command) {return Some(value);}
    if let Some(value)=resource_cmd::run(session,command) {return Some(value);}
    if crate::text_cmd::can_execute(command) { return Some(crate::text_cmd::run(command, session)); }
    if let Some(value)=selection_curves::run(session,command) {return Some(value);}
    if let Some(value)=warp::run(session,command) {return Some(value);}
    if let Some(value)=text_case::run(session,command) {return Some(value);}
    if let Some(value)=text_styles_cmd::run(session,command) {return Some(value);}
    if let Some(value) = image_trace::run(session, command) { return Some(value); }
    if let Some(value) = symbols::run(session, command) { return Some(value); }
    if let Some(v) = selection_modify::run(session, command) { return Some(v); }
    if let Some(v) = slice::run(session, command) { return Some(v); }
    if let Some(v) = layer_ops::run(session, command) { return Some(v); }
    if let Some(v) = layer_kinds::run(session, command) { return Some(v); }
    if let Some(v) = pixel_presets::run(session, command) { return Some(v); }
    if let Some(v) = tone_blend::run(session, command) { return Some(v); }
    if let Some(v) = pixel_brush::run(session, command) { return Some(v); }
    if let Some(v) = layer_meta::run(session, command) { return Some(v); }
    if let Some(v) = isolate::run(session, command) { return Some(v); }
    if let Some(v) = layer_state::run(session, command) { return Some(v); }
    if let Some(v) = picture_frame::run(session, command) { return Some(v); }
    if let Some(v) = arrange::run(session, command) { return Some(v); }
    if let Some(v) = style_pick::run(session, command) { return Some(v); }
    if let Some(v) = export_presets::run(session, command) { return Some(v); }
    if let Some(v) = curves::run(session, command) { return Some(v); }
    if let Some(v) = crate::boolean::run(session, command) { return Some(v); }
    if let Some(v) = vector_crop::run(session, command) { return Some(v); }
    if let Some(v) = curve_edit::run(session, command) { return Some(v); }
    if let Some(v) = curve_nodes::run(session, command) { return Some(v); }
    if let Some(v) = measure::run(session, command) { return Some(v); }
    if let Some(v) = view_actual::run(session, command) { return Some(v); }
    if let Some(v) = view_flags::run(session, command) { return Some(v); }
    if let Some(v) = text_host::run(session, command) { return Some(v); }
    if let Some(v) = macro_host::run(session, command) { return Some(v); }
    if let Some(v) = align::run(session, command) { return Some(v); }
    if let Some(v) = selection_key::run(session, command) { return Some(v); }
    if let Some(v) = toggles::run(session, command) { return Some(v); }
    if let Some(v) = duplicate::run(session, command) { return Some(v); }
    if let Some(v) = fill::run(session, command) { return Some(v); }
    if let Some(v) = stroke::run(session, command) { return Some(v); }
    if let Some(v) = appearance::run(session, command) { return Some(v); }
    if let Some(v) = caption::run(session, command) { return Some(v); }
    if let Some(v) = node_icc::run(session, command) { return Some(v); }
    if let Some(v) = assets::run(session, command) { return Some(v); }
    if let Some(v) = defaults::run(session, command) { return Some(v); }
    if let Some(v) = recent::run(session, command) { return Some(v); }
    if let Some(v) = colour_pick::run(session, command) { return Some(v); }
    if let Some(v) = context_toolbar::run(session, command) { return Some(v); }
    if let Some(v) = lasso::run(session, command) { return Some(v); }
    if let Some(v) = shape::run(session, command) { return Some(v); }
    if let Some(v) = crate::artboard::edit::run(session, command) { return Some(v); }
    if let Some(v) = qr::run(session, command) { return Some(v); }
    if let Some(v) = crate::data_merge::commands::run(session, command) { return Some(v); }
    if let Some(v) = crate::constraints::run(session, command) { return Some(v); }
    if let Some(v) = paint_order::run(session, command) { return Some(v); }
    if let Some(v) = feature_restore::run(session, command) { return Some(v); }
    if let Some(v) = context_menu::run(session, command) { return Some(v); }
    if let Some(v) = tools::run(session, command) { return Some(v); }
    if let Some(v) = crate::colour::proof::run(session, command) { return Some(v); }
    if let Some(v) = ui::run(session, command) { return Some(v); }
    if let Some(v) = pixel_selection::run(session, command) { return Some(v); }
    if let Some(v) = selection_magic_wand::run(session, command) { return Some(v); }
    if let Some(v) = red_eye::run(session, command) { return Some(v); }
    if let Some(v) = history_menu::run(session, command) { return Some(v); }
    if let Some(v) = crate::pixel_perspective::run(session, command) { return Some(v); }
    if let Some(v) = crate::perspective::run(session, command) { return Some(v); }
    if let Some(v) = persona::run(session, command) { return Some(v); }
    if let Some(v) = prefs_cmd::run(session, command) { return Some(v); }
    if let Some(v) = text_wrap_cmd::run(session, command) { return Some(v); }
    if let Some(v) = page_artboards::run(session, command) { return Some(v); }
    if let Some(v) = masters::run(session, command) { return Some(v); }
    if let Some(v) = pages::run(session, command) { return Some(v); }
    if let Some(v) = longdoc::run(session, command) { return Some(v); }
    if let Some(v) = selection::run(session, command) { return Some(v); }
    if let Some(v) = selection_edit::run(session, command) { return Some(v); }
    if let Some(v) = path_nodes::run(session, command) { return Some(v); }
    if let Some(v) = path_points::run(session, command) { return Some(v); }
    if let Some(v) = shortcuts::run(session, command) { return Some(v); }
    if let Some(v) = brush::run(session, command) { return Some(v); }
    if let Some(v) = quickmask::run(session, command) {
        return Some(v);
    }
    if package::can_execute(command) {return Some(package::run(command,session).and_then(|v|v));}
    if trace_presets::can_execute(&command.name) {return Some(trace_presets::run(session,&command.name,command.params.clone()).and_then(|v|v));}
    if let Some(value) = selection_from_layer::run(session, command) { return Some(value); }
    if let Some(value)=selection_ranges::run(session,command) {return Some(value);}
    if let Some(value)=doc_setup::run(session,command) {return Some(value);}
    if let Some(value)=crate::canvas_size::run(session,command) {return Some(value);}
    if let Some(value)=colour::run(session,command) {return Some(value);}
    if let Some(value)=colour_assign::run(session,command) {return Some(value);}
    if let Some(value)=swatches::run(session,command) {return Some(value);}
    if let Some(value)=hatch::run(session,command) {return Some(value);}
    if let Some(value)=palettes::run(session,command) {return Some(value);}
    if let Some(value)=menus::run(session,command) {return Some(value);}
    if let Some(value)=menu_invoke::run(session,command) {return Some(value);}
    if let Some(value)=table_styles::run(session,command) {return Some(value);}
    None

}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if let Some(value) = selection_key::apply(session, command) { return Some(value); }
    if let Some(value)=text_wrap_cmd::apply(session,command) {return Some(value);}
    if let Some(value) = layer_style::apply(session, command) { return Some(value); }
    if let Some(value)=text_host::apply(session,command) {return Some(value);}
    if let Some(value)=grid::apply(session,command) {return Some(value);}
    if let Some(value)=rulers::apply(session,command) {return Some(value);}
    if let Some(value)=frame_text::apply(session,command) {return Some(value);}
    if let Some(value)=resource_cmd::apply(session,command) {return Some(value);}
    if crate::text_cmd::can_execute(command) { return Some(crate::text_cmd::apply_session(command, session)); }
    if let Some(value)=selection_curves::apply(session,command) {return Some(value);}
    if let Some(value)=warp::apply(session,command) {return Some(value);}
    if let Some(value)=text_case::apply(session,command) {return Some(value);}
    if let Some(value)=text_styles_cmd::apply(session,command) {return Some(value);}
    if let Some(value) = image_trace::apply(session, command) { return Some(value); }
    if let Some(v) = selection_modify::apply(session, command) { return Some(v); }
    if let Some(v) = slice::apply(session, command) { return Some(v); }
    if let Some(v) = isolate::apply(session, command) { return Some(v); }
    if let Some(v) = layer_state::apply(session, command) { return Some(v); }
    if let Some(v) = picture_frame::apply(session, command) { return Some(v); }
    if let Some(v) = vector_crop::apply(session, command) { return Some(v); }
    if let Some(v) = arrange::apply(session, command) { return Some(v); }
    if let Some(v) = duplicate::apply(session, command) { return Some(v); }
    if let Some(v) = fill::apply(session, command) { return Some(v); }
    if let Some(v) = stroke::apply(session, command) { return Some(v); }
    if let Some(v) = shape::apply(session, command) { return Some(v); }
    if let Some(v) = appearance::apply(session, command) { return Some(v); }
    if let Some(v) = caption::apply(session, command) { return Some(v); }
    if let Some(v) = node_icc::apply(session, command) { return Some(v); }
    if let Some(v) = assets::apply(session, command) { return Some(v); }
    if let Some(v) = defaults::apply(session, command) { return Some(v); }
    if let Some(v) = recent::apply(session, command) { return Some(v); }
    if let Some(v) = context_toolbar::apply(session, command) { return Some(v); }
    if let Some(v) = lasso::apply(session, command) { return Some(v); }
    if let Some(v) = path_points::apply(session, command) { return Some(v); }
    if let Some(v) = paint_order::apply(session, command) { return Some(v); }
    if let Some(v) = feature_restore::apply(session, command) { return Some(v); }
    if let Some(v) = tools::apply(session, command) { return Some(v); }
    if let Some(v) = crate::colour::proof::apply(session, command) { return Some(v); }
    if let Some(v) = ui::apply(session, command) { return Some(v); }
    if let Some(v) = pixel_selection::apply(session, command) { return Some(v); }
    if let Some(v) = selection_magic_wand::apply(session, command) { return Some(v); }
    if let Some(v) = red_eye::apply(session, command) { return Some(v); }
    if let Some(v) = history_menu::apply(session, command) { return Some(v); }
    if let Some(v) = crate::pixel_perspective::apply(session, command) { return Some(v); }
    if let Some(v) = longdoc::apply(session, command) { return Some(v); }
    if let Some(v) = selection::apply(session, command) { return Some(v); }
    if let Some(v) = brush::apply(session, command) { return Some(v); }
    if let Some(v) = quickmask::apply(session, command) {
        return Some(v);
    }
    if package::can_execute(command) { return Some(Ok(())); }
    if trace_presets::can_execute(&command.name) { return Some(Ok(())); }
    if let Some(value)=selection_ranges::apply(session,command) {return Some(value);}
    if let Some(value)=doc_setup::apply(session,command) {return Some(value);}
    if let Some(value)=crate::canvas_size::apply(session,command) {return Some(value.map(|_| ()));}
    if let Some(value)=colour_assign::apply(session,command) {return Some(value);}
    if let Some(value)=swatches::apply(session,command) {return Some(value);}
    if let Some(value)=hatch::apply(session,command) {return Some(value);}
    if let Some(value)=palettes::apply(session,command) {return Some(value);}
    None

}

pub(crate) fn param_id(command: &Command, keys: &[&str]) -> Option<crate::object::ObjectId> {
    for key in keys {
        if let Some(value) = command.params.get(*key) {
            if let Some(id) = value.as_u64() {
                return Some(crate::object::ObjectId(id));
            }
            if let Some(text) = value.as_str() {
                let raw = text.strip_prefix("node:").unwrap_or(text);
                if let Ok(id) = raw.parse::<u64>() {
                    return Some(crate::object::ObjectId(id));
                }
            }
        }
    }
    None
}

pub mod slice;

pub mod selection_modify;
pub mod preview;
pub mod persona;
pub mod prefs_cmd;
pub mod text_wrap_cmd;
pub mod quickmask;
pub mod ai { pub use crate::ai::*; }
pub mod package;
pub mod trace_presets;

pub mod selection_from_layer;
pub mod selection_ranges;

pub mod doc_setup;

pub mod document_colour;
pub mod colour;
pub mod colour_assign;
pub mod swatches;
pub mod hatch;
pub mod palettes;
pub mod menus;
pub mod menu_invoke;
