pub mod setup;

mod comfy;
pub mod flux3;
pub mod outpaint;
pub mod qwen;
mod subject;
pub mod upscale;
pub use comfy::detail::output_size as detail_output_size;

use std::sync::{Arc, atomic::{AtomicBool, AtomicU8, Ordering}, mpsc};

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
pub const SELECT_SUBJECT: &str = "ai.image.select_subject";
pub const REMOVE_BACKGROUND: &str = "ai.image.remove_background";
pub const UPSCALE: &str = "ai.image.upscale";
pub const DEPTH_MAP: &str = "ai.image.depth_map";
pub const OUTPAINT: &str = "ai.image.outpaint";
pub const REGENERATE_DETAIL: &str = "ai.image.regenerate_detail";
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
    protect_selection: bool,
    scale: u32,
    upscale_model: upscale::Model,
    depth_color: bool,
    borders: outpaint::Borders,
    to_canvas: bool,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            port: 8188, model: String::new(), prompt: String::new(), negative: String::new(),
            width: 512, height: 512, steps: 20, count: 1, seed: 0, strength: 0.55, source: None, use_selection: false, protect_selection: false,
            scale: 2, upscale_model: upscale::Model::Standard, depth_color: false, borders: [0; 4], to_canvas: false,
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
    operation: String,
    source: Option<ObjectId>,
    original: Option<Arc<AiImage>>,
    page: Option<PageId>,
    state: JobState,
    cancel: Arc<AtomicBool>,
    progress: Option<Arc<AtomicU8>>,
    source_dpi: Option<f32>,
    depth_map: bool,
    borders: Option<outpaint::Borders>,
    regenerates_detail: bool,
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

    pub fn preview_png(&self, index: usize) -> Option<&[u8]> {
        let JobState::Done(images) = &self.job.as_ref()?.state else { return None };
        images.get(index).map(|item| item.png.as_slice())
    }

    pub fn original_image(&self) -> Option<&AiImage> {
        self.job.as_ref()?.original.as_deref()
    }

    pub fn source(&self) -> Option<ObjectId> {
        self.job.as_ref()?.source
    }

    pub fn supports_frame_replacement(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.progress.is_some())
    }

    pub fn is_depth_map(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.depth_map)
    }

    pub fn borders(&self) -> Option<outpaint::Borders> {
        self.job.as_ref()?.borders
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

pub fn reference_alignment(model: &str) -> Option<u32> {
    if qwen::is_model(model) {
        Some(qwen::ALIGNMENT)
    } else {
        flux3::steps(model).map(|_| flux3::ALIGNMENT)
    }
}

fn companion(model: &str, files: &[String], filename: &str) -> Option<String> {
    let model = model.replace('\\', "/");
    let same_package = model.rsplit_once('/').map(|(prefix, _)| format!("{prefix}/{filename}"));
    if let Some(file) = files.iter().find(|file| {
        same_package.as_deref() == Some(file.replace('\\', "/").as_str())
    }) {
        return Some(file.clone());
    }
    if let Some(file) = files.iter().find(|file| file.as_str() == filename) {
        return Some(file.clone());
    }
    let mut matches = files.iter().filter(|file| file.rsplit(['/', '\\']).next() == Some(filename));
    let file = matches.next()?;
    matches.next().is_none().then(|| file.clone())
}


pub fn models(port: u16) -> Result<Vec<String>, CommandError> {
    if port == 0 { return Err(CommandError::invalid_params()); }
    comfy::models(port)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let name = command.name.as_str();
    if !matches!(name, GENERATE | EDIT | SELECT_SUBJECT | REMOVE_BACKGROUND | UPSCALE | DEPTH_MAP | OUTPAINT | REGENERATE_DETAIL | STATUS | PREVIEW | APPLY | CANCEL) { return None; }
    if !session.has_document { return Some(Err(CommandError::no_document())); }
    session.ai_image.refresh();
    Some(match name {
        GENERATE | EDIT | SELECT_SUBJECT | REMOVE_BACKGROUND | UPSCALE | DEPTH_MAP | OUTPAINT | REGENERATE_DETAIL => submit(session, command),
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

fn submit(session: &mut Session, command: &Command) -> CommandResult {
    if session.ai_image.job.as_ref().is_some_and(|job| matches!(job.state, JobState::Running(_))) {
        return Err(CommandError::new("AI_IMAGE_BUSY"));
    }
    let mut request: Request = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
    let removing_background = matches!(command.name.as_str(), REMOVE_BACKGROUND | SELECT_SUBJECT);
    let upscaling = command.name == UPSCALE;
    let depth_map = command.name == DEPTH_MAP;
    let outpainting = command.name == OUTPAINT;
    let regenerates_detail = command.name == REGENERATE_DETAIL;
    let uses_prompt = matches!(command.name.as_str(), GENERATE | EDIT | OUTPAINT | REGENERATE_DETAIL);
    let editing = command.name != GENERATE;
    if request.protect_selection && !regenerates_detail {
        return Err(CommandError::invalid_params());
    }
    if regenerates_detail {
        if command.params.get("strength").is_none() {
            request.strength = 0.20;
        }
        if reference_alignment(&request.model).is_some() || request.use_selection || request.count != 1 || !(0.08..=0.60).contains(&request.strength) {
            return Err(CommandError::new("AI_IMAGE_DETAIL_OPTIONS"));
        }
        if request.prompt.trim().is_empty() {
            request.prompt = comfy::detail::DEFAULT_PROMPT.into();
        }
    }
    if outpainting && (reference_alignment(&request.model).is_some() || request.use_selection) {
        return Err(CommandError::new("AI_IMAGE_OUTPAINT_MODEL"));
    }
    if (!upscaling && request.port == 0) || (!uses_prompt && (request.use_selection || request.count != 1))
        || (uses_prompt && (request.model.trim().is_empty() || request.model.len() > 512
        || request.prompt.trim().is_empty() || request.prompt.len() > 8192 || request.negative.len() > 8192
        || !(1..=4).contains(&request.count) || !(1..=100).contains(&request.steps) || !(0.0..=1.0).contains(&request.strength)
        || (!editing && (request.use_selection || request.source.is_some() || [request.width, request.height].iter()
            .any(|&side| {
                let alignment = reference_alignment(&request.model).unwrap_or(8);
                !(64..=MAX_SIDE).contains(&side) || !side.is_multiple_of(alignment)
            })))))
    {
        return Err(CommandError::invalid_params());
    }
    let source = if editing {
        let id = request.source.or_else(|| match session.selection.ids.as_slice() { [id] => Some(*id), _ => None })
            .ok_or_else(|| CommandError::new("AI_IMAGE_SOURCE"))?;
        if command.name == SELECT_SUBJECT && !session.document.get_object(id)
            .is_some_and(|object| object.kind == ObjectKind::Image)
        {
            return Err(CommandError::new("AI_IMAGE_SUBJECT_SOURCE"));
        }
        Some((id, source_image(session, id)?))
    } else {
        None
    };
    let mask = if request.use_selection || request.protect_selection {
        let (id, image) = source.as_ref().ok_or_else(|| CommandError::new("AI_IMAGE_SELECTION"))?;
        Some(source_mask(session, *id, image)?)
    } else {
        None
    };
    let (source_id, original) = source.map_or((None, None), |(id, image)| {
        (Some(id), Some(Arc::new(image)))
    });
    let borders = if outpainting {
        let id = source_id.ok_or_else(|| CommandError::new("AI_IMAGE_SOURCE"))?;
        let size = outpaint::image_size(&session.document, id).ok_or_else(|| CommandError::new("AI_IMAGE_SOURCE"))?;
        let borders = if request.to_canvas {
            outpaint::canvas_borders(&session.document, id).ok_or_else(|| CommandError::new("AI_IMAGE_OUTPAINT_CANVAS"))?
        } else { request.borders };
        outpaint::output_size(size, borders).ok_or_else(|| CommandError::new("AI_IMAGE_OUTPAINT_SIZE"))?;
        request.strength = 1.0;
        Some(borders)
    } else { None };
    let source_dpi = source_id.filter(|_| upscaling || depth_map || outpainting || regenerates_detail).map(|id| {
        let document = &session.document;
        let frame = crate::commands::picture_frame::get_frame(document, id);
        let image = document.placed_resource(id).or_else(|| frame.as_ref()?.image)
            .and_then(|resource| document.image_resource(resource));
        let fallback = if frame.is_some() { 72.0 } else { document.dpi };
        image.map_or(fallback, ImageResource::effective_dpi)
    });
    let worker_source = original.clone();
    let target = if upscaling || regenerates_detail {
        let source = original.as_ref().ok_or_else(|| CommandError::new("AI_IMAGE_SOURCE"))?;
        let size = [source.width, source.height];
        Some(if regenerates_detail {
            detail_output_size(size, request.scale).ok_or_else(|| CommandError::new("AI_IMAGE_DETAIL_SIZE"))?
        } else {
            upscale::output_size(size, request.scale).ok_or_else(|| CommandError::new("AI_IMAGE_UPSCALE_SIZE"))?
        })
    } else { None };
    let progress = target.map(|_| Arc::new(AtomicU8::new(0)));
    let worker_progress = progress.clone();
    let page = crate::commands::pages::viewed_page(session).or_else(|| session.document.pages.first().map(|page| page.id));
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    std::thread::Builder::new().name("dtool-ai-image".into()).spawn(move || {
        let result = if let (Some(size), Some(progress), Some(source)) = (target, worker_progress, worker_source.as_deref()) {
            if regenerates_detail {
                comfy::detail::run(request, source, size, mask.as_deref(), &progress, &worker_cancel)
            } else {
                upscale::render(source, size, request.scale, request.upscale_model, progress, &worker_cancel)
            }
        } else if removing_background {
            worker_source.as_deref().ok_or_else(|| CommandError::new("AI_IMAGE_SOURCE"))
                .and_then(|source| comfy::remove_background(request.port, source, &worker_cancel))
        } else if depth_map {
            worker_source.as_deref().ok_or_else(|| CommandError::new("AI_IMAGE_SOURCE"))
                .and_then(|source| comfy::depth_map(request.port, source, request.depth_color, &worker_cancel))
        } else if let (Some(borders), Some(source)) = (borders, worker_source.as_deref()) {
            outpaint::run(&request, source, borders, &worker_cancel)
        } else {
            comfy::run(&request, worker_source.as_deref(), mask.as_deref(), &worker_cancel)
        };
        let _ = sender.send(result);
    }).map_err(|_| CommandError::new("AI_IMAGE_RESPONSE"))?;
    session.ai_image.next = session.ai_image.next.saturating_add(1);
    let id = session.ai_image.next;
    session.ai_image.job = Some(Job { id, operation: command.name.clone(), source: source_id, original, page, state: JobState::Running(receiver), cancel, progress, source_dpi, depth_map, borders, regenerates_detail });
    Ok(json!({"job_id": id, "state": "running"}))
}

pub fn source_image(session: &Session, id: ObjectId) -> Result<AiImage, CommandError> {
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
    let progress = job.progress.as_ref().map(|value| if state == "done" { 100 } else { value.load(Ordering::Relaxed) });
    Ok(json!({"job_id": job.id, "operation": job.operation, "state": state, "count": count, "error": error, "source": job.source, "progress": progress}))
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
    if job.operation == SELECT_SUBJECT {
        let mask = subject::mask(session, params, job, &candidate.image)?;
        let before = session.document.pixel_selection.replace(mask);
        let command = Command::new(SELECT_SUBJECT);
        return Ok(crate::commands::pixel_selection::record_mask(session, &command, before));
    }
    let replace = params["replace"].as_bool() == Some(true);
    if replace && job.depth_map {
        return Err(CommandError::new("AI_IMAGE_DEPTH_LAYER"));
    }
    if replace && job.borders.is_some() {
        return Err(CommandError::new("AI_IMAGE_OUTPAINT_LAYER"));
    }
    let mut document = session.document.clone();
    let width = candidate.image.width;
    let height = candidate.image.height;
    let source_frame = job.source.filter(|_| session.ai_image.supports_frame_replacement() || job.depth_map)
        .and_then(|id| crate::commands::picture_frame::get_frame(&document, id));
    let source_dpi = job.source_dpi.zip(job.original.as_ref()).map_or(document.dpi, |(dpi, original)| {
        if job.borders.is_some() { dpi } else { dpi * (width as f32 / original.width as f32) }
    });
    if replace {
        let id = job.source.ok_or_else(|| CommandError::new("AI_IMAGE_SOURCE"))?;
        let original = document.get_object(id).filter(|object| {
            (object.kind == ObjectKind::Image || source_frame.is_some()) && !document.is_object_locked(object)
        })
            .ok_or_else(|| CommandError::new("AI_IMAGE_SOURCE"))?;
        let mut resource = ImageResource::new_embedded(ImageFormat::Png, [width, height], source_dpi, candidate.png.clone());
        resource.name = original.name.clone();
        let resource = crate::resources::replace_placement(&mut document, id, resource, candidate.image.rgba.clone(), false, None)?;
        if let Some(mut frame) = source_frame {
            frame.image = Some(resource);
            crate::commands::picture_frame::put_frame(&mut document, frame);
        }
        return crate::commands::document_change::commit(session, APPLY, document, vec![id], json!({"id": id, "resource": resource}));
    }
    let unit = crate::doc_setup::unit_factor(Units::Px, document.dpi, document.units, document.dpi);
    let mut object = Object {
        name: dtool_strings::text(if job.depth_map { "ai.image.depth_map" }
            else if job.borders.is_some() { "ai.image.outpaint_result" }
            else if job.regenerates_detail { "ai.image.detail_result" }
            else { "ai.image.result_layer" }).to_string(),
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
        if let Some(borders) = job.borders {
            let original = job.original.as_ref().ok_or_else(|| CommandError::new("AI_IMAGE_SOURCE"))?;
            if source.kind != ObjectKind::Image || source.size.iter().any(|side| !side.is_finite() || side.abs() <= f32::EPSILON) {
                return Err(CommandError::new("AI_IMAGE_SOURCE"));
            }
            outpaint::place(source, &mut object, [original.width, original.height], borders);
        }
        if source_frame.is_some() {
            object.kind = ObjectKind::PictureFrame;
            object.fill = source.fill.clone();
            object.stroke = source.stroke.clone();
        }
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
    let mut resource = ImageResource::new_embedded(ImageFormat::Png, [width, height], source_dpi, candidate.png.clone());
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
    if let Some(mut frame) = source_frame {
        frame.object = id;
        frame.image = Some(resource);
        crate::commands::picture_frame::put_frame(&mut document, frame);
    }
    crate::commands::document_change::commit(session, APPLY, document, vec![id], json!({"id": id, "resource": resource}))
}
