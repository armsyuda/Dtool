use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex, TryLockError,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use std::time::{Duration, Instant};

use serde::Deserialize;
use rayon::prelude::*;

use crate::{CommandError, ai::AiImage};

use super::{Candidate, MAX_SIDE, comfy, setup};

pub const MAX_OUTPUT_SIDE: u32 = 4096;
const MODEL_SCALE: u32 = 4;
const RELEASE: &str = "realesrgan-ncnn-vulkan-20220424";
const URL: &str = "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesrgan-ncnn-vulkan-20220424-windows.zip";
const ARCHIVE_BYTES: u64 = 45_474_481;
const ARCHIVE_SHA256: &str = "abc02804e17982a3be33675e4d471e91ea374e65b70167abc09e31acb412802d";
const EXECUTABLE: &str = "realesrgan-ncnn-vulkan.exe";
const FILES: &[&str] = &[
    EXECUTABLE, "vcomp140.dll", "README_windows.md",
    "models/realesrgan-x4plus.bin", "models/realesrgan-x4plus.param",
    "models/realesrgan-x4plus-anime.bin", "models/realesrgan-x4plus-anime.param",
];
static INSTALL: Mutex<()> = Mutex::new(());
static GPU: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Model {
    #[default]
    Standard,
    Illustration,
    PreserveOriginal,
}

impl Model {
    pub fn key(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Illustration => "illustration",
            Self::PreserveOriginal => "preserve_original",
        }
    }

    fn engine_model(self) -> Option<&'static str> {
        match self {
            Self::Standard => Some("realesrgan-x4plus"),
            Self::Illustration => Some("realesrgan-x4plus-anime"),
            Self::PreserveOriginal => None,
        }
    }
}

pub fn supported() -> bool {
    cfg!(all(target_os = "windows", target_arch = "x86_64"))
}

fn directory() -> Result<PathBuf, String> {
    Ok(setup::root()?.join("upscale").join(RELEASE))
}

fn present(directory: &Path) -> bool {
    FILES.iter().all(|file| directory.join(file).is_file())
}

pub fn installed() -> bool {
    supported() && directory().is_ok_and(|dir| present(&dir))
}

pub fn install() -> Result<(), String> {
    if !supported() {
        return Err("이 확대 엔진은 Windows x64에서 사용할 수 있습니다.".into());
    }
    let _install = INSTALL.lock().map_err(|error| error.to_string())?;
    let target = directory()?;
    if present(&target) {
        return Ok(());
    }
    let parent = target.parent().ok_or_else(|| "확대 엔진 위치를 찾을 수 없습니다.".to_owned())?;
    let install = || -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(parent)?;
        let work = tempfile::tempdir_in(parent)?;
        let path = work.path().join("download.zip");
        setup::download(&setup::http(), URL, &path, ARCHIVE_BYTES, ARCHIVE_BYTES, Some(ARCHIVE_SHA256))?;
        let payload = work.path().join("payload");
        fs::create_dir_all(payload.join("models"))?;
        let mut archive = zip::ZipArchive::new(File::open(path)?)?;
        for name in FILES {
            let mut entry = archive.by_name(name)?;
            let mut file = File::create(payload.join(name))?;
            std::io::copy(&mut entry, &mut file)?;
        }
        match fs::rename(payload, &target) {
            Ok(()) => Ok(()),
            Err(_) if present(&target) => Ok(()),
            Err(error) => Err(error.into()),
        }
    };
    install().map_err(|error| error.to_string())
}

pub fn output_size(size: [u32; 2], scale: u32) -> Option<[u32; 2]> {
    if !(2..=4).contains(&scale) || !size.iter().all(|side| (8..=MAX_SIDE).contains(side)) {
        return None;
    }
    let output = size.map(|side| side.saturating_mul(scale));
    output.iter().all(|side| (16..=MAX_OUTPUT_SIDE).contains(side)).then_some(output)
}

pub(super) fn render(
    source: &AiImage,
    size: [u32; 2],
    scale: u32,
    model: Model,
    progress: Arc<AtomicU8>,
    cancelled: &AtomicBool,
) -> Result<Vec<Candidate>, CommandError> {
    let Some(model_name) = model.engine_model() else {
        comfy::check_cancelled(cancelled)?;
        let [width, height] = size;
        let rgba = crate::canvas_size::resize_rgba_lanczos(
            &source.rgba, source.width, source.height, width, height,
        );
        comfy::check_cancelled(cancelled)?;
        return Ok(vec![comfy::encode_candidate(AiImage { width, height, rgba })?]);
    };
    let failed = || CommandError::new("AI_IMAGE_UPSCALE_FAILED");
    let directory = directory().ok().filter(|dir| supported() && present(dir))
        .ok_or_else(|| CommandError::new("AI_IMAGE_UPSCALE_SETUP"))?;
    let _gpu = loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(CommandError::new("CANCELLED"));
        }
        match GPU.try_lock() {
            Ok(guard) => break guard,
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(60)),
            Err(TryLockError::Poisoned(_)) => return Err(failed()),
        }
    };
    let temp = tempfile::tempdir().map_err(|_| failed())?;
    let input = temp.path().join("source.png");
    let output = temp.path().join("result.png");
    let png = crate::path_brush::encode_resource(
        [source.width as usize, source.height as usize], &source.rgba, 96.0,
    ).and_then(|resource| resource.data).ok_or_else(failed)?;
    fs::write(&input, png).map_err(|_| failed())?;
    if cancelled.load(Ordering::Acquire) {
        return Err(CommandError::new("CANCELLED"));
    }
    let mut command = Command::new(directory.join(EXECUTABLE));
    command.current_dir(&directory).arg("-i").arg(input).arg("-o").arg(&output)
        .arg("-m").arg(directory.join("models")).args(["-n", model_name, "-s", "4"])
        .args(["-f", "png", "-j", "1:1:1"])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = setup::hidden(&mut command).spawn().map_err(|_| failed())?;
    let stderr = child.stderr.take().expect("requested piped stderr");
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(value) = line.trim().strip_suffix('%').and_then(|value| value.parse::<f32>().ok()) {
                progress.fetch_max(value.clamp(0.0, 99.0) as u8, Ordering::Relaxed);
            }
        }
    });
    let start = Instant::now();
    let result = loop {
        if cancelled.load(Ordering::Acquire) {
            break Err(CommandError::new("CANCELLED"));
        }
        if start.elapsed() >= Duration::from_secs(900) {
            break Err(CommandError::new("AI_IMAGE_TIMEOUT"));
        }
        match child.try_wait() {
            Ok(Some(exit)) if exit.success() => break Ok(()),
            Ok(Some(_)) | Err(_) => break Err(failed()),
            Ok(None) => std::thread::sleep(Duration::from_millis(60)),
        }
    };
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = reader.join();
    result?;
    let length = fs::metadata(&output).map_err(|_| failed())?.len();
    let native_size = [source.width, source.height].map(|side| side * MODEL_SCALE);
    if length > u64::from(native_size[0]) * u64::from(native_size[1]) * 5 {
        return Err(failed());
    }
    let png = fs::read(output).map_err(|_| failed())?;
    let image = comfy::decode_png(&png, MAX_SIDE * MODEL_SCALE).map_err(|_| failed())?;
    if [image.width, image.height] != native_size {
        return Err(failed());
    }
    if scale == MODEL_SCALE {
        return Ok(vec![Candidate { png, image }]);
    }
    drop(png);
    let image = reduce(image, size, scale, cancelled)?;
    Ok(vec![comfy::encode_candidate(image)?])
}

fn reduce(image: AiImage, [width, height]: [u32; 2], scale: u32, cancelled: &AtomicBool) -> Result<AiImage, CommandError> {
    // Both models infer at 4×. Asking the executable for 2× or 3× misplaces tiles.
    // Integrate each destination pixel's area in units of 1/scale of a source pixel.
    let range = |position: u32| {
        let start = position * MODEL_SCALE;
        let end = start + MODEL_SCALE;
        (start / scale..end.div_ceil(scale)).map(move |sample| {
            let overlap = end.min((sample + 1) * scale) - start.max(sample * scale);
            (sample as usize, overlap)
        })
    };
    let mut rgba = vec![0; width as usize * height as usize * 4];
    rgba.par_chunks_exact_mut(width as usize * 4).enumerate().try_for_each(|(y, row)| {
        if cancelled.load(Ordering::Acquire) {
            return Err(CommandError::new("CANCELLED"));
        }
        for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
            let mut colour = [0; 3];
            let mut alpha = 0;
            for (sy, wy) in range(y as u32) {
                for (sx, wx) in range(x as u32) {
                    let start = (sy * image.width as usize + sx) * 4;
                    let source = &image.rgba[start..start + 4];
                    let weight = u32::from(source[3]) * wx * wy;
                    alpha += weight;
                    for channel in 0..3 {
                        colour[channel] += u32::from(source[channel]) * weight;
                    }
                }
            }
            let area = MODEL_SCALE * MODEL_SCALE;
            pixel[3] = ((alpha + area / 2) / area) as u8;
            if alpha > 0 {
                for channel in 0..3 {
                    pixel[channel] = ((colour[channel] + alpha / 2) / alpha) as u8;
                }
            }
        }
        Ok(())
    })?;
    Ok(AiImage { width, height, rgba })
}
