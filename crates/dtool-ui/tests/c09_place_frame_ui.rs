use std::path::PathBuf;

use dtool_ui::menu_tree::{MenuTree, PERSONAS};
use dtool_ui::place_tool::{image_drop_paths, split_paths};

#[test]
fn place_dialog_splits_multiple_paths() {
    assert_eq!(split_paths(" a.png ; \"b c.jpg\"\nc.webp;; "), vec!["a.png", "b c.jpg", "c.webp"]);
    assert!(split_paths("  ").is_empty());
}

#[test]
fn dropped_files_keep_only_images() {
    let paths = vec![PathBuf::from("a.PNG"), PathBuf::from("b.txt"), PathBuf::from("c.jpeg"), PathBuf::from("d")];
    assert_eq!(image_drop_paths(&paths), vec!["a.PNG", "c.jpeg"]);
}

#[test]
fn convert_to_picture_frame_menu_runs_frame_convert() {
    let found = PERSONAS.iter().filter_map(|persona| MenuTree::for_persona(persona).find("layer.convert.to_picture_frame").map(|item| item.command)).collect::<Vec<_>>();
    assert!(!found.is_empty());
    assert!(found.iter().all(|command| *command == Some("frame.convert")));
}
