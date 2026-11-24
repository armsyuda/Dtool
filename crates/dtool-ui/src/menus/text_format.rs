use crate::panels::character::{read_selection, Field};
use dtool_core::{Command, CommandError, Session};
use serde_json::{json, Value};

#[derive(Clone, Copy)]
enum Edit { Toggle(&'static str), Decoration(&'static str), Step(&'static str, f32), ResetSpacing(bool), Size(f32), Position(&'static str), Case(&'static str, &'static str), Ligatures(Option<bool>), Align(&'static str), Leading(f32), Vertical(&'static str), List(&'static str), Invisibles }
fn edit(id: &str) -> Option<Edit> {
    Some(match id {
        "text.character_traits.bold" => Edit::Toggle("weight"),
        "text.character_traits.italic" => Edit::Toggle("italic"),
        "text.character_traits.underline" => Edit::Decoration("underline"),
        "text.character_traits.strikethrough" => Edit::Decoration("strike"),
        "text.size.bigger" => Edit::Size(1.0), "text.size.smaller" => Edit::Size(-1.0),
        "text.size.precise_bigger" => Edit::Size(0.1), "text.size.precise_smaller" => Edit::Size(-0.1),
        "text.spacing.use_default" => Edit::ResetSpacing(false), "text.spacing.use_none" => Edit::ResetSpacing(true),
        "text.spacing.tighten" => Edit::Step("tracking", -20.0), "text.spacing.loosen" => Edit::Step("tracking", 20.0),
        "text.spacing.tighten_more" => Edit::Step("tracking", -100.0), "text.spacing.loosen_more" => Edit::Step("tracking", 100.0),
        "text.baseline.use_default" => Edit::Position("Normal"), "text.baseline.superscript" => Edit::Position("Superscript"), "text.baseline.subscript" => Edit::Position("Subscript"),
        "text.baseline.raise" => Edit::Step("baseline_pt", 1.0), "text.baseline.lower" => Edit::Step("baseline_pt", -1.0),
        "text.baseline.precise_raise" => Edit::Step("baseline_pt", 0.1), "text.baseline.precise_lower" => Edit::Step("baseline_pt", -0.1),
        "text.capitalisation.none" => Edit::Case("style", "none"), "text.capitalisation.small_caps" => Edit::Case("style", "small_caps"), "text.capitalisation.all_caps" => Edit::Case("style", "all_caps"),
        "text.capitalisation.lower_case" => Edit::Case("change", "lower"), "text.capitalisation.upper_case" => Edit::Case("change", "upper"), "text.capitalisation.toggle_case" => Edit::Case("change", "toggle"),
        "text.capitalisation.title_case" => Edit::Case("change", "title"), "text.capitalisation.sentence_case" => Edit::Case("change", "sentence"),
        "text.ligatures.use_default" => Edit::Ligatures(None),
        "text.ligatures.use_none" => Edit::Ligatures(Some(false)),
        "text.ligatures.use_all" => Edit::Ligatures(Some(true)),
        "text.alignment.align_left" => Edit::Align("Left"),
        "text.alignment.align_centre" => Edit::Align("Center"),
        "text.alignment.align_right" => Edit::Align("Right"),
        "text.alignment.justify_left" => Edit::Align("JustifyLeft"),
        "text.alignment.justify_centre" => Edit::Align("JustifyCenter"),
        "text.alignment.justify_right" => Edit::Align("JustifyRight"),
        "text.alignment.justify_all" => Edit::Align("JustifyAll"),
        "text.alignment.align_towards_spine" => Edit::Align("TowardsSpine"),
        "text.alignment.align_away_from_spine" => Edit::Align("AwayFromSpine"),
        "text.paragraph_leading.increase" => Edit::Leading(1.0),
        "text.paragraph_leading.decrease" => Edit::Leading(-1.0),
        "text.paragraph_leading.precise_increase" => Edit::Leading(0.1),
        "text.paragraph_leading.precise_decrease" => Edit::Leading(-0.1),
        "text.vertical_alignment.top" => Edit::Vertical("Top"),
        "text.vertical_alignment.centre" => Edit::Vertical("Center"),
        "text.vertical_alignment.bottom" => Edit::Vertical("Bottom"),
        "text.vertical_alignment.justify" => Edit::Vertical("Justify"),
        "text.list.bullet" => Edit::List("bullet"),
        "text.list.number" => Edit::List("number"),
        "text.list.restart_numbering" => Edit::List("restart"),
        "text.list.increase_level" => Edit::List("increase"),
        "text.list.decrease_level" => Edit::List("decrease"),
        "text.show_special_characters" => Edit::Invisibles,
        _ => return None,
    })
}
pub fn handles(id: &str) -> bool { edit(id).is_some() || crate::table::menu::command_name(id).is_some() }
fn batch(commands: Vec<Command>) -> Option<Command> {
    if commands.is_empty() { None } else if commands.len() == 1 { commands.into_iter().next() } else { Some(Command::with_params("edit.batch", json!({"commands": commands}))) }
}
fn paragraphs(session: &Session, edit: Edit) -> Option<Command> {
    let (story_id, ranges) = crate::panels::paragraph::paragraph_ranges(session)?;
    let story = session.document.stories.iter().find(|story| story.id == story_id)?;
    let styles: Vec<_> = ranges.iter().map(|[start,end]| dtool_core::text_cmd::query_para_style(&story.paras,*start,*end)).collect();
    let all_kind = |key: &str| styles.iter().all(|style| style["list"]["kind"].get(key).is_some());
    let toggled_off = match edit { Edit::List("bullet") => all_kind("Bullet"), Edit::List("number") => all_kind("Number"), _ => false };
    let mut commands = Vec::new();
    let mut restarted = false;
    for ([start,end], current) in ranges.into_iter().zip(styles) {
        let patch = match edit {
            Edit::Align(align) => json!({"align":align}),
            Edit::Leading(delta) => {
                let size = crate::panels::character::selection_at(session,story_id.0,start,end)
                    .map(|selection| selection.segments.iter().map(|s| s.style.get("size_pt").and_then(Value::as_f64).unwrap_or(12.0)).reduce(f64::max).unwrap_or(12.0)).unwrap_or(12.0);
                let leading = current["leading"].as_f64().unwrap_or(1.2);
                let points = if leading < 4.0 { leading * size } else { leading };
                let next = (points + f64::from(delta)).max(0.1);
                json!({"leading": if next < 4.0 { next/size } else { next }})
            }
            Edit::List(action) => {
                let mut list: dtool_tools::dtool_text::para_style::ListProps = serde_json::from_value(current["list"].clone()).unwrap_or_default();
                use dtool_tools::dtool_text::para_style::{ListKind, NumberFormat};
                match action {
                    "bullet" => if toggled_off { list.kind = ListKind::None; } else if !matches!(list.kind,ListKind::Bullet(_)) { list.kind = ListKind::Bullet("•".to_string()); },
                    "number" => if toggled_off { list.kind = ListKind::None; } else if !matches!(list.kind,ListKind::Number(_)) { list.kind = ListKind::Number(NumberFormat::Arabic); },
                    "restart" if matches!(list.kind,ListKind::Number(_)) && !restarted => { list.restart = true; list.start_at = Some(1); restarted = true; },
                    "increase" if !matches!(list.kind,ListKind::None) => list.level = (list.level+1).min(9),
                    "decrease" if !matches!(list.kind,ListKind::None) => list.level = list.level.saturating_sub(1).max(1),
                    _ => continue,
                }
                json!({"list":list})
            }
            _ => return None,
        };
        commands.push(Command::with_params("text.para.set",json!({"story":story_id.0,"range":[start,end],"patch":patch})));
    }
    batch(commands)
}

pub fn command(session: &Session, id: &str) -> Option<Command> {
    if crate::table::menu::command_name(id).is_some() {
        return crate::table::menu::command(session, id);
    }
    let edit = edit(id)?;
    match edit {
        Edit::Align(_) | Edit::Leading(_) | Edit::List(_) => return paragraphs(session,edit),
        Edit::Vertical(value) => return crate::panels::text_frame::read_selection(session)?.set("valign",json!(value)),
        Edit::Invisibles => return session.has_document.then(||Command::with_params("view.flags.set",json!({"show_invisibles":!session.view.canvas_flags.show_invisibles}))),
        _ => {}
    }
    let selection = read_selection(session)?; if selection.start >= selection.end { return None; }
    match edit {
        Edit::Ligatures(value) => {
            let features = selection.per_segment("opentype", |old| {
                let mut features: Vec<Value> = old.and_then(Value::as_array).cloned().unwrap_or_default().into_iter()
                    .filter(|pair| !matches!(pair.get(0).and_then(Value::as_str),Some("liga"|"clig"|"dlig"|"hlig"))).collect();
                if let Some(on) = value { for tag in ["liga","clig","dlig","hlig"] { features.push(json!([tag,u32::from(on)])); } }
                Some(json!(features))
            });
            batch([selection.set("ligatures",json!(value)),features].into_iter().flatten().collect())
        }
        Edit::Toggle("weight") => selection.set("weight", if selection.number("weight",400.0).is_some_and(|v| v>=700.0) {json!(400)} else {json!(700)}),
        Edit::Toggle(key) => selection.set(key, json!(selection.field(key) != Field::Same(json!(true)))),
        Edit::Decoration(key) => selection.decoration(key, selection.decoration_on(key) != Some(true)),
        Edit::Step(key, delta) => selection.per_segment(key, |value| Some(json!(value.and_then(Value::as_f64).unwrap_or(0.0) + f64::from(delta)))),
        Edit::Size(delta) => selection.per_segment("size_pt", |value| Some(json!((value.and_then(Value::as_f64).unwrap_or(12.0) + f64::from(delta)).max(0.1)))),
        Edit::ResetSpacing(none) => batch([selection.set("tracking",json!(0)), selection.set("kerning",if none {json!({"Manual":0.0})} else {json!("Auto")})].into_iter().flatten().collect()),
        Edit::Position(position) => batch([selection.set("position",json!(position)), (position=="Normal").then(|| selection.set("baseline_pt",json!(0))).flatten()].into_iter().flatten().collect()),
        Edit::Case(mode,op) => Some(Command::with_params("text.case",json!({"story":selection.story,"range":{"start":selection.start,"end":selection.end},"mode":mode,"op":op}))),
        Edit::Align(_) | Edit::Leading(_) | Edit::Vertical(_) | Edit::List(_) | Edit::Invisibles => unreachable!(),
    }
}
pub fn run(session: &mut Session, id: &str) -> Result<Value, CommandError> {
    let command=command(session,id).ok_or_else(CommandError::invalid_params)?; session.run(&command)
}
fn checked(session: &Session, id: &str) -> Option<bool> {
    match edit(id)? {
        Edit::Invisibles => Some(session.view.canvas_flags.show_invisibles),
        Edit::Vertical(value) => Some(crate::panels::text_frame::read_selection(session)?.valign().as_deref() == Some(value)),
        Edit::Align(value) => {
            let (story_id,ranges) = crate::panels::paragraph::paragraph_ranges(session)?;
            let story = session.document.stories.iter().find(|s| s.id == story_id)?;
            Some(ranges.iter().all(|[start,end]| dtool_core::text_cmd::query_para_style(&story.paras,*start,*end)["align"].as_str().unwrap_or("Left") == value))
        }
        Edit::List(action @ ("bullet"|"number")) => {
            let (story_id,ranges) = crate::panels::paragraph::paragraph_ranges(session)?;
            let story = session.document.stories.iter().find(|s| s.id == story_id)?;
            Some(ranges.iter().all(|[start,end]| dtool_core::text_cmd::query_para_style(&story.paras,*start,*end)["list"]["kind"].get(if action == "bullet" {"Bullet"} else {"Number"}).is_some()))
        }
        Edit::Toggle("weight") => Some(read_selection(session)?.number("weight",400.0).is_some_and(|v|v>=700.0)),
        Edit::Toggle(key) => Some(read_selection(session)?.field(key) == Field::Same(json!(true))),
        Edit::Decoration(key) => read_selection(session)?.decoration_on(key),
        Edit::Position(value) => Some(read_selection(session)?.text_value("position").filter(|s|!s.is_empty()).as_deref().unwrap_or("Normal") == value),
        Edit::Ligatures(value) => Some(read_selection(session)?.field("ligatures") == Field::Same(json!(value))),
        _ => None,
    }
}

pub fn annotate(tree: &mut Value, session: Option<&Session>) {
    if let Some(items)=tree.as_array_mut() { for item in items {
        if let Some(children)=item.get_mut("children") {annotate(children,session);}
        let Some(id)=item["id"].as_str().filter(|id| handles(id)).map(str::to_owned) else {continue};
        let command=session.and_then(|s| command(s,&id));
        item["command"]=json!(command.as_ref().map(|c| c.name.as_str()).or_else(|| crate::table::menu::command_name(&id)).unwrap_or("text.style"));
        item["planned"]=json!(false); item["planned_key"]=Value::Null; item["error_code"]=Value::Null;
        item["enabled"]=json!(session.zip(command.as_ref()).is_some_and(|(s,c)| s.has_document && s.can_execute(c)));
        item["checked"] = json!(session.and_then(|s| checked(s,&id)));
        item["radio"] = json!(matches!(edit(&id),Some(Edit::Position(_) | Edit::Ligatures(_) | Edit::Align(_) | Edit::Vertical(_))));
        if let Some(key)=item["key"].as_str() {item["label"]=json!(dtool_strings::text(key));}
    }}
}
