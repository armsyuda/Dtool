mod comfy;

use std::sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc};

use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::ai::AiImage;
use crate::document::{PageId, Units};
use crate::object::{Object, ObjectId, ObjectKind};
use crate::resources::{ImageFormat, ImageResource};
use crate::{Command, CommandError, CommandResult, Session};

pub const GENERATE: &str = "ai.image.generate";
pub const EDIT: &str = "ai.image.edit";
pub const STATUS: &str = "ai.image.status";
pub const PREVIEW: &str = "ai.image.preview";
pub const APPLY: &str = "ai.image.apply";
pub const CANCEL: &str = "ai.image.cancel";
const MAX_SIDE: u32 = 2048;

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Request {
    port: u16,
    model: String,
    prompt: String,
    negative: String,
    width: u32,
    height: u32,
    steps: u32,
    count: u32,
    seed: u64,
    strength: f32,
    source: Option<ObjectId>,
    use_selection: bool,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            port: 8188, model: String::new(), prompt: String::new(), negative: String::new(),
            width: 512, height: 512, steps: 20, count: 1, seed: 0, strength: 0.55, source: None, use_selection: false,
        }
    }
}

#[derive(Debug)]
struct Candidate {
    png: Vec<u8>,
    image: AiImage,
}

#[derive(Debug)]
enum JobState {
    Running(mpsc::Receiver<Result<Vec<Candidate>, CommandError>>),
    Done(Vec<Candidate>),
    Failed(CommandError),
    Cancelled,
}

#[derive(Debug)]
struct Job {
    id: u64,
    source: Option<ObjectId>,
    page: Option<PageId>,
    state: JobState,
    cancel: Arc<AtomicBool>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

#[derive(Debug, Default)]
pub struct Jobs {
    next: u64,
    job: Option<Job>,
}

impl Jobs {
    pub fn preview_image(&self, index: usize) -> Option<&AiImage> {
        let JobState::Done(images) = &self.job.as_ref()?.state else { return None };
        images.get(index).map(|item| &item.image)
    }

    fn refresh(&mut self) {
        let Some(job) = &mut self.job else { return };
        let JobState::Running(receiver) = &job.state else { return };
        match receiver.try_recv() {
            Ok(Ok(images)) => job.state = JobState::Done(images),
            Ok(Err(error)) if error.code == "CANCELLED" => job.state = JobState::Cancelled,
            Ok(Err(error)) => job.state = JobState::Failed(error),
            Err(mpsc::TryRecvError::Disconnected) => job.state = JobState::Failed(CommandError::new("AI_IMAGE_RESPONSE")),
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    fn current(&self, params: &Value) -> Result<&Job, CommandError> {
        let job = self.job.as_ref().ok_or_else(CommandError::not_found)?;
        if params.get("job_id").is_some() && params["job_id"].as_u64() != Some(job.id) {
            return Err(CommandError::not_found());
        }
        Ok(job)
    }
}

fn candidate<'a>(job: &'a Job, params: &Value) -> Result<&'a Candidate, CommandError> {
    let JobState::Done(images) = &job.state else { return Err(CommandError::new("AI_IMAGE_PENDING")) };
    let index = params.get("index").map_or(Some(0), Value::as_u64).and_then(|i| usize::try_from(i).ok())
        .ok_or_else(CommandError::invalid_params)?;
    images.get(index).ok_or_else(CommandError::not_found)
}

pub fn models(port: u16) -> Result<Vec<String>, CommandError> {
    if port == 0 { return Err(CommandError::invalid_params()); }
    comfy::models(port)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let name = command.name.as_str();
    if !matches!(name, GENERATE | EDIT | STATUS | PREVIEW | APPLY | CANCEL) { return None; }
    if !session.has_document { return Some(Err(CommandError::no_document())); }
    session.ai_image.refresh();
    Some(match name {
        GENERATE | EDIT => submit(session, command, name == EDIT),
        STATUS => status(&session.ai_image, &command.params),
        PREVIEW => session.ai_image.current(&command.params).and_then(|job| {
            let image = candidate(job, &command.params)?;
            Ok(json!({
                "job_id": job.id, "width": image.image.width, "height": image.image.height,
                "png_base64": base64::engine::general_purpose::STANDARD.encode(&image.png),
            }))
        }),
        APPLY => apply(session, &command.params),
        CANCEL => cancel(&mut session.ai_image, &command.params),
        _ => unreachable!(),
    })
}

fn submit(session: &mut Session, command: &Command, editing: bool) -> CommandResult {
    if session.ai_image.job.as_ref().is_some_and(|job| matches!(job.state, JobState::Running(_))) {
        return Err(CommandError::new("AI_IMAGE_BUSY"));
    }
    let request: Request = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
    if request.port == 0 || request.model.trim().is_empty() || request.model.len() > 512
        || request.prompt.trim().is_empty() || request.prompt.len() > 8192 || request.negative.len() > 8192
        || !(1..=4).contains(&request.count) || !(1..=100).contains(&request.steps) || !(0.0..=1.0).contains(&request.strength)
        || (!editing && (request.use_selection || request.source.is_some() || [request.width, request.height].iter()
            .any(|&side| !(64..=MAX_SIDE).contains(&side) || !side.is_multiple_of(8))))
    {
        return Err(CommandError::invalid_params());
    }
    let source = if editing {
        let id = request.source.or_else(|| match session.selection.ids.as_slice() { [id] => Some(*id), _ => None })
            .ok_or_else(|| CommandError::new("AI_IMAGE_SOURCE"))?;
        Some((id, source_image(session, id)?))
    } else {
        None
    };
    let mask = if request.use_selection {
        let (id, image) = source.as_ref().ok_or_else(|| CommandError::new("AI_IMAGE_SELECTION"))?;
        Some(source_mask(session, *id, image)?)
    } else {
        None
    };
    let source_id = source.as_ref().map(|(id, _)| *id);
    let page = crate::commands::pages::viewed_page(session).or_else(|| session.document.pages.first().map(|page| page.id));
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    std::thread::Builder::new().name("dtool-ai-image".into()).spawn(move || {
        let result = comfy::run(&request, source.as_ref().map(|(_, image)| image), mask.as_deref(), &worker_cancel);
        let _ = sender.send(result);
    }).map_err(|_| CommandError::new("AI_IMAGE_RESPONSE"))?;
    session.ai_image.next = session.ai_image.next.saturating_add(1);
    let id = session.ai_image.next;
    session.ai_image.job = Some(Job { id, source: source_id, page, state: JobState::Running(receiver), cancel });
    Ok(json!({"job_id": id, "state": "running"}))
}

fn source_image(session: &Session, id: ObjectId) -> Result<AiImage, CommandError> {
    let invalid = || CommandError::new("AI_IMAGE_SOURCE");
    let object = session.document.get_object(id).ok_or_else(invalid)?;
    let (width, height) = if let Some(layer) = session.document.pixel_layers.get(id) {
        (layer.width, layer.height)
    } else {
        object.raster_size.ok_or_else(invalid)?
    };
    if !(8..=MAX_SIDE).contains(&width) || !(8..=MAX_SIDE).contains(&height) {
        return Err(invalid());
    }
    let rgba = if let Some(layer) = session.document.pixel_layers.get(id) {
        layer.to_rgba()
    } else {
        object.raster_data.clone().ok_or_else(invalid)?
    };
    if rgba.len() != width as usize * height as usize * 4 { return Err(invalid()); }
    Ok(AiImage { width, height, rgba })
}

fn source_mask(session: &Session, id: ObjectId, image: &AiImage) -> Result<Vec<u8>, CommandError> {
    let invalid = || CommandError::new("AI_IMAGE_SELECTION");
    let doc = &session.document;
    let object = doc.get_object(id).filter(|object| object.kind == ObjectKind::Image).ok_or_else(invalid)?;
    let selection = doc.pixel_selection.as_ref().ok_or_else(invalid)?;
    let surface = crate::selection::PixelSurface::for_page(doc, selection.page_id)
        .filter(|surface| surface.matches(doc, selection)).ok_or_else(invalid)?;
    let page = crate::data_merge::layout::object_page_index(doc, id);
    if doc.pages.get(page).map(|page| page.id) != Some(surface.page_id) { return Err(invalid()); }
    let transform = crate::geometry::object_matrix(object);
    let step = [object.size[0].abs() / image.width as f32, object.size[1].abs() / image.height as f32];
    let mut mask = Vec::with_capacity(image.width as usize * image.height as usize);
    for y in 0..image.height {
        for x in 0..image.width {
            let position = [object.pos[0] + (x as f32 + 0.5) * step[0], object.pos[1] + (y as f32 + 0.5) * step[1]];
            let world = crate::geometry::apply_matrix(&transform, position);
            mask.push(surface.coverage_at(selection, world));
        }
    }
    if mask.iter().all(|&coverage| coverage == 0) { return Err(invalid()); }
    Ok(mask)
}

fn status(jobs: &Jobs, params: &Value) -> CommandResult {
    if jobs.job.is_none() && params.get("job_id").is_none() {
        return Ok(json!({"state": "idle", "count": 0}));
    }
    let job = jobs.current(params)?;
    let (state, count, error) = match &job.state {
        JobState::Running(_) => ("running", 0, None),
        JobState::Done(images) => ("done", images.len(), None),
        JobState::Failed(error) => ("failed", 0, Some(error)),
        JobState::Cancelled => ("cancelled", 0, None),
    };
    Ok(json!({"job_id": job.id, "state": state, "count": count, "error": error, "source": job.source}))
}

fn cancel(jobs: &mut Jobs, params: &Value) -> CommandResult {
    let id = jobs.current(params)?.id;
    let job = jobs.job.as_mut().ok_or_else(CommandError::not_found)?;
    if matches!(job.state, JobState::Running(_)) {
        job.cancel.store(true, Ordering::Release);
        job.state = JobState::Cancelled;
    }
    Ok(json!({"job_id": id, "state": if matches!(job.state, JobState::Cancelled) { "cancelled" } else { "finished" }}))
}

fn apply(session: &mut Session, params: &Value) -> CommandResult {
    let job = session.ai_image.current(params)?;
    let candidate = candidate(job, params)?;
    let mut document = session.document.clone();
    let width = candidate.image.width;
    let height = candidate.image.height;
    let unit = crate::doc_setup::unit_factor(Units::Px, document.dpi, document.units, document.dpi);
    let mut object = Object {
        name: dtool_strings::text("ai.image.result_layer").to_string(),
        size: [width as f32 * unit, height as f32 * unit],
        raster_size: Some((width, height)),
        raster_data: Some(candidate.image.rgba.clone()),
        kind: ObjectKind::Image,
        ..Object::default()
    };
    let insert_after = if let Some(id) = job.source {
        let index = document.objects.iter().position(|item| item.id == id).ok_or_else(CommandError::not_found)?;
        let source = &document.objects[index];
        object.parent_id = source.parent_id;
        object.page_id = source.page_id;
        object.pos = source.pos;
        object.size = source.size;
        object.rotation_deg = source.rotation_deg;
        object.shear_deg = source.shear_deg;
        object.transform_origin = source.transform_origin;
        Some(index + 1)
    } else {
        let page = job.page.ok_or_else(CommandError::not_found)?;
        let index = document.pages.iter().position(|item| item.id == page).ok_or_else(CommandError::not_found)?;
        let [x, y, w, h] = crate::data_merge::layout::page_rect(&document, index);
        let fit = (w / object.size[0]).min(h / object.size[1]).min(1.0);
        object.size = object.size.map(|side| side * fit);
        object.pos = [x + (w - object.size[0]) / 2.0, y + (h - object.size[1]) / 2.0];
        crate::artboard::edit::assign_new_object(&document, &mut object, [x + w / 2.0, y + h / 2.0]);
        object.page_id = Some(page);
        None
    };
    let mut resource = ImageResource::new_embedded(ImageFormat::Png, [width, height], document.dpi, candidate.png.clone());
    resource.name = object.name.clone();
    let resource = document.add_image_resource(resource);
    let id = document.create_object(object);
    if let Some(index) = insert_after
        && let Some(object) = document.objects.pop()
    {
        document.objects.insert(index, object);
    }
    document.place_image(id, resource)?;
    document.pixel_layers.insert(id, crate::pixel::PixelLayer::from_rgba(width, height, &candidate.image.rgba));
    crate::commands::document_change::commit(session, APPLY, document, vec![id], json!({"id": id, "resource": resource}))
}
