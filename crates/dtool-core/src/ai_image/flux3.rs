use serde_json::{Value, json};

use crate::CommandError;

use super::{Request, companion};

pub const REPOSITORY: &str = "black-forest-labs/flux-3-image-klein-comfy";
pub const ALIGNMENT: u32 = 16;

pub fn steps(model: &str) -> Option<u32> {
    match model.trim().rsplit(['/', '\\']).next()? {
        "flux3-image-klein-bf16.safetensors"
        | "flux3-image-klein-int8-convrot.safetensors"
        | "flux3-image-klein-nvfp4.safetensors" => Some(16),
        "flux3-image-klein-base-bf16.safetensors"
        | "flux3-image-klein-base-int8-convrot.safetensors"
        | "flux3-image-klein-base-nvfp4.safetensors" => Some(50),
        _ => None,
    }
}

pub(super) struct Assets {
    clip: String,
    upsampler: String,
    vae: String,
}

impl Assets {
    pub(super) fn resolve(model: &str, clips: &[String], vaes: &[String]) -> Result<Self, CommandError> {
        let missing = || CommandError::new("AI_IMAGE_FLUX3_ASSETS");
        let pick_clip = |names: &[&str]| names.iter().find_map(|name| companion(model, clips, name));
        Ok(Self {
            clip: pick_clip(&["qwen3vl_4b_int8_convrot.safetensors", "qwen3vl_4b_bf16.safetensors"]).ok_or_else(missing)?,
            upsampler: pick_clip(&[
                "qwen3.5_9b_flux3_prompt_upsampler_w4a8.safetensors",
                "qwen3.5_9b_flux3_prompt_upsampler_int8_convrot.safetensors",
                "qwen3.5_9b_flux3_prompt_upsampler_bf16.safetensors",
            ]).ok_or_else(missing)?,
            vae: companion(model, vaes, "flux2-vae.safetensors").ok_or_else(missing)?,
        })
    }

    pub(super) fn workflow(&self, request: &Request, input: Option<&str>) -> Value {
        let editing = input.is_some();
        let steps = steps(&request.model).unwrap_or(16);
        let system = if editing {
            "Expand the user's edit using the attached reference. Output one English instruction specifying the change, its location, lighting and what stays unchanged. Keep faces, identity, and framing unless the request changes them. Preserve the original script of any quoted text. Give only the editing instruction."
        } else {
            "Expand the user's request into one concrete English image caption describing subject, placement, lighting, medium and background. Follow all constraints, keep quoted text in the requested language, and output only the caption."
        };
        let prompt = if request.negative.trim().is_empty() {
            request.prompt.clone()
        } else {
            format!("{}\nAvoid depicting: {}", request.prompt, request.negative)
        };
        let mut graph = json!({
            "model": { "class_type": "UNETLoader", "inputs": { "unet_name": request.model, "weight_dtype": "default" } },
            "encoder": { "class_type": "CLIPLoader", "inputs": { "clip_name": self.clip, "type": "flux3", "device": "default" } },
            "pup": { "class_type": "CLIPLoader", "inputs": { "clip_name": self.upsampler, "type": "flux3", "device": "default" } },
            "vae": { "class_type": "VAELoader", "inputs": { "vae_name": self.vae } },
            "caption": { "class_type": "TextGenerateV2", "inputs": {
                "clip": ["pup", 0], "prompt": prompt, "system_prompt": system,
                "max_length": 3584, "thinking": false, "use_default_template": true, "mtp": "off",
                "sampling_mode": "on", "sampling_mode.temperature": 1.0, "sampling_mode.top_k": 1,
                "sampling_mode.top_p": 0.95, "sampling_mode.min_p": 0.0,
                "sampling_mode.repetition_penalty": 1.05, "sampling_mode.presence_penalty": 0.0,
                "sampling_mode.seed": request.seed,
            }},
            "positive": { "class_type": "CLIPTextEncode", "inputs": { "clip": ["encoder", 0], "text": ["caption", 0] } },
            "negative": { "class_type": "ConditioningZeroOut", "inputs": { "conditioning": ["positive", 0] } },
            "sample": { "class_type": "KSampler", "inputs": {
                "model": ["model", 0], "positive": ["positive", 0], "negative": ["negative", 0],
                "latent_image": ["latent", 0], "seed": request.seed, "steps": steps,
                "cfg": if steps == 16 { 1.0 } else { 3.5 }, "sampler_name": "euler", "scheduler": "simple", "denoise": 1.0,
            }},
            "decode": { "class_type": "VAEDecode", "inputs": { "samples": ["sample", 0], "vae": ["vae", 0] } },
            "preview": { "class_type": "PreviewImage", "inputs": { "images": ["decode", 0] } },
        });
        if let Some(image) = input {
            graph["input"] = json!({ "class_type": "LoadImage", "inputs": { "image": image } });
            graph["caption"]["inputs"]["images.image1"] = json!(["input", 0]);
            graph["reference"] = json!({ "class_type": "ReferenceMultiLatent", "inputs": {
                "positive": ["positive", 0], "negative": ["negative", 0], "vae": ["vae", 0],
                "images.image0": ["input", 0], "resolution": "keep resolution",
            }});
            graph["sample"]["inputs"]["positive"] = json!(["reference", 0]);
            graph["sample"]["inputs"]["negative"] = json!(["reference", 1]);
            if request.count > 1 {
                graph["latent"] = json!({ "class_type": "RepeatLatentBatch", "inputs": {
                    "samples": ["reference", 2], "amount": request.count,
                }});
            } else {
                graph["sample"]["inputs"]["latent_image"] = json!(["reference", 2]);
            }
        } else {
            graph["latent"] = json!({ "class_type": "EmptyFlux2LatentImage", "inputs": {
                "width": request.width, "height": request.height, "batch_size": request.count,
            }});
        }
        graph
    }
}
