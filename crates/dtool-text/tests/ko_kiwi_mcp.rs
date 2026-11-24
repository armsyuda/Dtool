use dtool_core::serde_json::{json, Value};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_core::{ObjectId, StoryId};
use dtool_mcp::{Config, Server};
use dtool_text::spelling::ko_grammar;

fn kiwi_or_skip() -> bool {
    if ko_grammar::kiwi_dir().is_some() {
        return true;
    }
    assert!(std::env::var_os("DTOOL_KIWI_REQUIRED").is_none(), "Kiwi bundle missing in {:?}", ko_grammar::candidate_dirs());
    eprintln!("KIWI_SKIP bundle not found in {:?}", ko_grammar::candidate_dirs());
    false
}

fn run(server: &mut Server, name: &str, params: Value) -> Value {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "params": params } }));
    assert!(reply.get("error").is_none(), "{name}: {reply}");
    reply["result"].clone()
}

fn story_text(server: &Server, doc: ContextId, story: StoryId) -> String {
    server.workspace().lock().unwrap().get(doc).unwrap().session.document.stories.iter().find(|s| s.id == story).unwrap().text.clone()
}

#[test]
fn mcp_spell_check_reports_kiwi_issues_on_the_live_document_and_replace_undoes() {
    if !kiwi_or_skip() {
        return;
    }
    assert!(dtool_text::text_host::install());
    let mut workspace = Workspace::default();
    let doc = workspace.new_document("ko-kiwi");
    workspace.activate(doc).unwrap();
    let mut server = Server::new(Config::default(), workspace);
    let frame = run(&mut server, "text.frame.add", json!({"rect": [20, 20, 400, 300], "text": "아버지가방에들어가신다\n정말 잘했따"}))["id"].as_u64().unwrap();
    let story = {
        let shared = server.workspace();
        let lock = shared.lock().unwrap();
        dtool_core::frame::frame_story(&lock.get(doc).unwrap().session.document, ObjectId(frame)).unwrap()
    };
    let out = run(&mut server, "text.spell.check", json!({"story": story.0}));
    let issues = out["issues"].as_array().unwrap().clone();
    assert_eq!(issues.len(), 2, "{out}");
    assert_eq!((issues[0]["kind"].as_str(), issues[0]["start"].as_u64(), issues[0]["end"].as_u64()), (Some("spacing"), Some(0), Some(11)), "{out}");
    assert_eq!(issues[0]["suggestion"], json!("아버지가 방에 들어가신다"));
    assert_eq!((issues[1]["kind"].as_str(), issues[1]["start"].as_u64(), issues[1]["end"].as_u64()), (Some("grammar"), Some(15), Some(18)), "{out}");
    assert_eq!(issues[1]["suggestion"], json!("잘했다"));
    for issue in issues.iter().rev() {
        run(&mut server, "text.spell.replace", json!({"story": story.0, "start": issue["start"], "end": issue["end"], "word": issue["word"], "with": issue["suggestion"]}));
    }
    assert_eq!(story_text(&server, doc, story), "아버지가 방에 들어가신다\n정말 잘했다");
    let again = run(&mut server, "text.spell.check", json!({"story": story.0}));
    assert!(again["issues"].as_array().unwrap().is_empty(), "{again}");
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 2, "method": "dtool.undo", "params": {} }));
    assert!(reply.get("error").is_none(), "{reply}");
    assert_eq!(story_text(&server, doc, story), "아버지가방에들어가신다\n정말 잘했다");
}
