use crate::{ObjectId,CommandError};
use crate::selection::PixelSelection;
use serde::{Serialize,Deserialize};
use std::sync::atomic::{AtomicBool,Ordering};
use std::sync::Arc;

pub const ENGINE_VERSION:&str="patch-1";

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

pub fn find_patches(src:&RasterView,mask:&PixelSelection,seed:u64,patch:u32,max_iters:u32)->Vec<PatchCandidate>{
    let w=src.width;let h=src.height;
    if w==0 || h==0 { return Vec::new(); }
    
    let mut rng=Rng(seed);
    let mut candidates=Vec::new();
    
    let half_patch=(patch/2) as i32;
    
    for y in 0..h{
        for x in 0..w{
            if mask.coverage[y as usize*w as usize+x as usize]==0 { continue; }
            
            let mut best_source=[x,y];
            let mut best_cost=u32::MAX;
            
            for _iter in 0..max_iters{
                let rand_val=rng.next();
                let search_radius=1u32<<(_iter+1); 
                
                let dx=(rand_val % (search_radius as u64*2)) as i32 - search_radius as i32;
                let dy=((rand_val >> 32) % (search_radius as u64*2)) as i32 - search_radius as i32;
                
                let sx=(x as i32+dx).max(0).min(w as i32-1) as u32;
                let sy=(y as i32+dy).max(0).min(h as i32-1) as u32;
                
                if mask.coverage[sy as usize*w as usize+sx as usize]!=0 { continue; }
                
                let mut cost=0u32;
                let mut valid=true;
                
                for py in -half_patch..=half_patch{
                    for px in -half_patch..=half_patch{
                        let tx=x as i32+px;let ty=y as i32+py;
                        if tx<0 || tx>=w as i32 || ty<0 || ty>=h as i32 { continue; }
                        
                        if mask.coverage[ty as usize*w as usize+tx as usize]!=0 { continue; }
                        
                        let src_x=sx as i32+px;let src_y=sy as i32+py;
                        if src_x<0 || src_x>=w as i32 || src_y<0 || src_y>=h as i32 { valid=false; break; }
                        
                        let target_px=src.get(tx as u32,ty as u32);
                        let source_px=src.get(src_x as u32,src_y as u32);
                        
                        let dr=(target_px[0] as i32-source_px[0] as i32).abs() as u32;
                        let dg=(target_px[1] as i32-source_px[1] as i32).abs() as u32;
                        let db=(target_px[2] as i32-source_px[2] as i32).abs() as u32;
                        let da=(target_px[3] as i32-source_px[3] as i32).abs() as u32;
                        
                        cost+=dr*dr+dg*dg+db*db+da*da;
                    }
                    if !valid { break; }
                }
                
                if valid && cost<best_cost{
                    best_cost=cost;
                    best_source=[sx,sy];
                }
            }
            
            if best_cost!=u32::MAX{
                candidates.push(PatchCandidate{target:[x,y],source:best_source,cost:best_cost});
            }
        }
    }
    
    candidates
}

#[derive(Debug,Clone)]
pub struct StagedRegion { pub rect:[u32;4], pub rgba:Vec<u8> }

pub fn apply_patches(src:&RasterView,mask:&PixelSelection,c:&[PatchCandidate],opacity:f32)->StagedRegion{
    let mut min_x=u32::MAX;let mut min_y=u32::MAX;let mut max_x=0u32;let mut max_y=0u32;
    
    for cand in c {
        if cand.target[0]<min_x { min_x=cand.target[0]; }
        if cand.target[1]<min_y { min_y=cand.target[1]; }
        if cand.target[0]>max_x { max_x=cand.target[0]; }
        if cand.target[1]>max_y { max_y=cand.target[1]; }
    }
    
    if c.is_empty() { return StagedRegion{rect:[0,0,0,0],rgba:Vec::new()}; }
    
    let width=max_x-min_x+1;let height=max_y-min_y+1;
    let mut rgba=vec![0u8;width as usize*height as usize*4];
    
    for y in 0..height{
        for x in 0..width{
            let global_x=min_x+x;
            let global_y=min_y+y;
            
            let mask_idx=(global_y as usize)*mask.width as usize+(global_x as usize);
            let coverage=if mask_idx < mask.coverage.len() { mask.coverage[mask_idx] } else { 0 };
            
            if coverage == 0 {
                let src_px=src.get(global_x,global_y);
                let idx=(y as usize*width as usize+x as usize)*4;
                rgba[idx]=src_px[0];
                rgba[idx+1]=src_px[1];
                rgba[idx+2]=src_px[2];
                rgba[idx+3]=src_px[3];
            } else {
                let mut vote_r=0u64;
                let mut vote_g=0u64;
                let mut vote_b=0u64;
                let mut vote_a=0u64;
                let mut count=0u64;
                
                for cand in c {
                    if cand.target[0]==global_x && cand.target[1]==global_y {
                        let src_px=src.get(cand.source[0],cand.source[1]);
                        vote_r+=src_px[0] as u64;
                        vote_g+=src_px[1] as u64;
                        vote_b+=src_px[2] as u64;
                        vote_a+=src_px[3] as u64;
                        count+=1;
                    }
                }
                
                let orig_px=src.get(global_x,global_y);
                
                if count == 0 {
                    let idx=(y as usize*width as usize+x as usize)*4;
                    rgba[idx]=orig_px[0];
                    rgba[idx+1]=orig_px[1];
                    rgba[idx+2]=orig_px[2];
                    rgba[idx+3]=orig_px[3];
                } else {
                    let avg_r=vote_r/count;
                    let avg_g=vote_g/count;
                    let avg_b=vote_b/count;
                    let avg_a=vote_a/count;
                    
                    let mask_factor=f64::from(coverage)/255.0;
                    let blend_factor=mask_factor*f64::from(opacity);
                    
                    let out_r=((orig_px[0] as f64)*(1.0-blend_factor)+(avg_r as f64)*blend_factor).round().clamp(0.0,255.0) as u8;
                    let out_g=((orig_px[1] as f64)*(1.0-blend_factor)+(avg_g as f64)*blend_factor).round().clamp(0.0,255.0) as u8;
                    let out_b=((orig_px[2] as f64)*(1.0-blend_factor)+(avg_b as f64)*blend_factor).round().clamp(0.0,255.0) as u8;
                    let out_a=((orig_px[3] as f64)*(1.0-blend_factor)+(avg_a as f64)*blend_factor).round().clamp(0.0,255.0) as u8;
                    
                    let idx=(y as usize*width as usize+x as usize)*4;
                    rgba[idx]=out_r;
                    rgba[idx+1]=out_g;
                    rgba[idx+2]=out_b;
                    rgba[idx+3]=out_a;
                }
            }
        }
    }
    
    StagedRegion{rect:[min_x,min_y,width,height],rgba}
}

pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken {
    pub fn new()->Self{CancelToken(Arc::new(AtomicBool::new(false)))}
    pub fn cancel(&self){self.0.store(true,Ordering::Relaxed);}
    pub fn is_cancelled(&self)->bool{self.0.load(Ordering::Relaxed)}
}

#[derive(Debug,Clone,Copy)]
pub struct Progress { pub done:u32, pub total:u32 }

pub fn run_inpaint(req:&InpaintRequest,src:&RasterView,token:&CancelToken,progress:&mut dyn FnMut(Progress))->Result<StagedRegion,CommandError>{
    if token.is_cancelled(){return Err(CommandError::new("CANCELLED"));}
    
    let w=src.width;let h=src.height;
    if w==0 || h==0 { return Ok(StagedRegion{rect:[0,0,0,0],rgba:Vec::new()}); }

    let chunk_size=64u32;
    let total_chunks=(h+chunk_size-1)/chunk_size;
    
    let mut all_candidates=Vec::new();
    
    for chunk_idx in 0..total_chunks{
        if token.is_cancelled(){return Err(CommandError::new("CANCELLED"));}
        
        let y_start=chunk_idx*chunk_size;
        let y_end=(y_start+chunk_size).min(h);
        
        let mut chunk_mask=PixelSelection::empty(w,h);
        for y in y_start..y_end{
            for x in 0..w{
                let idx=y as usize*w as usize+x as usize;
                if req.mask.coverage[idx]!=0 {
                    chunk_mask.coverage[idx]=255;
                }
            }
        }
        
        let mut rng=Rng(req.seed);
        let patch=7u32;
        let max_iters=5u32;
        let half_patch=(patch/2) as i32;
        
        for y in y_start..y_end{
            for x in 0..w{
                if chunk_mask.coverage[y as usize*w as usize+x as usize]==0 { continue; }
                
                let mut best_source=[x,y];
                let mut best_cost=u32::MAX;
                
                for _iter in 0..max_iters{
                    let rand_val=rng.next();
                    let search_radius=1u32<<(_iter+1); 
                    
                    let dx=(rand_val % (search_radius as u64*2)) as i32 - search_radius as i32;
                    let dy=((rand_val >> 32) % (search_radius as u64*2)) as i32 - search_radius as i32;
                    
                    let sx=(x as i32+dx).max(0).min(w as i32-1) as u32;
                    let sy=(y as i32+dy).max(0).min(h as i32-1) as u32;
                    
                    if req.mask.coverage[sy as usize*w as usize+sx as usize]!=0 { continue; }
                    
                    let mut cost=0u32;
                    let mut valid=true;
                    
                    for py in -half_patch..=half_patch{
                        for px in -half_patch..=half_patch{
                            let tx=x as i32+px;let ty=y as i32+py;
                            if tx<0 || tx>=w as i32 || ty<0 || ty>=h as i32 { continue; }
                            
                            if req.mask.coverage[ty as usize*w as usize+tx as usize]!=0 { continue; }
                            
                            let src_x=sx as i32+px;let src_y=sy as i32+py;
                            if src_x<0 || src_x>=w as i32 || src_y<0 || src_y>=h as i32 { valid=false; break; }
                            
                            let target_px=src.get(tx as u32,ty as u32);
                            let source_px=src.get(src_x as u32,src_y as u32);
                            
                            let dr=(target_px[0] as i32-source_px[0] as i32).abs() as u32;
                            let dg=(target_px[1] as i32-source_px[1] as i32).abs() as u32;
                            let db=(target_px[2] as i32-source_px[2] as i32).abs() as u32;
                            let da=(target_px[3] as i32-source_px[3] as i32).abs() as u32;
                            
                            cost+=dr*dr+dg*dg+db*db+da*da;
                        }
                        if !valid { break; }
                    }
                    
                    if valid && cost<best_cost{
                        best_cost=cost;
                        best_source=[sx,sy];
                    }
                }
                
                if best_cost!=u32::MAX{
                    all_candidates.push(PatchCandidate{target:[x,y],source:best_source,cost:best_cost});
                }
            }
        }
        
        progress(Progress{done:(chunk_idx+1),total:total_chunks});
    }
    
    if token.is_cancelled(){return Err(CommandError::new("CANCELLED"));}
    
    let staged=apply_patches(src,&req.mask,&all_candidates,req.opacity);
    
    Ok(staged)
}
