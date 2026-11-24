use dtool_export::raster::{encode, ExportError, ExportOptions};
use dtool_export::webp::{encode_rgba, is_lossy_webp, lossy_encoder_available, register_lossy_encoder, WebpEncodeError};
use dtool_export::ExportFormat;
use image::RgbaImage;
use std::sync::atomic::{AtomicU32, Ordering};

static LAST_QUALITY: AtomicU32 = AtomicU32::new(0);

fn chunk(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = fourcc.to_vec();
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&body);
    out
}

fn fake_lossy(pixels: &[u8], width: u32, height: u32, quality: f32) -> Result<Vec<u8>, String> {
    if pixels.len() != (width * height * 4) as usize {
        return Err("size".to_string());
    }
    LAST_QUALITY.store(quality as u32, Ordering::SeqCst);
    if quality >= 99.0 {
        return Ok(riff(&[chunk(b"VP8L", &[0x2f, 0, 0, 0, 0])]));
    }
    Ok(riff(&[chunk(b"VP8X", &[0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0]), chunk(b"ALPH", &[0]), chunk(b"VP8 ", &[0, 0, 0])]))
}

#[test]
fn lossy_webp_goes_through_the_registered_encoder_and_must_be_lossy() {
    assert!(register_lossy_encoder(fake_lossy));
    assert!(lossy_encoder_available());
    let source = RgbaImage::from_pixel(8, 4, image::Rgba([10, 20, 30, 128]));
    let opts = ExportOptions { lossless: false, quality: 42, ..ExportOptions::for_format(ExportFormat::Webp) };
    let bytes = encode(&source, &opts).unwrap();
    assert!(is_lossy_webp(&bytes));
    assert_eq!(LAST_QUALITY.load(Ordering::SeqCst), 42);
    let raw = source.into_raw();
    assert_eq!(encode_rgba(8, 4, &raw, false, 100), Err(WebpEncodeError::Failed("webp_not_lossy".to_string())));
    assert!(matches!(encode(&RgbaImage::from_pixel(8, 4, image::Rgba([0, 0, 0, 255])), &ExportOptions { quality: 100, ..opts }), Err(ExportError::Io(_))));
    assert!(!is_lossy_webp(&encode_rgba(8, 4, &raw, true, 42).unwrap()));
}