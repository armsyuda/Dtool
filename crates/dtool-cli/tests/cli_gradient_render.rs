use dtool_core::paint::{Colour, GradientGeom, GradientKind, GradientStop, Paint};
use dtool_core::workspace::Workspace;
use serde_json::{json, Value};

fn run(shared: &dtool_core::workspace::SharedWorkspace, name: &str, params: Value) -> Value {
    dtool_cli::run_cli_args(shared, &["run".into(), name.into(), params.to_string()]).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn rgba(reply: &Value) -> Vec<u64> {
    reply["colour"]["rgba"].as_array().unwrap_or_else(|| panic!("no rgba in {reply}")).iter().map(|v| v.as_u64().unwrap()).collect()
}

#[test]
fn cli_gradient_fill_renders_more_than_one_colour() {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("cli-gradient");
    workspace.activate(id).unwrap();
    let shared = workspace.into_shared();
    let stops = vec![GradientStop { offset: 0.0, colour: Colour::new(255, 0, 0, 255) }, GradientStop { offset: 1.0, colour: Colour::new(0, 0, 255, 255) }];
    let fill = Paint::Gradient { kind: GradientKind::Linear, stops, geom: GradientGeom { start: [0.0, 10.0], end: [100.0, 10.0], ratio: 1.0 } };
    run(&shared, "object.create", json!({ "pos": [0.0, 0.0], "size": [100.0, 20.0], "fill": serde_json::to_value(&fill).unwrap() }));
    let samples: Vec<Vec<u64>> = [2.0, 50.0, 98.0].iter().map(|x| rgba(&run(&shared, "color.pick", json!({ "x": x, "y": 10.0 })))).collect();
    assert!(samples.iter().all(|c| c[3] == 255), "{samples:?}");
    assert!(samples[0][0] > 200 && samples[0][2] < 55, "left should be red: {samples:?}");
    assert!(samples[2][2] > 200 && samples[2][0] < 55, "right should be blue: {samples:?}");
    let mut distinct = samples.clone();
    distinct.dedup();
    assert_eq!(distinct.len(), 3, "gradient rendered flat through the CLI path: {samples:?}");
}
