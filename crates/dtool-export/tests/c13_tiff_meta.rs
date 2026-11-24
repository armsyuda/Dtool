//! C13 TIFF prep: tiff_meta::attach adds ICC/XMP/baseline/EXIF tags without touching pixels.
//! Module is not wired into lib.rs/raster.rs yet (hub + bot10 lock), so it is included by path.
#[path = "../src/tiff_meta.rs"]
mod tiff_meta;

use dtool_core::metadata::DocMetadata;
use image::{ImageDecoder, ImageEncoder};
use tiff_meta::{attach, Error};

struct Ifd { le: bool, entries: Vec<(u16, u16, u32, Vec<u8>)>, next: u32 }

fn u16_at(b: &[u8], le: bool) -> u16 { if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) } }
fn u32_at(b: &[u8], le: bool) -> u32 { let a = b[..4].try_into().unwrap(); if le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) } }

fn ifd_at(bytes: &[u8], le: bool, pos: usize) -> Ifd {
    let n = u16_at(&bytes[pos..], le) as usize;
    let mut entries = Vec::new();
    for i in 0..n {
        let e = &bytes[pos + 2 + i * 12..pos + 14 + i * 12];
        let (id, kind, count) = (u16_at(e, le), u16_at(&e[2..], le), u32_at(&e[4..], le));
        let unit = match kind { 1 | 2 | 7 => 1, 3 => 2, 4 => 4, 5 => 8, k => panic!("kind {k}") };
        let size = unit * count as usize;
        let data = if size <= 4 { e[8..8 + size].to_vec() } else { let at = u32_at(&e[8..], le) as usize; bytes[at..at + size].to_vec() };
        entries.push((id, kind, count, data));
    }
    Ifd { le, entries, next: u32_at(&bytes[pos + 2 + n * 12..], le) }
}

fn ifd0(bytes: &[u8]) -> Ifd {
    let le = &bytes[..2] == b"II";
    ifd_at(bytes, le, u32_at(&bytes[4..], le) as usize)
}

impl Ifd {
    fn get(&self, id: u16) -> Option<&Vec<u8>> { self.entries.iter().find(|e| e.0 == id).map(|e| &e.3) }
    fn count(&self, id: u16) -> usize { self.entries.iter().filter(|e| e.0 == id).count() }
    fn sorted(&self) -> bool { self.entries.windows(2).all(|w| w[0].0 < w[1].0) }
}

fn rgba_tiff() -> (Vec<u8>, Vec<u8>) {
    let pixels: Vec<u8> = (0..3 * 2 * 4).map(|i| (i * 11) as u8).collect();
    let mut cursor = std::io::Cursor::new(Vec::new());
    image::codecs::tiff::TiffEncoder::new(&mut cursor).write_image(&pixels, 3, 2, image::ExtendedColorType::Rgba8).unwrap();
    (cursor.into_inner(), pixels)
}

/// Minimal big-endian 1x1 8-bit gray TIFF; with `pages` 2 a second IFD follows.
fn mm_gray(pages: usize) -> Vec<u8> {
    let mut b = b"MM\0\x2a\0\0\0\x08".to_vec();
    let ifd = |b: &mut Vec<u8>, pixel_at: u32, next: u32| {
        let tags: [(u16, u16, u32); 8] = [(256, 3, 1), (257, 3, 1), (258, 3, 8), (259, 3, 1), (262, 3, 1), (273, 4, pixel_at), (278, 3, 1), (279, 4, 1)];
        b.extend(8u16.to_be_bytes());
        for (id, kind, v) in tags {
            b.extend(id.to_be_bytes());
            b.extend(kind.to_be_bytes());
            b.extend(1u32.to_be_bytes());
            if kind == 3 { b.extend((v as u16).to_be_bytes()); b.extend([0, 0]); } else { b.extend(v.to_be_bytes()); }
        }
        b.extend(next.to_be_bytes());
    };
    let first_len = 2 + 8 * 12 + 4;
    let pixel1 = 8 + first_len as u32;
    let second = if pages == 2 { pixel1 + 2 } else { 0 };
    ifd(&mut b, pixel1, second);
    b.extend([0x5a, 0]);
    if pages == 2 { ifd(&mut b, second + first_len as u32, 0); b.extend([0x22, 0]); }
    b
}

fn meta() -> DocMetadata {
    DocMetadata {
        title: "Poster".into(),
        author: "Park".into(),
        subject: "subj".into(),
        keywords: vec!["a".into(), "b".into()],
        copyright: "(c) 2026".into(),
        description: "plain desc".into(),
        ..DocMetadata::default()
    }
}

fn decode(bytes: &[u8]) -> (Vec<u8>, Option<Vec<u8>>) {
    let mut decoder = image::codecs::tiff::TiffDecoder::new(std::io::Cursor::new(bytes)).unwrap();
    let icc = decoder.icc_profile().unwrap();
    let mut buf = vec![0; decoder.total_bytes() as usize];
    decoder.read_image(&mut buf).unwrap();
    (buf, icc)
}

#[test]
fn little_endian_rgba_gets_icc_xmp_baseline_and_exif_pixels_unchanged() {
    let (tiff, pixels) = rgba_tiff();
    let icc = vec![7u8; 300];
    let m = meta();
    let out = attach(&tiff, Some(&icc), Some(&m)).unwrap();
    assert_eq!(&out[..tiff.len()], &tiff[..4].iter().chain(&out[4..8]).chain(&tiff[8..]).copied().collect::<Vec<_>>()[..], "original bytes kept except header offset");
    let (decoded, read_icc) = decode(&out);
    assert_eq!(decoded, pixels);
    assert_eq!(read_icc.as_deref(), Some(&icc[..]));
    let ifd = ifd0(&out);
    assert!(ifd.le && ifd.sorted());
    assert_eq!(ifd.get(700).unwrap(), &dtool_core::metadata::write_xmp(&m).into_bytes());
    assert_eq!(ifd.get(269).unwrap(), b"Poster\0");
    assert_eq!(ifd.get(270).unwrap(), b"plain desc\0");
    assert_eq!(ifd.get(315).unwrap(), b"Park\0");
    assert_eq!(ifd.get(33432).unwrap(), b"(c) 2026\0");
    assert_eq!(ifd.get(34675).unwrap(), &icc);
    let exif_at = u32_at(ifd.get(34665).unwrap(), true) as usize;
    let exif = ifd_at(&out, true, exif_at);
    assert_eq!(exif.get(36864).unwrap(), b"0232");
    assert_eq!(exif.get(37510).unwrap(), b"ASCII\0\0\0plain desc");
    let original = ifd0(&tiff);
    for (id, _, _, data) in &original.entries {
        assert_eq!(ifd.get(*id), Some(data), "tag {id} kept");
    }
}

#[test]
fn big_endian_multipage_keeps_next_ifd_and_second_page() {
    let tiff = mm_gray(2);
    let before = ifd0(&tiff);
    assert_ne!(before.next, 0);
    let out = attach(&tiff, Some(&[1, 2, 3, 4, 5, 6]), None).unwrap();
    let ifd = ifd0(&out);
    assert!(!ifd.le && ifd.sorted());
    assert_eq!(ifd.next, before.next);
    assert_eq!(ifd.get(34675).unwrap(), &vec![1, 2, 3, 4, 5, 6]);
    assert!(ifd.get(700).is_none() && ifd.get(34665).is_none(), "metadata None leaves metadata alone");
    assert_eq!(ifd_at(&out, false, ifd.next as usize).entries, ifd_at(&tiff, false, before.next as usize).entries);
    let (decoded, icc) = decode(&out);
    assert_eq!(decoded, vec![0x5a]);
    assert_eq!(icc, Some(vec![1, 2, 3, 4, 5, 6]));
}

#[test]
fn second_attach_replaces_tags_and_keeps_existing_exif() {
    let (tiff, pixels) = rgba_tiff();
    let once = attach(&tiff, Some(&[9; 10]), Some(&meta())).unwrap();
    let mut other = meta();
    other.author = "Kim".into();
    other.description = String::new();
    let twice = attach(&once, Some(&[8; 12]), Some(&other)).unwrap();
    let ifd = ifd0(&twice);
    for id in [700, 315, 34675, 34665] { assert_eq!(ifd.count(id), 1, "{id}"); }
    assert_eq!(ifd.get(315).unwrap(), b"Kim\0");
    assert_eq!(ifd.get(270).unwrap(), b"plain desc\0", "absent new value leaves old tag");
    assert_eq!(ifd.get(34665), ifd0(&once).get(34665), "existing EXIF IFD kept");
    assert_eq!(ifd.get(34675).unwrap(), &vec![8; 12]);
    assert_eq!(decode(&twice).0, pixels);
}

#[test]
fn unicode_goes_to_xmp_ascii_tags_fall_back() {
    let (tiff, _) = rgba_tiff();
    let mut m = meta();
    m.description = "한글 설명".into();
    m.author = "박".into();
    let out = attach(&tiff, None, Some(&m)).unwrap();
    let ifd = ifd0(&out);
    assert_eq!(ifd.get(315).unwrap(), b"?\0");
    assert_eq!(ifd.get(270).unwrap(), b"?? ??\0");
    let xmp = String::from_utf8(ifd.get(700).unwrap().clone()).unwrap();
    assert!(xmp.contains("한글 설명") && xmp.contains("박"), "{xmp}");
    let exif = ifd_at(&out, true, u32_at(ifd.get(34665).unwrap(), true) as usize);
    assert!(exif.get(37510).is_none(), "non-ASCII comment not written as ASCII");
    assert!(ifd.get(34675).is_none());
}

#[test]
fn invalid_inputs_are_refused_and_noop_is_identity() {
    let (tiff, _) = rgba_tiff();
    assert_eq!(attach(&tiff, None, None).unwrap(), tiff);
    assert_eq!(attach(b"II*", None, None), Err(Error::InvalidTiff));
    assert_eq!(attach(b"XX\x2a\0\x08\0\0\0", Some(&[1]), None), Err(Error::InvalidTiff));
    assert_eq!(attach(b"II\x2b\0\x08\0\0\0\0\0\0\0\0\0\0\0", Some(&[1]), None), Err(Error::UnsupportedBigTiff));
    let mut bad = tiff.clone();
    bad[4..8].copy_from_slice(&(tiff.len() as u32 + 10).to_le_bytes());
    assert_eq!(attach(&bad, Some(&[1]), None), Err(Error::InvalidTiff));
    assert_eq!(attach(&tiff, Some(&[]), None), Err(Error::InvalidTiff));
    let mut broken = mm_gray(1);
    let ifd = 8usize;
    broken[ifd + 2 + 2..ifd + 2 + 4].copy_from_slice(&4u16.to_be_bytes());
    broken[ifd + 2 + 4..ifd + 2 + 8].copy_from_slice(&100u32.to_be_bytes());
    broken[ifd + 2 + 8..ifd + 2 + 12].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(attach(&broken, Some(&[1]), None), Err(Error::InvalidTiff), "out-of-range retained tag payload");
}
