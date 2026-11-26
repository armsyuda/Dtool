use std::io::{Cursor, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::CommandError;
use crate::ai::AiImage;

use super::{Candidate, Request, MAX_SIDE};

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
    let padded = source.zip(mask).map(|(image, mask)| masked_input(image, mask));
    let input = padded.as_ref().map(|(image, _)| image).or(source);
    let image = input.map(|image| upload(&agent, &base, image)).transpose()?;
    let mask_file = padded.as_ref().map(|(_, mask)| upload(&agent, &base, mask)).transpose()?;
    if cancelled.load(Ordering::Acquire) {
        return Err(CommandError::new("CANCELLED"));
    }

    let response = agent.post(&format!("{base}/prompt"))
        .set("Content-Type", "application/json").send_string(&json!({ "prompt": workflow(request, image.as_deref(), mask_file.as_deref()) }).to_string())
        .map_err(http_error)?;
    let response = read_json(response)?;
    let id = response["prompt_id"].as_str().filter(|id| {
        !id.is_empty() && id.len() <= 100 && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }).ok_or_else(|| CommandError::new("AI_IMAGE_RESPONSE"))?;

    let started = Instant::now();
    while started.elapsed() < WAIT_LIMIT {
        if cancelled.load(Ordering::Acquire) {
            let _ = agent.post(&format!("{base}/queue")).set("Content-Type", "application/json").send_string(&json!({"delete": [id]}).to_string());
            return Err(CommandError::new("CANCELLED"));
        }
        let response = agent.get(&format!("{base}/history/{id}")).call().map_err(http_error)?;
        let history = read_json(response)?;
        if let Some(item) = history.get(id) {
            if item["status"]["status_str"].as_str() == Some("error") {
                return Err(CommandError::new("AI_IMAGE_WORKFLOW"));
            }
            if let Some(images) = item["outputs"][OUTPUT_NODE]["images"].as_array() {
                let candidates = images.iter().take(request.count as usize)
                    .map(|image| {
                        let candidate = fetch(&agent, &base, image)?;
                        match source.zip(mask) {
                            Some((source, mask)) => merge_masked(source, mask, &candidate.image),
                            None => Ok(candidate),
                        }
                    }).collect::<Result<Vec<_>, _>>()?;
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
    let _ = agent.post(&format!("{base}/queue")).set("Content-Type", "application/json").send_string(&json!({"delete": [id]}).to_string());
    Err(CommandError::new("AI_IMAGE_TIMEOUT"))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().redirects(0)
        .timeout_connect(Duration::from_secs(3))
        .timeout(Duration::from_secs(20))
        .build()
}

pub(super) fn models(port: u16) -> Result<Vec<String>, CommandError> {
    let response = agent().get(&format!("http://127.0.0.1:{port}/object_info/CheckpointLoaderSimple"))
        .call().map_err(http_error)?;
    let data = read_json(response)?;
    let models = data["CheckpointLoaderSimple"]["input"]["required"]["ckpt_name"][0]
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
    let invalid = || CommandError::new("AI_IMAGE_RESPONSE");
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| invalid())?;
    let (width, height) = (reader.info().width, reader.info().height);
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
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

fn merge_masked(source: &AiImage, mask: &[u8], edited: &AiImage) -> Result<Candidate, CommandError> {
    let invalid = || CommandError::new("AI_IMAGE_RESPONSE");
    if edited.width != source.width.next_multiple_of(8) || edited.height != source.height.next_multiple_of(8) {
        return Err(invalid());
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
    let png = crate::path_brush::encode_resource([image.width as usize, image.height as usize], &image.rgba, 96.0)
        .and_then(|image| image.data).ok_or_else(invalid)?;
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
