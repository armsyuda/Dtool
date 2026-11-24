//! C18 recorder follow-ups: doc.setup.* steps are recorded; object ids become selection / new-object refs.
use dtool_core::document::Units;
use dtool_core::macro_recorder::{self, is_recordable, MacroRecorder, ReplayRefs, MACRO_REF_UNRESOLVED};
use dtool_core::object::ObjectId;
use dtool_core::selection::SelectOp;
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn create(name: &str) -> Command {
    Command::with_params("object.create", json!({ "name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0] }))
}

fn last_id(session: &Session) -> u64 {
    session.document.objects.last().unwrap().id.0
}

fn object(session: &Session, id: u64) -> Value {
    serde_json::to_value(session.document.objects.iter().find(|o| o.id.0 == id).unwrap()).unwrap()
}

fn select(session: &mut Session, ids: &[u64]) {
    session.selection.set(ids.iter().map(|id| ObjectId(*id)).collect(), SelectOp::Replace);
}

/// Mirrors the Session::run hook (context before, tap_step after) with a standalone recorder.
fn record(recorder: &mut MacroRecorder, session: &mut Session, command: Command) {
    let context = recorder.step_context(session);
    session.run(&command).unwrap_or_else(|e| panic!("{} failed: {e:?}", command.name));
    let catalog = session.catalog.clone();
    recorder.tap_step(&catalog, &command, context, Some(&session.document));
}

/// Mirrors macros::play_steps: resolve each step against the playing session, then note what it created.
fn play(session: &mut Session, steps: &[macro_recorder::RecordedStep]) -> Result<(), dtool_protocol::CommandError> {
    let mut refs = ReplayRefs::default();
    for step in steps {
        let (params, context) = refs.prepare(session, &step.command, &step.params)?;
        session.run(&Command::with_params(&step.command, params))?;
        refs.finish(session, &context);
    }
    Ok(())
}

fn session_with(names: &[&str]) -> (Session, Vec<u64>) {
    let mut session = Session::new(dtool_core::document::Document::new([500.0, 500.0], Units::Pt, 72.0, 1));
    let ids = names.iter().map(|name| { session.run(&create(name)).unwrap(); last_id(&session) }).collect();
    (session, ids)
}

#[test]
fn doc_setup_changes_are_recordable_but_preview_query_cancel_restore_are_not() {
    let catalog = dtool_protocol::CommandCatalog::new();
    assert!(is_recordable(&catalog, "doc.setup.apply"));
    assert!(is_recordable(&catalog, "doc.setup.transparent_background"));
    for name in ["doc.setup.preview", "doc.setup.query", "doc.setup.cancel", "doc.setup.restore", "doc.save", "doc.open", "doc.query"] {
        assert!(!is_recordable(&catalog, name), "{name}");
    }
}

#[test]
fn recorded_doc_setup_apply_drops_preview_token_and_replays_on_another_doc() {
    let mut ws = Workspace::default();
    let a = ws.new_document("A");
    let b = ws.new_document("B");
    ws.get_mut(a).unwrap().session.macro_recorder.start();
    let token = ws.run(Scope::of(a), Command::with_params("doc.setup.preview", json!({ "units": "millimeters" }))).1.unwrap()["token"].clone();
    assert!(token.is_u64());
    ws.run(Scope::of(a), Command::with_params("doc.setup.apply", json!({ "token": token, "units": "millimeters" }))).1.unwrap();
    ws.run(Scope::of(a), Command::with_params("doc.setup.transparent_background", json!({ "value": true }))).1.unwrap();
    let steps = ws.get_mut(a).unwrap().session.macro_recorder.stop();
    let names: Vec<&str> = steps.iter().map(|s| s.command.as_str()).collect();
    assert_eq!(names, vec!["doc.setup.apply", "doc.setup.transparent_background"]);
    assert!(steps[0].params.get("token").is_none(), "{:?}", steps[0].params);
    assert_eq!(ws.get(a).unwrap().session.document.units, Units::Mm);
    assert_ne!(ws.get(b).unwrap().session.document.units, Units::Mm);
    for step in &steps {
        ws.run(Scope::of(b), Command::with_params(&step.command, step.params.clone())).1.unwrap();
    }
    let doc_b = &ws.get(b).unwrap().session.document;
    assert_eq!(doc_b.units, Units::Mm);
    assert!(doc_b.setup.transparent_background);
}

#[test]
fn relativize_maps_selection_and_created_ids_and_keeps_others_absolute() {
    let sel = [5, 6];
    let created = [9];
    let r = |command: &str, params: Value| macro_recorder::relativize(command, &params, &sel, &created);
    assert_eq!(r("object.delete_many", json!({ "ids": [5, 6] })), json!({ "ids": "$sel" }));
    assert_eq!(r("object.update", json!({ "id": 6, "name": "x" })), json!({ "id": "$sel:1", "name": "x" }));
    assert_eq!(r("object.update", json!({ "id": 9 })), json!({ "id": "$new:0" }));
    assert_eq!(r("object.update", json!({ "id": 42 })), json!({ "id": 42 }));
    assert_eq!(r("object.group", json!({ "ids": [6, 9, 42] })), json!({ "ids": ["$sel:1", "$new:0", 42] }));
    assert_eq!(r("guides.update", json!({ "id": 5 })), json!({ "id": 5 }), "guide ids are not object ids");
    assert_eq!(r("object.update", json!({ "id": 5, "size": [5, 6] })), json!({ "id": "$sel:0", "size": [5, 6] }), "non-id keys untouched");
    assert_eq!(
        r("edit.batch", json!({ "commands": [{ "name": "object.update", "params": { "id": 5 } }] })),
        json!({ "commands": [{ "name": "object.update", "params": { "id": "$sel:0" } }] })
    );
}

#[test]
fn resolve_expands_refs_against_current_selection_and_created_ids() {
    let res = |params: Value, sel: &[u64], created: &[u64]| macro_recorder::resolve("object.update", &params, sel, created);
    assert_eq!(res(json!({ "ids": "$sel" }), &[20, 21], &[]).unwrap(), json!({ "ids": [20, 21] }));
    assert_eq!(res(json!({ "ids": ["$sel:1", "$new:0", 42] }), &[20, 21], &[30]).unwrap(), json!({ "ids": [21, 30, 42] }));
    assert_eq!(res(json!({ "id": "$sel" }), &[20], &[]).unwrap(), json!({ "id": 20 }));
    assert_eq!(res(json!({ "id": 7, "name": "$sel" }), &[20], &[]).unwrap(), json!({ "id": 7, "name": "$sel" }), "old absolute + text untouched");
    for bad in [json!({ "id": "$new:3" }), json!({ "id": "$sel:2" }), json!({ "id": "$sel" }), json!({ "ids": "$sel" })] {
        let sel: &[u64] = if bad == json!({ "id": "$sel" }) { &[20, 21] } else if bad == json!({ "ids": "$sel" }) { &[] } else { &[20, 21] };
        assert_eq!(res(bad.clone(), sel, &[30]).unwrap_err().code, MACRO_REF_UNRESOLVED, "{bad}");
    }
    let original = json!({ "ids": [5, 9, 42], "id": 6 });
    let relative = macro_recorder::relativize("object.update", &original, &[5, 6], &[9]);
    assert_eq!(macro_recorder::resolve("object.update", &relative, &[5, 6], &[9]).unwrap(), original);
}

#[test]
fn selection_macro_recorded_on_a_acts_on_bs_selection() {
    let (mut a, ids_a) = session_with(&["a0", "a1"]);
    select(&mut a, &[ids_a[1]]);
    let mut recorder = MacroRecorder::default();
    recorder.start();
    record(&mut recorder, &mut a, Command::with_params("object.update", json!({ "id": ids_a[1], "pos": [40.0, 50.0] })));
    record(&mut recorder, &mut a, Command::with_params("object.update", json!({ "id": ids_a[1], "fill": "#FF0000" })));
    let steps = recorder.stop();
    assert!(steps.iter().all(|s| s.params["id"] == "$sel:0"), "{steps:?}");

    let (mut b, ids_b) = session_with(&["b0", "b1", "b2"]);
    select(&mut b, &[ids_b[2]]);
    let untouched = object(&b, ids_b[1]);
    play(&mut b, &steps).unwrap();
    let moved = object(&b, ids_b[2]);
    assert_eq!(moved["pos"], json!([40.0, 50.0]));
    assert_eq!(moved["fill"], object(&a, ids_a[1])["fill"]);
    assert_eq!(object(&b, ids_b[1]), untouched, "same absolute id as A's object stays put");
    select(&mut b, &[]);
    assert_eq!(play(&mut b, &steps).unwrap_err().code, MACRO_REF_UNRESOLVED);
}

#[test]
fn create_then_modify_macro_targets_objects_created_during_play() {
    let (mut a, _) = session_with(&["seed"]);
    let mut recorder = MacroRecorder::default();
    recorder.start();
    record(&mut recorder, &mut a, create("made"));
    let made = last_id(&a);
    record(&mut recorder, &mut a, Command::with_params("object.update", json!({ "id": made, "name": "renamed", "pos": [7.0, 8.0] })));
    let steps = recorder.stop();
    assert_eq!(steps[1].params["id"], "$new:0", "{steps:?}");

    let (mut b, ids_b) = session_with(&["x", "y", "z", "w"]);
    let before: Vec<Value> = ids_b.iter().map(|id| object(&b, *id)).collect();
    play(&mut b, &steps).unwrap();
    let new_obj = b.document.objects.last().unwrap();
    assert!(!ids_b.contains(&new_obj.id.0));
    assert_eq!(new_obj.name, "renamed");
    assert_eq!(new_obj.pos, [7.0, 8.0]);
    let after: Vec<Value> = ids_b.iter().map(|id| object(&b, *id)).collect();
    assert_eq!(after, before);
}

#[test]
fn old_absolute_steps_and_ids_outside_selection_still_play() {
    let (mut b, ids_b) = session_with(&["p", "q"]);
    select(&mut b, &[ids_b[0]]);
    let old = vec![macro_recorder::RecordedStep { command: "object.update".into(), params: json!({ "id": ids_b[1], "name": "abs" }) }];
    play(&mut b, &old).unwrap();
    assert_eq!(object(&b, ids_b[1])["name"], "abs");
    assert_eq!(object(&b, ids_b[0])["name"], "p");

    let (mut a, ids_a) = session_with(&["s", "t"]);
    select(&mut a, &[ids_a[0]]);
    let mut recorder = MacroRecorder::default();
    recorder.start();
    record(&mut recorder, &mut a, Command::with_params("object.update", json!({ "id": ids_a[1], "name": "doc-state" })));
    assert_eq!(recorder.steps()[0].params["id"], json!(ids_a[1]), "not selected, not created -> absolute");
}

#[test]
fn nested_runs_and_stopped_recorder_have_no_context() {
    let (session, _) = session_with(&["n"]);
    let mut recorder = MacroRecorder::default();
    assert!(recorder.step_context(&session).is_none());
    recorder.start();
    assert!(recorder.step_context(&session).is_some());
    recorder.reset();
    assert!(recorder.step_context(&session).is_none());
}
