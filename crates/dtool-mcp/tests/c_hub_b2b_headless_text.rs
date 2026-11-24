#[path = "../../dtool-cli/src/harness.rs"]
#[allow(dead_code)]
mod profile_fixture;

use dtool_core::workspace::SharedWorkspace;
use dtool_core::{Story, StoryId, Workspace};
use dtool_mcp::{serve_stdio, Config, Server};
use profile_fixture::TestProfile;
use serde_json::{json, Value};
use std::sync::Arc;

fn fixture(text: &str) -> (Server, SharedWorkspace, TestProfile) {
    let profile = TestProfile::new();
    let mut workspace = Workspace::default();
    let id = workspace.new_document("b2b-headless");
    let session = &mut workspace.get_mut(id).unwrap().session;
    let mut story = Story { id: StoryId(7), text: text.into(), ..Default::default() };
    story.ranges.push(Default::default());
    story.ranges[0]["start"] = 0u64.into();
    story.ranges[0]["end"] = (text.chars().count() as u64).into();
    story.ranges[0]["size_pt"] = 12.0f64.into();
    session.document.stories.push(story);
    let shared = workspace.into_shared();
    let server = Server::with_workspace(Config { allow_dirs: vec![profile.path().to_path_buf()], ..Config::default() }, Arc::clone(&shared));
    server.gui_connection_handle().set_from_gui(true);
    (server, shared, profile)
}

fn stdio(server: &mut Server, name: &str, params: Value) -> Value {
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "params": params } });
    let mut bytes = serde_json::to_vec(&request).unwrap();
    bytes.push(b'\n');
    let mut output = Vec::new();
    serve_stdio(server, std::io::Cursor::new(bytes), &mut output);
    serde_json::from_slice(&output).unwrap()
}

#[test]
fn headless_mcp_font_list_returns_real_families_and_recent() {
    let (mut server, _shared, _profile) = fixture("Hello");
    let wire = stdio(&mut server, "font.list", json!({}));
    assert!(wire.get("error").is_none(), "{wire}");
    let families = wire["result"]["families"].as_array().expect("families");
    assert!(!families.is_empty(), "system fonts expected on the owner PC");
    assert!(wire["result"]["recent"].is_array());
}

#[test]
fn headless_mcp_text_find_matches_live_story() {
    let (mut server, shared, _profile) = fixture("one two one");
    let before = shared.lock().unwrap().session().unwrap().generation;
    let wire = stdio(&mut server, "text.find", json!({ "pattern": "one", "from": { "story": 7, "at": 1 } }));
    assert_eq!(wire["result"]["match"], json!({ "story": 7, "start": 8, "end": 11 }), "{wire}");
    assert_eq!(shared.lock().unwrap().session().unwrap().generation, before);
}