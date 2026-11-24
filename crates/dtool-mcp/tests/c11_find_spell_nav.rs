use dtool_core::workspace::Workspace;
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(server: &mut Server, name: &str, params: Value) -> Value {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "params": params } }));
    assert!(reply.get("error").is_none(), "{name}: {reply}");
    reply["result"].clone()
}

fn run_err(server: &mut Server, name: &str, params: Value) -> Value {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "params": params } }));
    reply.get("error").cloned().unwrap_or(reply)
}

fn undo(server: &mut Server) {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 2, "method": "dtool.undo", "params": {} }));
    assert!(reply.get("error").is_none(), "{reply}");
}

fn live(text: &str) -> (Server, u64) {
    let mut workspace = Workspace::default();
    let doc = workspace.new_document("c11-find-spell");
    workspace.activate(doc).unwrap();
    let mut server = Server::new(Config::default(), workspace);
    let added = run(&mut server, "text.art.add", json!({ "pos": [20.0, 20.0], "text": text }));
    let story = added["story"].as_u64().expect("story");
    (server, story)
}

fn story_text(server: &mut Server, story: u64) -> String {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 3, "method": "dtool.run", "params": { "name": "document.state", "params": {} } }));
    if let Some(stories) = reply["result"]["document"]["stories"].as_array().or_else(|| reply["result"]["stories"].as_array()) {
        if let Some(found) = stories.iter().find(|item| item["id"] == json!(story) || item["id"] == json!(story.to_string())) {
            if let Some(text) = found["text"].as_str() {
                return text.to_string();
            }
        }
    }
    String::new()
}

#[test]
fn literal_and_regex_find_respect_scope() {
    let (mut server, story) = live("cat and catalog\nThe CAT sat");
    let first = run(&mut server, "text.find", json!({ "pattern": "cat", "case_sensitive": false, "scope": "document" }));
    assert_eq!(first["match"]["story"], json!(story));
    assert_eq!(first["match"]["start"], json!(0));
    assert_eq!(first["match"]["end"], json!(3));
    let second = run(&mut server, "text.find", json!({ "pattern": "cat", "case_sensitive": false, "scope": "document", "from": { "story": story, "at": 3 } }));
    assert_eq!(second["match"]["start"], json!(8));
    let story_only = run(&mut server, "text.find", json!({ "pattern": "CAT", "case_sensitive": true, "scope": "story", "story": story }));
    assert_eq!(story_only["match"]["start"], json!(20));
    let regex = run(&mut server, "text.find", json!({ "pattern": "c[ao]t", "regex": true, "case_sensitive": false, "scope": "document" }));
    assert_eq!(regex["match"]["start"], json!(0));
    let missing = run(&mut server, "text.find", json!({ "pattern": "zzz", "scope": "document" }));
    assert!(missing["match"].is_null(), "{missing}");
}

#[test]
fn style_formatting_find_replace() {
    let (mut server, story) = live("cat cat");
    run(&mut server, "text.style", json!({"story": story, "range": {"start": 4, "end": 7}, "patch": {"weight": 700}}));
    let filter = json!({"pattern": "cat", "format": {"weight": 700}, "scope": "story", "story": story});
    let found = run(&mut server, "text.find", filter.clone());
    assert_eq!((found["match"]["start"].clone(), found["match"]["end"].clone()), (json!(4), json!(7)));
    let replaced = run(&mut server, "text.replace_all", json!({"pattern": "cat", "format": {"weight": 700}, "replacement": "dog", "scope": "story", "story": story}));
    assert_eq!(replaced["replaced"], 1);
    assert!(run(&mut server, "text.find", filter.clone())["match"].is_null());
    assert_eq!(run(&mut server, "text.find", json!({"pattern": "dog", "format": {"weight": 700}, "scope": "story", "story": story}))["match"]["start"], 4);
    undo(&mut server);
    assert_eq!(run(&mut server, "text.find", filter)["match"]["start"], 4);
}

#[test]
fn dictionaries_and_live_spell_mark_teh() {
    let (mut server, story) = live("I saw teh cat");
    let check = run(&mut server, "text.spell.check", json!({ "story": story }));
    assert_eq!(check["available"], json!(true), "{check}");
    let marks = check["marks"].as_array().expect("marks");
    assert!(marks.iter().any(|mark| mark["word"] == json!("teh")), "{check}");
    let suggest = run(&mut server, "text.spell.suggest", json!({ "word": "teh" }));
    assert_eq!(suggest["correct"], json!(false));
    assert!(suggest["suggestions"].as_array().unwrap().iter().any(|item| item == "the"), "{suggest}");
}

#[test]
fn next_and_replace_all_cat_to_dog_is_one_undo() {
    let (mut server, story) = live("cat one cat two cat three cat");
    let first = run(&mut server, "text.find", json!({ "pattern": "cat", "scope": "document" }));
    assert_eq!(first["match"]["start"], json!(0));
    let second = run(&mut server, "text.find", json!({ "pattern": "cat", "scope": "document", "from": { "story": story, "at": first["match"]["end"] } }));
    assert_eq!(second["match"]["start"], json!(8));
    let replaced = run(&mut server, "text.replace_all", json!({ "pattern": "cat", "replacement": "dog", "scope": "document" }));
    assert_eq!(replaced["replaced"], json!(4), "{replaced}");
    let check = run(&mut server, "text.find", json!({ "pattern": "cat", "scope": "document" }));
    assert!(check["match"].is_null(), "{check}");
    let dogs = run(&mut server, "text.find", json!({ "pattern": "dog", "scope": "document" }));
    assert_eq!(dogs["match"]["start"], json!(0));
    undo(&mut server);
    let restored = run(&mut server, "text.find", json!({ "pattern": "cat", "scope": "document" }));
    assert_eq!(restored["match"]["start"], json!(0), "{restored}");
    let _ = story_text(&mut server, story);
    let _ = run_err(&mut server, "text.replace_all", json!({ "pattern": "(", "regex": true, "replacement": "x", "scope": "document" }));
}

#[test]
fn find_and_spell_panel_commands_are_the_live_handlers() {
    let (mut server, story) = live("teh cat");
    let found = run(&mut server, "text.find", json!({ "pattern": "teh", "scope": "document" }));
    run(&mut server, "text.spell.replace", json!({ "story": story, "start": found["match"]["start"], "end": found["match"]["end"], "word": "teh", "with": "the" }));
    let after = run(&mut server, "text.spell.check", json!({ "story": story }));
    assert!(after["marks"].as_array().unwrap().iter().all(|mark| mark["word"] != json!("teh")), "{after}");
    undo(&mut server);
    let back = run(&mut server, "text.spell.check", json!({ "story": story }));
    assert!(back["marks"].as_array().unwrap().iter().any(|mark| mark["word"] == json!("teh")), "{back}");
}
