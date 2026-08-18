//! Wavelength sampling for dispersion (spectral rendering).
//!
//! Prime stays an RGB renderer, with a wavelength bolted on only where it
//! matters: when a scene contains a *dispersive* dielectric, every path
//! samples one wavelength λ, dielectrics refract with that wavelength's
//! Cauchy index, and the path's RGB contribution is weighted by
//! [`rgb_weight`] — a CIE-based responsivity normalized so a
//! λ-independent path averages back to exactly `(1, 1, 1)`. Scenes without
//! dispersion never draw a wavelength and render bit-identically to before.
//!
//! This gives smooth spectral rainbows through prisms and glass while
//! keeping textures, lights, and every other BSDF in plain RGB (their
//! spectral response is treated as flat, which is exact in expectation for
//! any path whose geometry does not depend on λ).

use crate::{Color, Float};

/// Sampled visible range, in nanometers.
pub const LAMBDA_MIN: Float = 380.0;
pub const LAMBDA_MAX: Float = 730.0;

/// The sodium D line (nm): the reference wavelength at which a dielectric's
/// `ior` is specified, and the neutral wavelength used when dispersion is off.
pub const LAMBDA_D: Float = 589.3;

/// Map a uniform sample in `[0, 1)` to a wavelength.
#[inline]
pub fn sample_lambda(u: Float) -> Float {
    LAMBDA_MIN + u * (LAMBDA_MAX - LAMBDA_MIN)
}

/// Cauchy dispersion: the index of refraction at `lambda` (nm) of a material
/// with index `ior` at the D line and Cauchy coefficient `b` (µm²). Real BK7
/// glass is b ≈ 0.0042; exaggerated values (0.02–0.05) make demo rainbows.
#[inline]
pub fn cauchy_ior(ior: Float, b: Float, lambda: Float) -> Float {
    if b == 0.0 {
        return ior;
    }
    let um = lambda * 1e-3;
    const D2: Float = 0.5893 * 0.5893;
    ior + b * (1.0 / (um * um) - 1.0 / D2)
}

/// RGB responsivity of wavelength λ: the CIE 1931 color-matching functions
/// (multi-lobe Gaussian fits, Wyman et al. 2013) converted to linear sRGB and
/// normalized per channel so the mean over `[LAMBDA_MIN, LAMBDA_MAX]` is
/// exactly white — a λ-independent path keeps its color in expectation.
/// Values go negative where spectral colors leave the sRGB gamut; that is
/// intentional (energy balances; clamping would tint whites).
pub fn rgb_weight(lambda: Float) -> Color {
    let x = 1.056 * gauss(lambda, 599.8, 37.9, 31.0) + 0.362 * gauss(lambda, 442.0, 16.0, 26.7)
        - 0.065 * gauss(lambda, 501.1, 20.4, 26.2);
    let y = 0.821 * gauss(lambda, 568.8, 46.9, 40.5) + 0.286 * gauss(lambda, 530.9, 16.3, 31.1);
    let z = 1.217 * gauss(lambda, 437.0, 11.8, 36.0) + 0.681 * gauss(lambda, 459.0, 26.0, 13.8);
    // XYZ → linear sRGB, then the flat-spectrum normalization (precomputed
    // for this exact λ range; guarded by `flat_spectrum_stays_white`).
    Color::new(
        (3.2406 * x - 1.5372 * y - 0.4986 * z) * 2.726_631,
        (-0.9689 * x + 1.8758 * y + 0.0415 * z) * 3.446_770_5,
        (0.0557 * x - 0.2040 * y + 1.0570 * z) * 3.606_385_5,
    )
}

/// Piecewise Gaussian with different widths on each side of the peak.
#[inline]
fn gauss(x: Float, mu: Float, s1: Float, s2: Float) -> Float {
    let s = if x < mu { s1 } else { s2 };
    let t = (x - mu) / s;
    (-0.5 * t * t).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_spectrum_stays_white() {
        // The normalization constants must make E[rgb_weight(λ)] = (1, 1, 1)
        // over the sampled range, so non-dispersive paths keep their color.
        let n = 100_000;
        let mut mean = Color::ZERO;
        for i in 0..n {
            let l = sample_lambda((i as Float + 0.5) / n as Float);
            mean += rgb_weight(l);
        }
        mean = mean * (1.0 / n as Float);
        assert!(
            (mean - Color::ONE).length() < 5e-3,
            "flat-spectrum mean drifted: {mean:?}"
        );
    }

    #[test]
    fn cauchy_blue_bends_more() {
        // ior is anchored at the D line and increases toward the blue end.
        assert!((cauchy_ior(1.5, 0.01, LAMBDA_D) - 1.5).abs() < 1e-4);
        let blue = cauchy_ior(1.5, 0.01, 440.0);
        let red = cauchy_ior(1.5, 0.01, 680.0);
        assert!(blue > 1.5 && red < 1.5 && blue - red > 0.01);
        // Zero coefficient is a no-op at any wavelength.
        assert_eq!(cauchy_ior(1.5, 0.0, 440.0), 1.5);
    }

    #[test]
    fn weights_peak_in_the_right_channels() {
        assert!(rgb_weight(450.0).z > 2.0, "blue λ should weight blue");
        assert!(rgb_weight(550.0).y > 2.0, "green λ should weight green");
        assert!(rgb_weight(610.0).x > 2.0, "red λ should weight red");
    }
}
