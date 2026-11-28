//! Local ComfyUI support. Network and disk operations are invoked by background UI workers.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn checksum_matches(actual: &[u8], expected: &str) -> bool {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    expected.len() == actual.len() * 2
        && actual.iter().zip(expected.as_bytes().chunks_exact(2)).all(|(&byte, pair)| {
            HEX[(byte >> 4) as usize].eq_ignore_ascii_case(&pair[0])
                && HEX[(byte & 0x0f) as usize].eq_ignore_ascii_case(&pair[1])
        })
}

pub const RUNTIME_VERSION: &str = "v0.45.0";
pub const STABLE_PORT: u16 = 18188;
pub const LAB_PORT: u16 = 18189;
const ARCHIVE: &str = "ComfyUI_windows_portable_nvidia.7z";
const MAX_MODEL: u64 = 48 * 1024 * 1024 * 1024;
type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Stable,
    Lab,
}

impl Environment {
    fn slug(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Lab => "lab",
        }
    }

    pub fn port(self) -> u16 {
        match self {
            Self::Stable => STABLE_PORT,
            Self::Lab => LAB_PORT,
        }
    }
}

fn profile_root() -> Result<PathBuf> {
    let profile = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "LOCALAPPDATA 위치를 찾을 수 없습니다.".to_owned())?;
    Ok(PathBuf::from(profile).join("DTool").join("AI"))
}

fn saved_path(root: &Path, config: &str) -> Result<Option<PathBuf>> {
    match File::open(root.join(config)) {
        Ok(file) => serde_json::from_reader(file).map(Some).map_err(err),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(err(error)),
    }
}

pub fn root() -> Result<PathBuf> {
    let profile = profile_root()?;
    Ok(saved_path(&profile, "install-directory.json")?.unwrap_or(profile))
}

pub fn storage_configured() -> bool {
    profile_root().is_ok_and(|profile| {
        profile.join("install-directory.json").is_file()
            || profile.join("model-directory.json").is_file()
    })
}

pub fn runtime(env: Environment) -> Result<PathBuf> {
    Ok(root()?.join(env.slug()).join(RUNTIME_VERSION))
}

pub fn models_path() -> Result<PathBuf> {
    let profile = profile_root()?;
    match saved_path(&profile, "model-directory.json")? {
        Some(path) => Ok(path),
        None => Ok(root()?.join("models")),
    }
}

fn save_directory(config: &str, directory: &Path) -> Result<()> {
    fs::create_dir_all(directory).map_err(err)?;
    let directory = directory.canonicalize().map_err(err)?;
    // Verify that downloads can create their temporary files at the chosen destination.
    let _probe = tempfile::NamedTempFile::new_in(&directory).map_err(err)?;
    let profile = profile_root()?;
    fs::create_dir_all(&profile).map_err(err)?;
    let temporary = tempfile::NamedTempFile::new_in(&profile).map_err(err)?;
    serde_json::to_writer(temporary.as_file(), &directory).map_err(err)?;
    temporary.as_file().sync_all().map_err(err)?;
    temporary.persist(profile.join(config)).map(|_| ()).map_err(err)
}

pub fn set_install_parent(parent: &Path) -> Result<()> {
    save_directory("install-directory.json", &parent.join("DTool ComfyUI"))
}

pub fn set_models_folder(directory: &Path) -> Result<()> {
    save_directory("model-directory.json", directory)
}

pub fn list_installed() -> Result<Vec<String>> {
    let root = models_path()?;
    let mut result = Vec::new();
    for category in CATEGORIES {
        let Ok(repositories) = fs::read_dir(root.join(category)) else {
            continue;
        };
        for author in repositories
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        {
            let Ok(revisions) = fs::read_dir(author.path()) else {
                continue;
            };
            for revision in revisions
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            {
                let rev = revision.file_name();
                let rev = rev.to_string_lossy();
                if rev.len() != 40 || !rev.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    continue;
                }
                let Ok(files) = fs::read_dir(revision.path()) else {
                    continue;
                };
                for file in files
                    .flatten()
                    .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                {
                    let name = file.file_name();
                    let name = name.to_string_lossy();
                    if name.ends_with(".safetensors") {
                        let relative = format!(
                            "{category}/{}/{rev}/{name}",
                            author.file_name().to_string_lossy()
                        );
                        if safe_relative(&relative) {
                            result.push(relative);
                        }
                    }
                }
            }
        }
    }
    result.sort_unstable();
    Ok(result)
}

pub fn remove_installed(relative: &str) -> Result<()> {
    if !safe_relative(relative) || !list_installed()?.iter().any(|path| path == relative) {
        return Err("디툴에서 받은 모델 파일을 선택하세요.".into());
    }
    let root = models_path()?.canonicalize().map_err(err)?;
    let target = root.join(relative).canonicalize().map_err(err)?;
    if !target.starts_with(&root) {
        return Err("모델 저장 폴더 밖의 파일입니다.".into());
    }
    fs::remove_file(target).map_err(err)
}

pub const CATEGORIES: [&str; 8] = [
    "checkpoints",
    "diffusion_models",
    "text_encoders",
    "vae",
    "loras",
    "upscale_models",
    "background_removal",
    "geometry_estimation",
];

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

pub(super) fn http() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(8)
        .redirect_auth_headers(ureq::RedirectAuthHeaders::Never)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(90))
        .build()
}

fn data(request: ureq::Request) -> Result<Value> {
    let response = request
        .set("User-Agent", "DTool-AI")
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(401 | 403, _) => {
                "접근 권한이 필요합니다. 게시자 페이지에서 이용 조건과 접근 승인을 확인하세요."
                    .into()
            }
            other => err(other),
        })?;
    serde_json::from_reader(response.into_reader().take(16 * 1024 * 1024)).map_err(err)
}

fn safe_segment(s: &str) -> bool {
    let base = s.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let device = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            base.strip_prefix(prefix).is_some_and(|port| {
                port.len() == 1 && port.bytes().all(|byte| byte.is_ascii_digit())
            })
        });
    !device
        && !s.is_empty()
        && s.len() <= 150
        && !s.starts_with('.')
        && !s.ends_with('.')
        && s.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn safe_relative(s: &str) -> bool {
    !s.is_empty() && s.split('/').all(safe_segment)
}

fn sha(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub enum InstallProgress {
    Step(&'static str),
    Download { name: String, file: Option<(usize, usize)>, received: u64, total: u64 },
    Verify { name: String, file: Option<(usize, usize)> },
}

pub(super) fn download(
    agent: &ureq::Agent,
    url: &str,
    path: &Path,
    size: u64,
    limit: u64,
    digest: Option<&str>,
) -> Result<()> {
    if !url.starts_with("https://") {
        return Err("다운로드 정보가 올바르지 않습니다.".into());
    }
    download_request(
        agent.get(url), path, size, limit, digest,
        "게시자 페이지에서 접근 승인이 필요합니다.", &mut |_| {},
    )
}

fn download_request(
    request: ureq::Request,
    path: &Path,
    size: u64,
    limit: u64,
    digest: Option<&str>,
    access_error: &str,
    transferred: &mut dyn FnMut(u64),
) -> Result<()> {
    if size > limit || digest.is_some_and(|value| !sha(value)) {
        return Err("다운로드 정보가 올바르지 않습니다.".into());
    }
    transferred(0);
    let response = request
        .set("User-Agent", "DTool-AI")
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(401 | 403, _) => access_error.to_owned(),
            other => err(other),
        })?;
    let mut reader = response.into_reader().take(limit + 1);
    let mut file = File::options()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(err)?;
    let mut hash = Sha256::new();
    let mut count = 0;
    let mut buf = vec![0; 256 * 1024];
    let mut updated = Instant::now();
    loop {
        let read = reader.read(&mut buf).map_err(err)?;
        if read == 0 {
            break;
        }
        count += read as u64;
        if count > limit {
            return Err("다운로드 용량이 허용 범위를 넘었습니다.".into());
        }
        hash.update(&buf[..read]);
        file.write_all(&buf[..read]).map_err(err)?;
        if updated.elapsed() >= Duration::from_millis(150) {
            transferred(count);
            updated = Instant::now();
        }
    }
    transferred(count);
    if count == 0 || size != 0 && count != size {
        return Err("다운로드 파일 크기가 배포 정보와 일치하지 않습니다.".into());
    }
    if digest
        .is_some_and(|expected| !checksum_matches(&hash.finalize(), expected))
    {
        return Err("다운로드 파일의 SHA-256 검사가 실패했습니다.".into());
    }
    file.sync_all().map_err(err)
}

pub(super) fn hidden(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn portable_present(dir: &Path) -> bool {
    dir.join("python_embeded/python.exe").is_file() && dir.join("ComfyUI/main.py").is_file()
}

pub fn runtime_installed(env: Environment) -> bool {
    runtime(env).is_ok_and(|dir| portable_present(&dir))
}

pub fn install_runtime(env: Environment, progress: &mut dyn FnMut(InstallProgress)) -> Result<()> {
    progress(InstallProgress::Step("공식 ComfyUI 패키지 확인 중"));
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err("이 패키지는 Windows x64 NVIDIA용입니다.".into());
    }
    let dest = runtime(env)?;
    if portable_present(&dest) {
        return Ok(());
    }
    if dest.exists() {
        return Err("런타임 폴더가 이미 있습니다. 설치 로그와 폴더를 확인하세요.".into());
    }
    let parent = dest
        .parent()
        .ok_or_else(|| "런타임 폴더를 찾을 수 없습니다.".to_owned())?;
    fs::create_dir_all(parent).map_err(err)?;
    let work = tempfile::tempdir_in(parent).map_err(err)?;
    let agent = http();
    let release = data(agent.get(&format!(
        "https://api.github.com/repos/Comfy-Org/ComfyUI/releases/tags/{RUNTIME_VERSION}"
    )))?;
    let asset = release["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|entry| entry["name"] == ARCHIVE))
        .ok_or_else(|| "공식 릴리스에서 NVIDIA 패키지를 찾을 수 없습니다.".to_owned())?;
    let digest = asset["digest"]
        .as_str()
        .and_then(|value| value.strip_prefix("sha256:"))
        .ok_or_else(|| "공식 릴리스의 SHA-256을 확인할 수 없습니다.".to_owned())?;
    let size = asset["size"]
        .as_u64()
        .filter(|size| *size > 0)
        .ok_or_else(|| "배포 파일 크기를 확인할 수 없습니다.".to_owned())?;
    let archive = work.path().join(ARCHIVE);
    let url = format!(
        "https://github.com/Comfy-Org/ComfyUI/releases/download/{RUNTIME_VERSION}/{ARCHIVE}"
    );
    download_request(
        agent.get(&url), &archive, size, 12 * 1024 * 1024 * 1024, Some(digest),
        "공식 ComfyUI 패키지에 접근할 수 없습니다.",
        &mut |received| progress(InstallProgress::Download {
            name: ARCHIVE.into(), file: None, received, total: size,
        }),
    )?;
    progress(InstallProgress::Step("다운로드 확인 완료 · ComfyUI 압축 해제 중"));
    let extract = work.path().join("extract");
    fs::create_dir(&extract).map_err(err)?;
    let windows = std::env::var_os("SystemRoot")
        .filter(|path| !path.is_empty())
        .ok_or_else(|| "Windows 시스템 폴더를 확인할 수 없습니다.".to_owned())?;
    let tar = PathBuf::from(windows).join("System32").join("tar.exe");
    let result = hidden(
        Command::new(tar)
            .args(["-xf"])
            .arg(&archive)
            .arg("-C")
            .arg(&extract),
    )
    .output()
    .map_err(err)?;
    if !result.status.success() {
        return Err(format!("공식 패키지 압축 해제 실패 ({})", result.status));
    }
    let portable = extract.join("ComfyUI_windows_portable");
    if !portable_present(&portable) {
        return Err("ComfyUI 실행 파일 또는 내장 Python을 찾을 수 없습니다.".into());
    }
    fs::rename(portable, dest).map_err(err)
}

pub fn start_runtime(env: Environment) -> Result<Child> {
    let dir = runtime(env)?;
    if !portable_present(&dir) {
        return Err("ComfyUI를 먼저 설치하세요.".into());
    }
    std::net::TcpListener::bind(("127.0.0.1", env.port()))
        .map_err(|_| "ComfyUI 포트가 사용 중입니다.".to_owned())?;
    let models = models_path()?;
    for category in CATEGORIES {
        fs::create_dir_all(models.join(category)).map_err(err)?;
    }
    let mut paths = serde_json::json!({ "dtool": { "base_path": models, "is_default": true } });
    for category in CATEGORIES {
        paths["dtool"][category] = Value::String(category.into());
    }
    let config = dir.join("dtool-model-paths.yaml");
    fs::write(&config, paths.to_string()).map_err(err)?;
    let log = File::create(dir.join("dtool-comfy.log")).map_err(err)?;
    let stderr = log.try_clone().map_err(err)?;
    let mut command = Command::new(dir.join("python_embeded/python.exe"));
    command.current_dir(&dir);
    if env == Environment::Lab {
        const LAB_BOOTSTRAP: &str = r#"
import pathlib, runpy, sys
comfy = pathlib.Path('ComfyUI').resolve()
sys.path.insert(0, str(comfy))
for vendor in sorted((comfy / 'custom_nodes').glob('*/.dtool-python')):
    sys.path.append(str(vendor))
sys.argv[0] = str(comfy / 'main.py')
runpy.run_path(sys.argv[0], run_name='__main__')
"#;
        command.args(["-s", "-c", LAB_BOOTSTRAP]);
    } else {
        command.args(["-s", "ComfyUI/main.py"]);
    }
    hidden(
        command
            .args([
                "--windows-standalone-build",
                "--disable-auto-launch",
                "--listen",
                "127.0.0.1",
                "--port",
            ])
            .arg(env.port().to_string())
            .arg("--extra-model-paths-config")
            .arg(config)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(stderr),
    )
    .spawn()
    .map_err(err)
}

pub fn ready(port: u16) -> bool {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(3))
        .build();
    data(agent.get(&format!("http://127.0.0.1:{port}/system_stats")))
        .is_ok_and(|reply| reply["system"].is_object())
}

#[derive(Clone)]
pub struct Model {
    pub repository: String,
    pub downloads: u64,
    pub gated: bool,
}

pub fn search_models(query: &str, popular: bool, edit: bool) -> Result<Vec<Model>> {
    let agent = http();
    let sort = if popular { "downloads" } else { "createdAt" };
    let task = if edit {
        "image-to-image"
    } else {
        "text-to-image"
    };
    let reply = data(
        agent
            .get("https://huggingface.co/api/models")
            .query("search", query.trim())
            .query("pipeline_tag", task)
            .query("expand[]", "gated")
            .query("expand[]", "downloads")
            .query("sort", sort)
            .query("direction", "-1")
            .query("limit", "30"),
    )?;
    let rows = reply
        .as_array()
        .ok_or_else(|| "모델 목록 응답이 올바르지 않습니다.".to_owned())?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let repository = row["id"]
                .as_str()
                .filter(|id| repository_id(id))?
                .to_owned();
            Some(Model {
                repository,
                downloads: row["downloads"].as_u64().unwrap_or(0),
                gated: row["gated"]
                    .as_bool()
                    .unwrap_or_else(|| row["gated"].as_str().is_some_and(|item| item != "false")),
            })
        })
        .collect())
}

fn repository_id(s: &str) -> bool {
    s.split_once('/')
        .is_some_and(|(author, name)| safe_segment(author) && safe_segment(name))
}

#[derive(Clone)]
pub struct ModelFile {
    pub name: String,
    pub size: u64,
    pub checksum: String,
}

#[derive(Clone)]
pub struct ModelFiles {
    pub repository: String,
    pub revision: String,
    pub files: Vec<ModelFile>,
    pub license: Option<String>,
}

pub fn model_files(repo: &str) -> Result<ModelFiles> {
    model_files_at(repo, None)
}

fn model_files_at(repo: &str, revision: Option<&str>) -> Result<ModelFiles> {
    if !repository_id(repo) {
        return Err("Hugging Face 저장소 이름을 확인하세요.".into());
    }
    let url = format!("https://huggingface.co/api/models/{repo}");
    let url = match revision {
        Some(revision) => format!("{url}/revision/{revision}"),
        None => url,
    };
    let reply = data(http().get(&url).query("blobs", "true"))?;
    let revision = reply["sha"]
        .as_str()
        .filter(|id| id.len() == 40 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| "모델 파일의 배포 리비전을 확인할 수 없습니다.".to_owned())?;
    let files = reply["siblings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| {
            let name = file["rfilename"]
                .as_str()
                .filter(|name| safe_relative(name) && name.ends_with(".safetensors"))?;
            let size = file["size"]
                .as_u64()
                .filter(|size| (1..=MAX_MODEL).contains(size))?;
            let checksum = file["lfs"]["sha256"].as_str().filter(|value| sha(value))?;
            Some(ModelFile {
                name: name.into(),
                size,
                checksum: checksum.into(),
            })
        })
        .collect();
    Ok(ModelFiles {
        repository: repo.into(),
        revision: revision.into(),
        files,
        license: reply["cardData"]["license"].as_str().map(str::to_owned),
    })
}

pub const QWEN_REPOSITORY: &str = "Comfy-Org/Qwen-Image-2.1";
pub const BIREFNET_REPOSITORY: &str = "Comfy-Org/BiRefNet";
pub const DEPTH_REPOSITORY: &str = "Comfy-Org/Depth-Anything-3";

pub fn install_birefnet() -> Result<String> {
    const REVISION: &str = "884af709e9b795d89dafbf97f29c8d6a2386f5c6";
    let model = model_files_at(BIREFNET_REPOSITORY, Some(REVISION))?;
    let file = model.files.iter().find(|file| file.name == "background_removal/birefnet.safetensors")
        .ok_or_else(|| "BiRefNet 배포에서 모델 파일을 찾을 수 없습니다.".to_owned())?;
    download_model(&model, file, "background_removal")
}

pub fn install_depth() -> Result<String> {
    const REVISION: &str = "8265be070134dc7459217308cdc03109bde199b0";
    let model = model_files_at(DEPTH_REPOSITORY, Some(REVISION))?;
    let file = model.files.iter().find(|file| file.name == "geometry_estimation/depth_anything_3_small.safetensors")
        .ok_or_else(|| "Depth Anything 3 Small 배포에서 모델 파일을 찾을 수 없습니다.".to_owned())?;
    download_model(&model, file, "geometry_estimation")
}

fn install_model_bundle(
    repository: &str,
    revision: &str,
    files: &[&str],
    progress: &mut dyn FnMut(InstallProgress),
) -> Result<Vec<String>> {
    progress(InstallProgress::Step("공식 모델 파일 목록 확인 중"));
    let model = model_files_at(repository, Some(revision))?;
    let files = files.iter().map(|name| {
        model.files.iter().find(|file| file.name == *name)
            .ok_or_else(|| format!("{repository} 배포에서 {name} 파일을 찾을 수 없습니다."))
    }).collect::<Result<Vec<_>>>()?;
    for (index, file) in files.iter().enumerate() {
        let category = file.name.split('/').next().unwrap_or_default();
        download_model_file(&model, file, category, Some((index + 1, files.len())), progress)?;
    }
    list_installed()
}

pub fn install_qwen_turbo(progress: &mut dyn FnMut(InstallProgress)) -> Result<Vec<String>> {
    install_model_bundle(
        QWEN_REPOSITORY,
        "aacf31a6d48e707f352ecba714cee0680da3479b",
        &[
            "text_encoders/qwen3vl_8b_int8_convrot.safetensors",
            "vae/qwen_image_2.1_vae_bf16.safetensors",
            "diffusion_models/qwen_image_2.1_turbo_int8_convrot.safetensors",
        ],
        progress,
    )
}

pub fn install_flux3_klein(progress: &mut dyn FnMut(InstallProgress)) -> Result<Vec<String>> {
    install_model_bundle(
        super::flux3::REPOSITORY,
        "09192d1abae1e57e1a2fcbc5b701cd1d9e2abd51",
        &[
            "vae/flux2-vae.safetensors",
            "text_encoders/qwen3vl_4b_int8_convrot.safetensors",
            "text_encoders/qwen3.5_9b_flux3_prompt_upsampler_w4a8.safetensors",
            "diffusion_models/flux3-image-klein-int8-convrot.safetensors",
        ],
        progress,
    )
}

pub fn download_model(model: &ModelFiles, file: &ModelFile, category: &str) -> Result<String> {
    download_model_file(model, file, category, None, &mut |_| {})
}

pub fn download_model_with_progress(
    model: &ModelFiles, file: &ModelFile, category: &str, progress: &mut dyn FnMut(InstallProgress),
) -> Result<String> {
    download_model_file(model, file, category, None, progress)
}

fn download_model_file(
    model: &ModelFiles, file: &ModelFile, category: &str, position: Option<(usize, usize)>,
    progress: &mut dyn FnMut(InstallProgress),
) -> Result<String> {
    if !repository_id(&model.repository)
        || !safe_relative(&file.name)
        || !file.name.ends_with(".safetensors")
        || model.revision.len() != 40
        || !model.revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !CATEGORIES.contains(&category)
    {
        return Err("모델 파일 정보가 올바르지 않습니다.".into());
    }
    let source = model.repository.replace('/', "--");
    let relative = format!(
        "{source}/{}/{}",
        model.revision,
        file.name.rsplit('/').next().unwrap_or_default()
    );
    let destination = models_path()?.join(category).join(&relative);
    if destination.exists() {
        progress(InstallProgress::Verify { name: file.name.clone(), file: position });
        let mut existing = File::open(&destination).map_err(err)?;
        if existing.metadata().map_err(err)?.len() != file.size {
            return Err("같은 위치의 파일이 배포 정보와 일치하지 않습니다.".into());
        }
        let mut hash = Sha256::new();
        let mut buffer = vec![0; 256 * 1024];
        loop {
            let count = existing.read(&mut buffer).map_err(err)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        if !checksum_matches(&hash.finalize(), &file.checksum) {
            return Err("같은 위치의 파일이 배포 정보와 일치하지 않습니다.".into());
        }
        return Ok(format!("{category}/{relative}"));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| "모델 폴더를 찾을 수 없습니다.".to_owned())?;
    fs::create_dir_all(parent).map_err(err)?;
    let staging = tempfile::tempdir_in(parent).map_err(err)?;
    let part = staging.path().join("model.part");
    let url = format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        model.repository, model.revision, file.name
    );
    let mut request = http().get(&url);
    let access_error = match std::env::var("HF_TOKEN") {
        Ok(token) if !token.trim().is_empty() => {
            let token = token.trim();
            if !token.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')) {
                return Err("HF_TOKEN의 형식을 확인하고 디툴을 다시 시작하세요.".into());
            }
            request = request.set("Authorization", &format!("Bearer {token}"));
            "HF_TOKEN 계정의 모델 접근 승인과 토큰의 읽기 권한을 확인하세요."
        }
        _ => "게시자 페이지에서 접근을 승인한 계정의 읽기 토큰을 HF_TOKEN으로 설정한 뒤 디툴을 다시 시작하세요.",
    };
    download_request(request, &part, file.size, MAX_MODEL, Some(&file.checksum), access_error,
        &mut |received| progress(InstallProgress::Download {
            name: file.name.clone(), file: position, received, total: file.size,
        }),
    )?;
    progress(InstallProgress::Verify { name: file.name.clone(), file: position });
    fs::hard_link(&part, &destination).map_err(err)?;
    Ok(format!("{category}/{relative}"))
}

#[derive(Clone)]
pub struct NodePlan {
    pub id: String,
    pub version: String,
    pub dependencies: Vec<String>,
    url: String,
}

pub fn node_plan(id: &str, version: &str) -> Result<NodePlan> {
    if !safe_segment(id) || !safe_segment(version) {
        return Err("Registry 노드 ID와 정확한 버전을 입력하세요.".into());
    }
    let values = data(http().get(&format!("https://api.comfy.org/nodes/{id}/versions")))?;
    let item = values
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["version"] == version))
        .ok_or_else(|| "Registry에서 지정한 버전을 찾을 수 없습니다.".to_owned())?;
    let active = item["status"].as_str().is_some_and(|status| {
        status.eq_ignore_ascii_case("active")
            || status.eq_ignore_ascii_case("NodeVersionStatusActive")
    });
    if !active || item["deprecated"].as_bool() == Some(true) {
        return Err("Registry에서 활성 상태인 노드 버전이 필요합니다.".into());
    }
    let url = item["downloadUrl"]
        .as_str()
        .filter(|url| {
            url.starts_with("https://cdn.comfy.org/")
                || url.starts_with("https://storage.googleapis.com/comfy-registry/")
        })
        .ok_or_else(|| "Registry 배포 URL을 확인할 수 없습니다.".to_owned())?;
    if !url.ends_with(".zip") {
        return Err("Registry에서 ZIP으로 배포한 노드 버전만 설치할 수 있습니다.".into());
    }
    let dependencies = item["dependencies"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "노드 의존성 정보가 올바르지 않습니다.".to_owned())
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(NodePlan {
        id: id.into(),
        version: version.into(),
        dependencies,
        url: url.into(),
    })
}

pub fn install_node(plan: &NodePlan) -> Result<()> {
    let dir = runtime(Environment::Lab)?;
    if !portable_present(&dir) {
        return Err("실험 환경을 먼저 준비하세요.".into());
    }
    if std::net::TcpListener::bind(("127.0.0.1", LAB_PORT)).is_err() {
        return Err("노드를 설치하기 전에 실험 환경을 종료하세요.".into());
    }
    let nodes = dir.join("ComfyUI/custom_nodes");
    fs::create_dir_all(&nodes).map_err(err)?;
    let dest = nodes.join(&plan.id);
    if dest.exists() {
        return Err("같은 ID의 노드가 이미 있습니다.".into());
    }
    let temp = tempfile::tempdir_in(
        nodes
            .parent()
            .ok_or_else(|| "노드 폴더를 찾을 수 없습니다.".to_owned())?,
    )
    .map_err(err)?;
    let zip = temp.path().join("node.zip");
    download(&http(), &plan.url, &zip, 0, 512 * 1024 * 1024, None)?;
    let mut archive = zip::ZipArchive::new(File::open(zip).map_err(err)?).map_err(err)?;
    let payload = temp.path().join("payload");
    fs::create_dir(&payload).map_err(err)?;
    let mut bytes = 0u64;
    for i in 0..archive.len() {
        let mut item = archive.by_index(i).map_err(err)?;
        let path = item
            .enclosed_name()
            .ok_or_else(|| "노드에 안전하지 않은 파일 경로가 있습니다.".to_owned())?;
        if matches!(
            path.components().next(),
            Some(std::path::Component::Normal(name))
                if matches!(name.to_str(), Some(".github" | ".gitignore" | ".gitattributes"))
        ) {
            continue;
        }
        if path.components().count() == 0 || path.components().any(|part| {
            !matches!(part, std::path::Component::Normal(value) if value.to_str().is_some_and(safe_segment))
        }) || item.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000) {
            return Err("노드에 안전하지 않은 파일이 있습니다.".into());
        }
        let count = item.size();
        bytes = bytes.saturating_add(count);
        if bytes > 1024 * 1024 * 1024 {
            return Err("노드 압축 해제 용량을 초과했습니다.".into());
        }
        let output = payload.join(path);
        if item.is_dir() {
            fs::create_dir_all(output).map_err(err)?;
        } else {
            fs::create_dir_all(
                output
                    .parent()
                    .ok_or_else(|| "노드 경로가 잘못되었습니다.".to_owned())?,
            )
            .map_err(err)?;
            let mut file = File::options()
                .create_new(true)
                .write(true)
                .open(&output)
                .map_err(err)?;
            if io::copy(&mut item.by_ref().take(count + 1), &mut file).map_err(err)? != count {
                return Err("노드 파일 크기가 맞지 않습니다.".into());
            }
        }
    }
    if !payload.join("__init__.py").is_file() {
        return Err("Registry 노드의 패키지 구조를 확인하세요.".into());
    }
    // Install only wheels into a new local vendor directory. Never run a package's arbitrary install script.
    if !plan.dependencies.is_empty() {
        if plan
            .dependencies
            .iter()
            .any(|spec| !wheel_requirement(spec))
        {
            return Err("고정 버전의 PyPI 휠로 설치할 수 없는 의존성이 있습니다. 실험 환경 안내서를 참고하세요.".into());
        }
        let wheel_dir = payload.join(".dtool-python");
        let result = hidden(
            Command::new(dir.join("python_embeded/python.exe"))
                .args([
                    "-I",
                    "-m",
                    "pip",
                    "--isolated",
                    "install",
                    "--only-binary=:all:",
                    "--no-deps",
                    "--index-url",
                    "https://pypi.org/simple",
                    "--target",
                ])
                .arg(&wheel_dir)
                .args(&plan.dependencies),
        )
        .output()
        .map_err(err)?;
        if !result.status.success() {
            return Err(format!("노드 의존성 설치 실패 ({})", result.status));
        }
    }
    fs::rename(payload, dest).map_err(err)
}

fn wheel_requirement(s: &str) -> bool {
    s.split_once("==").is_some_and(|(name, version)| {
        safe_segment(name)
            && safe_segment(version)
            && !["torch", "torchvision", "numpy", "comfyui"]
                .contains(&name.to_ascii_lowercase().as_str())
    })
}
