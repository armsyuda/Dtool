use std::os::raw::{c_int, c_void};

pub fn encode(pixels: &[u8], width: u32, height: u32, quality: f32) -> Result<Vec<u8>, String> {
    let w = c_int::try_from(width).map_err(|_| "width".to_string())?;
    let h = c_int::try_from(height).map_err(|_| "height".to_string())?;
    let stride = width.checked_mul(4).and_then(|value| c_int::try_from(value).ok()).ok_or_else(|| "stride".to_string())?;
    if width == 0 || height == 0 || pixels.len() != (width as usize) * (height as usize) * 4 {
        return Err("size".to_string());
    }
    let mut output: *mut u8 = std::ptr::null_mut();
    let size = unsafe { libwebp_sys::WebPEncodeRGBA(pixels.as_ptr(), w, h, stride, quality.clamp(0.0, 100.0), &mut output) };
    if size == 0 || output.is_null() {
        return Err("encode".to_string());
    }
    let bytes = unsafe { std::slice::from_raw_parts(output, size) }.to_vec();
    unsafe { libwebp_sys::WebPFree(output as *mut c_void) };
    Ok(bytes)
}

pub fn install() -> bool {
    crate::webp::register_lossy_encoder(encode)
}