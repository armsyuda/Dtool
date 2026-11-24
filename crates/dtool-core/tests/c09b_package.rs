use dtool_core::document::Units;
use dtool_core::resources::{ImageFormat, ImageResource};
use dtool_core::{Command, CommandResult, Document, Session, Story, StoryId};
use serde_json::{json, Value};
use std::io::Read;
use std::path::PathBuf;
use std::sync::OnceLock;

static FONT_DIR: OnceLock<PathBuf> = OnceLock::new();

fn host_run(_session: &mut Session, command: &Command) -> Option<CommandResult> {
    if command.name != "font.files" {
        return None;
    }
    let dir = FONT_DIR.get().unwrap();
    let faces = command.params["faces"].as_array().cloned().unwrap_or_default();
    let files: Vec<Value> = faces
        .iter()
        .map(|face| {
            let family = face["family"].as_str().unwrap();
            let paths: Vec<String> = match family {
                "Alpha" => vec![dir.join("alpha.ttf").display().to_string()],
                "Beta" => vec![dir.join("beta.otf").display().to_string(), dir.join("alpha.ttf").display().to_string()],
                _ => Vec::new(),
            };
            json!({"family": family, "style": face["style"], "paths": paths})
        })
        .collect();
    Some(Ok(json!({"files": files})))
}

fn host_can(_session: &Session, command: &Command) -> Option<bool> {
    (command.name == "font.files").then_some(true)
}

fn root(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c09b_package_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn story(id: u64, family: &str) -> Story {
    Story { id: StoryId(id), text: "Hi".into(), ranges: vec![json!({"start": 0, "end": 2, "family": family, "style": "Regular"})], specials: Vec::new(), paras: Vec::new() }
}

fn names(dir: &PathBuf) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).map(|entries| entries.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    names.sort();
    names
}

#[test]
fn package_copies_fonts_zips_and_prunes_only_its_own_links() {
    let root = root("fonts");
    let fonts = root.join("fonts_src");
    std::fs::create_dir_all(&fonts).unwrap();
    std::fs::write(fonts.join("alpha.ttf"), b"alpha-font").unwrap();
    std::fs::write(fonts.join("beta.otf"), b"beta-font").unwrap();
    FONT_DIR.set(fonts.clone()).unwrap();
    assert!(dtool_core::commands::text_host::install(host_run, host_can));

    let one = root.join("one.png");
    let two = root.join("two.png");
    std::fs::write(&one, b"one").unwrap();
    std::fs::write(&two, b"two").unwrap();
    let mut document = Document::new([100.0, 100.0], Units::Px, 72.0, 1);
    let first = document.add_image_resource(ImageResource::new_linked(one.clone(), ImageFormat::Png, [1, 1], 72.0));
    let second = document.add_image_resource(ImageResource::new_linked(two.clone(), ImageFormat::Png, [1, 1], 72.0));
    document.stories.push(story(1, "Alpha"));
    document.stories.push(story(2, "Beta"));
    document.stories.push(story(3, "Gone"));
    let mut session = Session::new(document);
    let folder = root.join("pkg");
    std::fs::create_dir_all(folder.join("Links")).unwrap();
    std::fs::write(folder.join("Links").join("user_note.txt"), b"mine").unwrap();
    std::fs::write(root.join("outside.png"), b"outside").unwrap();

    let report = session.run_named("doc.package.save", json!({"folder": folder, "fonts": true, "zip": true})).unwrap();
    let links = folder.join("Links");
    assert_eq!(names(&links), vec![format!("{}-one.png", first.0), format!("{}-two.png", second.0), "user_note.txt".to_string()]);
    assert_eq!(names(&folder.join("Fonts")), vec!["alpha.ttf".to_string(), "beta.otf".to_string()]);
    assert_eq!(std::fs::read(folder.join("Fonts").join("beta.otf")).unwrap(), b"beta-font");
    let fonts_report = report["fonts"].as_array().unwrap();
    assert_eq!(fonts_report.len(), 3);
    assert_eq!(report["fonts_missing"], json!(["Gone"]));
    let zip_path = PathBuf::from(report["zip"].as_str().unwrap());
    assert_eq!(zip_path, std::fs::canonicalize(&root).unwrap().join("pkg.zip"));
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&zip_path).unwrap()).unwrap();
    let mut entries: Vec<String> = (0..archive.len()).map(|i| archive.by_index(i).unwrap().name().unwrap().to_string()).collect();
    entries.sort();
    assert!(entries.contains(&"document.dtool".to_string()));
    assert!(entries.contains(&"Fonts/alpha.ttf".to_string()));
    assert!(entries.contains(&format!("Links/{}-one.png", first.0)));
    assert!(!entries.iter().any(|e| e.contains(".dtool_package")));
    let mut body = String::new();
    archive.by_name("Fonts/beta.otf").unwrap().read_to_string(&mut body).unwrap();
    assert_eq!(body, "beta-font");

    session.document.resources.retain(|resource| resource.id != second);
    session.document.stories.retain(|story| story.id != StoryId(2));
    let again = session.run_named("doc.package.resave", json!({"folder": folder, "fonts": true})).unwrap();
    assert_eq!(names(&links), vec![format!("{}-one.png", first.0), "user_note.txt".to_string()]);
    assert_eq!(names(&folder.join("Fonts")), vec!["alpha.ttf".to_string()]);
    assert_eq!(again["removed"].as_array().unwrap().len(), 2);
    assert!(again["zip"].is_null());
    assert!(root.join("outside.png").is_file());
    assert!(two.is_file());
    assert!(fonts.join("beta.otf").is_file());

    let images_off = session.run_named("doc.package.resave", json!({"folder": folder, "images": false})).unwrap();
    assert_eq!(images_off["removed"], json!([]));
    assert_eq!(images_off["fonts"], json!("skipped"));
    assert_eq!(names(&links), vec![format!("{}-one.png", first.0), "user_note.txt".to_string()]);
    assert_eq!(names(&folder.join("Fonts")), vec!["alpha.ttf".to_string()]);

    let manifest = folder.join(".dtool_package.json");
    std::fs::write(&manifest, br#"{"links":["../outside.png","..","user_note.txt\\..\\..\\outside.png"],"fonts":["../fonts_src/alpha.ttf"]}"#).unwrap();
    session.document.resources.clear();
    session.document.stories.clear();
    let hostile = session.run_named("doc.package.resave", json!({"folder": folder, "fonts": true})).unwrap();
    assert_eq!(hostile["removed"], json!([]));
    assert!(root.join("outside.png").is_file());
    assert!(fonts.join("alpha.ttf").is_file());
    assert!(links.join("user_note.txt").is_file());
    assert!(links.join(format!("{}-one.png", first.0)).is_file());

    let reopened = session.run_named("doc.package.open", json!({"folder": folder})).unwrap();
    assert!(reopened["document"]["resources"].as_array().unwrap().is_empty());
}