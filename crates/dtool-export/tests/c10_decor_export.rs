use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind, PathFlow, PathTextData};
use dtool_core::paint::{Colour, Paint};
use dtool_export::pdf::{export_pdf, ExportArea, ExportControl, ExportRequest, PdfOptions};
use dtool_text::decorations::{object_decorations_with, DecorationKind, DecorationRect};
use dtool_text::FontLibrary;
use serde_json::{json, Value};

const W: f32 = 200.0;
const H: f32 = 160.0;
const PATH_FROM: [f32; 2] = [20.0, 150.0];
const PATH_TO: [f32; 2] = [180.0, 80.0];

fn solid(r: u8, g: u8, b: u8) -> Value {
    serde_json::to_value(Paint::Solid { colour: Colour::new(r, g, b, 255) }).unwrap()
}

fn ranges(size: f64) -> Vec<Value> {
    let base = |start: usize, end: usize| json!({"start": start, "end": end, "family": "Arial", "style": "Regular", "size_pt": size});
    let mut first = base(0, 5);
    first["underline"] = json!({"on": true, "colour": solid(255, 0, 0)});
    first["background"] = solid(0, 255, 0);
    let mut second = base(5, 11);
    second["strike"] = json!({"on": true, "colour": solid(0, 0, 255)});
    vec![first, second]
}

fn story(doc: &mut Document, id: u64, ranges: Vec<Value>) -> StoryId {
    doc.stories.push(Story { id: StoryId(id), text: "Hello world".to_owned(), ranges, ..Story::default() });
    StoryId(id)
}

fn text_doc() -> (Document, ObjectId, ObjectId) {
    let mut doc = Document::new([W, H], Units::Pt, 72.0, 1);
    let art = story(&mut doc, 1, ranges(24.0));
    let frame = story(&mut doc, 2, ranges(24.0));
    doc.objects.push(Object { id: ObjectId(1), name: "art".into(), pos: [20.0, 10.0], size: [170.0, 30.0], kind: ObjectKind::ArtText { story: art }, ..Object::default() });
    doc.objects.push(Object { id: ObjectId(2), name: "frame".into(), pos: [20.0, 60.0], size: [170.0, 40.0], kind: ObjectKind::FrameText { story: frame }, ..Object::default() });
    doc.next_object_id = 3;
    (doc, ObjectId(1), ObjectId(2))
}

fn path_doc(underline_only: bool) -> (Document, ObjectId) {
    let mut doc = Document::new([W, H], Units::Pt, 72.0, 1);
    let mut range = json!({"start": 0, "end": 11, "family": "Arial", "style": "Regular", "size_pt": 18.0});
    range["underline"] = json!({"on": true, "colour": solid(255, 0, 0)});
    if !underline_only {
        range["background"] = solid(0, 255, 0);
    }
    let text = story(&mut doc, 1, vec![range]);
    doc.objects.push(Object {
        id: ObjectId(1),
        name: "path".into(),
        pos: [0.0, 0.0],
        size: [W, H],
        kind: ObjectKind::Curve(Curve { contour: Contour { points: vec![PathPoint::sharp(PointId(1), wide(PATH_FROM)), PathPoint::sharp(PointId(2), wide(PATH_TO))], closed: false }, holes: Vec::new(), source_object: None }),
        ..Object::default()
    });
    doc.objects.push(Object {
        id: ObjectId(2),
        name: "path_text".into(),
        pos: PATH_FROM,
        size: [160.0, 20.0],
        fill: Paint::Solid { colour: Colour::new(0, 0, 0, 255) },
        kind: ObjectKind::PathText(PathTextData { path: ObjectId(1), story: text, flows: vec![PathFlow::default()], source_paint: None }),
        ..Object::default()
    });
    doc.next_object_id = 3;
    (doc, ObjectId(2))
}

fn wide(p: [f32; 2]) -> [f64; 2] {
    [f64::from(p[0]), f64::from(p[1])]
}

fn expected(doc: &Document, id: ObjectId) -> Vec<DecorationRect> {
    object_decorations_with(&FontLibrary::system(), doc, id)
}

fn one(rects: &[DecorationRect], kind: DecorationKind) -> [f32; 4] {
    let found: Vec<_> = rects.iter().filter(|r| r.kind == kind).collect();
    assert_eq!(found.len(), 1, "{kind:?} in {rects:?}");
    found[0].rect
}

fn is(pixel: &image::Rgba<u8>, colour: [u8; 3]) -> bool {
    pixel[3] > 200 && (0..3).all(|i| (i32::from(pixel[i]) - i32::from(colour[i])).abs() < 60)
}

fn png(doc: &Document) -> image::RgbaImage {
    let bytes = dtool_export::export_png(doc, None).expect("png export");
    image::load_from_memory(&bytes).unwrap().to_rgba8()
}

fn pixels_in(img: &image::RgbaImage, rect: [f32; 4], colour: [u8; 3]) -> usize {
    let sx = img.width() as f32 / W;
    let sy = img.height() as f32 / H;
    let x0 = ((rect[0] - 1.0) * sx).floor().max(0.0) as u32;
    let y0 = ((rect[1] - 1.0) * sy).floor().max(0.0) as u32;
    let x1 = (((rect[0] + rect[2] + 1.0) * sx).ceil() as u32).min(img.width());
    let y1 = (((rect[1] + rect[3] + 1.0) * sy).ceil() as u32).min(img.height());
    let mut n = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            if is(img.get_pixel(x, y), colour) {
                n += 1;
            }
        }
    }
    n
}

fn coloured(img: &image::RgbaImage, colour: [u8; 3]) -> Vec<[f32; 2]> {
    let sx = W / img.width() as f32;
    let sy = H / img.height() as f32;
    img.enumerate_pixels().filter(|(_, _, p)| is(p, colour)).map(|(x, y, _)| [(x as f32 + 0.5) * sx, (y as f32 + 0.5) * sy]).collect()
}

fn line_distance(p: [f32; 2]) -> f32 {
    let (a, b) = (PATH_FROM, PATH_TO);
    let d = [b[0] - a[0], b[1] - a[1]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
    ((p[0] - a[0]) * d[1] - (p[1] - a[1]) * d[0]).abs() / len
}

fn slope(points: &[[f32; 2]]) -> f32 {
    let n = points.len() as f32;
    let mx = points.iter().map(|p| p[0]).sum::<f32>() / n;
    let my = points.iter().map(|p| p[1]).sum::<f32>() / n;
    let sxy: f32 = points.iter().map(|p| (p[0] - mx) * (p[1] - my)).sum();
    let sxx: f32 = points.iter().map(|p| (p[0] - mx) * (p[0] - mx)).sum();
    sxy / sxx
}

fn path_slope() -> f32 {
    (PATH_TO[1] - PATH_FROM[1]) / (PATH_TO[0] - PATH_FROM[0])
}

fn assert_follows_path(points: &[[f32; 2]], what: &str) {
    assert_follows_path_within(points, what, 12.0);
}

fn assert_follows_path_within(points: &[[f32; 2]], what: &str, limit: f32) {
    assert!(points.len() >= 5, "{what}: too few marks {points:?}");
    let far = points.iter().map(|p| line_distance(*p)).fold(0.0f32, f32::max);
    assert!(far < limit, "{what}: mark {far} away from the path");
    let xs = points.iter().map(|p| p[0]);
    let span = xs.clone().fold(f32::MIN, f32::max) - xs.fold(f32::MAX, f32::min);
    assert!(span > 60.0, "{what}: span {span}");
    let s = slope(points);
    assert!((s - path_slope()).abs() < 0.15, "{what}: slope {s} vs path {}", path_slope());
}

#[test]
fn c10_export_png_art_and_frame_text_draw_decorations_in_their_rects() {
    let (doc, art, frame) = text_doc();
    let img = png(&doc);
    assert_eq!((img.width() as f32 / img.height() as f32 * H).round(), W);
    for id in [art, frame] {
        let rects = expected(&doc, id);
        assert!(pixels_in(&img, one(&rects, DecorationKind::Underline), [255, 0, 0]) > 0, "underline {id:?} {rects:?}");
        assert!(pixels_in(&img, one(&rects, DecorationKind::Strike), [0, 0, 255]) > 0, "strike {id:?} {rects:?}");
        assert!(pixels_in(&img, one(&rects, DecorationKind::Background), [0, 255, 0]) > 0, "background {id:?} {rects:?}");
    }
    let all: Vec<DecorationRect> = [art, frame].iter().flat_map(|id| expected(&doc, *id)).collect();
    let inside = |p: &[f32; 2]| all.iter().any(|r| p[0] >= r.rect[0] - 1.5 && p[0] <= r.rect[0] + r.rect[2] + 1.5 && p[1] >= r.rect[1] - 1.5 && p[1] <= r.rect[1] + r.rect[3] + 1.5);
    for colour in [[255, 0, 0], [0, 0, 255], [0, 255, 0]] {
        let stray: Vec<_> = coloured(&img, colour).into_iter().filter(|p| !inside(p)).collect();
        assert!(stray.is_empty(), "{colour:?} outside decoration rects: {:?}", &stray[..stray.len().min(5)]);
    }
}

#[test]
fn c10_export_png_without_decorations_has_no_decoration_colours() {
    let (mut doc, _, _) = text_doc();
    for story in &mut doc.stories {
        story.ranges = vec![json!({"start": 0, "end": 11, "family": "Arial", "style": "Regular", "size_pt": 24.0})];
    }
    let img = png(&doc);
    for colour in [[255, 0, 0], [0, 0, 255], [0, 255, 0]] {
        assert!(coloured(&img, colour).is_empty());
    }
}

#[test]
fn c10_path_text_decorations_follow_the_path_in_document_space() {
    let (doc, id) = path_doc(true);
    let rects = expected(&doc, id);
    let centres: Vec<[f32; 2]> = rects.iter().filter(|r| r.kind == DecorationKind::Underline).map(|r| [r.rect[0] + r.rect[2] * 0.5, r.rect[1] + r.rect[3] * 0.5]).collect();
    assert_follows_path(&centres, "path text decoration rects");
    assert!(rects.iter().all(|r| r.paint == Paint::Solid { colour: Colour::new(255, 0, 0, 255) }));
}

#[test]
fn c10_export_png_path_text_underline_follows_the_path() {
    let (doc, _) = path_doc(true);
    let img = png(&doc);
    assert_follows_path(&coloured(&img, [255, 0, 0]), "png path underline");
    let (moved, _) = {
        let (mut doc, id) = path_doc(true);
        let path = doc.objects.iter_mut().find(|o| o.id == ObjectId(1)).unwrap();
        let ObjectKind::Curve(curve) = &mut path.kind else { unreachable!() };
        for point in &mut curve.contour.points {
            point.pos[1] -= 20.0;
        }
        (doc, id)
    };
    let before = coloured(&img, [255, 0, 0]);
    let after = coloured(&png(&moved), [255, 0, 0]);
    let mean = |p: &[[f32; 2]]| p.iter().map(|q| q[1]).sum::<f32>() / p.len() as f32;
    assert!((mean(&before) - mean(&after) - 20.0).abs() < 2.0, "{} {}", mean(&before), mean(&after));
}

#[test]
fn c10_export_png_path_text_background_sits_on_the_path() {
    let (doc, _) = path_doc(false);
    let img = png(&doc);
    assert_follows_path_within(&coloured(&img, [0, 255, 0]), "png path background", 26.0);
}

fn pdf_bytes(doc: &Document, name: &str) -> Vec<u8> {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("c10_decor_export");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.pdf"));
    let req = ExportRequest { format: dtool_export::raster::parse_format("pdf").unwrap(), area: ExportArea::WholeDocument, pages: None, path: path.clone(), preset: None, options: json!({}) };
    // Check the decoration geometry against the RGB fixture colours.
    let options = PdfOptions { colour_space: "RGB".to_string(), ..PdfOptions::default() };
    export_pdf(doc, &req, &options, &ExportControl::new()).expect("pdf export");
    std::fs::read(&path).unwrap()
}

#[derive(Debug, Clone)]
struct Filled {
    colour: Vec<f32>,
    bbox: [f32; 4],
}

fn filled_paths(pdf: &[u8]) -> Vec<Filled> {
    let mut out = Vec::new();
    for stream in streams(pdf) {
        let content = if stream.first() == Some(&0x78) { std::panic::catch_unwind(|| inflate::zlib(&stream)).ok() } else { Some(stream.clone()) };
        if let Some(content) = content {
            interpret(&content, &mut out);
        }
    }
    out
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

fn interpret(content: &[u8], out: &mut Vec<Filled>) {
    let text = String::from_utf8_lossy(content);
    let mut stack: Vec<f32> = Vec::new();
    let mut ctm = [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut saved = Vec::new();
    let mut colour: Vec<f32> = Vec::new();
    let mut points: Vec<[f32; 2]> = Vec::new();
    let apply = |m: &[f32; 6], x: f32, y: f32| [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
    for token in text.split_ascii_whitespace() {
        if let Ok(value) = token.parse::<f32>() {
            stack.push(value);
            continue;
        }
        match token {
            "q" => saved.push((ctm, colour.clone())),
            "Q" => {
                if let Some((m, c)) = saved.pop() {
                    ctm = m;
                    colour = c;
                }
            }
            "cm" if stack.len() >= 6 => {
                let m: Vec<f32> = stack[stack.len() - 6..].to_vec();
                ctm = [
                    m[0] * ctm[0] + m[1] * ctm[2],
                    m[0] * ctm[1] + m[1] * ctm[3],
                    m[2] * ctm[0] + m[3] * ctm[2],
                    m[2] * ctm[1] + m[3] * ctm[3],
                    m[4] * ctm[0] + m[5] * ctm[2] + ctm[4],
                    m[4] * ctm[1] + m[5] * ctm[3] + ctm[5],
                ];
            }
            "rg" | "sc" | "scn" | "k" | "g" => colour = stack.clone(),
            "m" | "l" if stack.len() >= 2 => points.push(apply(&ctm, stack[stack.len() - 2], stack[stack.len() - 1])),
            "c" if stack.len() >= 6 => {
                let s = &stack[stack.len() - 6..];
                for i in 0..3 {
                    points.push(apply(&ctm, s[i * 2], s[i * 2 + 1]));
                }
            }
            "re" if stack.len() >= 4 => {
                let s = &stack[stack.len() - 4..];
                for (x, y) in [(s[0], s[1]), (s[0] + s[2], s[1]), (s[0] + s[2], s[1] + s[3]), (s[0], s[1] + s[3])] {
                    points.push(apply(&ctm, x, y));
                }
            }
            "f" | "f*" | "F" | "B" | "B*" | "b" | "b*" => {
                if !points.is_empty() {
                    let xs = points.iter().map(|p| p[0]);
                    let ys = points.iter().map(|p| p[1]);
                    let (x0, x1) = (xs.clone().fold(f32::MAX, f32::min), xs.fold(f32::MIN, f32::max));
                    let (y0, y1) = (ys.clone().fold(f32::MAX, f32::min), ys.fold(f32::MIN, f32::max));
                    out.push(Filled { colour: colour.clone(), bbox: [x0, H - y1, x1 - x0, y1 - y0] });
                }
                points.clear();
            }
            "n" | "S" | "s" => points.clear(),
            "h" => {}
            _ => {}
        }
        stack.clear();
    }
}

fn rgb_of(colour: &[f32]) -> Option<[f32; 3]> {
    match colour.len() {
        3 => Some([colour[0], colour[1], colour[2]]),
        4 => Some([(1.0 - colour[0]) * (1.0 - colour[3]), (1.0 - colour[1]) * (1.0 - colour[3]), (1.0 - colour[2]) * (1.0 - colour[3])]),
        _ => None,
    }
}

fn of_colour(fills: &[Filled], rgb: [f32; 3]) -> Vec<[f32; 4]> {
    fills.iter().filter(|f| rgb_of(&f.colour).is_some_and(|c| (0..3).all(|i| (c[i] - rgb[i]).abs() < 0.2))).map(|f| f.bbox).collect()
}

fn near(a: [f32; 4], b: [f32; 4]) -> bool {
    (0..4).all(|i| (a[i] - b[i]).abs() < 1.0)
}

#[test]
fn c10_export_pdf_contains_decoration_rects() {
    let (doc, art, frame) = text_doc();
    let fills = filled_paths(&pdf_bytes(&doc, "text"));
    for id in [art, frame] {
        let rects = expected(&doc, id);
        for (kind, rgb) in [(DecorationKind::Underline, [1.0, 0.0, 0.0]), (DecorationKind::Strike, [0.0, 0.0, 1.0]), (DecorationKind::Background, [0.0, 1.0, 0.0])] {
            let want = one(&rects, kind);
            let got = of_colour(&fills, rgb);
            assert!(got.iter().any(|b| near(*b, want)), "{kind:?} {id:?}: want {want:?}, pdf fills of that colour {got:?}");
        }
    }
}

#[test]
fn c10_export_pdf_path_text_decorations_follow_the_path() {
    let (doc, _) = path_doc(true);
    let fills = filled_paths(&pdf_bytes(&doc, "path"));
    let centres: Vec<[f32; 2]> = of_colour(&fills, [1.0, 0.0, 0.0]).iter().map(|b| [b[0] + b[2] * 0.5, b[1] + b[3] * 0.5]).collect();
    assert_follows_path(&centres, "pdf path underline");
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
