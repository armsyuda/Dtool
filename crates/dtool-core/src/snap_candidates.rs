use crate::{Document, ViewState};
use crate::guides::Guides;
use crate::grid::{Grid, GridKind};
use crate::snap_prefs::SnapPrefs;

pub struct SnapDocument<'a> {
    pub document: &'a Document,
    pub guides: &'a Guides,
    pub grid: &'a Grid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateKind { Guide, Grid, Page, ObjectBounds, ObjectGeometry, Pixel }

impl CandidateKind {
    pub fn priority(self) -> u8 {
        match self {
            CandidateKind::Guide => 0,
            CandidateKind::ObjectGeometry => 1,
            CandidateKind::ObjectBounds => 2,
            CandidateKind::Grid => 3,
            CandidateKind::Page => 4,
            CandidateKind::Pixel => 5,
        }
    }
}

fn lattice_point(point:[f32;2],origin:[f32;2],u:[f32;2],v:[f32;2])->Option<[f32;2]> {
    let det=u[0]*v[1]-u[1]*v[0];
    if !det.is_finite() || det.abs()<=1e-4*u[0].hypot(u[1])*v[0].hypot(v[1]) {return None;}
    let d=[point[0]-origin[0],point[1]-origin[1]];
    let i=((d[0]*v[1]-d[1]*v[0])/det).round();
    let j=((u[0]*d[1]-u[1]*d[0])/det).round();
    Some([origin[0]+i*u[0]+j*v[0],origin[1]+i*u[1]+j*v[1]])
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate { pub point:[f32;2], pub distance:f32, pub kind:CandidateKind, pub owner:Option<u64> }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SmartGuideKind { Horizontal, Vertical }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmartGuide {
    pub kind: SmartGuideKind,
    pub position: f32,
    pub source_owner: Option<u64>,
    pub target_owner: Option<u64>,
}

pub fn snap_candidates(doc:&SnapDocument<'_>,view:&ViewState,prefs:&SnapPrefs,point:[f32;2])->Vec<Candidate> {
    let mut result=Vec::new();
    if !prefs.enabled || !view.zoom.is_finite() || view.zoom<=0.0 || point.iter().any(|v|!v.is_finite()){return result;}
    let tolerance=prefs.screen_tolerance as f32/view.zoom;
    let mut offer=|target:[f32;2],kind,owner| {
        let distance=(target[0]-point[0]).hypot(target[1]-point[1]);
        if distance.is_finite() && distance<=tolerance { result.push(Candidate{point:target,distance,kind,owner}); }
    };
    if prefs.snap_to_guides {
        for guide in doc.guides.list() {
            let axis = guide.kind.axis();
            let mut target = point;
            if guide.page_id.is_some() {
                let Some([a, b]) = guide.page_segment(doc.document) else { continue };
                target[axis] = a[axis];
                target[1 - axis] = point[1 - axis].clamp(a[1 - axis], b[1 - axis]);
            } else {
                target[axis] = guide.position;
            }
            offer(target,CandidateKind::Guide,Some(guide.id.0));
        }
        if !view.canvas_flags.preview_mode {
            if doc.guides.show_columns {
                for (_, grid) in crate::guides::column_layout::layouts(doc.document) {
                    let [x, y, w, h] = grid.bounds;
                    let from = [x, y];
                    let to = [x + w, y + h];
                    if (0..2).any(|axis| point[axis] < from[axis] - tolerance || point[axis] > to[axis] + tolerance) {
                        continue;
                    }
                    for axis in 0..2 {
                        let cross = 1 - axis;
                        let along = point[cross].clamp(from[cross], to[cross]);
                        for edge in grid.boundaries(axis) {
                            let mut target = point;
                            target[axis] = edge;
                            target[cross] = along;
                            offer(target, CandidateKind::Guide, None);
                        }
                    }
                }
            }
            for (axis, segments) in crate::page_fold::segments(doc.document) {
                let axis = axis.index();
                for [a, b] in segments {
                    let mut target = point;
                    target[axis] = a[axis];
                    target[1 - axis] = point[1 - axis].clamp(a[1 - axis], b[1 - axis]);
                    offer(target, CandidateKind::Guide, None);
                }
            }
        }
    }
    if prefs.snap_to_baseline_grid && !view.canvas_flags.preview_mode {
        for page in crate::grid::baseline::pages(doc.document) {
            let [x, y, w, h] = page.rect;
            if point[0] < x - tolerance || point[0] > x + w + tolerance || point[1] < y - tolerance || point[1] > y + h + tolerance {
                continue;
            }
            let last = ((y + h - page.first) / page.spacing).floor();
            let line = ((point[1] - page.first) / page.spacing).round().clamp(0.0, last);
            offer([point[0].clamp(x, x + w), page.first + line * page.spacing], CandidateKind::Grid, None);
        }
    }
    if prefs.snap_to_grid && doc.grid.show && doc.grid.spacing.iter().all(|v|v.is_finite() && *v>0.0) {
        let sx=doc.grid.spacing[0]/doc.grid.subdivisions.max(1) as f32;
        let sy=doc.grid.spacing[1]/doc.grid.subdivisions.max(1) as f32;
        let [ox,oy]=if doc.grid.origin.iter().all(|v|v.is_finite()) {doc.grid.origin} else {[0.0,0.0]};
        match doc.grid.kind {
            GridKind::Standard=>{
                offer([ox+((point[0]-ox)/sx).round()*sx,point[1]],CandidateKind::Grid,None);
                offer([point[0],oy+((point[1]-oy)/sy).round()*sy],CandidateKind::Grid,None);
            }
            GridKind::Isometric=>{
                let row=((point[1]-oy)/sy).round();
                let x=((point[0]-ox-row*sx*0.5)/sx).round()*sx+row*sx*0.5+ox;
                offer([x,oy+row*sy],CandidateKind::Grid,None);
            }
            GridKind::Axonometric=>{
                let [a,b]=[doc.grid.angles[0].to_radians(),doc.grid.angles[1].to_radians()];
                if let Some(target)=lattice_point(point,[ox,oy],[sx*a.cos(),-sx*a.sin()],[-sy*b.cos(),-sy*b.sin()]) {offer(target,CandidateKind::Grid,None);}
            }
            GridKind::Custom=>{
                let a=doc.grid.angles[0].to_radians();
                if let Some(target)=lattice_point(point,[ox,oy],[sx*a.cos(),sx*a.sin()],[-sy*a.sin(),sy*a.cos()]) {offer(target,CandidateKind::Grid,None);}
            }
        }
    }
    if prefs.page_snapping {
        if let Some(size)=doc.document.pages.first().map(|page|page.size).filter(|size|size.iter().all(|v|v.is_finite() && *v>0.0)) {
            for x in [0.0,size[0]] {offer([x,point[1]],CandidateKind::Page,None);}
            for y in [0.0,size[1]] {offer([point[0],y],CandidateKind::Page,None);}
        }
    }
    if doc.grid.show_pixel || prefs.force_pixel_alignment || view.canvas_flags.force_pixel_alignment {
        offer([point[0].round(),point[1].round()],CandidateKind::Pixel,None);
    }
    if prefs.object_snapping {
        for object in &doc.document.objects {
            if !object.pickable || (prefs.only_visible_objects && object.opacity<=0.0) {continue;}
            let pos=object.pos;let end=[pos[0]+object.size[0],pos[1]+object.size[1]];
            if prefs.snap_to_object_bounds {
                for x in [pos[0],end[0]] {offer([x,point[1]],CandidateKind::ObjectBounds,Some(object.id.0));}
                for y in [pos[1],end[1]] {offer([point[0],y],CandidateKind::ObjectBounds,Some(object.id.0));}
                if prefs.include_bbox_midpoints {offer([(pos[0]+end[0])*0.5,point[1]],CandidateKind::ObjectBounds,Some(object.id.0));offer([point[0],(pos[1]+end[1])*0.5],CandidateKind::ObjectBounds,Some(object.id.0));}
            }
            if prefs.snap_to_shape_key_points || prefs.snap_to_object_geometry {
                let corners=crate::geometry::object_world_quad(object);
                for index in 0..4 {
                    let a=corners[index];let b=corners[(index+1)%4];
                    if prefs.snap_to_shape_key_points {offer(a,CandidateKind::ObjectGeometry,Some(object.id.0));}
                    if prefs.snap_to_object_geometry {
                        let delta=[b[0]-a[0],b[1]-a[1]];let length=delta[0]*delta[0]+delta[1]*delta[1];
                        if length>0.0 {let t=(((point[0]-a[0])*delta[0]+(point[1]-a[1])*delta[1])/length).clamp(0.0,1.0);offer([a[0]+t*delta[0],a[1]+t*delta[1]],CandidateKind::ObjectGeometry,Some(object.id.0));}
                    }
                }
            }
        }
    }
    result.sort_by(|a,b|a.distance.total_cmp(&b.distance).then(a.kind.priority().cmp(&b.kind.priority())));
    result.dedup_by(|a,b|a.point==b.point && a.kind==b.kind && a.owner==b.owner);
    result.truncate(prefs.max_candidates as usize);result
}

pub fn smart_guides(doc:&SnapDocument<'_>,view:&ViewState,prefs:&SnapPrefs,point:[f32;2],moving_owner:Option<u64>)->Vec<SmartGuide> {
    let mut guides=Vec::new();
    if !prefs.enabled || !prefs.dynamic_guides || !prefs.object_snapping || !prefs.snap_to_object_bounds {return guides;}
    if !view.zoom.is_finite() || view.zoom<=0.0 || point.iter().any(|v|!v.is_finite()){return guides;}
    let tolerance=prefs.screen_tolerance as f32/view.zoom;

    for object in &doc.document.objects {
        if Some(object.id.0)==moving_owner {continue;}
        if !object.pickable || (prefs.only_visible_objects && object.opacity<=0.0) {continue;}
        
        let pos=object.pos;
        let end=[pos[0]+object.size[0],pos[1]+object.size[1]];
        let center=[(pos[0]+end[0])*0.5,(pos[1]+end[1])*0.5];

        if prefs.snap_to_object_bounds {
            for x in [pos[0], end[0]] {
                if (x - point[0]).abs() <= tolerance {
                    guides.push(SmartGuide{kind:SmartGuideKind::Vertical,position:x,source_owner:Some(object.id.0),target_owner:moving_owner});
                }
            }
            for y in [pos[1], end[1]] {
                if (y - point[1]).abs() <= tolerance {
                    guides.push(SmartGuide{kind:SmartGuideKind::Horizontal,position:y,source_owner:Some(object.id.0),target_owner:moving_owner});
                }
            }
        }

        if prefs.include_bbox_midpoints {
            let cx=center[0];
            let cy=center[1];
            if (cx - point[0]).abs() <= tolerance {
                guides.push(SmartGuide{kind:SmartGuideKind::Vertical,position:cx,source_owner:Some(object.id.0),target_owner:moving_owner});
            }
            if (cy - point[1]).abs() <= tolerance {
                guides.push(SmartGuide{kind:SmartGuideKind::Horizontal,position:cy,source_owner:Some(object.id.0),target_owner:moving_owner});
            }
        }
    }

    guides.sort_by(|a,b| {
        let da=(a.position - if a.kind==SmartGuideKind::Vertical {point[0]} else {point[1]}).abs();
        let db=(b.position - if b.kind==SmartGuideKind::Vertical {point[0]} else {point[1]}).abs();
        da.total_cmp(&db)
    });
    guides.dedup_by(|a,b| a.kind==b.kind && (a.position-b.position).abs()<1e-4);
    guides.truncate(prefs.max_candidates as usize);
    guides
}
