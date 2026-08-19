//! Textures: spatially varying surface parameters.
//!
//! A [`Texture`] is `Constant`, a procedural `Checker`, a procedural solid
//! `Wood`, or an `Image`. Surface-parameterized textures sample by `(u, v)`;
//! solid textures sample by the world-space hit position, so they need no UV
//! unwrap (CSG meshes have none). As with the environment map, decoding image
//! *files* is a front-end concern; the core works on already-decoded pixels
//! ([`ImageData`]). An `Image` texture stores only its path until
//! [`Texture::resolve`] (driven by a front-end decoder) fills in the pixels —
//! which keeps the scene description serializable and the core codec-free.

use crate::math::Vec3;
use crate::{Color, Float};
use std::path::Path;
use std::sync::Arc;

/// Decoded image pixels handed to the core by a front-end decoder.
pub struct ImageData {
    pub width: usize,
    pub height: usize,
    /// Row-major, row 0 = top. Linear (already converted from sRGB if needed).
    pub pixels: Vec<Color>,
}

/// An immutable, bilinearly sampled image.
#[derive(Debug)]
pub struct ImageTexture {
    width: usize,
    height: usize,
    pixels: Vec<Color>,
}

impl ImageTexture {
    pub fn new(width: usize, height: usize, pixels: Vec<Color>) -> ImageTexture {
        assert_eq!(pixels.len(), width * height, "image pixel count mismatch");
        ImageTexture {
            width,
            height,
            pixels,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    /// The decoded pixels (row-major, row 0 = top; linear if sRGB was converted).
    pub fn pixels(&self) -> &[Color] {
        &self.pixels
    }

    /// Bilinear lookup with wrap-around addressing. `v = 0` is the top row.
    fn sample(&self, u: Float, v: Float) -> Color {
        let fx = u * self.width as Float - 0.5;
        let fy = v * self.height as Float - 0.5;
        let x0 = fx.floor();
        let y0 = fy.floor();
        let dx = fx - x0;
        let dy = fy - y0;
        let texel = |x: isize, y: isize| -> Color {
            let xi = x.rem_euclid(self.width as isize) as usize;
            let yi = y.rem_euclid(self.height as isize) as usize;
            self.pixels[yi * self.width + xi]
        };
        let (x0, y0) = (x0 as isize, y0 as isize);
        let top = texel(x0, y0) * (1.0 - dx) + texel(x0 + 1, y0) * dx;
        let bot = texel(x0, y0 + 1) * (1.0 - dx) + texel(x0 + 1, y0 + 1) * dx;
        top * (1.0 - dy) + bot * dy
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug)]
pub enum Texture {
    /// A single color everywhere.
    Constant(Color),
    /// Procedural checkerboard in texture space.
    Checker {
        even: Color,
        odd: Color,
        scale: Float,
    },
    /// Procedural wood grain: a solid texture of noise-warped growth rings
    /// around `axis`, evaluated at the world-space hit position (see
    /// [`crate::noise::wood_grain`]). LuaCAD addition, not in upstream Prime.
    Wood {
        /// Earlywood (light) color.
        early: Color,
        /// Latewood (dark ring) color.
        late: Color,
        /// Growth rings per world unit.
        frequency: Float,
        /// Ring waviness, in ring widths.
        distortion: Float,
        /// Grain axis — the log's long direction. Need not be unit length.
        axis: Vec3,
        /// A point the grain axis passes through — where the log's center
        /// line sits in world space (the rings are concentric around it).
        #[cfg_attr(feature = "serde", serde(default))]
        offset: Vec3,
    },
    /// An image file (resolved by the front-end via [`Texture::resolve`]).
    Image {
        path: String,
        #[cfg_attr(feature = "serde", serde(default = "default_true"))]
        srgb: bool,
        /// Decoded pixels; `None` until resolved. Not serialized.
        #[cfg_attr(feature = "serde", serde(skip))]
        data: Option<Arc<ImageTexture>>,
    },
}

#[cfg(feature = "serde")]
fn default_true() -> bool {
    true
}

impl Texture {
    pub fn constant(c: Color) -> Texture {
        Texture::Constant(c)
    }

    /// Sample the texture color at surface coordinates `(u, v)` and
    /// world-space position `p` (only solid textures read `p`).
    pub fn sample(&self, u: Float, v: Float, p: Vec3) -> Color {
        match self {
            Texture::Constant(c) => *c,
            Texture::Checker { even, odd, scale } => {
                let parity = (u * scale).floor() as i64 + (v * scale).floor() as i64;
                if parity.rem_euclid(2) == 0 {
                    *even
                } else {
                    *odd
                }
            }
            Texture::Wood {
                early,
                late,
                frequency,
                distortion,
                axis,
                offset,
            } => {
                let w = crate::noise::wood_grain(p - *offset, *axis, *frequency, *distortion);
                *early + (*late - *early) * w
            }
            Texture::Image { data, .. } => match data {
                Some(img) => img.sample(u, v),
                // Unresolved image: a visible placeholder rather than a panic.
                None => Color::new(1.0, 0.0, 1.0),
            },
        }
    }

    /// Resolve an `Image` texture's pixels using `decoder` (relative to
    /// `base_dir`). No-op for procedural textures.
    pub fn resolve<F>(&mut self, base_dir: &Path, decoder: &mut F) -> Result<(), String>
    where
        F: FnMut(&Path) -> Result<ImageData, String>,
    {
        if let Texture::Image {
            path, srgb, data, ..
        } = self
        {
            if data.is_none() {
                let full = base_dir.join(&*path);
                let mut img = decoder(&full)?;
                if *srgb {
                    for p in img.pixels.iter_mut() {
                        *p = srgb_to_linear(*p);
                    }
                }
                *data = Some(Arc::new(ImageTexture::new(
                    img.width, img.height, img.pixels,
                )));
            }
        }
        Ok(())
    }
}

impl From<Color> for Texture {
    fn from(c: Color) -> Texture {
        Texture::Constant(c)
    }
}

#[inline]
fn srgb_to_linear(c: Color) -> Color {
    let f = |x: Float| {
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    Color::new(f(c.x), f(c.y), f(c.z))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_is_uniform() {
        let t = Texture::constant(Color::new(0.2, 0.4, 0.6));
        assert_eq!(t.sample(0.1, 0.9, Vec3::ZERO), Color::new(0.2, 0.4, 0.6));
    }

    #[test]
    fn checker_alternates() {
        let t = Texture::Checker {
            even: Color::ZERO,
            odd: Color::ONE,
            scale: 2.0,
        };
        // scale 2 -> cells of width 0.5. (0.25,0.25)->(0,0) even; (0.75,0.25)->(1,0) odd.
        assert_eq!(t.sample(0.25, 0.25, Vec3::ZERO), Color::ZERO);
        assert_eq!(t.sample(0.75, 0.25, Vec3::ZERO), Color::ONE);
        assert_eq!(t.sample(0.75, 0.75, Vec3::ZERO), Color::ZERO);
    }

    #[test]
    fn wood_blends_between_early_and_late_and_varies_with_position() {
        let early = Color::new(0.8, 0.6, 0.4);
        let late = Color::new(0.4, 0.25, 0.1);
        let t = Texture::Wood {
            early,
            late,
            frequency: 0.4,
            distortion: 0.4,
            axis: Vec3::new(0.0, 0.0, 1.0),
            offset: Vec3::ZERO,
        };
        let mut distinct = std::collections::HashSet::new();
        for i in 0..100 {
            let p = Vec3::new(i as Float * 0.1, 0.0, 0.0);
            let c = t.sample(0.0, 0.0, p);
            // Every sample lies on the early↔late segment.
            for k in 0..3 {
                let (lo, hi) = (
                    late.axis(k).min(early.axis(k)),
                    late.axis(k).max(early.axis(k)),
                );
                assert!(c.axis(k) >= lo - 1e-6 && c.axis(k) <= hi + 1e-6, "{c:?}");
            }
            distinct.insert(c.x.to_bits());
        }
        assert!(distinct.len() > 10, "wood should vary spatially");
    }

    #[test]
    fn image_bilinear_interpolates() {
        // 2x1 image: black | white. Sampling the midpoint gives ~grey.
        let img = ImageTexture::new(2, 1, vec![Color::ZERO, Color::ONE]);
        let mid = img.sample(0.5, 0.5);
        assert!((mid.x - 0.5).abs() < 1e-5, "got {mid:?}");
    }

    #[test]
    fn unresolved_image_is_placeholder_and_resolves() {
        let mut t = Texture::Image {
            path: "x.png".into(),
            srgb: false,
            data: None,
        };
        assert_eq!(t.sample(0.5, 0.5, Vec3::ZERO), Color::new(1.0, 0.0, 1.0));
        let mut decoder = |_p: &Path| {
            Ok(ImageData {
                width: 1,
                height: 1,
                pixels: vec![Color::new(0.25, 0.5, 0.75)],
            })
        };
        t.resolve(Path::new("."), &mut decoder).unwrap();
        assert_eq!(t.sample(0.5, 0.5, Vec3::ZERO), Color::new(0.25, 0.5, 0.75));
    }
}
