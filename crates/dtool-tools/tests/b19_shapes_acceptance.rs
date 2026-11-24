use dtool_core::command::Session;
use dtool_core::shape::{ShapeKind, ShapeRecipe, SpiralStyle};
use dtool_core::{Command, Document, ObjectId};
use dtool_tools::shape_tool::{shape_kind_key, ShapeTool};

const B19_KINDS: [ShapeKind; 19] = [
    ShapeKind::Star,
    ShapeKind::Arrow,
    ShapeKind::CalloutEllipse,
    ShapeKind::CalloutRectangle,
    ShapeKind::Donut,
    ShapeKind::Pie,
    ShapeKind::Heart,
    ShapeKind::Cog,
    ShapeKind::Tear,
    ShapeKind::Crescent,
    ShapeKind::Cloud,
    ShapeKind::Segment,
    ShapeKind::Cat,
    ShapeKind::SquareStar,
    ShapeKind::DoubleStar,
    ShapeKind::Diamond,
    ShapeKind::Trapezoid,
    ShapeKind::Polygon,
    ShapeKind::Spiral,
];

fn make_session() -> Session {
    Session::new(Document::default())
}

fn add(session: &mut Session, kind: ShapeKind) -> u64 {
    let out = session.run(&ShapeTool { kind }.create([10.0, 20.0], [100.0, 80.0])).unwrap_or_else(|e| panic!("shape.add {kind:?} failed: {}", e.code));
    out["id"].as_u64().expect("id")
}

fn recipe(session: &Session, id: u64) -> ShapeRecipe {
    session.document.get_object(ObjectId(id)).and_then(|object| object.shape.clone()).expect("shape recipe")
}

fn has_own_recipe(recipe: &ShapeRecipe) -> bool {
    match recipe.kind {
        ShapeKind::Star => recipe.star.is_some(),
        ShapeKind::Arrow => recipe.arrow.is_some(),
        ShapeKind::CalloutEllipse => recipe.callout_ellipse.is_some(),
        ShapeKind::CalloutRectangle => recipe.callout_rectangle.is_some(),
        ShapeKind::Donut => recipe.donut.is_some(),
        ShapeKind::Pie => recipe.pie.is_some(),
        ShapeKind::Heart => recipe.heart.is_some(),
        ShapeKind::Cog => recipe.cog.is_some(),
        ShapeKind::Tear => recipe.tear.is_some(),
        ShapeKind::Crescent => recipe.crescent.is_some(),
        ShapeKind::Cloud => recipe.cloud.is_some(),
        ShapeKind::Segment => recipe.segment.is_some(),
        ShapeKind::Cat => recipe.cat.is_some(),
        ShapeKind::SquareStar => recipe.square_star.is_some(),
        ShapeKind::DoubleStar => recipe.double_star.is_some(),
        ShapeKind::Diamond => recipe.diamond.is_some(),
        ShapeKind::Trapezoid => recipe.trapezoid.is_some(),
        ShapeKind::Polygon => recipe.polygon.is_some(),
        ShapeKind::Spiral => recipe.spiral.is_some(),
        _ => false,
    }
}

fn params_set(id: u64) -> Command {
    let mut command = Command::new("shape.params.set");
    command.params["id"] = id.into();
    command
}

fn undo(session: &mut Session) {
    session.run(&Command::new("edit.undo")).expect("undo failed");
}

#[test]
fn every_b19_kind_created_by_tool_validates_with_nested_recipe() {
    let mut session = make_session();
    for kind in B19_KINDS {
        let id = add(&mut session, kind);
        let recipe = recipe(&session, id);
        assert_eq!(recipe.kind, kind);
        assert_eq!(recipe.size, [100.0, 80.0]);
        assert!(has_own_recipe(&recipe), "{kind:?} lost its recipe");
        assert!(recipe.corner.is_none() && recipe.triangle.is_none());
    }
}

#[test]
fn tool_nests_fields_under_kind_key() {
    for kind in B19_KINDS {
        let command = ShapeTool { kind }.create([0.0, 0.0], [10.0, 10.0]);
        let key = shape_kind_key(kind);
        assert!(command.params[key].is_object(), "{key} not nested");
        let nested: Vec<String> = command.params[key].as_object().unwrap().keys().cloned().collect();
        for field in nested {
            assert!(command.params.get(field.as_str()).is_none(), "{key}.{field} also sent flat");
        }
    }
}

#[test]
fn modifier_drag_creates_every_b19_kind() {
    let mut session = make_session();
    for kind in B19_KINDS {
        let command = ShapeTool { kind }.create_with_modifiers([50.0, 50.0], [80.0, 60.0], true, true);
        let out = session.run(&command).unwrap_or_else(|e| panic!("{kind:?} drag failed: {}", e.code));
        let recipe = recipe(&session, out["id"].as_u64().unwrap());
        assert_eq!(recipe.size, [30.0, 30.0]);
        assert!(has_own_recipe(&recipe));
        let object = session.document.get_object(ObjectId(out["id"].as_u64().unwrap())).unwrap();
        assert_eq!(object.pos, [35.0, 35.0]);
    }
}

#[test]
fn query_returns_nested_recipe() {
    let mut session = make_session();
    let id = add(&mut session, ShapeKind::Star);
    let mut query = Command::new("shape.params.query");
    query.params["id"] = id.into();
    let out = session.run(&query).expect("query");
    assert_eq!(out["kind"].as_str(), Some("star"));
    assert_eq!(out["star"]["number_of_points"].as_u64(), Some(5));
}

#[test]
fn params_set_patches_star_and_undo_restores() {
    let mut session = make_session();
    let id = add(&mut session, ShapeKind::Star);
    let mut command = params_set(id);
    command.params["star"]["number_of_points"] = 7_i64.into();
    session.run(&command).expect("star patch");
    let star = recipe(&session, id).star.unwrap();
    assert_eq!(star.number_of_points, 7);
    assert_eq!(star.inner_radius, 50.0);
    assert_eq!(star.outer_radius, 100.0);
    undo(&mut session);
    assert_eq!(recipe(&session, id).star.unwrap().number_of_points, 5);
}

#[test]
fn params_set_patches_polygon_spiral_and_cog() {
    let mut session = make_session();
    let polygon = add(&mut session, ShapeKind::Polygon);
    let spiral = add(&mut session, ShapeKind::Spiral);
    let cog = add(&mut session, ShapeKind::Cog);
    let mut command = params_set(polygon);
    command.params["polygon"]["number_of_sides"] = 8_i64.into();
    command.params["polygon"]["smooth_points"] = true.into();
    session.run(&command).expect("polygon patch");
    let p = recipe(&session, polygon).polygon.unwrap();
    assert_eq!((p.number_of_sides, p.smooth_points), (8, true));
    let mut command = params_set(spiral);
    command.params["spiral"]["style"] = "fibonacci".into();
    command.params["spiral"]["turns"] = 5.0_f64.into();
    session.run(&command).expect("spiral patch");
    let sp = recipe(&session, spiral).spiral.unwrap();
    assert_eq!(sp.style, SpiralStyle::Fibonacci);
    assert_eq!(sp.turns, 5.0);
    let mut command = params_set(cog);
    command.params["cog"]["number_of_teeth"] = 20_i64.into();
    session.run(&command).expect("cog patch");
    assert_eq!(recipe(&session, cog).cog.unwrap().number_of_teeth, 20);
}

#[test]
fn params_set_rejects_wrong_kind_and_invalid_values() {
    let mut session = make_session();
    let star = add(&mut session, ShapeKind::Star);
    let before = recipe(&session, star);
    let mut wrong = params_set(star);
    wrong.params["polygon"]["number_of_sides"] = 8_i64.into();
    assert_eq!(session.run(&wrong).unwrap_err().code, "INVALID_PARAMS");
    let mut bad = params_set(star);
    bad.params["star"]["number_of_points"] = 2_i64.into();
    assert_eq!(session.run(&bad).unwrap_err().code, "INVALID_PARAMS");
    let mut garbage = params_set(star);
    garbage.params["star"]["number_of_points"] = "many".into();
    assert_eq!(session.run(&garbage).unwrap_err().code, "INVALID_PARAMS");
    assert_eq!(recipe(&session, star), before);
}

#[test]
fn params_set_every_kind_round_trip_with_own_values() {
    let mut session = make_session();
    for kind in B19_KINDS {
        let id = add(&mut session, kind);
        let key = shape_kind_key(kind);
        let sent = ShapeTool { kind }.create([0.0, 0.0], [1.0, 1.0]);
        let mut command = params_set(id);
        command.params[key] = sent.params[key].clone();
        command.params["size"] = vec![40.0_f64, 30.0].into();
        session.run(&command).unwrap_or_else(|e| panic!("{key} round trip failed: {}", e.code));
        let after = recipe(&session, id);
        assert_eq!(after.size, [40.0, 30.0]);
        assert!(has_own_recipe(&after));
        assert_eq!(session.document.get_object(ObjectId(id)).unwrap().size, [40.0, 30.0]);
    }
}