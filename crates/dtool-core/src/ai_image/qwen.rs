use serde_json::{Value, json};

use super::{Request, companion};
use crate::CommandError;

pub const ALIGNMENT: u32 = 32;
pub const STEPS: u32 = 8;
const ENCODER: &str = "qwen3vl_8b_int8_convrot.safetensors";
const VAE: &str = "qwen_image_2.1_vae_bf16.safetensors";
// Comfy-Org/workflow_templates cd93398290419239c6e79feddbbb1f5df238c64b,
// Qwen Image 2.1 Turbo generation and edit templates, with prompt enhancement off.
const SIGMAS: &str = "1.0, 0.978453, 0.95418, 0.926626, 0.89508, 0.845148, 0.704534, 0.414568, 0";

pub fn is_model(model: &str) -> bool {
    matches!(
        model.trim().rsplit(['/', '\\']).next(),
        Some("qwen_image_2.1_turbo_int8_convrot.safetensors" | "qwen_image_2.1_turbo_bf16.safetensors")
    )
}

pub(super) struct Assets {
    clip: String,
    vae: String,
}

impl Assets {
    pub(super) fn resolve(model: &str, clips: Vec<String>, vaes: Vec<String>) -> Result<Self, CommandError> {
        let missing = || CommandError::new("AI_IMAGE_QWEN_ASSETS");
        Ok(Self {
            clip: companion(model, &clips, ENCODER).ok_or_else(missing)?,
            vae: companion(model, &vaes, VAE).ok_or_else(missing)?,
        })
    }

    pub(super) fn workflow(&self, request: &Request, image: Option<&str>) -> Value {
        let mut nodes = json!({
            "model": { "class_type": "UNETLoader", "inputs": { "unet_name": request.model, "weight_dtype": "default" } },
            "clip": { "class_type": "CLIPLoader", "inputs": { "clip_name": self.clip, "type": "qwen_image", "device": "default" } },
            "vae": { "class_type": "VAELoader", "inputs": { "vae_name": self.vae } },
            "cache": { "class_type": "QwenImage21Cache", "inputs": { "model": ["model", 0], "device": "auto", "dtype": "default" } },
            "conditioning": { "class_type": "TextEncodeQwenImage21", "inputs": {
                "clip": ["clip", 0], "prompt": request.prompt, "negative_prompt": request.negative,
                "resolution": 0,
            }},
            "guider": { "class_type": "CFGGuider", "inputs": {
                "model": ["cache", 0], "positive": ["conditioning", 0], "negative": ["conditioning", 1],
                "cfg": 1.0,
            }},
            "sampler": { "class_type": "KSamplerSelect", "inputs": { "sampler_name": "euler" } },
            "sigmas": { "class_type": "ManualSigmas", "inputs": { "sigmas": SIGMAS } },
            "noise": { "class_type": "RandomNoise", "inputs": { "noise_seed": request.seed } },
            "sample": { "class_type": "SamplerCustomAdvanced", "inputs": {
                "noise": ["noise", 0], "guider": ["guider", 0], "sampler": ["sampler", 0],
                "sigmas": ["sigmas", 0], "latent_image": ["latent", 0],
            }},
            "decode": { "class_type": "VAEDecode", "inputs": { "samples": ["sample", 0], "vae": ["vae", 0] } },
            "preview": { "class_type": "PreviewImage", "inputs": { "images": ["decode", 0] } },
        });
        if let Some(image) = image {
            nodes["input"] = json!({ "class_type": "LoadImage", "inputs": { "image": image } });
            nodes["conditioning"]["inputs"]["images.image_1"] = json!(["input", 0]);
            nodes["conditioning"]["inputs"]["vae"] = json!(["vae", 0]);
            if request.count > 1 {
                nodes["latent"] = json!({ "class_type": "RepeatLatentBatch", "inputs": {
                    "samples": ["conditioning", 2], "amount": request.count,
                }});
            } else {
                nodes["sample"]["inputs"]["latent_image"] = json!(["conditioning", 2]);
            }
        } else {
            nodes["latent"] = json!({ "class_type": "EmptyLatentImage", "inputs": {
                "width": request.width, "height": request.height, "batch_size": request.count,
            }});
        }
        nodes
    }
}
