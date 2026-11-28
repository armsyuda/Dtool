use super::workspace::{ACCENT, workspace_button};
use dtool_core::ai_image::setup::{self, CATEGORIES, Environment, InstallProgress, Model, ModelFiles, NodePlan};
use dtool_core::ai_image::upscale;
use dtool_design::{ControlState, button, button_with, tab, toggle};
use std::process::Child;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

enum Reply {
    Started(Environment, Child),
    Models(Vec<Model>),
    Files(ModelFiles),
    ModelInstalled(String),
    BundleInstalled(Vec<String>, &'static str),
    UpscaleInstalled,
    NodePlan(NodePlan),
    NodeInstalled,
    Probe(u16, bool),
    LabInstalled,
    InstalledList(Vec<String>),
    StorageChanged(Vec<String>),
}

pub(super) struct SetupPanel {
    open: bool,
    page: usize,
    first_visit: bool,
    query: String,
    popular: bool,
    results: Vec<Model>,
    files: Option<ModelFiles>,
    file_index: usize,
    category: usize,
    node_id: String,
    node_version: String,
    node: Option<NodePlan>,
    approve: bool,
    installed: Vec<String>,
    pending_remove: Option<String>,
    worker: Option<Receiver<Result<Reply, String>>>,
    progress_rx: Option<Receiver<InstallProgress>>,
    progress: Option<InstallProgress>,
    message: Option<String>,
    child: Option<(Environment, Child)>,
    ready: bool,
    probing: Option<Receiver<bool>>,
    next_probe: Instant,
    started: Option<Instant>,
    pub(super) upscale_ready: bool,
    upscale_installing: bool,
    upscale_error: Option<String>,
}

fn file_category(name: &str) -> usize {
    name.split_once('/')
        .and_then(|(dir, _)| CATEGORIES.iter().position(|category| *category == dir))
        .unwrap_or_default()
}

fn progress_caption(stage: &str, name: &str, file: Option<(usize, usize)>) -> String {
    let name = name.rsplit('/').next().unwrap_or(name);
    match file {
        Some((index, total)) => format!("{stage} · 파일 {index}/{total} · {name}"),
        None => format!("{stage} · {name}"),
    }
}

fn visible_path(path: &std::path::Path) -> String {
    let text = path.to_string_lossy();
    if let Some(network) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{network}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    }
}

fn transfer_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB", bytes as f64 / 1_000_000_000.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    }
}

impl Default for SetupPanel {
    fn default() -> Self {
        Self {
            open: false,
            page: 0,
            first_visit: true,
            query: String::new(),
            popular: true,
            results: Vec::new(),
            files: None,
            file_index: 0,
            category: 0,
            node_id: String::new(),
            node_version: String::new(),
            node: None,
            approve: false,
            installed: Vec::new(),
            pending_remove: None,
            worker: None,
            progress_rx: None,
            progress: None,
            message: None,
            child: None,
            ready: false,
            probing: None,
            next_probe: Instant::now(),
            started: None,
            upscale_ready: upscale::installed(),
            upscale_installing: false,
            upscale_error: None,
        }
    }
}

impl SetupPanel {
    pub(super) fn upscale_install(&mut self, ui: &mut egui::Ui) {
        if self.upscale_ready {
            return;
        }
        if !upscale::supported() {
            ui.label(dtool_strings::text("ai.image.upscale_platform"));
            return;
        }
        if button_with(ui, dtool_strings::text("ai.image.upscale_install"), &ControlState::enabled(self.worker.is_none())).clicked() {
            self.upscale_installing = true;
            self.upscale_error = None;
            self.work(ui.ctx(), || upscale::install().map(|()| Reply::UpscaleInstalled));
        }
        if self.upscale_installing {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(dtool_strings::text("ai.image.upscale_installing"));
            });
        }
        if let Some(error) = &self.upscale_error {
            ui.label(error);
        }
    }

    fn work(
        &mut self,
        ctx: &egui::Context,
        operation: impl FnOnce() -> Result<Reply, String> + Send + 'static,
    ) {
        if self.worker.is_some() {
            return;
        }
        self.message = None;
        let (send, receive) = mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let result = operation();
            if let Err(mpsc::SendError(Ok(Reply::Started(_, mut child)))) = send.send(result) {
                let _ = child.kill();
                let _ = child.wait();
            }
            repaint.request_repaint();
        });
        self.worker = Some(receive);
    }

    fn progress_work(
        &mut self, ctx: &egui::Context,
        operation: impl FnOnce(&mut dyn FnMut(InstallProgress)) -> Result<Reply, String> + Send + 'static,
    ) {
        if self.worker.is_some() {
            return;
        }
        let (send, receive) = mpsc::channel();
        let repaint = ctx.clone();
        self.progress_rx = Some(receive);
        self.work(ctx, move || operation(&mut |progress| {
            let _ = send.send(progress);
            repaint.request_repaint();
        }));
    }

    fn start(&mut self, ctx: &egui::Context, env: Environment) {
        if self.child.is_some() {
            return;
        }
        self.progress_work(ctx, move |progress| {
            setup::install_runtime(env, progress)?;
            progress(InstallProgress::Step("ComfyUI 서버 시작 중"));
            setup::start_runtime(env).map(|child| Reply::Started(env, child))
        });
    }

    pub(super) fn poll(&mut self, ctx: &egui::Context, port: &mut f64) {
        if let Some(progress) = &self.progress_rx
            && let Some(latest) = progress.try_iter().last()
        {
            self.progress = Some(latest);
        }
        if let Some(worker) = &self.worker {
            let result = match worker.try_recv() {
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err("설치 작업이 중단되었습니다.".into())),
                Ok(result) => Some(result),
            };
            if let Some(result) = result {
                self.worker = None;
                self.progress_rx = None;
                self.progress = None;
                if std::mem::take(&mut self.upscale_installing) {
                    self.upscale_error = result.as_ref().err().cloned();
                }
                match result {
                    Ok(Reply::Started(env, child)) => {
                        self.child = Some((env, child));
                        *port = f64::from(env.port());
                        self.ready = false;
                        self.started = Some(Instant::now());
                        self.next_probe = Instant::now();
                    }
                    Ok(Reply::Models(models)) => self.results = models,
                    Ok(Reply::InstalledList(paths)) => self.installed = paths,
                    Ok(Reply::StorageChanged(paths)) => {
                        self.installed = paths;
                        self.upscale_ready = upscale::installed();
                        self.message = Some("저장 위치를 적용했습니다. 다음 다운로드부터 이 위치를 사용합니다.".into());
                    }
                    Ok(Reply::Files(files)) => {
                        self.file_index = 0;
                        self.category = files.files.first().map_or(0, |file| file_category(&file.name));
                        self.files = Some(files);
                    }
                    Ok(Reply::ModelInstalled(path)) => {
                        let depth = path.starts_with("geometry_estimation/");
                        if !self.installed.contains(&path) {
                            self.installed.push(path);
                        }
                        self.message = Some(
                            if depth { dtool_strings::text("ai.image.depth_model_ready") }
                            else { "모델 파일을 저장했습니다. 체크포인트는 목록 새로고침으로 불러오세요." }.into(),
                        );
                    }
                    Ok(Reply::BundleInstalled(paths, model)) => {
                        self.installed = paths;
                        self.message = Some(format!("{model} 준비 완료. 생성·편집 옵션에서 모델 목록을 새로고침하세요."));
                    }
                    Ok(Reply::UpscaleInstalled) => self.upscale_ready = true,
                    Ok(Reply::NodePlan(node)) => {
                        self.node = Some(node);
                        self.approve = false;
                    }
                    Ok(Reply::NodeInstalled) => {
                        self.node = None;
                        self.approve = false;
                        self.message = Some(
                            "실험 환경에 설치했습니다. 다음 실행에서 노드 로드 상태를 확인하세요."
                                .into(),
                        );
                    }
                    Err(error) => self.message = Some(error),
                    Ok(Reply::LabInstalled) => {
                        self.message = Some("실험 환경을 준비했습니다.".into())
                    }
                    Ok(Reply::Probe(probed_port, connected)) if probed_port == *port as u16 => {
                        self.ready = connected;
                        self.message = Some(if connected {
                            "기존 ComfyUI에 연결했습니다. 파일을 받으려면 모델 폴더를 확인하세요."
                        } else {
                            "외부 ComfyUI에 연결할 수 없습니다. ComfyUI가 실행 중인지, 포트가 맞는지 확인하세요."
                        }.into());
                    }
                    Ok(Reply::Probe(_, _)) => {}
                }
            }
        }
        if self
            .child
            .as_mut()
            .is_some_and(|(_, child)| child.try_wait().ok().flatten().is_some())
        {
            self.child = None;
            self.probing = None;
            self.started = None;
            self.ready = false;
            self.message =
                Some("ComfyUI가 종료됐습니다. 런타임 폴더의 dtool-comfy.log를 확인하세요.".into());
        }
        if let Some(probe) = &self.probing {
            match probe.try_recv() {
                Ok(ready) => {
                    self.ready = ready;
                    if ready && self.started.take().is_some() {
                        self.message = Some("ComfyUI에 연결했습니다. 추천 탭에서 사용할 모델을 선택하세요.".into());
                    }
                    self.probing = None;
                }
                Err(TryRecvError::Disconnected) => self.probing = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some((env, _)) = &self.child
            && self.probing.is_none()
            && Instant::now() >= self.next_probe
        {
            let port = env.port();
            let (send, receive) = mpsc::channel();
            let repaint = ctx.clone();
            std::thread::spawn(move || {
                let _ = send.send(setup::ready(port));
                repaint.request_repaint();
            });
            self.probing = Some(receive);
            self.next_probe = Instant::now() + Duration::from_secs(2);
        }
        if self
            .started
            .is_some_and(|start| start.elapsed() > Duration::from_secs(120))
        {
            self.started = None;
            self.message = Some("ComfyUI 응답이 늦습니다. dtool-comfy.log를 확인하세요.".into());
        }
        if self.child.is_some() || self.worker.is_some() {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
    }

    pub(super) fn toolbar(
        &mut self, ui: &mut egui::Ui, icons: &mut crate::icon_cache::IconCache, running: bool,
    ) {
        let busy = self.worker.is_some();
        if workspace_button(ui, icons, "ctx-settings", "모델 및 연결", self.open).clicked() {
            self.open = !self.open;
        }
        ui.separator();
        if ui.add_enabled_ui(self.child.is_none() && !busy, |ui| {
            workspace_button(ui, icons, "ai-play", "시작", false)
        }).inner.clicked() {
            if setup::runtime_installed(Environment::Stable) {
                self.start(ui.ctx(), Environment::Stable);
            } else {
                self.open = true;
                self.page = 0;
            }
        }
        if ui.add_enabled_ui(self.child.is_some() && !running, |ui| {
            workspace_button(ui, icons, "ai-stop", "종료", false)
        }).inner.clicked() {
            self.stop();
        }
        if busy || self.started.is_some() {
            ui.spinner();
        } else if self.ready {
            ui.label(egui::RichText::new("ComfyUI 연결됨").color(ACCENT));
        } else {
            ui.weak("ComfyUI 연결 전");
        }
    }

    pub(super) fn options_window(&mut self, ctx: &egui::Context, port: &mut f64, editing: bool) {
        if std::mem::take(&mut self.first_visit)
            && !setup::storage_configured()
            && !setup::runtime_installed(Environment::Stable)
            && !self.ready
        {
            self.open = true;
        }
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("AI 모델 및 연결")
            .id(egui::Id::new("dtool.ai.setup"))
            .default_width(560.0)
            .default_height(520.0)
            .max_height((ctx.content_rect().height() - 42.0).max(110.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (index, title) in ["설치", "추천", "모델 찾기", "노드", "설치됨"]
                        .into_iter()
                        .enumerate()
                    {
                        if tab(ui, self.page == index, title).clicked() {
                            self.page = index;
                        }
                    }
                });
                ui.separator();
                if let Some(message) = &self.message {
                    ui.label(message);
                }
                self.install_progress(ui);
                egui::ScrollArea::vertical()
                    .id_salt(("dtool.ai.setup.body", self.page))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 7.0;
                        let busy = self.worker.is_some();
                        ui.add_enabled_ui(!busy, |ui| match self.page {
                            0 => self.install_page(ui, port),
                            1 => self.recommended(ui),
                            2 => self.search_page(ui, editing),
                            3 => self.node_page(ui),
                            _ => self.installed_page(ui, *port as u16),
                        });
                    });
            });
        self.open = open;
    }

    fn install_progress(&self, ui: &mut egui::Ui) {
        if self.worker.is_none() && self.started.is_none() {
            return;
        }
        dtool_design::dialog_section(ui, |ui| {
            ui.set_min_width(ui.available_width());
            match &self.progress {
                Some(InstallProgress::Download { name, file, received, total }) => {
                    let fraction = (*received as f64 / (*total).max(1) as f64).clamp(0.0, 1.0);
                    ui.strong(progress_caption("다운로드", name, *file));
                    let caption = ui.label(format!(
                        "{} / {} · {:.0}%", transfer_size(*received), transfer_size(*total), fraction * 100.0,
                    ));
                    ui.add(egui::ProgressBar::new(fraction as f32).fill(ACCENT).desired_height(6.0))
                        .labelled_by(caption.id);
                }
                Some(InstallProgress::Verify { name, file }) => {
                    ui.spinner();
                    ui.label(progress_caption("파일 확인 중", name, *file));
                }
                Some(InstallProgress::Step(step)) => {
                    ui.spinner();
                    ui.label(*step);
                }
                None => {
                    ui.spinner();
                    ui.label(if self.started.is_some() { "ComfyUI 접속 대기 중" } else { "백그라운드 처리 중" });
                }
            }
        });
    }

    fn install_page(&mut self, ui: &mut egui::Ui, port: &mut f64) {
        super::option_section(ui, "1. 저장 위치", false);
        ui.label("용량이 큰 모델과 ComfyUI를 다른 드라이브에 저장할 수 있습니다.");
        ui.strong("ComfyUI 설치 위치");
        if let Ok(path) = setup::root() {
            ui.label(visible_path(&path));
        }
        #[cfg(windows)]
        if button_with(ui, "설치 폴더 선택…", &ControlState::enabled(self.child.is_none())).clicked() {
            match crate::dialogs::pick_folder_path_with_address("DTool ComfyUI 폴더를 만들 위치") {
                Ok(Some(folder)) => self.work(ui.ctx(), move || {
                    setup::set_install_parent(&folder)?;
                    setup::list_installed().map(Reply::StorageChanged)
                }),
                Ok(None) => {}
                Err(error) => self.message = Some(error.to_string()),
            }
        }
        ui.strong("모델 저장 위치");
        if let Ok(path) = setup::models_path() {
            ui.label(visible_path(&path));
        }
        #[cfg(windows)]
        if button_with(ui, "모델 폴더 선택…", &ControlState::enabled(self.child.is_none())).clicked() {
            match crate::dialogs::pick_folder_path_with_address("모델을 저장할 폴더 또는 기존 ComfyUI의 models 폴더") {
                Ok(Some(folder)) => self.work(ui.ctx(), move || {
                    setup::set_models_folder(&folder)?;
                    setup::list_installed().map(Reply::StorageChanged)
                }),
                Ok(None) => {}
                Err(error) => self.message = Some(error.to_string()),
            }
        }

        super::option_section(ui, "2. ComfyUI 설치 또는 연결", true);
        ui.label(format!("새 설치 · ComfyUI {} · Windows NVIDIA", setup::RUNTIME_VERSION));
        let installed = setup::runtime_installed(Environment::Stable);
        if button_with(
            ui,
            if installed { "ComfyUI 시작" } else { "설치 및 시작" },
            &ControlState::enabled(self.child.is_none()),
        ).clicked() {
            self.start(ui.ctx(), Environment::Stable);
        }
        ui.label("이미 설치되어 있다면 ComfyUI를 실행하고 연결을 확인하세요.");
        ui.horizontal_wrapped(|ui| {
            let spec = dtool_design::NumericSpec::new("").range(1.0..=65535.0).width(86.0).decimals(0);
            let caption = ui.label("포트");
            let previous_port = *port;
            ui.add_enabled_ui(self.child.is_none(), |ui| {
                dtool_design::numeric_value(ui, ui.make_persistent_id("ai.setup.port"), port, &spec)
                    .labelled_by(caption.id);
            });
            if previous_port != *port {
                self.ready = false;
                self.message = None;
            }
            if button_with(ui, "연결 확인", &ControlState::enabled(self.child.is_none())).clicked() {
                let port = *port as u16;
                self.ready = false;
                self.work(ui.ctx(), move || Ok(Reply::Probe(port, setup::ready(port))));
            }
        });
        super::option_section(ui, "3. 첫 모델 받기", true);
        ui.label("추천 모델을 설치한 뒤 생성·편집에서 모델 목록을 새로고침하세요.");
        if button(ui, "추천 모델 보기").clicked() {
            self.page = 1;
        }
    }

    fn recommended(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("모델 저장 위치");
            if button(ui, "위치 변경…").clicked() {
                self.page = 0;
            }
        });
        if let Ok(path) = setup::models_path() {
            ui.label(visible_path(&path));
        }
        super::option_section(ui, "생성·편집 모델", true);
        for (title, repo, note, download) in [
            (
                "FLUX 3 Image Klein",
                dtool_core::ai_image::flux3::REPOSITORY,
                "로컬 생성·참조 편집 · 게시자 페이지에서 접근 승인 필요",
                "모델 받기 · 약 17.75 GB",
            ),
            (
                "Qwen-Image 2.1 Turbo",
                setup::QWEN_REPOSITORY,
                "8단계 생성·참조 이미지 편집 · 모델 파일 3개 · Qwen Research 라이선스",
                "Qwen Turbo 받기",
            ),
            (
                "BiRefNet",
                setup::BIREFNET_REPOSITORY,
                "사진·이미지의 배경을 투명하게 분리",
                "BiRefNet 받기",
            ),
            (
                "Depth Anything 3 Small",
                setup::DEPTH_REPOSITORY,
                dtool_strings::text("ai.image.depth_model_note"),
                dtool_strings::text("ai.image.depth_install"),
            ),
        ] {
            if repo == setup::BIREFNET_REPOSITORY {
                super::option_section(ui, "작업 전용 모델", true);
            }
            dtool_design::dialog_section(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.strong(title);
                ui.label(note);
                let flux = repo == dtool_core::ai_image::flux3::REPOSITORY;
                if flux {
                    ui.label(dtool_strings::text("ai.image.flux3_files"));
                }
                ui.horizontal_wrapped(|ui| {
                    if button(ui, download).clicked() {
                        match repo {
                            setup::QWEN_REPOSITORY => self.progress_work(ui.ctx(), |progress| {
                                setup::install_qwen_turbo(progress).map(|paths| Reply::BundleInstalled(paths, "Qwen Turbo"))
                            }),
                            setup::BIREFNET_REPOSITORY => {
                                self.work(ui.ctx(), || setup::install_birefnet().map(Reply::ModelInstalled));
                            }
                            setup::DEPTH_REPOSITORY => {
                                self.work(ui.ctx(), || setup::install_depth().map(Reply::ModelInstalled));
                            }
                            _ => {
                                self.progress_work(ui.ctx(), |progress| {
                                    setup::install_flux3_klein(progress)
                                        .map(|paths| Reply::BundleInstalled(paths, "FLUX 3 Image Klein"))
                                });
                            }
                        }
                    }
                    if button(ui, "파일 목록").clicked() {
                        self.fetch_files(ui.ctx(), repo);
                    }
                    ui.hyperlink_to(
                        if flux { "게시자 페이지에서 접근 확인" } else { "모델 정보" },
                        format!("https://huggingface.co/{repo}"),
                    );
                });
            });
            ui.add_space(3.0);
        }
        self.file_options(ui);
    }

    fn fetch_files(&mut self, ctx: &egui::Context, repo: &str) {
        let repo = repo.to_owned();
        self.work(ctx, move || setup::model_files(&repo).map(Reply::Files));
    }

    fn search_page(&mut self, ui: &mut egui::Ui, editing: bool) {
        super::option_section(ui, "Hugging Face 모델 검색", false);
        ui.horizontal_wrapped(|ui| {
            for (popular, label) in [(true, "인기순"), (false, "최신순")] {
                if tab(ui, self.popular == popular, label).clicked() {
                    self.popular = popular;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.query)
                .hint_text("모델 이름 또는 게시자")
                .desired_width((ui.available_width() - 74.0).clamp(96.0, 360.0)));
            if button(ui, "검색").clicked() {
                let query = self.query.clone();
                let popular = self.popular;
                self.work(ui.ctx(), move || {
                    setup::search_models(&query, popular, editing).map(Reply::Models)
                });
            }
        });
        self.file_options(ui);
        let mut picked = None;
        for model in &self.results {
            ui.horizontal_wrapped(|ui| {
                ui.label(&model.repository);
                ui.weak(format!("다운로드 {}", model.downloads));
                if model.gated {
                    ui.hyperlink_to(
                        "접근 승인",
                        format!("https://huggingface.co/{}", model.repository),
                    );
                }
                if button(ui, "파일 목록").clicked() {
                    picked = Some(model.repository.clone());
                }
            });
        }
        if let Some(repo) = picked {
            self.fetch_files(ui.ctx(), &repo);
        }
    }

    fn file_options(&mut self, ui: &mut egui::Ui) {
        let Some(model) = &self.files else {
            return;
        };
        super::option_section(ui, "모델 파일", true);
        ui.strong(&model.repository);
        ui.hyperlink_to(
            "게시자 및 이용 조건",
            format!("https://huggingface.co/{}", model.repository),
        );
        if let Some(license) = &model.license {
            ui.label(license);
        }
        let Some(file) = model.files.get(self.file_index) else {
            ui.label("다운로드할 수 있는 SHA-256 포함 safetensors 파일이 없습니다.");
            return;
        };
        let previous_file = self.file_index;
        egui::ComboBox::from_id_salt("dtool.ai.file")
            .truncate()
            .selected_text(&file.name)
            .show_ui(ui, |ui| {
                for (i, file) in model.files.iter().enumerate() {
                    ui.selectable_value(&mut self.file_index, i, &file.name);
                }
            });
        if self.file_index != previous_file {
            self.category = file_category(&model.files[self.file_index].name);
        }
        egui::ComboBox::from_id_salt("dtool.ai.category")
            .selected_text(CATEGORIES[self.category])
            .show_ui(ui, |ui| {
                for (i, category) in CATEGORIES.iter().enumerate() {
                    ui.selectable_value(&mut self.category, i, *category);
                }
            });
        let file = &model.files[self.file_index];
        ui.label(format!(
            "다운로드 {:.2} GB",
            file.size as f64 / 1_000_000_000.0
        ));
        if button(ui, "다운로드").clicked() {
            let model = model.clone();
            let file = file.clone();
            let category = CATEGORIES[self.category];
            self.progress_work(ui.ctx(), move |progress| {
                setup::download_model_with_progress(&model, &file, category, progress).map(Reply::ModelInstalled)
            });
        }
        ui.separator();
    }

    fn node_page(&mut self, ui: &mut egui::Ui) {
        super::option_section(ui, "실험 런타임", false);
        ui.label("외부 노드는 분리된 실험 환경에서 실행합니다.");
        if !setup::runtime_installed(Environment::Lab) {
            if button_with(
                ui,
                "실험 환경 설치",
                &ControlState::enabled(self.child.is_none()),
            )
            .clicked()
            {
                self.progress_work(ui.ctx(), |progress| {
                    setup::install_runtime(Environment::Lab, progress).map(|_| Reply::LabInstalled)
                });
            }
            return;
        }
        if button_with(
            ui,
            "실험 환경 시작",
            &ControlState::enabled(self.child.is_none()),
        )
        .clicked()
        {
            self.start(ui.ctx(), Environment::Lab);
        }
        super::option_section(ui, "노드와 버전", true);
        ui.label("Comfy Registry 노드 ID");
        ui.text_edit_singleline(&mut self.node_id);
        ui.label("정확한 버전");
        ui.text_edit_singleline(&mut self.node_version);
        if button(ui, "설치 내용 확인").clicked() {
            let id = self.node_id.trim().to_owned();
            let version = self.node_version.trim().to_owned();
            self.node = None;
            self.work(ui.ctx(), move || {
                setup::node_plan(&id, &version).map(Reply::NodePlan)
            });
        }
        if let Some(plan) = &self.node {
            ui.label(format!("{} · {}", plan.id, plan.version));
            for dependency in &plan.dependencies {
                ui.label(dependency);
            }
            toggle(ui,
                &mut self.approve,
                "이 노드와 표시된 의존성의 설치를 승인합니다.",
            );
            if button_with(
                ui,
                "실험 환경에 설치",
                &ControlState::enabled(self.approve && self.child.is_none()),
            )
            .clicked()
            {
                let plan = plan.clone();
                self.work(ui.ctx(), move || {
                    setup::install_node(&plan).map(|_| Reply::NodeInstalled)
                });
            }
        }
    }

    fn installed_page(&mut self, ui: &mut egui::Ui, port: u16) {
        super::option_section(ui, "런타임 상태", false);
        for (env, title) in [
            (Environment::Stable, "기본 런타임"),
            (Environment::Lab, "실험 런타임"),
        ] {
            let present = if setup::runtime_installed(env) {
                "설치됨"
            } else {
                "미설치"
            };
            ui.label(format!("{title} · {present} · {}", env.port()));
        }
        ui.label(format!("선택 포트 {port}"));
        super::option_section(ui, "설치된 모델 파일", true);
        if let Ok(path) = setup::models_path() {
            ui.label(visible_path(&path));
        }
        if button(ui, "설치 목록 새로고침").clicked() {
            self.work(ui.ctx(), || {
                setup::list_installed().map(Reply::InstalledList)
            });
        }
        let mut remove = None;
        for path in &self.installed {
            ui.horizontal_wrapped(|ui| {
                ui.label(path);
                if button_with(ui, "삭제", &ControlState::enabled(self.child.is_none())).clicked()
                {
                    remove = Some(path.clone());
                }
            });
        }
        if let Some(path) = remove.take() {
            self.pending_remove = Some(path);
        }
        if let Some(path) = &self.pending_remove {
            ui.label(format!("이 모델 파일을 삭제하시겠습니까? {path}"));
            ui.horizontal(|ui| {
                if button(ui, "취소").clicked() {
                    remove = Some(String::new());
                }
                if button_with(
                    ui,
                    "파일 삭제",
                    &ControlState::enabled(self.child.is_none()),
                )
                .clicked()
                {
                    remove = Some(path.clone());
                }
            });
            if let Some(path) = remove {
                self.pending_remove = None;
                if !path.is_empty() {
                    self.work(ui.ctx(), move || {
                        setup::remove_installed(&path)?;
                        setup::list_installed().map(Reply::InstalledList)
                    });
                }
            }
        }
    }

    fn stop(&mut self) {
        if let Some((env, mut child)) = self.child.take() {
            if let Err(error) = child.kill() {
                self.message = Some(error.to_string());
                // Retain ownership so that an explicit stop can be retried.
                self.child = Some((env, child));
                return;
            }
            let _ = child.wait();
        }
        self.ready = false;
        self.started = None;
        self.probing = None;
    }
}

impl Drop for SetupPanel {
    fn drop(&mut self) {
        if let Some((_, mut child)) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
