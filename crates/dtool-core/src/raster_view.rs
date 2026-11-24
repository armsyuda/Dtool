use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct RasterView<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

impl<'a> RasterView<'a> {
    pub fn new(width: u32, height: u32, rgba: &'a [u8]) -> Self {
        Self { width, height, rgba }
    }

    pub fn as_view(&self) -> RasterView<'_> {
        RasterView {
            width: self.width,
            height: self.height,
            rgba: self.rgba,
        }
    }

    pub fn contains(&self, x: u32, y: u32) -> bool {
        x < self.width && y < self.height
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if !self.contains(x, y) {
            return None;
        }
        let idx = (y as usize * self.width as usize + x as usize) * 4;
        Some([
            self.rgba[idx],
            self.rgba[idx + 1],
            self.rgba[idx + 2],
            self.rgba[idx + 3],
        ])
    }

    pub fn get_pixel(&self, x: i64, y: i64) -> [u8; 4] {
        if x < 0 || y < 0 {
            return [0, 0, 0, 0];
        }
        self.pixel(x as u32, y as u32).unwrap_or([0, 0, 0, 0])
    }
}

#[derive(Debug)]
pub struct RasterViewMut<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a mut [u8],
}

impl<'a> RasterViewMut<'a> {
    pub fn new(width: u32, height: u32, rgba: &'a mut [u8]) -> Self {
        Self { width, height, rgba }
    }

    pub fn as_view(&self) -> RasterView<'_> {
        RasterView {
            width: self.width,
            height: self.height,
            rgba: self.rgba,
        }
    }

    pub fn contains(&self, x: u32, y: u32) -> bool {
        x < self.width && y < self.height
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if !self.contains(x, y) {
            return None;
        }
        let idx = (y as usize * self.width as usize + x as usize) * 4;
        Some([
            self.rgba[idx],
            self.rgba[idx + 1],
            self.rgba[idx + 2],
            self.rgba[idx + 3],
        ])
    }

    pub fn get_pixel(&self, x: i64, y: i64) -> [u8; 4] {
        if x < 0 || y < 0 {
            return [0, 0, 0, 0];
        }
        self.pixel(x as u32, y as u32).unwrap_or([0, 0, 0, 0])
    }

    pub fn set_pixel(&mut self, x: i64, y: i64, r: u8, g: u8, b: u8, a: u8) -> bool {
        if x < 0 || y < 0 {
            return false;
        }
        let x = x as u32;
        let y = y as u32;
        if !self.contains(x, y) {
            return false;
        }
        let idx = (y as usize * self.width as usize + x as usize) * 4;
        self.rgba[idx] = r;
        self.rgba[idx + 1] = g;
        self.rgba[idx + 2] = b;
        self.rgba[idx + 3] = a;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Colour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Colour {
    pub fn from_rgba(rgba: [u8; 4]) -> Self {
        Self {
            r: rgba[0],
            g: rgba[1],
            b: rgba[2],
            a: rgba[3],
        }
    }

    pub fn to_rgba(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}
