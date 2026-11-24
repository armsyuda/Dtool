//! K08 clip/mask in PDF (true-path `W* n`) and SVG (`<clipPath>` / `<mask>`), checked against the PNG export.
use dtool_core::document::{Document, Units};
use dtool_core::object::{ClipRole, Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_export::pdf::{export_pdf, PdfOptions};
use dtool_export::svg::{export_svg, SvgOptions};
use dtool_export::{ExportArea, ExportControl, ExportRequest};
use serde_json::json;

const H: f32 = 80.0;

fn rect(id: u64, pos: [f32; 2], size: [f32; 2], c: [u8; 3]) -> Object {
    Object { id: ObjectId(id), name: format!("o{id}"), pos, size, fill: Paint::Solid { colour: Colour::new(c[0], c[1], c[2], 255) }, ..Object::default() }
}

fn base() -> Document {
    let mut doc = Document::new([80.0, 80.0], Units::Pt, 72.0, 1);
    doc.objects.push(rect(1, [0.0, 0.0], [40.0, 40.0], [255, 0, 0]));
    doc.objects.push(rect(2, [20.0, 20.0], [40.0, 40.0], [0, 0, 255]));
    doc.next_object_id = 10;
    doc
}

fn clipped() -> Document {
    let mut d = base();
    d.objects[1].parent_id = Some(ObjectId(1));
    d.objects[1].clip_role = ClipRole::Clip;
    d
}

fn masked() -> Document {
    let mut d = clipped();
    let mut mask = rect(3, [0.0, 0.0], [30.0, 80.0], [0, 255, 0]);
    mask.parent_id = Some(ObjectId(1));
    mask.clip_role = ClipRole::Mask;
    d.objects.push(mask);
    d
}

fn out_path(name: &str, ext: &str) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("k08_pdf_svg_clip");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{name}.{ext}"))
}

fn pdf_content(doc: &Document, name: &str) -> String {
    let path = out_path(name, "pdf");
    let req = ExportRequest { format: dtool_export::raster::parse_format("pdf").unwrap(), area: ExportArea::WholeDocument, pages: None, path: path.clone(), preset: None, options: json!({}) };
    export_pdf(doc, &req, &PdfOptions::default(), &ExportControl::new()).expect("pdf export");
    let pdf = std::fs::read(&path).unwrap();
    let mut text = String::new();
    for stream in streams(&pdf) {
        let content = if stream.first() == Some(&0x78) { std::panic::catch_unwind(|| inflate::zlib(&stream)).ok() } else { Some(stream.clone()) };
        if let Some(content) = content {
            text.push_str(&String::from_utf8_lossy(&content));
            text.push('\n');
        }
    }
    text
}

fn svg_text(doc: &Document, name: &str) -> String {
    let path = out_path(name, "svg");
    let req = ExportRequest { format: dtool_export::raster::parse_format("svg").unwrap(), area: ExportArea::WholeDocument, pages: None, path: path.clone(), preset: None, options: json!({}) };
    export_svg(doc, &req, &SvgOptions::default(), &ExportControl::new()).expect("svg export");
    std::fs::read_to_string(&path).unwrap()
}

type Rings = Vec<Vec<[f32; 2]>>;

/// Clip paths (`W n` / `W* n`) in device space mapped back to document space, plus bboxes of filled paths.
fn pdf_clips_and_fills(content: &str) -> (Vec<Rings>, Vec<[f32; 4]>) {
    let mut clips = Vec::new();
    let mut fills = Vec::new();
    let mut stack: Vec<f32> = Vec::new();
    let mut ctm = [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut saved = Vec::new();
    let mut rings: Rings = Vec::new();
    let mut clip_pending = false;
    let map = |m: &[f32; 6], x: f32, y: f32| {
        let d = [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
        [d[0], H - d[1]]
    };
    for token in content.split_ascii_whitespace() {
        if let Ok(v) = token.parse::<f32>() {
            stack.push(v);
            continue;
        }
        let n = stack.len();
        match token {
            "q" => saved.push(ctm),
            "Q" => ctm = saved.pop().unwrap_or(ctm),
            "cm" if n >= 6 => {
                let [a, b, c, d, e, f] = [stack[n - 6], stack[n - 5], stack[n - 4], stack[n - 3], stack[n - 2], stack[n - 1]];
                let [ca, cb, cc, cd, ce, cf] = ctm;
                ctm = [a * ca + b * cc, a * cb + b * cd, c * ca + d * cc, c * cb + d * cd, e * ca + f * cc + ce, e * cb + f * cd + cf];
            }
            "m" if n >= 2 => rings.push(vec![map(&ctm, stack[n - 2], stack[n - 1])]),
            "l" if n >= 2 => {
                let p = map(&ctm, stack[n - 2], stack[n - 1]);
                if let Some(r) = rings.last_mut() {
                    r.push(p);
                }
            }
            "re" if n >= 4 => {
                let [x, y, w, h] = [stack[n - 4], stack[n - 3], stack[n - 2], stack[n - 1]];
                rings.push(vec![map(&ctm, x, y), map(&ctm, x + w, y), map(&ctm, x + w, y + h), map(&ctm, x, y + h)]);
            }
            "W" | "W*" => clip_pending = true,
            "n" | "f" | "f*" | "F" | "B" | "B*" | "b" | "b*" | "S" | "s" => {
                if clip_pending {
                    clips.push(std::mem::take(&mut rings));
                    clip_pending = false;
                } else if matches!(token, "f" | "f*" | "F") {
                    let mut bb = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
                    for p in rings.iter().flatten() {
                        bb = [bb[0].min(p[0]), bb[1].min(p[1]), bb[2].max(p[0]), bb[3].max(p[1])];
                    }
                    if bb[0].is_finite() {
                        fills.push(bb);
                    }
                }
                rings.clear();
            }
            "h" | "c" | "v" | "y" => {}
            _ => {}
        }
        stack.clear();
    }
    (clips, fills)
}

fn svg_region(svg: &str, element: &str, id: &str) -> Rings {
    let start = svg.find(&format!("<{element} id=\"{id}\"")).unwrap_or_else(|| panic!("missing <{element} id={id}> in\n{svg}"));
    let rest = &svg[start..];
    let d0 = rest.find(" d=\"").unwrap() + 4;
    let d = &rest[d0..d0 + rest[d0..].find('"').unwrap()];
    let mut rings: Rings = Vec::new();
    let mut nums = Vec::new();
    for tok in d.split_ascii_whitespace() {
        let (op, num) = if tok.starts_with(['M', 'L', 'Z']) { (Some(&tok[..1]), &tok[1..]) } else { (None, tok) };
        match op {
            Some("M") => rings.push(Vec::new()),
            Some("Z") => continue,
            _ => {}
        }
        if let Ok(v) = num.parse::<f32>() {
            nums.push(v);
            if nums.len() == 2 {
                rings.last_mut().unwrap().push([nums[0], nums[1]]);
                nums.clear();
            }
        }
    }
    rings
}

fn inside(rings: &Rings, x: f32, y: f32) -> bool {
    let mut odd = false;
    for r in rings {
        let mut j = r.len() - 1;
        for i in 0..r.len() {
            let (a, b) = (r[i], r[j]);
            if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
                odd = !odd;
            }
            j = i;
        }
    }
    odd
}

fn png(doc: &Document) -> image::RgbaImage {
    let bytes = dtool_export::export_png(doc, None).expect("png export");
    image::load_from_memory(&bytes).unwrap().to_rgba8()
}

fn is(img: &image::RgbaImage, x: f32, y: f32, c: [u8; 3]) -> bool {
    let p = img.get_pixel(((x * img.width() as f32 / 80.0) as u32).min(img.width() - 1), ((y * img.height() as f32 / 80.0) as u32).min(img.height() - 1));
    p[3] > 200 && (0..3).all(|i| (i32::from(p[i]) - i32::from(c[i])).abs() < 40)
}

/// Rasterise "object rect ∩ exported clip region" on a 1-unit grid and compare with the PNG export's pixels.
fn mismatch(img: &image::RgbaImage, obj: [f32; 4], region: &Rings, colour: [u8; 3]) -> f32 {
    let (mut bad, mut total) = (0, 0);
    for gy in 0..80 {
        for gx in 0..80 {
            let (x, y) = (gx as f32 + 0.5, gy as f32 + 0.5);
            let in_obj = x > obj[0] && x < obj[0] + obj[2] && y > obj[1] && y < obj[1] + obj[3];
            if !in_obj {
                continue;
            }
            // ignore the 1-unit anti-aliased rim of the region
            let edge = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)].iter().any(|(dx, dy)| inside(region, x + dx, y + dy) != inside(region, x, y));
            if edge {
                continue;
            }
            total += 1;
            if inside(region, x, y) != is(img, x, y, colour) {
                bad += 1;
            }
        }
    }
    bad as f32 / total.max(1) as f32
}

fn rough_bbox(rings: &Rings) -> [f32; 4] {
    let mut b = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
    for p in rings.iter().flatten() {
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
    }
    b
}

fn near(a: [f32; 4], b: [f32; 4]) -> bool {
    (0..4).all(|i| (a[i] - b[i]).abs() < 0.6)
}

#[test]
fn pdf_clip_child_and_vector_mask_use_true_clip_paths_matching_png() {
    let plain = pdf_content(&base(), "plain");
    let (clips, _) = pdf_clips_and_fills(&plain);
    assert!(clips.iter().all(|c| !near(rough_bbox(c), [0.0, 0.0, 40.0, 40.0])), "plain doc has no K08 clip: {clips:?}");

    let doc = clipped();
    let content = pdf_content(&doc, "clip");
    assert!(content.contains("W* n") || content.contains("W*\nn"), "clip path operators present");
    let (clips, _) = pdf_clips_and_fills(&content);
    let clip = clips.iter().find(|c| near(rough_bbox(c), [0.0, 0.0, 40.0, 40.0])).unwrap_or_else(|| panic!("parent clip path in PDF: {clips:?}")).clone();
    let m = mismatch(&png(&doc), [20.0, 20.0, 40.0, 40.0], &clip, [0, 0, 255]);
    assert!(m < 0.03, "pdf clip vs png mismatch {m}");

    let doc = masked();
    let content = pdf_content(&doc, "mask");
    let (clips, fills) = pdf_clips_and_fills(&content);
    assert!(fills.iter().all(|b| !near(*b, [0.0, 0.0, 30.0, 80.0])), "mask path itself not painted: {fills:?}");
    let parent = clips.iter().find(|c| near(rough_bbox(c), [0.0, 0.0, 30.0, 80.0])).unwrap_or_else(|| panic!("parent mask clip in PDF: {clips:?}")).clone();
    let child = clips.iter().find(|c| near(rough_bbox(c), [0.0, 0.0, 30.0, 40.0])).unwrap_or_else(|| panic!("child clip in PDF: {clips:?}")).clone();
    let img = png(&doc);
    let m1 = mismatch(&img, [0.0, 0.0, 40.0, 20.0], &parent, [255, 0, 0]);
    let m2 = mismatch(&img, [20.0, 20.0, 40.0, 40.0], &child, [0, 0, 255]);
    assert!(m1 < 0.03 && m2 < 0.03, "pdf mask vs png mismatch {m1} {m2}");
}

#[test]
fn svg_clip_child_uses_clip_path_and_vector_mask_uses_mask_matching_png() {
    let plain = svg_text(&base(), "plain");
    assert!(!plain.contains("k08-"), "plain doc has no K08 defs");

    let doc = clipped();
    let svg = svg_text(&doc, "clip");
    assert!(svg.contains("clip-path=\"url(#k08-clip-2)\""), "{svg}");
    assert!(!svg.contains("k08-clip-1") && !svg.contains("<mask"), "parent itself unclipped");
    let clip = svg_region(&svg, "clipPath", "k08-clip-2");
    assert!(near(rough_bbox(&clip), [0.0, 0.0, 40.0, 40.0]), "{clip:?}");
    let m = mismatch(&png(&doc), [20.0, 20.0, 40.0, 40.0], &clip, [0, 0, 255]);
    assert!(m < 0.03, "svg clip vs png mismatch {m}");

    let doc = masked();
    let svg = svg_text(&doc, "mask");
    assert!(svg.contains("mask=\"url(#k08-mask-1)\"") && svg.contains("mask=\"url(#k08-mask-2)\""), "{svg}");
    assert!(!svg.to_ascii_lowercase().contains("#00ff00"), "mask path itself not drawn");
    let parent = svg_region(&svg, "mask", "k08-mask-1");
    let child = svg_region(&svg, "mask", "k08-mask-2");
    let img = png(&doc);
    let m1 = mismatch(&img, [0.0, 0.0, 40.0, 20.0], &parent, [255, 0, 0]);
    let m2 = mismatch(&img, [20.0, 20.0, 40.0, 40.0], &child, [0, 0, 255]);
    assert!(m1 < 0.03 && m2 < 0.03, "svg mask vs png mismatch {m1} {m2}");
}

#[test]
fn clip_child_outside_parent_is_invisible_in_pdf_svg_and_png() {
    let mut doc = clipped();
    doc.objects[1].pos = [60.0, 60.0];
    doc.objects[1].size = [10.0, 10.0];
    let svg = svg_text(&doc, "gone");
    let region = svg_region(&svg, "clipPath", "k08-clip-2");
    let b = rough_bbox(&region);
    assert!(b[2] <= 60.5 || b[3] <= 60.5, "svg clip excludes the child: {b:?}");
    let (clips, _) = pdf_clips_and_fills(&pdf_content(&doc, "gone"));
    assert!(clips.iter().any(|c| near(rough_bbox(c), [0.0, 0.0, 40.0, 40.0])), "{clips:?}");
    assert!(!png(&doc).pixels().any(|p| p[3] > 200 && p[2] > 200 && p[0] < 60), "nothing of the child visible");
}
#[test]
fn commands_on_live_session_drive_pdf_and_svg_clipping_and_undo_removes_it() {
    let mut doc = base();
    doc.objects.push(rect(3, [0.0, 0.0], [30.0, 80.0], [0, 255, 0]));
    let mut s = dtool_core::Session::new(doc);
    let cmd = |name: &str, params: serde_json::Value| {
        let mut c = dtool_core::Command::new(name);
        c.params = params;
        c
    };
    s.run(&cmd("layer.clip.set", json!({ "id": 2, "parent": 1 }))).expect("clip.set");
    s.run(&cmd("layer.vector_mask.set", json!({ "id": 3, "target": 1 }))).expect("vector_mask.set");
    let svg = svg_text(&s.document, "live_mask");
    assert!(svg.contains("mask=\"url(#k08-mask-1)\"") && svg.contains("mask=\"url(#k08-mask-2)\"") && !svg.to_ascii_lowercase().contains("#00ff00"), "{svg}");
    let (clips, _) = pdf_clips_and_fills(&pdf_content(&s.document, "live_mask"));
    assert!(clips.iter().any(|c| near(rough_bbox(c), [0.0, 0.0, 30.0, 40.0])), "{clips:?}");
    s.run(&dtool_core::Command::new("edit.undo")).expect("undo mask");
    let svg = svg_text(&s.document, "live_clip");
    assert!(!svg.contains("<mask") && svg.contains("clip-path=\"url(#k08-clip-2)\"") && svg.to_ascii_lowercase().contains("#00ff00"), "{svg}");
    s.run(&dtool_core::Command::new("edit.undo")).expect("undo clip");
    assert!(!svg_text(&s.document, "live_plain").contains("k08-"));
    let (clips, _) = pdf_clips_and_fills(&pdf_content(&s.document, "live_plain"));
    assert!(clips.iter().all(|c| !near(rough_bbox(c), [0.0, 0.0, 40.0, 40.0])), "{clips:?}");
}
fn streams(pdf: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = find(&pdf[at..], b"stream") {
        let mut start = at + found + 6;
        if pdf[..at + found].ends_with(b"end") {
            at = start;
            continue;
        }
        if pdf.get(start) == Some(&b'\r') {
            start += 1;
        }
        if pdf.get(start) == Some(&b'\n') {
            start += 1;
        }
        let Some(end) = find(&pdf[start..], b"endstream") else { break };
        out.push(pdf[start..start + end].to_vec());
        at = start + end + 9;
    }
    out
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

mod inflate {
    struct Bits<'a> {
        data: &'a [u8],
        pos: usize,
        buf: u32,
        cnt: u32,
    }

    impl Bits<'_> {
        fn bits(&mut self, need: u32) -> u32 {
            while self.cnt < need {
                self.buf |= u32::from(self.data[self.pos]) << self.cnt;
                self.pos += 1;
                self.cnt += 8;
            }
            let value = self.buf & ((1u32 << need) - 1);
            self.buf = if need == 32 { 0 } else { self.buf >> need };
            self.cnt -= need;
            value
        }

        fn decode(&mut self, h: &Huffman) -> usize {
            let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
            for len in 1..16 {
                code |= self.bits(1) as i32;
                let count = i32::from(h.count[len]);
                if code - count < first {
                    return h.symbol[(index + code - first) as usize] as usize;
                }
                index += count;
                first += count;
                first <<= 1;
                code <<= 1;
            }
            panic!("bad code")
        }
    }

    struct Huffman {
        count: [u16; 16],
        symbol: Vec<u16>,
    }

    fn huffman(lengths: &[u8]) -> Huffman {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        Huffman { count, symbol }
    }

    const LBASE: [usize; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
    const LEXT: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
    const DBASE: [usize; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
    const DEXT: [u32; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

    fn codes(bits: &mut Bits, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman) {
        loop {
            let sym = bits.decode(lit);
            if sym < 256 {
                out.push(sym as u8);
            } else if sym == 256 {
                return;
            } else {
                let sym = sym - 257;
                let len = LBASE[sym] + bits.bits(LEXT[sym]) as usize;
                let d = bits.decode(dist);
                let back = DBASE[d] + bits.bits(DEXT[d]) as usize;
                let start = out.len() - back;
                for i in 0..len {
                    out.push(out[start + i]);
                }
            }
        }
    }

    pub fn zlib(data: &[u8]) -> Vec<u8> {
        let mut bits = Bits { data, pos: 2, buf: 0, cnt: 0 };
        let mut out = Vec::new();
        loop {
            let last = bits.bits(1);
            match bits.bits(2) {
                0 => {
                    bits.buf = 0;
                    bits.cnt = 0;
                    let len = u16::from_le_bytes([data[bits.pos], data[bits.pos + 1]]) as usize;
                    bits.pos += 4;
                    out.extend_from_slice(&data[bits.pos..bits.pos + len]);
                    bits.pos += len;
                }
                1 => {
                    let mut lengths = [0u8; 288];
                    for (i, l) in lengths.iter_mut().enumerate() {
                        *l = match i {
                            0..=143 => 8,
                            144..=255 => 9,
                            256..=279 => 7,
                            _ => 8,
                        };
                    }
                    let lit = huffman(&lengths);
                    let dist = huffman(&[5u8; 30]);
                    codes(&mut bits, &mut out, &lit, &dist);
                }
                2 => {
                    let nlen = bits.bits(5) as usize + 257;
                    let ndist = bits.bits(5) as usize + 1;
                    let ncode = bits.bits(4) as usize + 4;
                    let mut lengths = [0u8; 320];
                    for &o in ORDER.iter().take(ncode) {
                        lengths[o] = bits.bits(3) as u8;
                    }
                    let lencode = huffman(&lengths[..19]);
                    let mut index = 0;
                    let mut table = [0u8; 320];
                    while index < nlen + ndist {
                        let sym = bits.decode(&lencode);
                        if sym < 16 {
                            table[index] = sym as u8;
                            index += 1;
                        } else {
                            let (value, repeat) = match sym {
                                16 => (table[index - 1], 3 + bits.bits(2)),
                                17 => (0, 3 + bits.bits(3)),
                                _ => (0, 11 + bits.bits(7)),
                            };
                            for _ in 0..repeat {
                                table[index] = value;
                                index += 1;
                            }
                        }
                    }
                    let lit = huffman(&table[..nlen]);
                    let dist = huffman(&table[nlen..nlen + ndist]);
                    codes(&mut bits, &mut out, &lit, &dist);
                }
                _ => panic!("bad block"),
            }
            if last == 1 {
                return out;
            }
        }
    }
}
