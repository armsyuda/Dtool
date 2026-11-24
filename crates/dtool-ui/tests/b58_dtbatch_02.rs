use dtool_core::{Command, CommandResult, Scope, Workspace};
use dtool_export::ExportFormat;
use dtool_export::batch::{
    self, BatchHandle, BatchManifest, BatchOutput, BatchSnapshot, BatchStatus, Destination,
    FileStatus, OverwritePolicy,
};
use dtool_export::batch_store::BatchStore;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PHASE_ENV: &str = "DTOOL_B58_ACCEPTANCE_PHASE";
const ROOT_ENV: &str = "DTOOL_B58_ACCEPTANCE_ROOT";
const PNG_INPUTS: [&[u8]; 3] = [
    &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 6,
        0, 0, 0, 114, 182, 13, 36, 0, 0, 0, 17, 73, 68, 65, 84, 120, 156, 99, 16, 209, 136, 250,
        15, 194, 12, 48, 6, 0, 51, 32, 6, 85, 123, 239, 145, 110, 0, 0, 0, 0, 73, 69, 78, 68, 174,
        66, 96, 130,
    ],
    &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 6,
        0, 0, 0, 114, 182, 13, 36, 0, 0, 0, 17, 73, 68, 65, 84, 120, 156, 99, 8, 208, 136, 250, 15,
        194, 12, 48, 6, 0, 60, 248, 7, 69, 44, 199, 63, 74, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
        96, 130,
    ],
    &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 6,
        0, 0, 0, 114, 182, 13, 36, 0, 0, 0, 17, 73, 68, 65, 84, 120, 156, 99, 88, 160, 17, 245, 31,
        132, 25, 96, 12, 0, 74, 24, 8, 133, 20, 207, 59, 124, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
        96, 130,
    ],
];

struct TemporaryRoot(PathBuf);
impl TemporaryRoot {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "dtool-b58-{}-{}-{}",
            std::process::id(),
            stamp,
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("fixture.marker"), b"dtool-b58-owned-fixture").unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}
impl Drop for TemporaryRoot {
    fn drop(&mut self) {
        let Ok(parent) = fs::canonicalize(std::env::temp_dir()) else {
            return;
        };
        let Ok(root) = fs::canonicalize(&self.0) else {
            return;
        };
        if root.parent() == Some(parent.as_path())
            && root
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("dtool-b58-"))
        {
            let _ = fs::remove_dir_all(root);
        }
    }
}

fn run_child(root: &Path, phase: &str) {
    let completed = root.join(format!("completed-{phase}"));
    assert!(!completed.exists());
    let mut child = ProcessCommand::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "batch_acceptance_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PHASE_ENV, phase)
        .env(ROOT_ENV, root)
        .env("APPDATA", root.join("profile"))
        .env("RUST_BACKTRACE", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "child timed out: {} / {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "phase {phase}: {} / {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(completed).expect("child did not execute the selected acceptance phase"),
        b"completed"
    );
}

fn command(workspace: &mut Workspace, name: &str, params: Value) -> Value {
    workspace
        .run(Scope::active(), Command::with_params(name, params))
        .1
        .unwrap_or_else(|error| panic!("{name}: {}", error.code))
}
fn snapshot(workspace: &mut Workspace, name: &str, handle: &BatchHandle) -> BatchSnapshot {
    serde_json::from_value(command(workspace, name, json!({"handle":handle.0}))).unwrap()
}
fn durable(handle: &BatchHandle) -> BatchSnapshot {
    BatchStore::default()
        .load(handle)
        .unwrap()
        .expect("missing durable snapshot")
}
fn same_snapshot(left: &BatchSnapshot, right: &BatchSnapshot) {
    assert_eq!(
        serde_json::to_value(left).unwrap(),
        serde_json::to_value(right).unwrap()
    );
}

struct Inputs {
    paths: Vec<PathBuf>,
    manifest: BatchManifest,
    output: PathBuf,
}
impl Inputs {
    fn new(root: &Path, format: ExportFormat) -> Self {
        let input = root.join("inputs");
        let output = root.join("outputs");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output).unwrap();
        let mut manifest = BatchManifest::default();
        let mut paths = Vec::new();
        for (index, bytes) in PNG_INPUTS.iter().enumerate() {
            let path = input.join(format!("input-{index}")).with_extension("png");
            fs::write(&path, bytes).unwrap();
            manifest.add(path.clone()).unwrap();
            paths.push(path);
        }
        manifest.outputs.push(BatchOutput {
            format,
            width: None,
            height: None,
            keep_aspect: true,
            dest: Destination::Folder(output.clone()),
            name: "{name}.{format}".into(),
            overwrite: OverwritePolicy::Fail,
            webp_lossy: false,
        });
        Self {
            paths,
            manifest,
            output,
        }
    }
    fn prepare(&self, workspace: &mut Workspace) -> BatchHandle {
        let reply = command(
            workspace,
            "export.batch.prepare",
            json!({"manifest":self.manifest}),
        );
        let handle = BatchHandle(reply["handle"].as_str().expect("missing handle").to_owned());
        let validation = command(
            workspace,
            "export.batch.validate",
            json!({"handle":handle.0}),
        );
        assert_eq!(validation["problems"].as_array().unwrap().len(), 0);
        handle
    }
    fn path(&self, index: usize, extension: &str) -> PathBuf {
        self.output
            .join(self.paths[index].file_stem().unwrap())
            .with_extension(extension)
    }
    fn unchanged_inputs(&self) {
        for (path, bytes) in self.paths.iter().zip(PNG_INPUTS) {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
}

struct Running {
    handle: BatchHandle,
    receive: mpsc::Receiver<CommandResult>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Running {
    fn start(handle: &BatchHandle, boundary: &str, input: u64) -> Self {
        let (send, receive) = mpsc::channel();
        let owned = handle.clone();
        let boundary = boundary.to_owned();
        let worker = thread::spawn(move || {
            let mut workspace = Workspace::default();
            let result = workspace
                .run(
                    Scope::active(),
                    Command::with_params(
                        "export.batch.start",
                        json!({"handle":owned.0,"pause_at":boundary,"pause_input":input}),
                    ),
                )
                .1;
            let _ = send.send(result);
        });
        Self {
            handle: handle.clone(),
            receive,
            thread: Some(worker),
        }
    }
    fn finish(&mut self) -> BatchSnapshot {
        let value = self
            .receive
            .recv_timeout(Duration::from_secs(10))
            .expect("start did not finish")
            .expect("start failed");
        self.thread.take().unwrap().join().expect("worker panicked");
        serde_json::from_value(value).unwrap()
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(worker) = self.thread.take() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !worker.is_finished() && Instant::now() < deadline {
                let mut workspace = Workspace::default();
                let _ = workspace.run(
                    Scope::active(),
                    Command::with_params("export.batch.cancel", json!({"handle":self.handle.0})),
                );
                batch::release_boundary(&self.handle);
                thread::sleep(Duration::from_millis(5));
            }
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}
fn wait_at(workspace: &mut Workspace, handle: &BatchHandle, boundary: &str) -> BatchSnapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let reply = command(workspace, "export.batch.query", json!({"handle":handle.0}));
        if reply["paused_at"].as_str() == Some(boundary) {
            return serde_json::from_value(reply).unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "never reached {boundary}: {reply}"
        );
        thread::sleep(Duration::from_millis(2));
    }
}
fn completed_png(file: &dtool_export::batch::FileReceipt, path: &Path) {
    assert_eq!(file.status, FileStatus::Done);
    assert_eq!(file.outputs.len(), 1);
    let receipt = &file.outputs[0];
    assert_eq!(receipt.path, path);
    assert!(receipt.complete);
    let bytes = fs::read(path).unwrap();
    assert_eq!(receipt.bytes, bytes.len() as u64);
    assert!(bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]));
    assert!(bytes.len() >= 24);
    assert_eq!(&bytes[16..24], &[0, 0, 0, 2, 0, 0, 0, 2]);
    assert!(file.error.is_none());
}
fn clean_temporary_files(directory: &Path) {
    for entry in fs::read_dir(directory).unwrap() {
        assert_ne!(
            entry.unwrap().path().extension().and_then(|v| v.to_str()),
            Some("tmp")
        );
    }
}

fn between_files(root: &Path) {
    let mut workspace = Workspace::default();
    let inputs = Inputs::new(root, ExportFormat::Png);
    let handle = inputs.prepare(&mut workspace);
    let second = inputs.manifest.inputs[1].id;
    let mut running = Running::start(&handle, "file", second.0);
    let parked = wait_at(&mut workspace, &handle, "file");
    completed_png(&parked.files[0], &inputs.path(0, "png"));
    assert!(!inputs.path(1, "png").exists());
    assert!(!inputs.path(2, "png").exists());
    let _ = snapshot(&mut workspace, "export.batch.cancel", &handle);
    let finished = running.finish();
    assert_eq!(finished.status, BatchStatus::Cancelled);
    assert_eq!(finished.files.len(), 3);
    completed_png(&finished.files[0], &inputs.path(0, "png"));
    for index in [1, 2] {
        assert_eq!(
            finished.files[index].input,
            inputs.manifest.inputs[index].id
        );
        assert!(finished.files[index].outputs.is_empty());
        assert!(finished.files[index].error.is_none());
        assert!(!inputs.path(index, "png").exists());
    }
    same_snapshot(
        &finished,
        &snapshot(&mut workspace, "export.batch.query", &handle),
    );
    same_snapshot(&finished, &durable(&handle));
    inputs.unchanged_inputs();
    clean_temporary_files(&inputs.output);
    for index in [1, 2] {
        assert_eq!(
            finished.files[index].status,
            FileStatus::Untouched,
            "file-boundary cancellation must not claim unstarted input"
        );
    }
}
fn during_encode(root: &Path) {
    let mut workspace = Workspace::default();
    let inputs = Inputs::new(root, ExportFormat::Jpeg);
    let handle = inputs.prepare(&mut workspace);
    let second = inputs.manifest.inputs[1].id;
    let mut running = Running::start(&handle, "encode", second.0);
    let parked = wait_at(&mut workspace, &handle, "encode");
    assert_eq!(parked.files[0].status, FileStatus::Done);
    assert_eq!(parked.files[1].input, second);
    assert_eq!(parked.files[1].status, FileStatus::Working);
    let first = inputs.path(0, "jpg");
    let before = fs::read(&first).unwrap();
    let modified = fs::metadata(&first).unwrap().modified().unwrap();
    let temporary = inputs.path(1, "jpg").with_extension("jpg.tmp");
    let encode_bytes = fs::read(&temporary).expect("encode boundary has no in-flight output");
    assert!(!inputs.path(1, "jpg").exists());
    let _ = snapshot(&mut workspace, "export.batch.cancel", &handle);
    let finished = running.finish();
    assert_eq!(finished.status, BatchStatus::Cancelled);
    assert_eq!(finished.files[0].status, FileStatus::Done);
    assert_eq!(finished.files[0].outputs.len(), 1);
    assert!(finished.files[0].outputs[0].complete);
    assert_eq!(finished.files[0].outputs[0].path, first);
    assert_eq!(finished.files[0].outputs[0].bytes, before.len() as u64);
    assert_eq!(fs::read(&first).unwrap(), before);
    assert_eq!(fs::metadata(&first).unwrap().modified().unwrap(), modified);
    assert_eq!(finished.files[1].status, FileStatus::Cancelled);
    assert!(finished.files[1].outputs.is_empty());
    assert_eq!(finished.files[2].status, FileStatus::Untouched);
    assert!(finished.files[2].outputs.is_empty());
    assert!(!inputs.path(1, "jpg").exists());
    assert!(!inputs.path(2, "jpg").exists());
    assert!(!temporary.exists());
    same_snapshot(
        &finished,
        &snapshot(&mut workspace, "export.batch.query", &handle),
    );
    same_snapshot(&finished, &durable(&handle));
    inputs.unchanged_inputs();
    clean_temporary_files(&inputs.output);
    assert!(
        before.starts_with(&[255, 216, 255]),
        "completed JPEG output is a relabelled source, not encoded JPEG"
    );
    assert!(
        encode_bytes.starts_with(&[255, 216, 255]),
        "Encode boundary contains copied PNG, not JPEG encoder bytes"
    );
}
fn partial_failure(root: &Path) {
    let mut workspace = Workspace::default();
    let mut inputs = Inputs::new(root, ExportFormat::Png);
    inputs.manifest.outputs[0].name = "{name}-good.{format}".into();
    let bad = root.join("bad-output");
    fs::create_dir(&bad).unwrap();
    let mut output = inputs.manifest.outputs[0].clone();
    output.dest = Destination::Folder(bad.clone());
    output.name = "{name}-bad.{format}".into();
    inputs.manifest.outputs.push(output);
    let handle = inputs.prepare(&mut workspace);
    let mut running = Running::start(&handle, "encode", inputs.manifest.inputs[0].id.0);
    wait_at(&mut workspace, &handle, "encode");
    let good = inputs.output.join("input-0-good").with_extension("png");
    assert!(
        fs::read(good.with_extension("png.tmp"))
            .unwrap()
            .starts_with(&[137, 80, 78, 71, 13, 10, 26, 10])
    );
    let blocked = bad.join("input-0-bad").with_extension("png");
    fs::create_dir(&blocked).unwrap();
    batch::release_boundary(&handle);
    let finished = running.finish();
    let file = &finished.files[0];
    assert_eq!(file.input, inputs.manifest.inputs[0].id);
    assert_eq!(file.outputs.len(), 1);
    assert!(file.outputs[0].complete);
    assert_eq!(file.outputs[0].path, good);
    assert_eq!(file.outputs[0].bytes, fs::metadata(&good).unwrap().len());
    assert_eq!(file.error.as_deref(), Some("FILE_EXISTS"));
    assert!(blocked.is_dir());
    for index in [1, 2] {
        assert_eq!(finished.files[index].status, FileStatus::Done);
        assert_eq!(finished.files[index].outputs.len(), 2);
        for receipt in &finished.files[index].outputs {
            assert!(receipt.complete);
            assert_eq!(receipt.bytes, fs::metadata(&receipt.path).unwrap().len());
        }
    }
    same_snapshot(
        &finished,
        &snapshot(&mut workspace, "export.batch.query", &handle),
    );
    same_snapshot(&finished, &durable(&handle));
    inputs.unchanged_inputs();
    clean_temporary_files(&inputs.output);
    clean_temporary_files(&bad);
    assert_eq!(
        file.status,
        FileStatus::Partial,
        "a completed output plus failed output must retain a Partial receipt"
    );
    assert!(
        matches!(finished.status, BatchStatus::Done | BatchStatus::Failed),
        "partial-failure batch must reach a terminal state"
    );
}
fn restart_seed(root: &Path) -> ! {
    let mut workspace = Workspace::default();
    let inputs = Inputs::new(root, ExportFormat::Png);
    let handle = inputs.prepare(&mut workspace);
    let _running = Running::start(&handle, "file", inputs.manifest.inputs[1].id.0);
    let parked = wait_at(&mut workspace, &handle, "file");
    completed_png(&parked.files[0], &inputs.path(0, "png"));
    let stored = durable(&handle);
    assert_eq!(stored.status, BatchStatus::Running);
    assert_eq!(stored.files.len(), 3);
    same_snapshot(&parked, &stored);
    fs::write(
        root.join("handle.json"),
        serde_json::to_vec(&handle).unwrap(),
    )
    .unwrap();
    fs::write(root.join("seed-pid.txt"), std::process::id().to_string()).unwrap();
    fs::write(
        root.join("seed-snapshot.json"),
        serde_json::to_vec(&stored).unwrap(),
    )
    .unwrap();
    fs::write(root.join("completed-restart-seed"), b"completed").unwrap();
    std::process::exit(0)
}
fn restart_observe(root: &Path) {
    let pid = fs::read_to_string(root.join("seed-pid.txt")).unwrap();
    assert_ne!(pid, std::process::id().to_string());
    let handle: BatchHandle =
        serde_json::from_slice(&fs::read(root.join("handle.json")).unwrap()).unwrap();
    let before: BatchSnapshot =
        serde_json::from_slice(&fs::read(root.join("seed-snapshot.json")).unwrap()).unwrap();
    let output = root.join("outputs/input-0").with_extension("png");
    let bytes = fs::read(&output).unwrap();
    let modified = fs::metadata(&output).unwrap().modified().unwrap();
    let store_file = BatchStore::default()
        .root
        .join(&handle.0)
        .with_extension("json");
    let receipt_bytes = fs::read(&store_file).unwrap();
    let receipt_modified = fs::metadata(&store_file).unwrap().modified().unwrap();
    let mut workspace = Workspace::default();
    for name in [
        "export.batch.query",
        "export.batch.start",
        "export.batch.start",
        "export.batch.cancel",
        "export.batch.query",
    ] {
        same_snapshot(&before, &snapshot(&mut workspace, name, &handle));
        assert_eq!(fs::read(&output).unwrap(), bytes);
        assert_eq!(fs::metadata(&output).unwrap().modified().unwrap(), modified);
        assert_eq!(fs::read(&store_file).unwrap(), receipt_bytes);
        assert_eq!(
            fs::metadata(&store_file).unwrap().modified().unwrap(),
            receipt_modified
        );
        for index in [1, 2] {
            assert!(
                !root
                    .join("outputs")
                    .join(format!("input-{index}"))
                    .with_extension("png")
                    .exists()
            );
        }
    }
    same_snapshot(&before, &durable(&handle));
    clean_temporary_files(&root.join("outputs"));
}

#[test]
fn batch_acceptance_child() {
    let Ok(phase) = std::env::var(PHASE_ENV) else {
        return;
    };
    let root = fs::canonicalize(PathBuf::from(
        std::env::var_os(ROOT_ENV).expect("missing fixture root"),
    ))
    .unwrap();
    assert_eq!(
        root.parent(),
        Some(fs::canonicalize(std::env::temp_dir()).unwrap().as_path())
    );
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("dtool-b58-")
    );
    assert_eq!(
        fs::read(root.join("fixture.marker")).unwrap(),
        b"dtool-b58-owned-fixture"
    );
    assert_eq!(BatchStore::default().root, root.join("profile/DTool/batch"));
    batch::install_workspace_bridge();
    match phase.as_str() {
        "between" => between_files(&root),
        "encode" => during_encode(&root),
        "partial" => partial_failure(&root),
        "restart-seed" => restart_seed(&root),
        "restart-observe" => restart_observe(&root),
        _ => panic!("unknown acceptance phase"),
    }
    fs::write(root.join(format!("completed-{phase}")), b"completed").unwrap();
}
#[test]
fn cancel_between_files() {
    let root = TemporaryRoot::new();
    run_child(&root.0, "between");
}
#[test]
fn cancel_during_encode() {
    let root = TemporaryRoot::new();
    run_child(&root.0, "encode");
}
#[test]
fn partial_failure_durable_receipt() {
    let root = TemporaryRoot::new();
    run_child(&root.0, "partial");
}
#[test]
fn restart_observe_no_replay() {
    let root = TemporaryRoot::new();
    run_child(&root.0, "restart-seed");
    fs::write(
        root.0.join("outputs/input-0").with_extension("png"),
        b"external replacement must survive restart observation",
    )
    .unwrap();
    for index in 0..3 {
        fs::remove_file(
            root.0
                .join("inputs")
                .join(format!("input-{index}"))
                .with_extension("png"),
        )
        .unwrap();
    }
    run_child(&root.0, "restart-observe");
}
