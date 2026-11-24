use crate::raster_view::RasterView;
use kurbo::{BezPath, CurveFitSample, ParamCurveFit, Point, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum TraceMode { Color, Grayscale, BlackWhite }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum PaletteKind { Automatic, Limited(u8), FullTone }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum TraceMethod { Abutting, Overlapping }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TraceSettings {
    pub mode:TraceMode,
    pub palette:PaletteKind,
    pub threshold:u8,
    pub tolerance:f32,
    pub min_area_px:u32,
    pub corner_angle:f32,
    pub method:TraceMethod,
    pub fills:bool,
    pub strokes:bool,
    pub stroke_max_width:f32,
    pub ignore_white:bool,
}

impl Default for TraceSettings {
    fn default()->Self { Self { mode:TraceMode::BlackWhite,palette:PaletteKind::Automatic,threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:false } }
}

#[derive(Debug, Clone)]
pub struct TracedPath { pub path:BezPath,pub fill:Option<[u8;4]>,pub stroke:Option<([u8;4],f32)> }

pub fn validate_settings(settings:&TraceSettings)->bool {
    settings.tolerance.is_finite() && settings.tolerance>=0.0
        && settings.corner_angle.is_finite() && settings.corner_angle>0.0 && settings.corner_angle<=180.0
        && settings.stroke_max_width.is_finite() && settings.stroke_max_width>0.0
        && !matches!(settings.palette,PaletteKind::Limited(n) if !(2..=30).contains(&n))
}

type Vertex=(u32,u32);

fn direction(a:Vertex,b:Vertex)->u8 { if b.0>a.0 {0} else if b.1>a.1 {1} else if b.0<a.0 {2} else {3} }

fn area(points:&[Point])->f64 {
    points.iter().zip(points.iter().cycle().skip(1)).take(points.len()).map(|(a,b)|a.x*b.y-b.x*a.y).sum::<f64>()*0.5
}

fn contains(points:&[Point],point:Point)->bool {
    let mut inside=false;
    for (a,b) in points.iter().zip(points.iter().cycle().skip(1)).take(points.len()) {
        if (a.y>point.y)!=(b.y>point.y) && point.x<(b.x-a.x)*(point.y-a.y)/(b.y-a.y)+a.x { inside=!inside; }
    }
    inside
}

fn remove_small(mask:&mut [bool],width:usize,height:usize,minimum:usize) {
    let mut seen=vec![false;mask.len()];
    for start in 0..mask.len() {
        if seen[start] || !mask[start] {continue;}
        let mut queue=VecDeque::from([start]);let mut component=Vec::new();seen[start]=true;
        while let Some(at)=queue.pop_front() {
            component.push(at);let x=at%width;let y=at/width;
            let neighbours=[if x>0 {Some(at-1)} else {None},if x+1<width {Some(at+1)} else {None},if y>0 {Some(at-width)} else {None},if y+1<height {Some(at+width)} else {None}];
            for next in neighbours.into_iter().flatten() { if mask[next] && !seen[next] {seen[next]=true;queue.push_back(next);} }
        }
        if component.len()<minimum {for at in component {mask[at]=false;}}
    }
}

fn boundaries(mask:&[bool],width:u32,height:u32)->Vec<Vec<Point>> {
    let mut edges:BTreeMap<Vertex,Vec<Vertex>>=BTreeMap::new();
    for y in 0..height {for x in 0..width {
        let at=y as usize*width as usize+x as usize;if !mask[at] {continue;}
        if y==0 || !mask[at-width as usize] {edges.entry((x,y)).or_default().push((x+1,y));}
        if x+1==width || !mask[at+1] {edges.entry((x+1,y)).or_default().push((x+1,y+1));}
        if y+1==height || !mask[at+width as usize] {edges.entry((x+1,y+1)).or_default().push((x,y+1));}
        if x==0 || !mask[at-1] {edges.entry((x,y+1)).or_default().push((x,y));}
    }}
    let mut rings=Vec::new();
    while let Some((&start,_))=edges.first_key_value() {
        let mut current=start;let mut incoming=0_u8;let mut points=Vec::new();
        loop {
            points.push(Point::new(current.0 as f64,current.1 as f64));
            let Some(outgoing)=edges.get_mut(&current) else {break;};
            let index=outgoing.iter().enumerate().min_by_key(|(_,next)| {
                let turn=(direction(current,**next)+4-incoming)%4;
                match turn {1=>0,0=>1,3=>2,_=>3}
            }).map(|(index,_)|index).unwrap();
            let next=outgoing.remove(index);if outgoing.is_empty() {edges.remove(&current);}
            incoming=direction(current,next);current=next;
            if current==start { if points.len()>=3 {rings.push(points);}break; }
        }
    }
    rings
}

fn segment_distance(point:Point,a:Point,b:Point)->f64 {
    let delta=b-a;let length=delta.x*delta.x+delta.y*delta.y;
    if length==0.0 {return point.distance(a);}
    let relative=point-a;let t=((relative.x*delta.x+relative.y*delta.y)/length).clamp(0.0,1.0);
    point.distance(a+delta*t)
}

fn simplify_open(points:&[Point],tolerance:f64)->Vec<Point> {
    if points.len()<3 {return points.to_vec();}
    let mut keep=vec![false;points.len()];keep[0]=true;keep[points.len()-1]=true;
    let mut stack=vec![(0,points.len()-1)];
    while let Some((start,end))=stack.pop() {
        let farthest=(start+1..end).map(|at|(at,segment_distance(points[at],points[start],points[end]))).max_by(|a,b|a.1.total_cmp(&b.1));
        if let Some((at,distance))=farthest {if distance>tolerance {keep[at]=true;stack.push((start,at));stack.push((at,end));}}
    }
    points.iter().zip(keep).filter_map(|(point,keep)|keep.then_some(*point)).collect()
}

fn simplify_closed(points:&[Point],tolerance:f64)->Vec<Point> {
    let far=(1..points.len()).max_by(|&a,&b|points[a].distance(points[0]).total_cmp(&points[b].distance(points[0]))).unwrap_or(1);
    let mut first=simplify_open(&points[..=far],tolerance);
    let mut second=points[far..].to_vec();second.push(points[0]);second=simplify_open(&second,tolerance);
    first.pop();second.pop();first.extend(second);
    if first.len()<3 || area(&first)*area(points)<=0.0 {points.to_vec()} else {first}
}

struct Polyline {points:Vec<Point>,lengths:Vec<f64>,corners:Vec<f64>}

impl Polyline {
    fn new(mut points:Vec<Point>,corner_angle:f64,closed:bool)->Self {
        if closed { points.push(points[0]); }
        let count=points.len();let mut corners=Vec::new();let mut lengths=vec![0.0];
        for i in 0..count-1 {lengths.push(lengths[i]+points[i].distance(points[i+1]));}
        let total=*lengths.last().unwrap();
        for i in 1..count-1 {
            let before=points[i]-points[i-1];let after=points[i+1]-points[i];
            let denominator=before.x.hypot(before.y)*after.x.hypot(after.y);
            if denominator>0.0 {
                let angle=((before.x*after.x+before.y*after.y)/denominator).clamp(-1.0,1.0).acos().to_degrees();
                if angle>=corner_angle {corners.push(lengths[i]/total);}
            }
        }
        Self {points,lengths,corners}
    }
    fn sample(&self,t:f64,sign:f64)->(Point,Vec2) {
        let total=*self.lengths.last().unwrap();let distance=t.clamp(0.0,1.0)*total;
        let index=self.lengths.partition_point(|&v|if sign<0.0 {v<distance} else {v<=distance}).saturating_sub(1).min(self.points.len()-2);
        let length=self.lengths[index+1]-self.lengths[index];
        let delta=self.points[index+1]-self.points[index];
        let offset=if length>0.0 {(distance-self.lengths[index])/length} else {0.0};
        (self.points[index]+delta*offset,if length>0.0 {delta*(total/length)} else {Vec2::ZERO})
    }
}

impl ParamCurveFit for Polyline {
    fn sample_pt_tangent(&self,t:f64,sign:f64)->CurveFitSample {let (p,tangent)=self.sample(t,sign);CurveFitSample {p,tangent}}
    fn sample_pt_deriv(&self,t:f64)->(Point,Vec2) {self.sample(t,1.0)}
    fn break_cusp(&self,range:std::ops::Range<f64>)->Option<f64> {self.corners.iter().copied().find(|&t|t>range.start+1e-10 && t<range.end-1e-10)}
}

fn append_fitted(path:&mut BezPath,points:&[Point],settings:&TraceSettings) {
    let tolerance=(f64::from(settings.tolerance)*0.5).max(1e-6);
    let simplified=simplify_closed(points,tolerance);
    let source=Polyline::new(simplified,f64::from(settings.corner_angle),true);
    let fitted=kurbo::fit_to_bezpath(&source,tolerance);
    path.extend(fitted.elements().iter().copied());path.close_path();
}

fn luminance(r:u8,g:u8,b:u8)->f32 { (54.0*r as f32 + 183.0*g as f32 + 19.0*b as f32) / 256.0 }

fn channel_range(pixels:&[[u8;4]])->(usize,u8) {
    let mut best=(0usize,0u8);
    for axis in 0..3 {
        let min=pixels.iter().map(|p|p[axis]).min().unwrap_or(0);
        let max=pixels.iter().map(|p|p[axis]).max().unwrap_or(0);
        if max-min>best.1 {best=(axis,max-min);}
    }
    best
}

fn median_cut(pixels:&[[u8;4]],max_colors:usize)->Vec<[u8;4]> {
    if pixels.is_empty() || max_colors==0 { return vec![]; }
    let mut boxes:Vec<Vec<[u8;4]>>=vec![pixels.to_vec()];
    while boxes.len()<max_colors {
        let Some((index,axis))=boxes.iter().enumerate().map(|(i,b)|(i,channel_range(b))).filter(|(_,(_,range))|*range>0).max_by_key(|(_,(_,range))|*range).map(|(i,(axis,_))|(i,axis)) else {break;};
        let mut sorted=boxes.swap_remove(index);
        sorted.sort_by_key(|p|p[axis]);
        let median=sorted[sorted.len()/2][axis];
        let mut cut=sorted.partition_point(|p|p[axis]<median);
        if cut==0 {cut=sorted.partition_point(|p|p[axis]<=median);}
        let right=sorted.split_off(cut);
        boxes.push(sorted);
        boxes.push(right);
    }
    boxes.iter().map(|b| {
        let count=b.len() as u32;
        let sum=b.iter().fold([0u32;3],|acc,p|[acc[0]+p[0] as u32,acc[1]+p[1] as u32,acc[2]+p[2] as u32]);
        [(sum[0]/count) as u8,(sum[1]/count) as u8,(sum[2]/count) as u8,255]
    }).collect()
}

pub fn quantize(src:&RasterView,s:&TraceSettings)->(Vec<u8>,Vec<[u8;4]>) {
    let count=src.width as usize * src.height as usize;
    if count==0 || src.rgba.len()<count*4 { return (vec![],vec![]); }
    
    let pixels:Vec<[u8;4]>=src.rgba[..count*4].chunks_exact(4).map(|c|[c[0],c[1],c[2],c[3]]).collect();
    
    let max_colors = match s.palette {
        PaletteKind::Automatic => 16,
        PaletteKind::Limited(n @ 2..=30) => usize::from(n),
        PaletteKind::Limited(_) => return (vec![], vec![]),
        PaletteKind::FullTone => 256,
    };
    let visible: Vec<_> = pixels.iter().copied().filter(|p| p[3] > 0).collect();
    let mut unique = visible.clone();
    unique.sort_by_key(|p| [p[0], p[1], p[2], 255 - p[3]]);
    unique.dedup_by_key(|p| [p[0], p[1], p[2]]);
    let palette = if unique.len() <= max_colors { unique } else { median_cut(&visible, max_colors) };
    
    if palette.is_empty() { return (vec![],vec![]); }
    
    let mut indices=Vec::with_capacity(count);
    for p in &pixels {
        let mut best_idx=0;let mut best_dist=f32::MAX;
        for (i,c) in palette.iter().enumerate() {
            let dr=p[0] as f32-c[0] as f32;
            let dg=p[1] as f32-c[1] as f32;
            let db=p[2] as f32-c[2] as f32;
            let dist=dr*dr+dg*dg+db*db;
            if dist<best_dist {best_dist=dist;best_idx=i;}
        }
        indices.push(best_idx as u8);
    }
    
    (indices,palette)
}

pub fn trace_image(source:&RasterView,settings:&TraceSettings)->Vec<TracedPath> {
    if !validate_settings(settings) || source.width==0 || source.height==0 || source.width==u32::MAX || source.height==u32::MAX || (!settings.fills && !settings.strokes) {return Vec::new();}
    
    let Some(count)=(source.width as usize).checked_mul(source.height as usize) else {return Vec::new();};
    let Some(bytes)=count.checked_mul(4) else {return Vec::new();};if source.rgba.len()<bytes {return Vec::new();}
    if settings.strokes && !settings.fills {
        return centreline(source, settings);
    }
    
    let mut result=Vec::new();
    
    match settings.mode {
        TraceMode::BlackWhite => {
            for black in [false,true] {
                if !black && settings.ignore_white {continue;}
                let colour=if black {[0,0,0,255]} else {[255,255,255,255]};
                let mut mask:Vec<bool>=source.rgba[..bytes].chunks_exact(4).map(|pixel| {
                    let lum=luminance(pixel[0],pixel[1],pixel[2]);
                    pixel[3]>0 && (lum<f32::from(settings.threshold))==black
                }).collect();
                remove_small(&mut mask,source.width as usize,source.height as usize,settings.min_area_px as usize);
                let rings=boundaries(&mask,source.width,source.height);
                let outers:Vec<_>=rings.iter().filter(|ring|area(ring)>0.0).collect();
                let holes:Vec<_>=rings.iter().filter(|ring|area(ring)<0.0).collect();
                let owners:Vec<_>=holes.iter().map(|hole|outers.iter().enumerate().filter(|(_,outer)|contains(outer,hole[0])).min_by(|(_,a),(_,b)|area(a).abs().total_cmp(&area(b).abs())).map(|(i,_)|i)).collect();
                for (index,outer) in outers.iter().enumerate() {
                    let mut path=BezPath::new();append_fitted(&mut path,outer,settings);
                    for (hole,owner) in holes.iter().zip(&owners) {if *owner==Some(index) {append_fitted(&mut path,hole,settings);}}
                    result.push(TracedPath {path,fill:settings.fills.then_some(colour),stroke:settings.strokes.then_some((colour,settings.stroke_max_width))});
                }
            }
        },
        TraceMode::Grayscale => {
            let levels=match settings.palette {
                PaletteKind::Automatic=>16,
                PaletteKind::Limited(n)=>n as usize,
                PaletteKind::FullTone=>256,
            };
            let mut masks:Vec<Vec<bool>>=vec![vec![false;count];levels];
            for i in 0..count {
                let pixel=&source.rgba[i*4..i*4+4];
                if pixel[3]==0 {continue;}
                let lum=luminance(pixel[0],pixel[1],pixel[2]);
                let level=((lum/256.0)*levels as f32).floor() as usize;
                if level<levels {masks[level][i]=true;}
            }
            for (level_idx,mask) in masks.iter_mut().enumerate() {
                remove_small(mask,source.width as usize,source.height as usize,settings.min_area_px as usize);
                let rings=boundaries(mask,source.width,source.height);
                let outers:Vec<_>=rings.iter().filter(|ring|area(ring)>0.0).collect();
                let holes:Vec<_>=rings.iter().filter(|ring|area(ring)<0.0).collect();
                let owners:Vec<_>=holes.iter().map(|hole|outers.iter().enumerate().filter(|(_,outer)|contains(outer,hole[0])).min_by(|(_,a),(_,b)|area(a).abs().total_cmp(&area(b).abs())).map(|(i,_)|i)).collect();
                let grey=((level_idx as f32+0.5)/levels as f32*255.0) as u8;
                let colour=[grey,grey,grey,255];
                if settings.ignore_white && grey>240 {continue;}
                for (index,outer) in outers.iter().enumerate() {
                    let mut path=BezPath::new();append_fitted(&mut path,outer,settings);
                    for (hole,owner) in holes.iter().zip(&owners) {if *owner==Some(index) {append_fitted(&mut path,hole,settings);}}
                    result.push(TracedPath {path,fill:settings.fills.then_some(colour),stroke:settings.strokes.then_some((colour,settings.stroke_max_width))});
                }
            }
        },
        TraceMode::Color => {
            let (indices,palette)=quantize(source,settings);
            if palette.is_empty() {return Vec::new();}
            
            let mut order: Vec<_> = (0..palette.len()).collect();
            order.sort_by(|&a, &b| {
                let [ar, ag, ab, _] = palette[a];
                let [br, bg, bb, _] = palette[b];
                luminance(ar, ag, ab).total_cmp(&luminance(br, bg, bb))
            });
            let mut ranks = vec![0; palette.len()];
            for (rank, &index) in order.iter().enumerate() {
                ranks[index] = rank;
            }
            for (rank, color_idx) in order.into_iter().enumerate() {
                let colour = palette[color_idx];
                let is_white = |[r, g, b, _]: [u8; 4]| r > 240 && g > 240 && b > 240;
                if settings.ignore_white && is_white(colour) { continue; }
                let mut mask: Vec<_> = indices.iter().zip(source.rgba.chunks_exact(4)).map(|(&index, pixel)| {
                    let index = usize::from(index);
                    pixel[3] > 0 && (!settings.ignore_white || !is_white(palette[index]))
                        && (index == color_idx || settings.method == TraceMethod::Overlapping && ranks[index] > rank)
                }).collect();
                remove_small(&mut mask, source.width as usize, source.height as usize, settings.min_area_px as usize);
                let rings = boundaries(&mask, source.width, source.height);
                let outers: Vec<_> = rings.iter().filter(|ring| area(ring) > 0.0).collect();
                let holes: Vec<_> = rings.iter().filter(|ring| area(ring) < 0.0).collect();
                let owners: Vec<_> = holes.iter().map(|hole| outers.iter().enumerate()
                    .filter(|(_, outer)| contains(outer, hole[0]))
                    .min_by(|(_, a), (_, b)| area(a).abs().total_cmp(&area(b).abs()))
                    .map(|(i, _)| i)).collect();
                for (index, outer) in outers.iter().enumerate() {
                    let mut path = BezPath::new();
                    append_fitted(&mut path, outer, settings);
                    for (hole, owner) in holes.iter().zip(&owners) {
                        if *owner == Some(index) { append_fitted(&mut path, hole, settings); }
                    }
                    result.push(TracedPath {
                        path,
                        fill: settings.fills.then_some(colour),
                        stroke: settings.strokes.then_some((colour, settings.stroke_max_width)),
                    });
                }
            }
        },
    }
    
    result
}

fn zhang_suen_thin(mask:&mut [bool],width:usize,height:usize) {
    let mut changed=true;
    while changed {
        changed=false;
        for step in 0..2 {
            let mut to_remove=Vec::new();
            for y in 1..height-1 {
                for x in 1..width-1 {
                    let at=y*width+x;
                    if !mask[at] {continue;}
                    let p=[mask[at-width],mask[at-width+1],mask[at+1],mask[at+width+1],mask[at+width],mask[at+width-1],mask[at-1],mask[at-width-1]];
                    let b0=p[0];let b1=p[1];let b2=p[2];let b3=p[3];let b4=p[4];let b5=p[5];let b6=p[6];let b7=p[7];
                    let neighbors=(b0 as u8)+(b1 as u8)+(b2 as u8)+(b3 as u8)+(b4 as u8)+(b5 as u8)+(b6 as u8)+(b7 as u8);
                    if neighbors<2 || neighbors>6 {continue;}
                    let transitions=(0..8).filter(|&i|!p[i] && p[(i+1)%8]).count();
                    if transitions!=1 {continue;}
                    let c1=!(b0 && b2 && b4) && !(b2 && b4 && b6);
                    let c2=!(b0 && b2 && b6) && !(b0 && b4 && b6);
                    let condition=if step==0 {c1} else {c2};
                    if condition {to_remove.push(at);}
                }
            }
            for at in to_remove {mask[at]=false;changed=true;}
        }
    }
}

fn walk_skeleton(component:&[usize],width:usize,settings:&TraceSettings)->BezPath {
    let members:HashSet<usize>=component.iter().copied().collect();
    let neighbours=|at:usize|->Vec<usize> {
        let (x,y)=((at%width) as i64,(at/width) as i64);
        let mut found=Vec::new();
        for (dx,dy) in [(0,-1),(1,0),(0,1),(-1,0),(1,-1),(1,1),(-1,1),(-1,-1)] {
            let (nx,ny)=(x+dx,y+dy);
            if nx<0 || ny<0 || nx>=width as i64 {continue;}
            if dx != 0 && dy != 0
                && (members.contains(&(y as usize * width + nx as usize))
                    || members.contains(&(ny as usize * width + x as usize)))
            {
                continue;
            }
            let next=ny as usize*width+nx as usize;
            if members.contains(&next) {found.push(next);}
        }
        found
    };
    let point=|at:usize|Point::new((at%width) as f64+0.5,(at/width) as f64+0.5);
    let edge = |a: usize, b: usize| (a.min(b), a.max(b));
    let mut seen = HashSet::new();
    let mut path = BezPath::new();
    let tolerance = (f64::from(settings.tolerance) * 0.5).max(1e-6);
    let starts = component.iter().copied().filter(|&at| neighbours(at).len() != 2)
        .chain(component.iter().copied());
    for start in starts {
        for first in neighbours(start) {
            if !seen.insert(edge(start, first)) { continue; }
            let mut points = vec![point(start), point(first)];
            let mut current = first;
            loop {
                let candidates = neighbours(current);
                if candidates.len() != 2 || current == start { break; }
                let Some(next) = candidates.into_iter().find(|&next| seen.insert(edge(current, next))) else { break; };
                points.push(point(next));
                current = next;
            }
            let closed = current == start;
            if closed {
                points.pop();
                append_fitted(&mut path, &points, settings);
                continue;
            }
            let points = simplify_open(&points, tolerance);
            let source = Polyline::new(points, f64::from(settings.corner_angle), false);
            path.extend(kurbo::fit_to_bezpath(&source, tolerance).elements().iter().copied());
        }
    }
    path
}

fn distance_to_edge(mask:&[bool],width:usize,height:usize,x:usize,y:usize)->f64 {
    let mut min_dist=f64::MAX;
    for dy in -10..=10 {
        for dx in -10..=10 {
            let nx=x as i32+dx;let ny=y as i32+dy;
            if nx<0 || ny<0 || nx>=width as i32 || ny>=height as i32 {continue;}
            let at=(ny as usize)*width+(nx as usize);
            if !mask[at] {
                let dist=((dx*dx+dy*dy) as f64).sqrt();
                if dist<min_dist {min_dist=dist;}
            }
        }
    }
    if min_dist==f64::MAX {1.0} else {min_dist}
}

pub fn centreline(source:&RasterView,settings:&TraceSettings)->Vec<TracedPath> {
    if !validate_settings(settings) || source.width==0 || source.height==0 || source.width==u32::MAX || source.height==u32::MAX || (!settings.fills && !settings.strokes) {return Vec::new();}
    let Some(count)=(source.width as usize).checked_mul(source.height as usize) else {return Vec::new();};
    let Some(bytes)=count.checked_mul(4) else {return Vec::new();};if source.rgba.len()<bytes {return Vec::new();}
    
    let mut mask:Vec<bool>=source.rgba[..bytes].chunks_exact(4).map(|pixel| {
        let lum=luminance(pixel[0],pixel[1],pixel[2]);
        pixel[3]>0 && lum<f32::from(settings.threshold)
    }).collect();
    
    remove_small(&mut mask,source.width as usize,source.height as usize,settings.min_area_px as usize);
    let solid=mask.clone();
    zhang_suen_thin(&mut mask,source.width as usize,source.height as usize);
    
    let mut result=Vec::new();
    let width=source.width as usize;
    let height=source.height as usize;
    let mut visited=vec![false;mask.len()];
    
    for y in 0..height {
        for x in 0..width {
            let at=y*width+x;
            if !mask[at] || visited[at] {continue;}
            
            let mut queue=VecDeque::from([at]);
            let mut component=Vec::new();
            visited[at]=true;
            
            while let Some(current)=queue.pop_front() {
                component.push(current);
                let cx=current%width;let cy=current/width;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx==0 && dy==0 {continue;}
                        let nx=cx as i32+dx;let ny=cy as i32+dy;
                        if nx<0 || ny<0 || nx>=width as i32 || ny>=height as i32 {continue;}
                        let next=(ny as usize)*width+(nx as usize);
                        if mask[next] && !visited[next] {
                            visited[next]=true;
                            queue.push_back(next);
                        }
                    }
                }
            }
            
            if component.len()<2 {continue;}
            
            let mut total_dist=0.0;
            for &at in &component {
                let x=at%width;let y=at/width;
                total_dist+=distance_to_edge(&solid,width,height,x,y);
            }
            let avg_dist=total_dist/component.len() as f64;
            let stroke_width=(avg_dist*2.0-1.0).max(1.0).min(settings.stroke_max_width as f64) as f32;
            
            let path=walk_skeleton(&component,width,settings);
            
            result.push(TracedPath {path,fill:None,stroke:Some(([0,0,0,255],stroke_width))});
        }
    }
    
    result
}

#[derive(Debug, Clone)]
#[derive(serde::Serialize,serde::Deserialize)]
pub struct TracePreset { pub name:String, pub builtin:bool, pub settings:TraceSettings }

pub fn builtin_presets()->Vec<TracePreset> {
    vec![
        TracePreset {name:"Auto Color".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::Color,palette:PaletteKind::Automatic,threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:false}},
        TracePreset {name:"High Color".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::Color,palette:PaletteKind::Limited(30),threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:false}},
        TracePreset {name:"Low Color".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::Color,palette:PaletteKind::Limited(6),threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:false}},
        TracePreset {name:"Grayscale".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::Grayscale,palette:PaletteKind::Automatic,threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:false}},
        TracePreset {name:"Black and White".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::BlackWhite,palette:PaletteKind::Automatic,threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:false}},
        TracePreset {name:"Outline".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::BlackWhite,palette:PaletteKind::Automatic,threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:false,strokes:true,stroke_max_width:10.0,ignore_white:true}},
        TracePreset {name:"High Fidelity Photo".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::Color,palette:PaletteKind::Automatic,threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:false}},
        TracePreset {name:"Low Fidelity Photo".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::Color,palette:PaletteKind::Limited(8),threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:false}},
        TracePreset {name:"3 Colors".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::Color,palette:PaletteKind::Limited(3),threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:false}},
        TracePreset {name:"6 Colors".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::Color,palette:PaletteKind::Limited(6),threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:false}},
        TracePreset {name:"16 Colors".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::Color,palette:PaletteKind::Limited(16),threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:false}},
        TracePreset {name:"Shades of Gray".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::Grayscale,palette:PaletteKind::Automatic,threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:false}},
        TracePreset {name:"Black and White Logo".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::BlackWhite,palette:PaletteKind::Automatic,threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:true}},
        TracePreset {name:"Sketched Art".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::BlackWhite,palette:PaletteKind::Automatic,threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:false,strokes:true,stroke_max_width:10.0,ignore_white:true}},
        TracePreset {name:"Silhouettes".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::BlackWhite,palette:PaletteKind::Automatic,threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:true,strokes:false,stroke_max_width:10.0,ignore_white:true}},
        TracePreset {name:"Line Art".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::BlackWhite,palette:PaletteKind::Automatic,threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:false,strokes:true,stroke_max_width:10.0,ignore_white:true}},
        TracePreset {name:"Technical Drawing".to_string(),builtin:true,settings:TraceSettings{mode:TraceMode::BlackWhite,palette:PaletteKind::Automatic,threshold:128,tolerance:1.0,min_area_px:4,corner_angle:60.0,method:TraceMethod::Abutting,fills:false,strokes:true,stroke_max_width:10.0,ignore_white:true}},
    ]
}

#[cfg(test)]
mod contracts {
    use super::*;
    use kurbo::Shape;
    fn opaque(width:u32,height:u32,colour:[u8;4])->Vec<u8> {
        let mut bytes=Vec::new();for _ in 0..width*height {bytes.extend_from_slice(&colour);}bytes
    }
    #[test]
    fn ten_pixel_square_has_one_closed_path_on_pixel_edges() {
        let mut bytes=opaque(12,12,[255,255,255,255]);
        for y in 0..10 {for x in 0..10 {let at=(y*12+x)*4;bytes[at..at+4].copy_from_slice(&[0,0,0,255]);}}
        let settings=TraceSettings {ignore_white:true,tolerance:0.25,..TraceSettings::default()};
        let paths=trace_image(&RasterView::new(12,12,&bytes),&settings);
        assert_eq!(paths.len(),1);let bounds=paths[0].path.bounding_box();
        for (actual,expected) in [(bounds.x0,0.0),(bounds.y0,0.0),(bounds.x1,10.0),(bounds.y1,10.0)] {assert!((actual-expected).abs()<0.001);}
        assert!(matches!(paths[0].path.elements().last(),Some(kurbo::PathEl::ClosePath)));
        assert_eq!(paths[0].fill,Some([0,0,0,255]));
    }
    #[test]
    fn white_hole_keeps_opposite_winding_in_one_black_path() {
        let mut bytes=opaque(7,7,[0,0,0,255]);
        for y in 2..5 {for x in 2..5 {let at=(y*7+x)*4;bytes[at..at+4].copy_from_slice(&[255,255,255,255]);}}
        let settings=TraceSettings {ignore_white:true,tolerance:0.25,..TraceSettings::default()};
        let paths=trace_image(&RasterView::new(7,7,&bytes),&settings);assert_eq!(paths.len(),1);
        assert_eq!(paths[0].path.elements().iter().filter(|el|matches!(el,kurbo::PathEl::ClosePath)).count(),2);
        assert!((paths[0].path.area().abs()-40.0).abs()<0.1);
    }
    #[test]
    fn regions_smaller_than_minimum_are_removed() {
        let mut bytes=opaque(4,4,[255,255,255,255]);bytes[..4].copy_from_slice(&[0,0,0,255]);
        let settings=TraceSettings {ignore_white:true,min_area_px:4,..TraceSettings::default()};
        assert!(trace_image(&RasterView::new(4,4,&bytes),&settings).is_empty());
    }
    #[test]
    fn three_pixel_line_traces_to_one_open_path_with_stroke_width_about_3() {
        let mut bytes=opaque(10,5,[255,255,255,255]);
        for y in 1..4 {for x in 2..8 {let at=(y*10+x)*4;bytes[at..at+4].copy_from_slice(&[0,0,0,255]);}}
        let settings=TraceSettings {strokes:true,fills:false,ignore_white:true,min_area_px:1,..TraceSettings::default()};
        let paths=centreline(&RasterView::new(10,5,&bytes),&settings);
        assert_eq!(paths.len(),1);
        assert!(paths[0].fill.is_none());
        if let Some((_,width))=paths[0].stroke {assert!((width-3.0).abs()<1.0);} else {panic!("expected stroke");}
    }
    #[test]
    fn limited_3_gives_3_colors_limited_31_rejected() {
        let mut bytes=opaque(4,4,[255,255,255,255]);
        for y in 0..2 {for x in 0..4 {let at=(y*4+x)*4;bytes[at..at+4].copy_from_slice(&[255,0,0,255]);}}
        for y in 2..3 {for x in 0..4 {let at=(y*4+x)*4;bytes[at..at+4].copy_from_slice(&[0,255,0,255]);}}
        for y in 3..4 {for x in 0..4 {let at=(y*4+x)*4;bytes[at..at+4].copy_from_slice(&[0,0,255,255]);}}
        
        let settings=TraceSettings{mode:TraceMode::Color,palette:PaletteKind::Limited(3),min_area_px:1,..Default::default()};
        let paths=trace_image(&RasterView::new(4,4,&bytes),&settings);
        assert_eq!(paths.len(),3);
        
        let settings_bad=TraceSettings{mode:TraceMode::Color,palette:PaletteKind::Limited(31),min_area_px:1,..Default::default()};
        let paths_bad=trace_image(&RasterView::new(4,4,&bytes),&settings_bad);
        assert!(paths_bad.is_empty());
    }
}
