//! A linear-space HDR image buffer that resolves to 8-bit sRGB bytes.

use crate::color::{self, Tonemap};
use crate::{Color, Float};

pub struct Framebuffer {
    pub width: usize,
    pub height: usize,
    /// Row-major, top-to-bottom, linear radiance per pixel.
    pixels: Vec<Color>,
}

impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Framebuffer {
        Framebuffer {
            width,
            height,
            pixels: vec![Color::ZERO; width * height],
        }
    }

    #[inline]
    pub fn pixel(&self, x: usize, y: usize) -> Color {
        self.pixels[y * self.width + x]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, color: Color) {
        self.pixels[y * self.width + x] = color;
    }

    /// Read-only access to the raw row-major pixel buffer.
    pub fn pixels(&self) -> &[Color] {
        &self.pixels
    }

    /// Mutable access to the raw row-major pixel buffer (used by the renderer to
    /// fill rows in parallel).
    pub fn pixels_mut(&mut self) -> &mut [Color] {
        &mut self.pixels
    }

    /// Resolve to interleaved RGB8 bytes (row 0 = top), ready for the `image`
    /// crate or a PPM writer.
    pub fn to_srgb_bytes(&self, tonemap: Tonemap, gamma: Float) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.width * self.height * 3);
        for &c in &self.pixels {
            bytes.extend_from_slice(&color::to_srgb8(c, tonemap, gamma));
        }
        bytes
    }
}
