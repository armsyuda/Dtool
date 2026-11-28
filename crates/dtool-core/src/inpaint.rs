use crate::{ObjectId,CommandError};
use crate::selection::PixelSelection;
use serde::{Serialize,Deserialize};
use std::sync::atomic::{AtomicBool,Ordering};
use std::sync::Arc;

pub const ENGINE_VERSION:&str="patch-2";

#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum SampleFrom {CurrentLayer,LayerBelow,AllLayers}

#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct InpaintRequest {pub node:ObjectId,pub mask:PixelSelection,pub opacity:f32,pub sample_from:SampleFrom,pub seed:u64}
impl InpaintRequest {
    pub fn from_value(value:&serde_json::Value)->Result<Self,CommandError>{serde_json::from_value(value.clone()).map_err(|_|CommandError::invalid_params())}
}

pub fn validate(req:&InpaintRequest,layer_size:[u32;2])->Result<(),CommandError>{
    let length=(layer_size[0] as usize).checked_mul(layer_size[1] as usize).ok_or_else(CommandError::invalid_params)?;
    if [req.mask.width,req.mask.height]!=layer_size || req.mask.coverage.len()!=length || req.mask.is_empty() || !req.opacity.is_finite() || !(0.0..=1.0).contains(&req.opacity){return Err(CommandError::invalid_params());}Ok(())
}

pub fn mask_from_stroke(w:u32,h:u32,stroke:&[[f32;2]],width_px:f32)->PixelSelection{
    let Some(length)=(w as usize).checked_mul(h as usize)else{return PixelSelection::empty(0,0);};
    let mut coverage=Vec::new();if coverage.try_reserve_exact(length).is_err(){return PixelSelection::empty(0,0);}coverage.resize(length,0);
    let mut mask=PixelSelection::from_coverage(w, h, coverage);
    if stroke.is_empty() || !width_px.is_finite() || width_px<=0.0 || stroke.iter().flatten().any(|value|!value.is_finite()){return mask;}
    let radius=f64::from(width_px)*0.5;let radius_squared=radius*radius;
    for index in 0..stroke.len(){
        let a=stroke[index];let b=stroke.get(index+1).copied().unwrap_or(a);
        let ax=f64::from(a[0]);let ay=f64::from(a[1]);let bx=f64::from(b[0]);let by=f64::from(b[1]);
        let x0=((ax.min(bx)-radius).floor().max(0.0).min(f64::from(w))) as u32;
        let y0=((ay.min(by)-radius).floor().max(0.0).min(f64::from(h))) as u32;
        let x1=((ax.max(bx)+radius).ceil().max(0.0).min(f64::from(w))) as u32;
        let y1=((ay.max(by)+radius).ceil().max(0.0).min(f64::from(h))) as u32;
        let dx=bx-ax;let dy=by-ay;let squared=dx*dx+dy*dy;
        for y in y0..y1{for x in x0..x1{
            let px=f64::from(x)+0.5;let py=f64::from(y)+0.5;
            let t=if squared>0.0{(((px-ax)*dx+(py-ay)*dy)/squared).clamp(0.0,1.0)}else{0.0};
            let distance_x=px-(ax+t*dx);let distance_y=py-(ay+t*dy);
            if distance_x*distance_x+distance_y*distance_y<=radius_squared{mask.coverage[y as usize*w as usize+x as usize]=255;}
        }}
    }
    mask
}

pub struct RasterView { pub width:u32, pub height:u32, pub rgba:Vec<u8> }
impl RasterView {
    pub fn get(&self,x:u32,y:u32)->[u8;4]{
        let index=(y as usize)*self.width as usize+(x as usize);
        [self.rgba[index*4],self.rgba[index*4+1],self.rgba[index*4+2],self.rgba[index*4+3]]
    }
}

#[derive(Debug,Clone,Copy)]
pub struct PatchCandidate { pub target:[u32;2], pub source:[u32;2], pub cost:u32 }

struct Rng(u64);
impl Rng {
    fn next(&mut self)->u64{
        self.0^=self.0<<13;
        self.0^=self.0>>7;
        self.0^=self.0<<17;
        self.0
    }
}

pub fn find_patches(src: &RasterView, mask: &PixelSelection, seed: u64, patch: u32, max_iters: u32) -> Vec<PatchCandidate> {
    search(src, mask, seed, patch, max_iters, &CancelToken::new(), &mut |_| {}).unwrap_or_default()
}

fn colour_distance(left: [u8; 4], right: [u8; 4]) -> u32 {
    left.into_iter().zip(right).map(|(a, b)| (i32::from(a) - i32::from(b)).pow(2) as u32).sum()
}

fn search(
    src: &RasterView, mask: &PixelSelection, seed: u64, patch: u32, max_iters: u32,
    token: &CancelToken, progress: &mut dyn FnMut(Progress),
) -> Result<Vec<PatchCandidate>, CommandError> {
    let Some([left, top, width, height]) = mask.bounds() else { return Ok(Vec::new()) };
    let index = |x: u32, y: u32| (y * src.width + x) as usize;
    let usable = |x, y| mask.coverage[index(x, y)] == 0 && src.get(x, y)[3] != 0;
    let mut rng = Rng(seed.max(1));
    let mut candidates = Vec::new();
    let mut missing = Vec::new();
    let half = (patch.clamp(1, 15) / 2) as i32;
    let [w, h] = [src.width as i32, src.height as i32];
    for y in top..top + height {
        if token.is_cancelled() { return Err(CommandError::new("CANCELLED")); }
        for x in left..left + width {
            if mask.coverage[index(x, y)] == 0 { continue; }
            let mut best = None;
            for trial in 0..max_iters.min(12) {
                let distance = 1i32 << (trial + 1).min(8);
                let random = rng.next();
                let dx = (random as u32 % (2 * distance) as u32) as i32 - distance;
                let dy = ((random >> 32) as u32 % (2 * distance) as u32) as i32 - distance;
                let sx = (x as i32 + dx).clamp(0, w - 1);
                let sy = (y as i32 + dy).clamp(0, h - 1);
                if !usable(sx as u32, sy as u32) { continue; }
                let center = src.get(sx as u32, sy as u32);
                let mut cost = 0u32;
                let mut center_cost = u32::MAX;
                let mut weight = 0u32;
                for row in -half..=half {
                    for col in -half..=half {
                        let [tx, ty, rx, ry] = [x as i32 + col, y as i32 + row, sx + col, sy + row];
                        if !(0..w).contains(&tx) || !(0..h).contains(&ty) || !(0..w).contains(&rx) || !(0..h).contains(&ry)
                            || mask.coverage[index(tx as u32, ty as u32)] != 0
                            || !usable(rx as u32, ry as u32)
                        { continue; }
                        let [a, b] = [src.get(tx as u32, ty as u32), src.get(rx as u32, ry as u32)];
                        cost += colour_distance(a, b);
                        center_cost = center_cost.min(colour_distance(a, center));
                        weight += 1;
                    }
                }
                // A masked interior has no local context to score a random patch against.
                // Let the boundary pass carry a nearby unmasked sample into those pixels.
                // A matching patch can still contain an unrelated isolated mark at its centre.
                if weight == 0 || center_cost > 4 * 64 * 64 { continue; }
                let score = cost / weight + center_cost / 4 + dx.unsigned_abs() + dy.unsigned_abs();
                if best.as_ref().is_none_or(|old: &PatchCandidate| score < old.cost) {
                    best = Some(PatchCandidate { target: [x, y], source: [sx as u32, sy as u32], cost: score });
                }
            }
            if let Some(candidate) = best { candidates.push(candidate); }
            else { missing.push([x, y]); }
        }
        if (y - top) % 16 == 0 { progress(Progress { done: y - top, total: height }); }
    }
    // Reuse sources from the mask boundary throughout holes without scanning the image for every pixel.
    if !missing.is_empty() {
        let mut sources = vec![[u32::MAX; 2]; width as usize * height as usize];
        let at = |x: u32, y: u32| ((y - top) * width + x - left) as usize;
        let neighbours = |x: u32, y: u32| {
            [x.checked_sub(1).map(|x| [x, y]), y.checked_sub(1).map(|y| [x, y]),
                (x + 1 < src.width).then_some([x + 1, y]), (y + 1 < src.height).then_some([x, y + 1])]
                .into_iter().flatten()
        };
        let mut queue = std::collections::VecDeque::new();
        for y in top..top + height {
            if token.is_cancelled() { return Err(CommandError::new("CANCELLED")); }
            for x in left..left + width {
                if mask.coverage[index(x, y)] != 0
                    && let Some(source) = neighbours(x, y).find(|[nx, ny]| usable(*nx, *ny))
                {
                    sources[at(x, y)] = source;
                    queue.push_back([x, y]);
                }
            }
        }
        let mut visited = 0usize;
        while let Some([x, y]) = queue.pop_front() {
            if visited % 4096 == 0 && token.is_cancelled() { return Err(CommandError::new("CANCELLED")); }
            visited += 1;
            let source = sources[at(x, y)];
            for [nx, ny] in neighbours(x, y) {
                if mask.coverage[index(nx, ny)] != 0 && sources[at(nx, ny)][0] == u32::MAX {
                    sources[at(nx, ny)] = source;
                    queue.push_back([nx, ny]);
                }
            }
        }
        let fallback = std::cell::OnceCell::new();
        for [x, y] in missing {
            let mut source = sources[at(x, y)];
            if source[0] == u32::MAX {
                // The mask may be separated from opaque content by transparent pixels.
                source = *fallback.get_or_init(|| {
                    (0..src.height).flat_map(|sy| (0..src.width).map(move |sx| [sx, sy]))
                        .find(|[sx, sy]| usable(*sx, *sy)).unwrap_or([u32::MAX; 2])
                });
            }
            if source[0] != u32::MAX {
                candidates.push(PatchCandidate { target: [x, y], source, cost: u32::MAX });
            }
        }
    }
    progress(Progress { done: height, total: height });
    Ok(candidates)
}

#[derive(Debug, Clone)]
pub struct StagedRegion { pub rect: [u32; 4], pub rgba: Vec<u8> }

pub fn apply_patches(src: &RasterView, mask: &PixelSelection, candidates: &[PatchCandidate], opacity: f32) -> StagedRegion {
    apply_over(src, src, mask, candidates, opacity)
}

fn apply_over(source: &RasterView, base: &RasterView, mask: &PixelSelection, candidates: &[PatchCandidate], opacity: f32) -> StagedRegion {
    let Some([left, top, width, height]) = mask.bounds().filter(|_| !candidates.is_empty()) else {
        return StagedRegion { rect: [0; 4], rgba: Vec::new() }
    };
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in top..top + height {
        let start = ((y * base.width + left) * 4) as usize;
        rgba.extend_from_slice(&base.rgba[start..start + width as usize * 4]);
    }
    let mut ordered: Vec<_> = candidates.iter().collect();
    ordered.sort_unstable_by_key(|candidate| candidate.target);
    for group in ordered.chunk_by(|a, b| a.target == b.target) {
        let [x, y] = group[0].target;
        if !(left..left + width).contains(&x) || !(top..top + height).contains(&y) { continue; }
        let coverage = f32::from(mask.coverage[(y * mask.width + x) as usize]) / 255.0;
        if coverage == 0.0 { continue; }
        let offset = (((y - top) * width + x - left) * 4) as usize;
        let pixel = &mut rgba[offset..offset + 4];
        let mut sum = [0u64; 4];
        for candidate in group {
            for (sum, value) in sum.iter_mut().zip(source.get(candidate.source[0], candidate.source[1])) {
                *sum += u64::from(value);
            }
        }
        let ratio = opacity * coverage;
        for (value, sum) in pixel.iter_mut().zip(sum) {
            let average = sum as f32 / group.len() as f32;
            *value = (f32::from(*value) * (1.0 - ratio) + average * ratio).round() as u8;
        }
    }
    StagedRegion { rect: [left, top, width, height], rgba }
}

pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken {
    pub fn new() -> Self { Self(Arc::new(AtomicBool::new(false))) }
    pub fn cancel(&self) { self.0.store(true, Ordering::Relaxed); }
    pub fn is_cancelled(&self) -> bool { self.0.load(Ordering::Relaxed) }
}

#[derive(Debug, Clone, Copy)]
pub struct Progress { pub done: u32, pub total: u32 }

pub fn run_inpaint(req: &InpaintRequest, src: &RasterView, token: &CancelToken, progress: &mut dyn FnMut(Progress)) -> Result<StagedRegion, CommandError> {
    run_over(req, src, src, token, progress)
}

pub fn run_over(
    req: &InpaintRequest, src: &RasterView, base: &RasterView, token: &CancelToken, progress: &mut dyn FnMut(Progress),
) -> Result<StagedRegion, CommandError> {
    if token.is_cancelled() { return Err(CommandError::new("CANCELLED")); }
    validate(req, [src.width, src.height])?;
    let bytes = (src.width as usize).checked_mul(src.height as usize).and_then(|pixels| pixels.checked_mul(4));
    if bytes != Some(src.rgba.len()) || bytes != Some(base.rgba.len())
        || [src.width, src.height] != [base.width, base.height]
    { return Err(CommandError::invalid_params()); }
    let candidates = search(src, &req.mask, req.seed, 7, 7, token, progress)?;
    if token.is_cancelled() { return Err(CommandError::new("CANCELLED")); }
    Ok(apply_over(src, base, &req.mask, &candidates, req.opacity))
}
