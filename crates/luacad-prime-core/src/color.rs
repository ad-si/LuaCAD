//! Tonemapping and linear-to-display color conversion.
//!
//! Replaces the legacy `Color3f.toRgb()`, which clamped raw linear values with
//! no gamma correction (producing dark, washed-out output).

use crate::{Color, Float};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tonemap {
    /// Clamp to `[0, 1]` after gamma.
    #[default]
    Clamp,
    /// Reinhard operator `c / (1 + c)`, which gracefully compresses highlights.
    Reinhard,
    /// ACES filmic curve (Narkowicz 2015 fit): a film-like S-curve with a
    /// gentle toe and a highlight shoulder that saturates toward white.
    Aces,
}

impl Tonemap {
    #[inline]
    fn apply(self, c: Color) -> Color {
        match self {
            Tonemap::Clamp => c,
            Tonemap::Reinhard => {
                Color::new(c.x / (1.0 + c.x), c.y / (1.0 + c.y), c.z / (1.0 + c.z))
            }
            Tonemap::Aces => Color::new(aces_fit(c.x), aces_fit(c.y), aces_fit(c.z)),
        }
    }
}

/// Narkowicz's fitted approximation of the ACES filmic tonescale.
#[inline]
fn aces_fit(x: Float) -> Float {
    let n = x * (2.51 * x + 0.03);
    let d = x * (2.43 * x + 0.59) + 0.14;
    (n / d).clamp(0.0, 1.0)
}

/// Rec. 709 relative luminance of a linear RGB color.
#[inline]
pub fn luminance(c: Color) -> Float {
    0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z
}

/// Convert a linear HDR color to 8-bit sRGB-ish bytes using `gamma` and the
/// given tonemap operator. NaNs (from degenerate samples) are treated as black.
#[inline]
pub fn to_srgb8(color: Color, tonemap: Tonemap, gamma: Float) -> [u8; 3] {
    let c = sanitize(color);
    let c = tonemap.apply(c);
    let inv_gamma = 1.0 / gamma;
    let encode = |v: Float| -> u8 {
        let v = v.max(0.0).powf(inv_gamma);
        (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
    };
    [encode(c.x), encode(c.y), encode(c.z)]
}

#[inline]
fn sanitize(c: Color) -> Color {
    let fix = |v: Float| if v.is_finite() { v } else { 0.0 };
    Color::new(fix(c.x), fix(c.y), fix(c.z))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_and_white_endpoints() {
        assert_eq!(to_srgb8(Color::ZERO, Tonemap::Clamp, 2.2), [0, 0, 0]);
        assert_eq!(to_srgb8(Color::ONE, Tonemap::Clamp, 2.2), [255, 255, 255]);
    }

    #[test]
    fn gamma_brightens_midtones() {
        // 0.5 linear -> ~0.73 with gamma 2.2 -> ~186.
        let [r, _, _] = to_srgb8(Color::splat(0.5), Tonemap::Clamp, 2.2);
        assert!((180..=192).contains(&r), "got {r}");
    }

    #[test]
    fn nan_becomes_black() {
        assert_eq!(
            to_srgb8(Color::new(Float::NAN, 1.0, 0.0), Tonemap::Clamp, 2.2),
            [0, 255, 0]
        );
    }

    #[test]
    fn aces_endpoints_and_monotonicity() {
        assert_eq!(aces_fit(0.0), 0.0);
        // Bright inputs saturate toward (but never exceed) white.
        assert!(aces_fit(20.0) > 0.98 && aces_fit(20.0) <= 1.0);
        let mut prev = 0.0;
        for i in 1..=100 {
            let v = aces_fit(i as Float * 0.1);
            assert!(v >= prev, "ACES curve must be monotonic");
            prev = v;
        }
        // The fit crosses ~1.0 near x = 1... clamp keeps it in range.
        assert!(to_srgb8(Color::splat(100.0), Tonemap::Aces, 2.2) == [255, 255, 255]);
    }
}
