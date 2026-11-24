use crate::command::Session;
use crate::document::Document;
use crate::object::ObjectId;
use crate::selection::PixelSelection;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTaskKind {
    SelectSubject,
    ObjectSelect,
    RemoveBackground,
    Fill,
    Expand,
    Edit,
    Generate,
    Variation,
    Upscale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpscaleMode {
    SuperResolution,
    Generative,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AiJobId(pub u64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiRequest {
    pub kind: AiTaskKind,
    pub image: Option<AiImage>,
    pub mask: Option<PixelSelection>,
    pub prompt: String,
    pub negative: String,
    pub count: u32,
    pub seed: u64,
    pub scale: u32,
    pub mode: Option<UpscaleMode>,
    pub expand: [u32; 4],
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AiJobState {
    Queued,
    Running { progress: f32 },
    Done,
    Failed { code: String },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiResult {
    pub images: Vec<AiImage>,
    pub mask: Option<PixelSelection>,
}

pub trait AiBackend: Send {
    fn name(&self) -> &str;
    fn submit(&mut self, r: AiRequest) -> Result<AiJobId, CommandError>;
    fn poll(&mut self, id: AiJobId) -> AiJobState;
    fn take_result(&mut self, id: AiJobId) -> Option<AiResult>;
    fn cancel(&mut self, id: AiJobId);
}

pub struct AiService {
    pub backend: Option<Box<dyn AiBackend>>,
    pub jobs: Vec<(AiJobId, ObjectId, AiTaskKind)>,
}

impl std::fmt::Debug for AiService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("AiService").field("backend", &self.backend.as_ref().map(|backend| backend.name())).field("jobs", &self.jobs).finish()
    }
}

pub fn service() -> &'static std::sync::Mutex<AiService> {
    static SERVICE: std::sync::OnceLock<std::sync::Mutex<AiService>> = std::sync::OnceLock::new();
    SERVICE.get_or_init(|| std::sync::Mutex::new(AiService::default()))
}

impl Default for AiService {
    fn default() -> Self {
        Self {
            backend: None,
            jobs: Vec::new(),
        }
    }
}

pub struct FakeBackend {
    next_id: u64,
    results: std::collections::HashMap<AiJobId, AiResult>,
    states: std::collections::HashMap<AiJobId, AiJobState>,
}

impl Default for FakeBackend {
    fn default() -> Self {
        Self {
            next_id: 1,
            results: std::collections::HashMap::new(),
            states: std::collections::HashMap::new(),
        }
    }
}

impl AiBackend for FakeBackend {
    fn name(&self) -> &str {
        "fake"
    }

    fn submit(&mut self, r: AiRequest) -> Result<AiJobId, CommandError> {
        let id = AiJobId(self.next_id);
        self.next_id += 1;
        self.states.insert(id, AiJobState::Done);

        let mask = match r.kind {
            AiTaskKind::SelectSubject | AiTaskKind::ObjectSelect => {
                if let Some(img) = &r.image {
                    let w = img.width as i32;
                    let h = img.height as i32;
                    let cx = w / 2;
                    let cy = h / 2;
                    let hw = (w / 4).max(1);
                    let hh = (h / 4).max(1);
                    Some(crate::selection::rect_mask(img.width, img.height, [(cx - hw) as f32, (cy - hh) as f32, (2 * hw) as f32, (2 * hh) as f32], false))
                } else {
                    None
                }
            }
            _ => r.mask.clone(),
        };

        let images = if r.kind == AiTaskKind::Generate || r.kind == AiTaskKind::Variation {
            vec![AiImage {
                width: 1,
                height: 1,
                rgba: vec![0u8; 4],
            }]
        } else {
            Vec::new()
        };

        self.results.insert(
            id,
            AiResult { images, mask },
        );

        Ok(id)
    }

    fn poll(&mut self, id: AiJobId) -> AiJobState {
        self.states.get(&id).cloned().unwrap_or(AiJobState::Queued)
    }

    fn take_result(&mut self, id: AiJobId) -> Option<AiResult> {
        self.results.remove(&id)
    }

    fn cancel(&mut self, id: AiJobId) {
        if let Some(state) = self.states.get_mut(&id) {
            *state = AiJobState::Cancelled;
        }
    }
}

pub fn handle_ai_select_subject(session: &mut Session, cmd: Command) -> CommandResult {
    let node_id = parse_node_id(&cmd)?;
    let doc = session.document().ok_or_else(CommandError::no_document)?;
    let obj = doc.get_object(node_id).ok_or_else(|| CommandError::not_found())?;

    let mut service = service().lock().map_err(|_| CommandError::new("AI_SERVICE_BUSY"))?;
    let ai_service = &mut *service;

    if ai_service.backend.is_none() {
        return Err(CommandError::new("AI_ENGINE_OFF"));
    }

    let image = extract_image(doc, node_id)?;
    let request = AiRequest {
        kind: AiTaskKind::SelectSubject,
        image: Some(image),
        mask: None,
        prompt: String::new(),
        negative: String::new(),
        count: 1,
        seed: 0,
        scale: 1,
        mode: None,
        expand: [0; 4],
        model: None,
    };

    let job_id = ai_service.backend.as_mut().unwrap().submit(request)?;
    ai_service.jobs.push((job_id, node_id, AiTaskKind::SelectSubject));

    Ok(serde_json::json!({ "job_id": job_id.0 }))
}

pub fn handle_ai_object_select(session: &mut Session, cmd: Command) -> CommandResult {
    let node_id = parse_node_id(&cmd)?;
    let rect = parse_rect(&cmd)?;
    let doc = session.document().ok_or_else(CommandError::no_document)?;
    let obj = doc.get_object(node_id).ok_or_else(|| CommandError::not_found())?;

    let mut service = service().lock().map_err(|_| CommandError::new("AI_SERVICE_BUSY"))?;
    let ai_service = &mut *service;

    if ai_service.backend.is_none() {
        return Err(CommandError::new("AI_ENGINE_OFF"));
    }

    let image = extract_image(doc, node_id)?;
    let mask = crate::selection::rect_mask(image.width, image.height, [rect.0 as f32, rect.1 as f32, (rect.2 - rect.0) as f32, (rect.3 - rect.1) as f32], false);
    let request = AiRequest {
        kind: AiTaskKind::ObjectSelect,
        image: Some(image),
        mask: Some(mask),
        prompt: String::new(),
        negative: String::new(),
        count: 1,
        seed: 0,
        scale: 1,
        mode: None,
        expand: [0; 4],
        model: None,
    };

    let job_id = ai_service.backend.as_mut().unwrap().submit(request)?;
    ai_service.jobs.push((job_id, node_id, AiTaskKind::ObjectSelect));

    Ok(serde_json::json!({ "job_id": job_id.0 }))
}

pub fn handle_ai_job_status(session: &mut Session, cmd: Command) -> CommandResult {
    let job_id = parse_job_id(&cmd)?;
    let mut service = service().lock().map_err(|_| CommandError::new("AI_SERVICE_BUSY"))?;
    let ai_service = &mut *service;

    if ai_service.backend.is_none() {
        return Err(CommandError::new("AI_ENGINE_OFF"));
    }

    let state = ai_service.backend.as_mut().unwrap().poll(job_id);
    let progress = match &state {
        AiJobState::Running { progress } => *progress,
        _ => 0.0,
    };

    Ok(serde_json::json!({
        "state": format!("{:?}", state),
        "progress": progress
    }))
}

pub fn handle_ai_job_cancel(session: &mut Session, cmd: Command) -> CommandResult {
    let job_id = parse_job_id(&cmd)?;
    let mut service = service().lock().map_err(|_| CommandError::new("AI_SERVICE_BUSY"))?;
    let ai_service = &mut *service;

    if ai_service.backend.is_none() {
        return Err(CommandError::new("AI_ENGINE_OFF"));
    }

    ai_service.backend.as_mut().unwrap().cancel(job_id);
    Ok(serde_json::json!({ "cancelled": true }))
}

pub fn apply_finished_jobs(session: &mut Session) {
    let Ok(mut service) = service().lock() else { return; };
    let AiService { backend, jobs } = &mut *service;
    let Some(backend) = backend.as_mut() else { return; };
    let mut finished = Vec::new();
    for (index, (job_id, node_id, kind)) in jobs.iter().enumerate() {
        match backend.poll(*job_id) {
            AiJobState::Done => finished.push((index, *node_id, *kind, backend.take_result(*job_id))),
            AiJobState::Failed { .. } | AiJobState::Cancelled => finished.push((index, *node_id, *kind, None)),
            AiJobState::Queued | AiJobState::Running { .. } => {},
        }
    }
    for (index, node_id, _kind, result) in finished.into_iter().rev() {
        jobs.remove(index);
        let Some(mask) = result.and_then(|result| result.mask) else { continue; };
        let Some(doc) = session.document_mut() else { continue; };
        let Some(object) = doc.get_object(node_id) else { continue; };
        if object.raster_size != Some((mask.width, mask.height)) || (mask.width as usize).checked_mul(mask.height as usize) != Some(mask.coverage.len()) { continue; }
        doc.pixel_selection = Some(mask);
    }
}

fn parse_node_id(cmd: &Command) -> Result<ObjectId, CommandError> {
    cmd.params.get("node")
        .and_then(|v| v.as_u64())
        .map(ObjectId)
        .ok_or_else(|| CommandError::invalid_params())
}

fn parse_job_id(cmd: &Command) -> Result<AiJobId, CommandError> {
    cmd.params.get("job_id")
        .and_then(|v| v.as_u64())
        .map(AiJobId)
        .ok_or_else(|| CommandError::invalid_params())
}

fn parse_rect(cmd: &Command) -> Result<(i32, i32, i32, i32), CommandError> {
    let rect = cmd.params.get("rect")
        .and_then(|v| v.as_array())
        .ok_or_else(|| CommandError::invalid_params())?;

    if rect.len() != 4 {
        return Err(CommandError::invalid_params());
    }

    let coordinate = |index: usize| rect[index].as_i64().and_then(|value| i32::try_from(value).ok()).ok_or_else(CommandError::invalid_params);
    let (x0, y0, x1, y1) = (coordinate(0)?, coordinate(1)?, coordinate(2)?, coordinate(3)?);
    if x0 >= x1 || y0 >= y1 || x1.checked_sub(x0).is_none() || y1.checked_sub(y0).is_none() { return Err(CommandError::invalid_params()); }

    Ok((x0, y0, x1, y1))
}

fn extract_image(doc: &Document, node_id: ObjectId) -> Result<AiImage, CommandError> {
    let object = doc.get_object(node_id).ok_or_else(CommandError::not_found)?;
    let (width, height) = object.raster_size.ok_or_else(|| CommandError::new("NO_PIXEL_LAYER"))?;
    let rgba = object.raster_data.as_ref().ok_or_else(|| CommandError::new("NO_PIXEL_LAYER"))?;
    let length = (width as usize).checked_mul(height as usize).and_then(|value| value.checked_mul(4)).ok_or_else(CommandError::invalid_params)?;
    if width == 0 || height == 0 || rgba.len() != length { return Err(CommandError::invalid_params()); }
    Ok(AiImage { width, height, rgba: rgba.clone() })
}
