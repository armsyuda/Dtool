use std::sync::Arc;
use dtool_core::serde_json::{json, Value};
use dtool_core::{Command, CommandResult, ContextId, Scope, SharedServices, Workspace};
use dtool_text::para_style::{validate_tabs, Leader, ListKind, ParaStylePatch, TabAlign, TabStop};
use dtool_text::{layout_story, story_from_core, Align, FontLibrary, Line, Story};

fn tab(pos: f32) -> TabStop {
    TabStop { pos, align: TabAlign::Left, decimal: '.', leader: Leader::None, from_right: false }
}

fn try_run(ws: &mut Workspace, id: ContextId, name: &str, params: Value) -> CommandResult {
    ws.run(Scope::of(id), Command::with_params(name, params)).1
}

fn run(ws: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
    try_run(ws, id, name, params).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn live(text: &str) -> (Workspace, ContextId, u64) {
    let mut ws = Workspace::new(Arc::new(SharedServices::default()));
    let id = ws.new_document("para");
    run(&mut ws, id, "text.art.add", json!({"pos": [10.0, 10.0], "text": text}));
    let story = ws.get(id).unwrap().session.document.stories.last().unwrap().id.0;
    (ws, id, story)
}

fn text_story(ws: &Workspace, id: ContextId) -> Story {
    story_from_core(ws.get(id).unwrap().session.document.stories.last().unwrap())
}

fn para_set(ws: &mut Workspace, id: ContextId, story: u64, range: [usize; 2], patch: Value) {
    run(ws, id, "text.para.set", json!({"story": story, "range": range, "patch": patch}));
}

fn para_lines(lines: &[Line]) -> Vec<&Line> {
    let mut starts: Vec<usize> = Vec::new();
    let mut out = Vec::new();
    for line in lines {
        if !starts.contains(&line.range.start) {
            starts.push(line.range.start);
            out.push(line);
        }
    }
    out
}

fn glyph_x(line: &Line, index: usize) -> f32 {
    line.glyphs[..index].iter().map(|g| g.advance).sum::<f32>() + line.glyphs[index].offset[0]
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

#[test]
fn b38_t002_align_and_indent_reach_live_layout() {
    let lib = FontLibrary::system();
    let (mut ws, id, story) = live("aa\nbb\ncc\ndd");
    para_set(&mut ws, id, story, [0, 1], json!({"indent_left": 12.0, "indent_first": 5.0}));
    para_set(&mut ws, id, story, [3, 4], json!({"align": "Center"}));
    para_set(&mut ws, id, story, [6, 7], json!({"align": "Right", "indent_right": 10.0}));
    let bound = text_story(&ws, id);
    assert_eq!(bound.paras[1].style.align, Align::Center);
    let lines = layout_story(&lib, &bound, Some(200.0));
    let firsts = para_lines(&lines);
    assert_eq!(firsts.len(), 4);
    assert!(near(firsts[0].glyphs[0].offset[0], 17.0), "indent_left+first {}", firsts[0].glyphs[0].offset[0]);
    let center = firsts[1];
    assert!(near(center.glyphs[0].offset[0], (200.0 - center.width) * 0.5), "center {} w {}", center.glyphs[0].offset[0], center.width);
    let right = firsts[2];
    assert!(near(right.glyphs[0].offset[0], 190.0 - right.width), "right {} w {}", right.glyphs[0].offset[0], right.width);
    assert!(near(firsts[3].glyphs[0].offset[0], 0.0));
    run(&mut ws, id, "edit.undo", json!({}));
    assert_ne!(text_story(&ws, id).paras.iter().filter(|p| p.style.align == Align::Right).count(), 1);
}

#[test]
fn b38_t003_t035_tab_rules_match_core_validator() {
    let (mut ws, id, story) = live("a\tb");
    let cases = [
        (json!([{"pos": 10.0}, {"pos": 20.0}]), true),
        (json!([{"pos": 10.0}, {"pos": 10.0}]), true),
        (json!([{"pos": 30.0}, {"pos": 10.0}]), false),
        (json!([{"pos": -1.0}]), false),
        (json!([{"pos": 5.0, "leader": {"Character": "\t"}}]), false),
        (json!([{"pos": 5.0, "leader": {"Character": "."}}]), true),
    ];
    for (tabs, ok) in cases {
        let core = try_run(&mut ws, id, "text.para.set", json!({"story": story, "range": [0, 1], "patch": {"tabs": tabs.clone()}})).is_ok();
        let patch: ParaStylePatch = dtool_core::serde_json::from_value(json!({"tabs": tabs.clone()})).unwrap();
        assert_eq!(core, ok, "core {tabs}");
        assert_eq!(patch.validate().is_ok(), ok, "patch {tabs}");
    }
    assert!(validate_tabs(&[tab(30.0), tab(10.0)]).is_err());
    assert!(validate_tabs(&[tab(f32::NAN)]).is_err());
}

#[test]
fn b38_t035_tab_stops_position_text_live() {
    let lib = FontLibrary::system();
    let (mut ws, id, story) = live("a\tb\nc\td\ne\tf");
    para_set(&mut ws, id, story, [0, 1], json!({"tabs": [{"pos": 50.0}]}));
    para_set(&mut ws, id, story, [4, 5], json!({"tabs": [{"pos": 120.0, "align": "Right"}]}));
    para_set(&mut ws, id, story, [8, 9], json!({"default_tab_interval": 30.0}));
    let bound = text_story(&ws, id);
    assert_eq!(bound.paras[0].style.tabs.len(), 1);
    assert_eq!(bound.paras[1].style.tabs[0].align, TabAlign::Right);
    let lines = layout_story(&lib, &bound, Some(300.0));
    let firsts = para_lines(&lines);
    assert!(near(glyph_x(firsts[0], 2), 50.0), "left tab {}", glyph_x(firsts[0], 2));
    let r = firsts[1];
    let end = glyph_x(r, 2) + r.glyphs[2].advance;
    assert!(near(end, 120.0), "right tab end {end}");
    assert!(near(glyph_x(firsts[2], 2), 30.0), "default interval {}", glyph_x(firsts[2], 2));
}

#[test]
fn b38_t007_list_numbering_bullets_and_restart_reach_layout() {
    let lib = FontLibrary::system();
    let (mut ws, id, story) = live("aa\nbb\ncc\ndd");
    let plain: Vec<usize> = para_lines(&layout_story(&lib, &text_story(&ws, id), None)).iter().map(|l| l.glyphs.len()).collect();
    assert_eq!(plain.len(), 4);
    para_set(&mut ws, id, story, [0, 1], json!({"list": {"kind": {"Number": "Arabic"}, "level": 1, "start_at": 9}}));
    para_set(&mut ws, id, story, [3, 4], json!({"list": {"kind": {"Number": "Arabic"}, "level": 1}}));
    para_set(&mut ws, id, story, [6, 7], json!({"list": {"kind": {"Number": "Arabic"}, "level": 1, "restart": true}}));
    para_set(&mut ws, id, story, [9, 10], json!({"list": {"kind": {"Bullet": "*"}, "level": 1}}));
    let bound = text_story(&ws, id);
    assert_eq!(bound.paras.len(), 4);
    assert_eq!(bound.paras[0].style.list.start_at, Some(9));
    assert!(bound.paras[2].style.list.restart);
    assert_eq!(bound.paras[3].style.list.kind, ListKind::Bullet("*".into()));
    let listed: Vec<usize> = para_lines(&layout_story(&lib, &bound, None)).iter().map(|l| l.glyphs.len()).collect();
    let extra: Vec<usize> = listed.iter().zip(&plain).map(|(a, b)| a - b).collect();
    assert_eq!(extra, vec![3, 4, 3, 2], "markers 9. / 10. / restart 1. / bullet");
    assert!(try_run(&mut ws, id, "text.para.set", json!({"story": story, "range": [0, 1], "patch": {"list": {"level": 0}}})).is_err());
    run(&mut ws, id, "edit.undo", json!({}));
    let undone = text_story(&ws, id);
    assert_eq!(undone.paras.iter().filter(|p| !matches!(p.style.list.kind, ListKind::None)).count(), 3);
}

#[test]
fn bad_para_field_from_core_keeps_other_fields() {
    let (mut ws, id, story) = live("aa\nbb");
    para_set(&mut ws, id, story, [0, 1], json!({"align": "Center", "indent_left": 7.0, "list": {"kind": "Bogus"}, "tabs": [{"pos": 10.0}, {"pos": 20.0, "align": "Sideways"}, {"pos": 30.0}]}));
    let bound = text_story(&ws, id);
    let style = &bound.paras[0].style;
    assert_eq!(style.align, Align::Center);
    assert_eq!(style.indent_left, 7.0);
    assert_eq!(style.list.kind, ListKind::None);
    assert_eq!(style.tabs.iter().map(|t| t.pos).collect::<Vec<_>>(), vec![10.0, 30.0]);
}
