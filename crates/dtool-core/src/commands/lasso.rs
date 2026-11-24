use crate::selection::magnetic::{MagneticImage, MAX_ANCHORS};
use crate::selection::{combine_page_mask, polygon_mask, LassoMode, PixelSurface, SelectionOp};
use crate::{Command, CommandError, CommandResult, Session};
use serde::Deserialize;

const LASSO: &str = "selection.lasso";

#[derive(Deserialize)]
struct Args {
    points: Vec<[f32; 2]>,
    #[serde(default)]
    op: SelectionOp,
    #[serde(default)]
    mode: LassoMode,
    #[serde(default)]
    antialias: bool,
    #[serde(default)]
    feather: f32,
}

fn apply_lasso(session: &mut Session, command: &Command) -> Result<LassoMode, CommandError> {
    let args: Args = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
    if args.points.len() < 3 || args.points.len() > 1_000_000
        || args.points.iter().flatten().any(|value| !value.is_finite())
        || !(0.0..=1000.0).contains(&args.feather)
    {
        return Err(CommandError::invalid_params());
    }
    let surface = PixelSurface::for_command(&session.document, &command.params)?;
    let [w, h] = surface.size;
    let points = if args.mode == LassoMode::Magnetic {
        if args.points.len() > MAX_ANCHORS { return Err(CommandError::invalid_params()); }
        if u64::from(w) * u64::from(h) > 32 * 1024 * 1024 {
            return Err(CommandError::new("REQUEST_TOO_LARGE"));
        }
        MagneticImage::capture_surface(&session.document, surface).map(|image| image.outline(&args.points)).unwrap_or(args.points)
    } else {
        args.points
    };
    let mut mask = polygon_mask(w, h, &points, args.antialias);
    if mask.width != w || mask.height != h { return Err(CommandError::new("REQUEST_TOO_LARGE")); }
    if args.feather > 0.0 { mask.feather(args.feather); }
    mask.page_id = command.params["page_id"].as_u64().map(crate::document::PageId);
    combine_page_mask(&mut session.document, mask, args.op)?;
    Ok(args.mode)
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    (command.name == LASSO).then_some(session.has_document)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    can_execute(session, command)?;
    let before = session.document.pixel_selection.clone();
    Some(apply_lasso(session, command).map(|mode| {
        if mode == LassoMode::Magnetic {
            super::pixel_selection::record_mask(session, command, before)
        } else {
            super::pixel_selection::record(session, command, before)
        }
    }))
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    (command.name == LASSO).then(|| apply_lasso(session, command).map(|_| ()))
}
