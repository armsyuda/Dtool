use dtool_core::{CanvasFlags, DocContext, ViewMode, ViewState};

pub fn get_view_state(ctx: &DocContext) -> ViewState {
    ctx.view_state.clone()
}

pub fn set_view_state(ctx: &mut DocContext, state: ViewState) {
    ctx.view_state = state;
}

pub fn query_flags(ctx: &DocContext) -> CanvasFlags {
    let mut flags = ctx.view_state.canvas_flags.clone();
    if !flags.preview_mode {
        flags.view_mode = ViewMode::Normal;
    }
    flags
}

pub fn set_flags(
    ctx: &mut DocContext,
    force_pixel_alignment: Option<bool>,
    clip_to_canvas: Option<bool>,
    preview_mode: Option<bool>,
    view_mode: Option<ViewMode>,
) {
    let flags = &mut ctx.view_state.canvas_flags;
    if let Some(v) = force_pixel_alignment { flags.force_pixel_alignment = v; }
    if let Some(v) = clip_to_canvas { flags.clip_to_canvas = v; }
    if let Some(v) = preview_mode { flags.preview_mode = v; }
    if let Some(v) = view_mode { flags.view_mode = v; }
}
