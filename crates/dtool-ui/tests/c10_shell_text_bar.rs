#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::context::text_bar::TextBarEdit;
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

fn frame(shell: &mut AppShell, ctx: &egui::Context) {
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn undo_steps(shell: &mut AppShell) -> usize {
    let session = shell.workspace.session_mut().unwrap();
    dtool_core::history_store::HistoryStore::from_undo(&session.undo).list().len()
}

fn style(shell: &mut AppShell, story: u64) -> Value {
    run(shell, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 5 } }))
}

#[test]
fn c10_text_bar_edits_the_live_story() {
    let _home = test_home::enter("c10_shell_text_bar");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c10 text bar");
    shell.workspace.activate(doc).unwrap();
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "hello" }));
    let object = added["id"].as_u64().unwrap();
    let story = added["story"].as_u64().unwrap();

    run(&mut shell, "selection.set", json!({ "ids": [], "op": "Replace" }));
    frame(&mut shell, &ctx);
    assert!(shell.text_bar.target.is_none(), "no text target, no bar");
    assert!(!shell.text_bar_edit(&TextBarEdit::Bold(true)).unwrap());

    run(&mut shell, "selection.set", json!({ "ids": [object], "op": "Replace" }));
    frame(&mut shell, &ctx);
    let target = shell.text_bar.target.expect("selected text object is the target");
    assert_eq!((target.story, target.start, target.end), (story, 0, 5));

    let before = undo_steps(&mut shell);
    assert!(shell.text_bar_edit(&TextBarEdit::Bold(true)).unwrap());
    assert_eq!(undo_steps(&mut shell), before + 1, "one text.style = one undo step");
    assert_eq!(style(&mut shell, story)["weight"], 700);
    assert_eq!(shell.text_bar.bold(), Some(true));

    assert!(shell.text_bar_edit(&TextBarEdit::Family("SourceSans".into())).unwrap());
    assert_eq!(style(&mut shell, story)["family"], "SourceSans");
    assert_eq!(shell.text_bar.family.as_deref(), Some("SourceSans"));
    assert!(shell.text_bar_edit(&TextBarEdit::Size(30.0)).unwrap());
    assert_eq!(style(&mut shell, story)["size_pt"].as_f64().map(|v| v.round()), Some(30.0));
    assert_eq!(undo_steps(&mut shell), before + 3);

    run(&mut shell, "edit.undo", json!({}));
    frame(&mut shell, &ctx);
    assert_ne!(shell.text_bar.size_pt, Some(30.0), "bar reloads after undo");
    assert_eq!(shell.text_bar.family.as_deref(), Some("SourceSans"));
}
