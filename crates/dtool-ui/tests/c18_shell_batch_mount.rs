#[path = "support/test_home.rs"]
mod test_home;
use dtool_export::batch::{self, BatchBoundary, BatchStatus, Destination, FileStatus};
use dtool_export::ExportFormat;
use dtool_ui::batch_host::{BATCH_JOB_MENU_ID, BATCH_WINDOW_MENU_ID, MACRO_WINDOW_MENU_ID};
use dtool_ui::menu_tree;
use dtool_ui::shell::AppShell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn frame(shell: &mut AppShell, ctx: &egui::Context) {
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn write_input(path: &Path, side: u32, seed: u32) {
    let mut state = seed.wrapping_mul(2654435761).max(1);
    let mut pixels = Vec::with_capacity((side * side * 4) as usize);
    for _ in 0..side * side {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let [r, g, b, _] = state.to_le_bytes();
        pixels.extend_from_slice(&[r, g, b, 255]);
    }
    let bytes = dtool_export::webp::encode_rgba(side, side, &pixels, true, 100).expect("lossless webp");
    std::fs::write(path, bytes).unwrap();
}

fn wait_for(limit: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    ready()
}

#[test]
fn c18_batch_and_macro_rows_are_live_shell_actions() {
    let _home = test_home::enter("c18_batch_rows_live");
    assert!(menu_tree::row_is_local(BATCH_JOB_MENU_ID, false));
    assert!(menu_tree::row_is_local(BATCH_WINDOW_MENU_ID, false));
    assert!(!menu_tree::row_is_local(MACRO_WINDOW_MENU_ID, false));
    assert!(menu_tree::row_is_local(MACRO_WINDOW_MENU_ID, true));

    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    frame(&mut shell, &ctx);
    assert!(!shell.batch_host.macro_open());
    let doc = shell.workspace.new_document("c18 macro mount");
    shell.workspace.activate(doc).unwrap();
    assert!(shell.batch_menu(MACRO_WINDOW_MENU_ID));
    assert!(shell.batch_host.macro_open());
    assert!(shell.batch_host.macro_library.open);
    frame(&mut shell, &ctx);
    assert!(shell.batch_host.macro_open());
    assert!(shell.batch_menu(MACRO_WINDOW_MENU_ID));
    assert!(!shell.batch_host.macro_open());
    assert!(!shell.batch_host.macro_library.open);
    assert!(!shell.batch_menu("file.export.export"));
}

#[test]
fn c18_shell_batch_two_files_cancel_after_first() {
    let home = test_home::enter("c18_shell_batch_cancel");
    let input_dir: PathBuf = home.root.join("batch_in");
    let output_dir: PathBuf = home.root.join("batch_out");
    std::fs::create_dir_all(&input_dir).unwrap();
    std::fs::create_dir_all(&output_dir).unwrap();
    let first = input_dir.join("first.webp");
    let second = input_dir.join("second.webp");
    write_input(&first, 1400, 7);
    write_input(&second, 8, 11);
    let first_bytes = std::fs::read(&first).unwrap();
    let second_bytes = std::fs::read(&second).unwrap();

    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    frame(&mut shell, &ctx);
    assert!(!shell.batch_host.batch_open());
    assert!(shell.batch_menu(BATCH_JOB_MENU_ID));
    assert!(shell.batch_host.batch_open());
    assert!(shell.dialog_open(&ctx));
    frame(&mut shell, &ctx);
    assert!(shell.batch_host.batch_open());

    let dialog = &mut shell.batch_host.batch;
    dialog.add_paths([first.clone(), second.clone()]);
    assert_eq!(dialog.manifest.inputs.len(), 2);
    let second_id = dialog.manifest.inputs[1].id;
    dialog.add_output(ExportFormat::Png, false);
    dialog.manifest.outputs[0].dest = Destination::Folder(output_dir.clone());
    let handle = dialog.start_run().expect("batch starts");
    batch::arm_boundary(&handle, BatchBoundary::File, Some(second_id));
    assert!(
        wait_for(Duration::from_secs(60), || batch::current_boundary(&handle) == Some(BatchBoundary::File)),
        "worker parks before the second file"
    );

    shell.batch_host.batch.refresh_snapshot();
    let running = shell.batch_host.batch.last_snapshot().cloned().expect("live snapshot");
    assert_eq!(running.status, BatchStatus::Running);
    assert_eq!(running.done_units, 1);
    assert_eq!(running.total_units, 2);
    assert!(output_dir.join("first.png").exists());
    assert!(!output_dir.join("second.png").exists());

    assert!(shell.batch_host.batch.is_busy());
    assert!(!shell.batch_host.batch.can_close());
    assert!(!shell.batch_host.close_batch(), "close refused while the run is busy");
    shell.batch_host.batch.open = false;
    frame(&mut shell, &ctx);
    assert!(shell.batch_host.batch_open(), "window cannot close while the run is busy");
    assert!(shell.batch_menu(BATCH_WINDOW_MENU_ID));
    assert_eq!(shell.batch_host.batch.active_handle(), Some(&handle), "second click does not start another run");

    shell.batch_host.batch.cancel_run();
    assert!(wait_for(Duration::from_secs(30), || !shell.batch_host.batch.is_busy()), "worker stops after cancel");
    shell.batch_host.batch.refresh_snapshot();
    let done = shell.batch_host.batch.last_snapshot().cloned().unwrap();
    assert_eq!(done.status, BatchStatus::Cancelled);
    assert_eq!(done.done_units, 1);
    let second_receipt = done.files.iter().find(|file| file.input == second_id);
    assert!(second_receipt.map_or(true, |file| matches!(file.status, FileStatus::Untouched | FileStatus::Cancelled)), "{:?}", second_receipt.map(|f| f.status));
    assert!(!output_dir.join("second.png").exists(), "second output untouched");
    assert!(std::fs::read_dir(&output_dir).unwrap().filter_map(Result::ok).all(|entry| entry.file_name() == "first.png"), "no temp files left");
    assert_eq!(std::fs::read(&first).unwrap(), first_bytes);
    assert_eq!(std::fs::read(&second).unwrap(), second_bytes);

    assert!(shell.batch_host.close_batch());
    frame(&mut shell, &ctx);
    assert!(!shell.batch_host.batch_open());
    assert!(!shell.dialog_open(&ctx));

    assert!(shell.batch_menu(BATCH_JOB_MENU_ID));
    frame(&mut shell, &ctx);
    std::thread::sleep(Duration::from_millis(100));
    frame(&mut shell, &ctx);
    let reopened = shell.batch_host.batch.last_snapshot().cloned().unwrap();
    assert_eq!(reopened.handle, handle);
    assert_eq!(reopened.status, BatchStatus::Cancelled);
    assert_eq!(reopened.done_units, 1);
    assert!(!shell.batch_host.batch.is_busy());
    assert!(!output_dir.join("second.png").exists(), "reopen does not replay the run");
}
