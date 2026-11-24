#[path = "../src/blend.rs"]
mod blend;

use blend::{blend, blend_pixel, blend_pixel_normal, BlendMode};

fn current_export_blend(pixels: &mut [u8], offset: usize, colour: [u8; 4], opacity: f32) {
    let alpha = f32::from(colour[3]) / 255.0 * opacity;
    if alpha <= 0.0 || !alpha.is_finite() {
        return;
    }
    let old_alpha = f32::from(pixels[offset + 3]) / 255.0;
    let out_alpha = alpha + old_alpha * (1.0 - alpha);
    if out_alpha <= 0.0 {
        return;
    }
    for channel in 0..3 {
        let value = (f32::from(colour[channel]) * alpha + f32::from(pixels[offset + channel]) * old_alpha * (1.0 - alpha)) / out_alpha;
        pixels[offset + channel] = value.round().clamp(0.0, 255.0) as u8;
    }
    pixels[offset + 3] = (out_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
}

fn samples() -> Vec<([u8; 4], [u8; 4], f32)> {
    let mut seed = 0x2545_f491u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed & 0xff) as u8
    };
    let mut out = Vec::new();
    for i in 0..4000 {
        let dst = [next(), next(), next(), if i % 7 == 0 { 0 } else { next() }];
        let src = [next(), next(), next(), if i % 11 == 0 { 255 } else { next() }];
        let opacity = [0.0, 0.25, 0.5, 1.0, f32::NAN, -1.0, 1.5][i % 7];
        out.push((dst, src, opacity));
    }
    out
}

#[test]
fn normal_is_byte_identical_to_current_export_blend() {
    for (dst, src, opacity) in samples() {
        let mut expected = dst;
        current_export_blend(&mut expected, 0, src, opacity);
        for mode in [BlendMode::Normal, BlendMode::PassThrough] {
            let mut got = dst;
            blend_pixel(&mut got, 0, src, opacity, mode);
            assert_eq!(got, expected, "{mode:?} {dst:?} {src:?} {opacity}");
        }
        let mut omitted = dst;
        blend_pixel_normal(&mut omitted, 0, src, opacity);
        assert_eq!(omitted, expected);
    }
    assert_eq!(BlendMode::default(), BlendMode::Normal);
}

#[test]
fn multiply_matches_expected_pixels() {
    let mut full = [200u8, 100, 50, 255];
    blend_pixel(&mut full, 0, [128, 128, 128, 255], 1.0, BlendMode::Multiply);
    assert_eq!(full, [100, 50, 25, 255]);
    let mut half = [200u8, 100, 50, 255];
    blend_pixel(&mut half, 0, [128, 128, 128, 255], 0.5, BlendMode::Multiply);
    assert_eq!(half, [150, 75, 38, 255]);
    let mut empty = [0u8, 0, 0, 0];
    blend_pixel(&mut empty, 0, [10, 20, 30, 255], 1.0, BlendMode::Multiply);
    assert_eq!(empty, [10, 20, 30, 255]);
    let mut offset = [9u8, 9, 9, 9, 200, 100, 50, 255];
    blend_pixel(&mut offset, 4, [128, 128, 128, 255], 1.0, BlendMode::Multiply);
    assert_eq!(offset, [9, 9, 9, 9, 100, 50, 25, 255]);
}

#[test]
fn separable_and_non_separable_modes_follow_w3c_on_opaque_pixels() {
    let b = [0.8, 0.4, 0.2, 1.0];
    let s = [0.5, 0.5, 0.5, 1.0];
    let close = |a: [f32; 4], e: [f32; 3]| (0..3).all(|i| (a[i] - e[i]).abs() < 1e-5) && (a[3] - 1.0).abs() < 1e-6;
    assert!(close(blend(b, s, BlendMode::Screen), [0.9, 0.7, 0.6]));
    assert!(close(blend(b, s, BlendMode::Darken), [0.5, 0.4, 0.2]));
    assert!(close(blend(b, s, BlendMode::Lighten), [0.8, 0.5, 0.5]));
    assert!(close(blend(b, s, BlendMode::Difference), [0.3, 0.1, 0.3]));
    assert!(close(blend(b, s, BlendMode::Exclusion), [0.5, 0.5, 0.5]));
    assert!(close(blend(b, s, BlendMode::Overlay), [0.8, 0.4, 0.2]));
    assert!(close(blend(b, s, BlendMode::HardLight), [0.8, 0.4, 0.2]));
    let grey = blend(b, s, BlendMode::Saturation);
    assert!((grey[0] - grey[1]).abs() < 1e-5 && (grey[1] - grey[2]).abs() < 1e-5);
    let lum = |c: [f32; 4]| 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
    assert!((lum(blend(b, s, BlendMode::Luminosity)) - lum(s)).abs() < 1e-5);
    assert!((lum(blend(b, s, BlendMode::Color)) - lum(b)).abs() < 1e-5);
    for mode in BlendMode::ALL {
        let out = blend(b, [0.3, 0.6, 0.9, 0.0], mode);
        assert!(close(out, [0.8, 0.4, 0.2]), "{mode:?} transparent source changes backdrop");
        let out = blend([0.0, 0.0, 0.0, 0.0], [0.3, 0.6, 0.9, 1.0], mode);
        assert!(close(out, [0.3, 0.6, 0.9]), "{mode:?} empty backdrop shows source");
    }
}
