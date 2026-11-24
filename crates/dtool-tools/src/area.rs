use dtool_core::document::{Document, StoryId};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::paint::Paint;
use dtool_core::DrawingScale;

#[derive(Clone, Debug)]
pub struct AreaTool {
    pub perimeter_mode: bool,
    pub full_edges: bool,
    pub cusped_segments: bool,
}

impl Default for AreaTool {
    fn default() -> Self {
        Self {
            perimeter_mode: false,
            full_edges: false,
            cusped_segments: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AreaResult {
    pub area: f64,
    pub perimeter: f64,
    pub scale: DrawingScale,
}

impl AreaTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn measure(&self, doc: &Document, object_id: ObjectId, scale: DrawingScale) -> Option<AreaResult> {
        let obj = doc.find_object(object_id)?;
        
        if !matches!(obj.kind, ObjectKind::Path | ObjectKind::Shape) {
            return None;
        }

        let mut total_area = 0.0;
        let mut total_perimeter = 0.0;

        for story in &obj.stories {
            let (area, perimeter) = self.measure_story(story);
            total_area += area;
            total_perimeter += perimeter;
        }

        Some(AreaResult {
            area: total_area * scale.scale_x * scale.scale_y,
            perimeter: total_perimeter * ((scale.scale_x + scale.scale_y) / 2.0),
            scale,
        })
    }

    fn measure_story(&self, story: &dtool_core::document::Story) -> (f64, f64) {
        let mut area = 0.0;
        let mut perimeter = 0.0;

        for path in &story.paths {
            if path.closed {
                let (a, p) = self.measure_path(path);
                area += a;
                perimeter += p;
            }
        }

        for hole in &story.holes {
            for path in &hole.paths {
                if path.closed {
                    let (a, _) = self.measure_path(path);
                    area -= a.abs();
                }
            }
        }

        (area, perimeter)
    }

    fn measure_path(&self, path: &dtool_core::document::Path) -> (f64, f64) {
        let mut area = 0.0;
        let mut perimeter = 0.0;
        let points = &path.points;

        if points.len() < 3 {
            return (0.0, 0.0);
        }

        for i in 0..points.len() {
            let p1 = points[i];
            let p2 = points[(i + 1) % points.len()];
            
            area += (p1[0] * p2[1] - p2[0] * p1[1]) / 2.0;

            if self.full_edges || i < points.len() - 1 {
                let dx = p2[0] - p1[0];
                let dy = p2[1] - p1[1];
                perimeter += (dx * dx + dy * dy).sqrt();
            }
        }

        if self.cusped_segments && path.curves.len() > 0 {
            for curve in &path.curves {
                let start = points[curve.start_idx];
                let end = points[curve.end_idx];
                let ctrl1 = curve.ctrl1;
                let ctrl2 = curve.ctrl2;

                let straight_dist = ((end[0] - start[0]).powi(2) + (end[1] - start[1]).powi(2)).sqrt();
                
                let approx_len = {
                    let mid1 = [
                        0.5 * (start[0] + ctrl1[0]),
                        0.5 * (start[1] + ctrl1[1]),
                    ];
                    let mid2 = [
                        0.5 * (ctrl1[0] + ctrl2[0]),
                        0.5 * (ctrl1[1] + ctrl2[1]),
                    ];
                    let mid3 = [
                        0.5 * (ctrl2[0] + end[0]),
                        0.5 * (ctrl2[1] + end[1]),
                    ];

                    let d1 = ((mid1[0] - start[0]).powi(2) + (mid1[1] - start[1]).powi(2)).sqrt();
                    let d2 = ((mid2[0] - mid1[0]).powi(2) + (mid2[1] - mid1[1]).powi(2)).sqrt();
                    let d3 = ((mid3[0] - mid2[0]).powi(2) + (mid3[1] - mid2[1]).powi(2)).sqrt();
                    
                    d1 + d2 + d3
                };

                perimeter += approx_len - straight_dist;
            }
        }

        (area.abs(), perimeter)
    }
}
