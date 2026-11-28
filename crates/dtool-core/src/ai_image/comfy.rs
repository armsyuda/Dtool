use std::io::{Cursor, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::CommandError;
use crate::ai::AiImage;

use super::{Candidate, Request, MAX_SIDE, flux3, qwen, reference_alignment};

pub(super) mod detail;

const WAIT_LIMIT: Duration = Duration::from_secs(900);
const MAX_RESPONSE: u64 = 32 * 1024 * 1024;
const OUTPUT_NODE: &str = "preview";

pub(super) fn run(
    request: &Request,
    source: Option<&AiImage>,
    mask: Option<&[u8]>,
    cancelled: &AtomicBool,
) -> Result<Vec<Candidate>, CommandError> {
    let base = format!("http://127.0.0.1:{}", request.port);
    let agent = agent();
    check_cancelled(cancelled)?;
    let qwen_assets = qwen::is_model(&request.model).then(|| {
        qwen::Assets::resolve(
            &request.model,
            loader_models(&agent, &base, "CLIPLoader", "clip_name")?,
            loader_models(&agent, &base, "VAELoader", "vae_name")?,
        )
    }).transpose()?;
    let flux_assets = flux3::steps(&request.model).map(|_| {
        let clips = loader_models(&agent, &base, "CLIPLoader", "clip_name")?;
        let vaes = loader_models(&agent, &base, "VAELoader", "vae_name")?;
        let diffusion = loader_models(&agent, &base, "UNETLoader", "unet_name")?;
        if !diffusion.contains(&request.model) {
            return Err(CommandError::new("AI_IMAGE_FLUX3_ASSETS"));
        }
        flux3::Assets::resolve(&request.model, &clips, &vaes)
    }).transpose()?;
    let alignment = reference_alignment(&request.model);
    let reference = alignment.zip(source).and_then(|(alignment, image)| {
        (!image.width.is_multiple_of(alignment) || !image.height.is_multiple_of(alignment))
            .then(|| pad_reference(image, alignment))
    });
    let padded = source.zip(mask).filter(|_| alignment.is_none())
        .map(|(image, mask)| masked_input(image, mask));
    let input = reference.as_ref().or_else(|| padded.as_ref().map(|(image, _)| image)).or(source);
    check_cancelled(cancelled)?;
    let image = input.map(|image| upload(&agent, &base, image)).transpose()?;
    check_cancelled(cancelled)?;
    let mask_file = padded.as_ref().map(|(_, mask)| upload(&agent, &base, mask)).transpose()?;
    check_cancelled(cancelled)?;

    let graph = if let Some(assets) = flux_assets {
        assets.workflow(request, image.as_deref())
    } else if let Some(assets) = qwen_assets {
        assets.workflow(request, image.as_deref())
    } else {
        workflow(request, image.as_deref(), mask_file.as_deref())
    };
    execute_graph(&agent, &base, graph, request.count, |candidate| {
        if let Some((original, coverage)) = source.zip(mask) {
            merge_masked(original, coverage, &candidate.image, alignment.unwrap_or(8))
        } else if let (Some(original), Some(padded)) = (source, reference.as_ref()) {
            let edited = &candidate.image;
            if (edited.width, edited.height) != (padded.width, padded.height) {
                return Err(CommandError::new("AI_IMAGE_RESPONSE"));
            }
            let input_width = edited.width as usize * 4;
            let output_width = original.width as usize * 4;
            let rgba = edited.rgba.chunks_exact(input_width).take(original.height as usize)
                .flat_map(|row| row[..output_width].iter().copied()).collect();
            encode_candidate(AiImage { width: original.width, height: original.height, rgba })
        } else {
            Ok(candidate)
        }
    }, cancelled)
}

pub(super) fn remove_background(
    port: u16,
    source: &AiImage,
    cancelled: &AtomicBool,
) -> Result<Vec<Candidate>, CommandError> {
    let base = format!("http://127.0.0.1:{port}");
    let agent = agent();
    check_cancelled(cancelled)?;
    let model = named_model(&agent, &base, "background_removal", "birefnet.safetensors", "AI_IMAGE_BACKGROUND_MODEL")?;
    check_cancelled(cancelled)?;
    let image = upload(&agent, &base, source)?;
    let graph = json!({
        "input": { "class_type": "LoadImage", "inputs": { "image": image } },
        "model": { "class_type": "LoadBackgroundRemovalModel", "inputs": { "bg_removal_name": model } },
        "foreground": { "class_type": "RemoveBackground", "inputs": {
            "bg_removal_model": ["model", 0], "image": ["input", 0],
        }},
        "matte": { "class_type": "MaskToImage", "inputs": { "mask": ["foreground", 0] } },
        "preview": { "class_type": "PreviewImage", "inputs": { "images": ["matte", 0] } },
    });
    execute_graph(&agent, &base, graph, 1, |mask| {
        if (mask.image.width, mask.image.height) != (source.width, source.height) {
            return Err(CommandError::new("AI_IMAGE_RESPONSE"));
        }
        let mut result = source.clone();
        for (pixel, matte) in result.rgba.chunks_exact_mut(4).zip(mask.image.rgba.chunks_exact(4)) {
            pixel[3] = ((u16::from(pixel[3]) * u16::from(matte[0]) + 127) / 255) as u8;
        }
        encode_candidate(result)
    }, cancelled)
}

pub(super) fn depth_map(
    port: u16,
    source: &AiImage,
    color: bool,
    cancelled: &AtomicBool,
) -> Result<Vec<Candidate>, CommandError> {
    let base = format!("http://127.0.0.1:{port}");
    let agent = agent();
    check_cancelled(cancelled)?;
    let model = named_model(&agent, &base, "geometry_estimation", "depth_anything_3_small.safetensors", "AI_IMAGE_DEPTH_MODEL")?;
    check_cancelled(cancelled)?;
    let image = upload(&agent, &base, source)?;
    let graph = json!({
        "input": { "class_type": "LoadImage", "inputs": { "image": image } },
        "model": { "class_type": "LoadDA3Model", "inputs": { "model_name": model, "weight_dtype": "default" } },
        "depth": { "class_type": "DA3Inference", "inputs": {
            "da3_model": ["model", 0], "image": ["input", 0], "resolution": 504,
            "resize_method": "upper_bound_resize", "mode": "mono",
        }},
        "map": { "class_type": "DA3Render", "inputs": {
            "da3_geometry": ["depth", 0], "output": if color { "depth_colored" } else { "depth" },
            "output.normalization": "v2_style", "output.apply_sky_clip": false,
        }},
        "preview": { "class_type": "PreviewImage", "inputs": { "images": ["map", 0] } },
    });
    execute_graph(&agent, &base, graph, 1, |mut candidate| {
        if (candidate.image.width, candidate.image.height) != (source.width, source.height) {
            return Err(CommandError::new("AI_IMAGE_RESPONSE"));
        }
        if source.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255) {
            return Ok(candidate);
        }
        for (output, input) in candidate.image.rgba.chunks_exact_mut(4).zip(source.rgba.chunks_exact(4)) {
            output[3] = input[3];
        }
        encode_candidate(candidate.image)
    }, cancelled)
}

fn named_model(
    agent: &ureq::Agent,
    base: &str,
    category: &str,
    filename: &str,
    missing: &str,
) -> Result<String, CommandError> {
    let models = agent.get(&format!("{base}/models/{category}")).call()
        .map_err(|error| match error {
            ureq::Error::Status(404, _) => CommandError::new("AI_IMAGE_RESPONSE"),
            other => http_error(other),
        }).and_then(read_json)?;
    models.as_array().ok_or_else(|| CommandError::new("AI_IMAGE_RESPONSE"))?
        .iter().filter_map(Value::as_str)
        .filter(|name| name.rsplit(['/', '\\']).next() == Some(filename))
        .min_by(|left, right| left.len().cmp(&right.len()).then_with(|| left.cmp(right)))
        .map(str::to_owned).ok_or_else(|| CommandError::new(missing))
}

fn execute_graph(
    agent: &ureq::Agent,
    base: &str,
    graph: Value,
    count: u32,
    postprocess: impl Fn(Candidate) -> Result<Candidate, CommandError>,
    cancelled: &AtomicBool,
) -> Result<Vec<Candidate>, CommandError> {
    check_cancelled(cancelled)?;
    let response = agent.post(&format!("{base}/prompt"))
        .set("Content-Type", "application/json").send_string(&json!({ "prompt": graph }).to_string())
        .map_err(http_error)?;
    let response = read_json(response)?;
    let id = response["prompt_id"].as_str().filter(|id| {
        !id.is_empty() && id.len() <= 100 && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }).ok_or_else(|| CommandError::new("AI_IMAGE_RESPONSE"))?;
    let result = wait_for_candidates(agent, base, id, count, postprocess, cancelled);
    if result.is_err() {
        // Delete only this queued prompt. A global interrupt could stop someone else's work.
        let _ = agent.post(&format!("{base}/queue")).set("Content-Type", "application/json")
            .send_string(&json!({"delete": [id]}).to_string());
    }
    result
}

pub(super) fn check_cancelled(cancelled: &AtomicBool) -> Result<(), CommandError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(CommandError::new("CANCELLED"));
    }
    Ok(())
}

fn wait_for_candidates(
    agent: &ureq::Agent,
    base: &str,
    id: &str,
    count: u32,
    postprocess: impl Fn(Candidate) -> Result<Candidate, CommandError>,
    cancelled: &AtomicBool,
) -> Result<Vec<Candidate>, CommandError> {
    let started = Instant::now();
    while started.elapsed() < WAIT_LIMIT {
        check_cancelled(cancelled)?;
        let history = agent.get(&format!("{base}/history/{id}")).call().map_err(http_error)
            .and_then(read_json);
        check_cancelled(cancelled)?;
        let history = history?;
        if let Some(item) = history.get(id) {
            if item["status"]["status_str"].as_str() == Some("error") {
                return Err(CommandError::new("AI_IMAGE_WORKFLOW"));
            }
            if let Some(images) = item["outputs"][OUTPUT_NODE]["images"].as_array() {
                let candidates = images.iter().take(count as usize)
                    .map(|image| {
                        check_cancelled(cancelled)?;
                        let candidate = fetch(agent, base, image)?;
                        postprocess(candidate)
                    }).collect::<Result<Vec<_>, _>>()?;
                check_cancelled(cancelled)?;
                if !candidates.is_empty() {
                    return Ok(candidates);
                }
            }
            if item["status"]["completed"].as_bool() == Some(true) {
                return Err(CommandError::new("AI_IMAGE_RESPONSE"));
            }
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    Err(CommandError::new("AI_IMAGE_TIMEOUT"))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().redirects(0)
        .timeout_connect(Duration::from_secs(3))
        .timeout(Duration::from_secs(20))
        .build()
}

pub(super) fn models(port: u16) -> Result<Vec<String>, CommandError> {
    let agent = agent();
    let base = format!("http://127.0.0.1:{port}");
    let mut checkpoints = loader_models(&agent, &base, "CheckpointLoaderSimple", "ckpt_name")?;
    if let Ok(models) = loader_models(&agent, &base, "UNETLoader", "unet_name") {
        for model in models.into_iter().filter(|model| reference_alignment(model).is_some()) {
            if !checkpoints.contains(&model) {
                checkpoints.push(model);
            }
        }
    }
    Ok(checkpoints)
}

fn loader_models(agent: &ureq::Agent, base: &str, loader: &str, input: &str) -> Result<Vec<String>, CommandError> {
    let response = agent.get(&format!("{base}/object_info/{loader}")).call().map_err(http_error)?;
    let data = read_json(response)?;
    let models = data[loader]["input"]["required"][input][0]
        .as_array().ok_or_else(|| CommandError::new("AI_IMAGE_RESPONSE"))?;
    Ok(models.iter().filter_map(Value::as_str).map(str::to_owned).collect())
}

fn http_error(error: ureq::Error) -> CommandError {
    match error {
        ureq::Error::Status(400 | 422, _) => CommandError::new("AI_IMAGE_WORKFLOW"),
        _ => CommandError::new("AI_IMAGE_OFFLINE"),
    }
}

fn read_json(response: ureq::Response) -> Result<Value, CommandError> {
    let mut bytes = response.into_reader().take(1024 * 1024);
    serde_json::from_reader(&mut bytes).map_err(|_| CommandError::new("AI_IMAGE_RESPONSE"))
}

fn upload(agent: &ureq::Agent, base: &str, image: &AiImage) -> Result<String, CommandError> {
    let image = crate::path_brush::encode_resource([image.width as usize, image.height as usize], &image.rgba, 96.0)
        .ok_or_else(|| CommandError::new("AI_IMAGE_SOURCE"))?;
    let bytes = image.data.ok_or_else(|| CommandError::new("AI_IMAGE_SOURCE"))?;
    let boundary = format!("dtool-{}", uuid_for_upload());
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{boundary}.png\"\r\nContent-Type: image/png\r\n\r\n",
    ).into_bytes();
    body.extend_from_slice(&bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let response = agent.post(&format!("{base}/upload/image"))
        .set("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
        .send_bytes(&body).map_err(http_error)?;
    let result = read_json(response)?;
    let file = result["name"].as_str().filter(|file| !file.is_empty())
        .ok_or_else(|| CommandError::new("AI_IMAGE_RESPONSE"))?;
    let subfolder = result["subfolder"].as_str().unwrap_or_default();
    Ok(if subfolder.is_empty() { file.to_owned() } else { format!("{subfolder}/{file}") })
}

fn uuid_for_upload() -> String {
    use std::sync::atomic::AtomicU64;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let tick = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{:x}-{tick:x}-{:x}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

fn fetch(agent: &ureq::Agent, base: &str, image: &Value) -> Result<Candidate, CommandError> {
    let file = image["filename"].as_str().filter(|file| !file.is_empty())
        .ok_or_else(|| CommandError::new("AI_IMAGE_RESPONSE"))?;
    let folder = image["subfolder"].as_str().unwrap_or_default();
    let kind = image["type"].as_str().filter(|kind| matches!(*kind, "temp" | "output"))
        .ok_or_else(|| CommandError::new("AI_IMAGE_RESPONSE"))?;
    let response = agent.get(&format!("{base}/view")).query("filename", file).query("subfolder", folder)
        .query("type", kind).call().map_err(http_error)?;
    let mut png = Vec::new();
    response.into_reader().take(MAX_RESPONSE + 1).read_to_end(&mut png)
        .map_err(|_| CommandError::new("AI_IMAGE_RESPONSE"))?;
    if png.len() as u64 > MAX_RESPONSE {
        return Err(CommandError::new("AI_IMAGE_RESPONSE"));
    }
    let image = decode(&png)?;
    Ok(Candidate { png, image })
}

fn decode(bytes: &[u8]) -> Result<AiImage, CommandError> {
    decode_png(bytes, MAX_SIDE)
}

pub(super) fn decode_png(bytes: &[u8], max_side: u32) -> Result<AiImage, CommandError> {
    let invalid = || CommandError::new("AI_IMAGE_RESPONSE");
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| invalid())?;
    let (width, height) = (reader.info().width, reader.info().height);
    if width == 0 || height == 0 || width > max_side || height > max_side {
        return Err(invalid());
    }
    let mut pixels = vec![0; reader.output_buffer_size().ok_or_else(invalid)?];
    let frame = reader.next_frame(&mut pixels).map_err(|_| invalid())?;
    pixels.truncate(frame.buffer_size());
    let rgba = match frame.color_type {
        png::ColorType::Rgba => pixels,
        png::ColorType::Rgb => pixels.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => pixels.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => pixels.into_iter().flat_map(|v| [v, v, v, 255]).collect(),
        png::ColorType::Indexed => return Err(invalid()),
    };
    Ok(AiImage { width, height, rgba })
}

fn masked_input(source: &AiImage, mask: &[u8]) -> (AiImage, AiImage) {
    let width = source.width.next_multiple_of(8);
    let height = source.height.next_multiple_of(8);
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    let mut coverage = Vec::with_capacity(rgba.capacity());
    for y in 0..height {
        for x in 0..width {
            let index = (y.min(source.height - 1) * source.width + x.min(source.width - 1)) as usize;
            rgba.extend_from_slice(&source.rgba[index * 4..index * 4 + 4]);
            let value = if x < source.width && y < source.height { mask[index] } else { 0 };
            coverage.extend_from_slice(&[value, value, value, 255]);
        }
    }
    (AiImage { width, height, rgba }, AiImage { width, height, rgba: coverage })
}

fn pad_reference(image: &AiImage, alignment: u32) -> AiImage {
    let width = image.width.next_multiple_of(alignment);
    let height = image.height.next_multiple_of(alignment);
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let p = (y.min(image.height - 1) * image.width + x.min(image.width - 1)) as usize * 4;
            rgba.extend_from_slice(&image.rgba[p..p + 4]);
        }
    }
    AiImage { width, height, rgba }
}

fn merge_masked(source: &AiImage, mask: &[u8], edited: &AiImage, alignment: u32) -> Result<Candidate, CommandError> {
    if edited.width != source.width.next_multiple_of(alignment)
        || edited.height != source.height.next_multiple_of(alignment)
    {
        return Err(CommandError::new("AI_IMAGE_RESPONSE"));
    }
    let mut image = source.clone();
    for (index, pixel) in image.rgba.chunks_exact_mut(4).enumerate() {
        let strength = u16::from(mask[index]);
        if strength == 0 { continue; }
        let (x, y) = (index % source.width as usize, index / source.width as usize);
        let offset = (y * edited.width as usize + x) * 4;
        for (sample, &result) in pixel.iter_mut().zip(&edited.rgba[offset..offset + 4]) {
            *sample = ((u16::from(*sample) * (255 - strength) + u16::from(result) * strength + 127) / 255) as u8;
        }
    }
    encode_candidate(image)
}

pub(super) fn encode_candidate(image: AiImage) -> Result<Candidate, CommandError> {
    let png = crate::path_brush::encode_resource([image.width as usize, image.height as usize], &image.rgba, 96.0)
        .and_then(|image| image.data).ok_or_else(|| CommandError::new("AI_IMAGE_RESPONSE"))?;
    Ok(Candidate { png, image })
}

fn workflow(request: &Request, image: Option<&str>, mask: Option<&str>) -> Value {
    let mut nodes = json!({
        "model": { "class_type": "CheckpointLoaderSimple", "inputs": { "ckpt_name": request.model }},
        "positive": { "class_type": "CLIPTextEncode", "inputs": { "text": request.prompt, "clip": ["model", 1] }},
        "negative": { "class_type": "CLIPTextEncode", "inputs": { "text": request.negative, "clip": ["model", 1] }},
        "sample": { "class_type": "KSampler", "inputs": {
            "model": ["model", 0], "positive": ["positive", 0], "negative": ["negative", 0],
            "latent_image": ["latent", 0], "seed": request.seed, "steps": request.steps,
            "cfg": 7.0, "sampler_name": "euler", "scheduler": "normal",
            "denoise": if image.is_some() { request.strength } else { 1.0 },
        }},
        "decode": { "class_type": "VAEDecode", "inputs": { "samples": ["sample", 0], "vae": ["model", 2] }},
        "preview": { "class_type": "PreviewImage", "inputs": { "images": ["decode", 0] }},
    });
    if let Some(image) = image {
        nodes["input"] = json!({ "class_type": "LoadImage", "inputs": { "image": image }});
        nodes["latent"] = json!({ "class_type": "VAEEncode", "inputs": { "pixels": ["input", 0], "vae": ["model", 2] }});
        let mut latent = "latent";
        if let Some(mask) = mask {
            nodes["mask"] = json!({ "class_type": "LoadImageMask", "inputs": { "image": mask, "channel": "red" }});
            nodes["masked"] = json!({ "class_type": "SetLatentNoiseMask", "inputs": { "samples": [latent, 0], "mask": ["mask", 0] }});
            latent = "masked";
        }
        if request.count > 1 {
            nodes["batch"] = json!({ "class_type": "RepeatLatentBatch", "inputs": { "samples": [latent, 0], "amount": request.count }});
            latent = "batch";
        }
        nodes["sample"]["inputs"]["latent_image"] = json!([latent, 0]);
    } else {
        nodes["latent"] = json!({ "class_type": "EmptyLatentImage", "inputs": {
            "width": request.width, "height": request.height, "batch_size": request.count,
        }});
    }
    nodes
}
