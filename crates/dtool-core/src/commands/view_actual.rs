use crate::command::{Session, ViewZoomSetArgs, parse_view_zoom_set};
use dtool_protocol::{Command, CommandError};

pub const VIEW_ZOOM_ACTUAL: &str = "view.zoom.actual";

pub fn parse(params: &serde_json::Value) -> Result<ViewZoomSetArgs, CommandError> {
    let mut params = match params {
        serde_json::Value::Null => serde_json::Map::new(),
        serde_json::Value::Object(params) => params.clone(),
        _ => return Err(CommandError::invalid_params()),
    };
    params.insert("zoom".to_string(), serde_json::json!(1.0));
    let args = parse_view_zoom_set(&serde_json::Value::Object(params))?;
    if args
        .around
        .is_some_and(|point| point.iter().any(|value| !value.is_finite()))
        || args
            .view_size
            .is_some_and(|size| size.iter().any(|value| !value.is_finite() || *value <= 0.0))
    {
        return Err(CommandError::invalid_params());
    }
    Ok(args)
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    (command.name == VIEW_ZOOM_ACTUAL)
        .then(|| session.has_document && parse(&command.params).is_ok())
}

pub fn run(
    session: &mut Session,
    command: &Command,
) -> Option<Result<serde_json::Value, CommandError>> {
    if command.name != VIEW_ZOOM_ACTUAL {
        return None;
    }
    Some((|| {
        if !session.has_document {
            return Err(CommandError::no_document());
        }
        let mut args = parse(&command.params)?;
        args.zoom = session.actual_zoom();
        let mut result = session.apply_view_zoom(args);
        result["changes"] = serde_json::json!([{ "name": VIEW_ZOOM_ACTUAL }]);
        Ok(result)
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, Units};

    fn session() -> Session {
        Session::new(Document::new([800.0, 600.0], Units::Px, 96.0, 1))
    }

    #[test]
    fn actual_keeps_cursor_rotation_and_document_stable() {
        for zoom in [0.01, 0.5, 2.0, 100.0] {
            let mut session = session();
            session.view.zoom = zoom;
            session.view.scroll = [23.0, -17.0];
            session.view.rotation_deg = 37.0;
            let point = [317.0, 241.0];
            let document = serde_json::to_value(&session.document).unwrap();
            let before = crate::view::view_to_doc(&session.view, point);
            let result = session
                .run_named(VIEW_ZOOM_ACTUAL, serde_json::json!({ "around": point }))
                .unwrap();
            let after = crate::view::view_to_doc(&session.view, point);
            assert_eq!(session.view.zoom, 1.0);
            assert_eq!(session.view.rotation_deg, 37.0);
            assert_eq!(serde_json::to_value(&session.document).unwrap(), document);
            assert_eq!(session.generation, 0);
            assert!(!session.undo.can_undo());
            assert_eq!(result["changes"][0]["name"], VIEW_ZOOM_ACTUAL);
            for axis in 0..2 {
                assert!(
                    (before[axis] - after[axis]).abs() <= before[axis].abs().max(1.0) * 0.00001
                );
            }
        }
    }

    #[test]
    fn actual_uses_view_centre_when_cursor_is_absent() {
        let mut session = session();
        session.view.zoom = 2.0;
        session.view.scroll = [20.0, 30.0];
        let point = [400.0, 300.0];
        let before = crate::view::view_to_doc(&session.view, point);
        session
            .run_named(
                VIEW_ZOOM_ACTUAL,
                serde_json::json!({ "view_size": [800.0, 600.0] }),
            )
            .unwrap();
        assert_eq!(crate::view::view_to_doc(&session.view, point), before);
        assert_eq!(session.view.zoom, 1.0);
    }

    #[test]
    fn actual_accepts_empty_params_and_rejects_invalid_pivots() {
        let mut session = session();
        let command = Command::new(VIEW_ZOOM_ACTUAL);
        assert!(session.can_execute(&command));
        session.run(&command).unwrap();
        for params in [
            serde_json::json!([]),
            serde_json::json!({ "around": [1.0] }),
            serde_json::json!({ "view_size": [0.0, 600.0] }),
        ] {
            let command = Command::with_params(VIEW_ZOOM_ACTUAL, params);
            assert!(!session.can_execute(&command));
            assert_eq!(session.run(&command).unwrap_err().code, "INVALID_PARAMS");
        }
    }

    #[test]
    fn actual_requires_a_document_and_leaves_other_handlers_available() {
        let mut session = Session::default();
        let command = Command::new(VIEW_ZOOM_ACTUAL);
        assert!(!session.can_execute(&command));
        assert_eq!(session.run(&command).unwrap_err().code, "NO_DOCUMENT");
        let other = Command::new("edit.duplicate_drag");
        assert!(run(&mut session, &other).is_none());
        assert!(can_execute(&session, &other).is_none());
    }
}
