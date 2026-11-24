use dtool_core::DrawingScale;

pub struct MeasureTool {
    pub persistent: bool,
}

impl MeasureTool {
    pub fn new() -> Self {
        Self { persistent: false }
    }
}

pub struct DimensionLine {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub scale: DrawingScale,
}

impl DimensionLine {
    pub fn new(a: [f64; 2], b: [f64; 2], scale: DrawingScale) -> Self {
        Self { a, b, scale }
    }

    pub fn length(&self) -> f64 {
        let dx = self.b[0] - self.a[0];
        let dy = self.b[1] - self.a[1];
        (dx * dx + dy * dy).sqrt() * self.scale.factor
    }

    pub fn angle_deg(&self) -> f64 {
        let dx = self.b[0] - self.a[0];
        let dy = self.b[1] - self.a[1];
        (dy.atan2(dx) * 180.0 / std::f64::consts::PI).to_degrees()
    }

    pub fn format(&self, precision: usize) -> String {
        let len = self.length();
        let ang = self.angle_deg();
        format!("{:.prec$} pt {:.prec$} deg", len, ang, prec = precision)
    }
}
