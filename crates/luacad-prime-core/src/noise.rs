//! Deterministic 3-D gradient noise for procedural solid textures.
//!
//! Classic Perlin noise (LuaCAD addition, not in upstream Prime) with Ken
//! Perlin's reference permutation table baked in, so a given point always
//! yields the same value — no seeding, and renders are reproducible.

use crate::math::Vec3;
use crate::Float;

/// Ken Perlin's reference permutation. Indexed with `& 255` wraparound.
#[rustfmt::skip]
const PERM: [u8; 256] = [
    151, 160, 137,  91,  90,  15, 131,  13, 201,  95,  96,  53, 194, 233,   7, 225,
    140,  36, 103,  30,  69, 142,   8,  99,  37, 240,  21,  10,  23, 190,   6, 148,
    247, 120, 234,  75,   0,  26, 197,  62,  94, 252, 219, 203, 117,  35,  11,  32,
     57, 177,  33,  88, 237, 149,  56,  87, 174,  20, 125, 136, 171, 168,  68, 175,
     74, 165,  71, 134, 139,  48,  27, 166,  77, 146, 158, 231,  83, 111, 229, 122,
     60, 211, 133, 230, 220, 105,  92,  41,  55,  46, 245,  40, 244, 102, 143,  54,
     65,  25,  63, 161,   1, 216,  80,  73, 209,  76, 132, 187, 208,  89,  18, 169,
    200, 196, 135, 130, 116, 188, 159,  86, 164, 100, 109, 198, 173, 186,   3,  64,
     52, 217, 226, 250, 124, 123,   5, 202,  38, 147, 118, 126, 255,  82,  85, 212,
    207, 206,  59, 227,  47,  16,  58,  17, 182, 189,  28,  42, 223, 183, 170, 213,
    119, 248, 152,   2,  44, 154, 163,  70, 221, 153, 101, 155, 167,  43, 172,   9,
    129,  22,  39, 253,  19,  98, 108, 110,  79, 113, 224, 232, 178, 185, 112, 104,
    218, 246,  97, 228, 251,  34, 242, 193, 238, 210, 144,  12, 191, 179, 162, 241,
     81,  51, 145, 235, 249,  14, 239, 107,  49, 192, 214,  31, 181, 199, 106, 157,
    184,  84, 204, 176, 115, 121,  50,  45, 127,   4, 150, 254, 138, 236, 205,  93,
    222, 114,  67,  29,  24,  72, 243, 141, 128, 195,  78,  66, 215,  61, 156, 180,
];

#[inline]
fn perm(i: i64) -> usize {
    PERM[(i & 255) as usize] as usize
}

/// Perlin's quintic fade curve `6t⁵ − 15t⁴ + 10t³`.
#[inline]
fn fade(t: Float) -> Float {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn lerp(a: Float, b: Float, t: Float) -> Float {
    a + (b - a) * t
}

/// Gradient dot product for one lattice corner (Perlin's 12-gradient set,
/// folded to 16 cases).
#[inline]
fn grad(hash: usize, x: Float, y: Float, z: Float) -> Float {
    match hash & 15 {
        0 => x + y,
        1 => -x + y,
        2 => x - y,
        3 => -x - y,
        4 => x + z,
        5 => -x + z,
        6 => x - z,
        7 => -x - z,
        8 => y + z,
        9 => -y + z,
        10 => y - z,
        11 => -y - z,
        12 => y + x,
        13 => -y + z,
        14 => y - x,
        _ => -y - z,
    }
}

/// Classic 3-D Perlin noise, in roughly `[-1, 1]`, zero at integer lattice
/// points.
pub fn perlin(p: Vec3) -> Float {
    let (fx, fy, fz) = (p.x.floor(), p.y.floor(), p.z.floor());
    let (xi, yi, zi) = (fx as i64, fy as i64, fz as i64);
    let (x, y, z) = (p.x - fx, p.y - fy, p.z - fz);
    let (u, v, w) = (fade(x), fade(y), fade(z));

    let a = perm(xi) as i64 + yi;
    let aa = perm(a) as i64 + zi;
    let ab = perm(a + 1) as i64 + zi;
    let b = perm(xi + 1) as i64 + yi;
    let ba = perm(b) as i64 + zi;
    let bb = perm(b + 1) as i64 + zi;

    lerp(
        lerp(
            lerp(grad(perm(aa), x, y, z), grad(perm(ba), x - 1.0, y, z), u),
            lerp(
                grad(perm(ab), x, y - 1.0, z),
                grad(perm(bb), x - 1.0, y - 1.0, z),
                u,
            ),
            v,
        ),
        lerp(
            lerp(
                grad(perm(aa + 1), x, y, z - 1.0),
                grad(perm(ba + 1), x - 1.0, y, z - 1.0),
                u,
            ),
            lerp(
                grad(perm(ab + 1), x, y - 1.0, z - 1.0),
                grad(perm(bb + 1), x - 1.0, y - 1.0, z - 1.0),
                u,
            ),
            v,
        ),
        w,
    )
}

/// Fractional Brownian motion: `octaves` layers of [`perlin`], each at twice
/// the frequency and half the amplitude, normalized back to roughly `[-1, 1]`.
pub fn fbm(p: Vec3, octaves: u32) -> Float {
    let mut sum = 0.0;
    let mut amplitude = 1.0;
    let mut total = 0.0;
    let mut q = p;
    for _ in 0..octaves {
        sum += perlin(q) * amplitude;
        total += amplitude;
        amplitude *= 0.5;
        q = q * 2.0;
    }
    sum / total
}

#[inline]
fn smoothstep(e0: Float, e1: Float, x: Float) -> Float {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Wood-grain blend factor in `[0, 1]` at point `p`: 0 in the light
/// earlywood, 1 in the dark latewood band of a growth ring.
///
/// Rings are concentric cylinders around the line through the origin along
/// `axis` (the log's long direction), `frequency` rings per world unit; to
/// put the log's center line elsewhere, shift `p` before calling. The ring coordinate is warped by
/// low-frequency noise — sampled in a frame compressed along the axis, so the
/// warp drifts slowly along the grain (long wavy streaks) and faster across
/// it — and each ring ramps gradually from earlywood into latewood, then
/// resets sharply where the next year's growth starts. A second, finer noise
/// layer stretched along the axis adds fiber streaks.
pub fn wood_grain(p: Vec3, axis: Vec3, frequency: Float, distortion: Float) -> Float {
    let axis = axis.normalize_or(Vec3::new(0.0, 0.0, 1.0));
    let along = p.dot(axis);
    let radial = p - axis * along;
    let r = radial.length();

    let warp_p = (radial + axis * (along * 0.25)) * (frequency * 0.55);
    let warp = fbm(warp_p, 3);
    let t = r * frequency + warp * distortion;

    let x = t - t.floor();
    let ring = smoothstep(0.35, 0.85, x) - smoothstep(0.95, 1.0, x);

    let fiber_p = (radial * 6.0 + axis * (along * 0.8)) * frequency;
    let fiber = fbm(fiber_p, 2);

    (ring + fiber * 0.15).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perlin_is_deterministic_bounded_and_zero_on_lattice() {
        let p = Vec3::new(1.3, -4.7, 2.9);
        assert_eq!(perlin(p), perlin(p));
        for i in 0..1000 {
            let q = Vec3::new(i as Float * 0.13, i as Float * -0.29, i as Float * 0.71);
            let n = perlin(q);
            assert!((-1.0..=1.0).contains(&n), "perlin({q:?}) = {n}");
        }
        assert_eq!(perlin(Vec3::new(3.0, -2.0, 7.0)), 0.0);
    }

    #[test]
    fn perlin_varies() {
        let a = perlin(Vec3::new(0.4, 0.6, 0.1));
        let b = perlin(Vec3::new(5.7, 1.2, 3.4));
        assert!(
            (a - b).abs() > 1e-4,
            "expected different values: {a} vs {b}"
        );
    }

    #[test]
    fn wood_grain_is_bounded_and_varies_radially() {
        let axis = Vec3::new(0.0, 0.0, 1.0);
        let mut lo: Float = 1.0;
        let mut hi: Float = 0.0;
        for i in 0..200 {
            let r = i as Float * 0.05;
            let w = wood_grain(Vec3::new(r, 0.0, 0.0), axis, 0.4, 0.4);
            assert!((0.0..=1.0).contains(&w), "wood_grain out of range: {w}");
            lo = lo.min(w);
            hi = hi.max(w);
        }
        // Walking outward crosses both earlywood and latewood.
        assert!(lo < 0.2, "never reached earlywood: min {lo}");
        assert!(hi > 0.7, "never reached latewood: max {hi}");
    }

    #[test]
    fn wood_grain_normalizes_axis() {
        let p = Vec3::new(1.7, 0.4, 2.2);
        let a = wood_grain(p, Vec3::new(0.0, 0.0, 1.0), 0.4, 0.4);
        let b = wood_grain(p, Vec3::new(0.0, 0.0, 10.0), 0.4, 0.4);
        assert_eq!(a, b);
    }
}
