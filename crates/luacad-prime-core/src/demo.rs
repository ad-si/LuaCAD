//! Built-in scenes, so the renderer runs with zero external assets.

use crate::camera::CameraConfig;
use crate::env::EnvMap;
use crate::geometry::{Primitive, Sphere, Triangle};
use crate::material::Material;
use crate::math::Vec3;
use crate::scene::{Background, Scene};
use crate::texture::Texture;
use crate::{Color, Float, MaterialId};
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

/// Add a quad (as two triangles) spanning corners `a, b, c, d` (in order).
fn quad(prims: &mut Vec<Primitive>, a: Vec3, b: Vec3, c: Vec3, d: Vec3, m: MaterialId) {
    prims.push(Triangle::new(a, b, c, m).into());
    prims.push(Triangle::new(a, c, d, m).into());
}

/// Add a textured quad with per-corner UVs (so image/checker textures map
/// across it continuously rather than per-triangle).
#[allow(clippy::too_many_arguments)]
fn quad_uv(
    prims: &mut Vec<Primitive>,
    a: Vec3,
    b: Vec3,
    c: Vec3,
    d: Vec3,
    ua: [Float; 2],
    ub: [Float; 2],
    uc: [Float; 2],
    ud: [Float; 2],
    m: MaterialId,
) {
    prims.push(Triangle::new(a, b, c, m).with_uvs([ua, ub, uc]).into());
    prims.push(Triangle::new(a, c, d, m).with_uvs([ua, uc, ud]).into());
}

/// The classic Cornell box: a great showcase of global illumination, soft
/// shadows, and color bleeding. Coordinates follow the canonical 0..555 layout.
pub fn cornell_box() -> Scene {
    let red = Material::Lambertian {
        albedo: Color::new(0.65, 0.05, 0.05).into(),
        normal: None,
    };
    let green = Material::Lambertian {
        albedo: Color::new(0.12, 0.45, 0.15).into(),
        normal: None,
    };
    let white = Material::Lambertian {
        albedo: Color::new(0.73, 0.73, 0.73).into(),
        normal: None,
    };
    let light = Material::Emissive {
        emit: Color::splat(15.0),
    };
    let glass = Material::Dielectric {
        ior: 1.5,
        absorption: Color::ZERO,
        roughness: 0.0,
        dispersion: 0.0,
    };
    let metal = Material::Metal {
        albedo: Color::new(0.8, 0.85, 0.88).into(),
        roughness: 0.08,
        normal: None,
    };

    let materials = vec![red, green, white, light, glass, metal];
    const RED: MaterialId = 0;
    const GREEN: MaterialId = 1;
    const WHITE: MaterialId = 2;
    const LIGHT: MaterialId = 3;
    const GLASS: MaterialId = 4;
    const METAL: MaterialId = 5;

    let mut prims = Vec::new();

    // Left wall (green) at x = 555, right wall (red) at x = 0.
    quad(
        &mut prims,
        Vec3::new(555.0, 0.0, 0.0),
        Vec3::new(555.0, 555.0, 0.0),
        Vec3::new(555.0, 555.0, 555.0),
        Vec3::new(555.0, 0.0, 555.0),
        GREEN,
    );
    quad(
        &mut prims,
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 555.0),
        Vec3::new(0.0, 555.0, 555.0),
        Vec3::new(0.0, 555.0, 0.0),
        RED,
    );
    // Floor, ceiling, back wall (white).
    quad(
        &mut prims,
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(555.0, 0.0, 0.0),
        Vec3::new(555.0, 0.0, 555.0),
        Vec3::new(0.0, 0.0, 555.0),
        WHITE,
    );
    quad(
        &mut prims,
        Vec3::new(0.0, 555.0, 0.0),
        Vec3::new(0.0, 555.0, 555.0),
        Vec3::new(555.0, 555.0, 555.0),
        Vec3::new(555.0, 555.0, 0.0),
        WHITE,
    );
    quad(
        &mut prims,
        Vec3::new(0.0, 0.0, 555.0),
        Vec3::new(555.0, 0.0, 555.0),
        Vec3::new(555.0, 555.0, 555.0),
        Vec3::new(0.0, 555.0, 555.0),
        WHITE,
    );
    // Ceiling light.
    quad(
        &mut prims,
        Vec3::new(213.0, 554.0, 227.0),
        Vec3::new(343.0, 554.0, 227.0),
        Vec3::new(343.0, 554.0, 332.0),
        Vec3::new(213.0, 554.0, 332.0),
        LIGHT,
    );

    // A glass and a metal sphere on the floor.
    prims.push(Primitive::Sphere(Sphere::new(
        Vec3::new(190.0, 90.0, 190.0),
        90.0,
        GLASS,
    )));
    prims.push(Primitive::Sphere(Sphere::new(
        Vec3::new(370.0, 90.0, 350.0),
        90.0,
        METAL,
    )));

    let camera = CameraConfig {
        look_from: Vec3::new(278.0, 278.0, -800.0),
        look_at: Vec3::new(278.0, 278.0, 0.0),
        vup: Vec3::new(0.0, 1.0, 0.0),
        vfov: 40.0,
        aperture: 0.0,
        focus_dist: None,
    };

    Scene::new(materials, prims, camera, Background::Solid(Color::ZERO))
}

/// A richer Cornell-style enclosure showing off the full material set: glass,
/// a perfect mirror, brushed and rough GGX metals, and several colored diffuse
/// spheres — all under a single area light with global illumination and color
/// bleeding. This is the default scene.
pub fn showcase() -> Scene {
    let red = Material::Lambertian {
        albedo: Color::new(0.65, 0.05, 0.05).into(),
        normal: None,
    };
    let green = Material::Lambertian {
        albedo: Color::new(0.12, 0.45, 0.15).into(),
        normal: None,
    };
    let white = Material::Lambertian {
        albedo: Color::new(0.73, 0.73, 0.73).into(),
        normal: None,
    };
    let light = Material::Emissive {
        emit: Color::splat(18.0),
    };
    let glass = Material::Dielectric {
        ior: 1.5,
        absorption: Color::ZERO,
        roughness: 0.0,
        dispersion: 0.0,
    };
    let mirror = Material::Metal {
        albedo: Color::new(0.95, 0.95, 0.97).into(),
        roughness: 0.0,
        normal: None,
    };
    let brushed = Material::Metal {
        albedo: Color::new(0.8, 0.82, 0.85).into(),
        roughness: 0.12,
        normal: None,
    };
    let gold = Material::Metal {
        albedo: Color::new(1.0, 0.78, 0.34).into(),
        roughness: 0.35,
        normal: None,
    };
    let teal = Material::Lambertian {
        albedo: Color::new(0.1, 0.6, 0.6).into(),
        normal: None,
    };
    let orange = Material::Lambertian {
        albedo: Color::new(0.85, 0.45, 0.1).into(),
        normal: None,
    };

    let materials = vec![
        red, green, white, light, glass, mirror, brushed, gold, teal, orange,
    ];
    const RED: MaterialId = 0;
    const GREEN: MaterialId = 1;
    const WHITE: MaterialId = 2;
    const LIGHT: MaterialId = 3;
    const GLASS: MaterialId = 4;
    const MIRROR: MaterialId = 5;
    const BRUSHED: MaterialId = 6;
    const GOLD: MaterialId = 7;
    const TEAL: MaterialId = 8;
    const ORANGE: MaterialId = 9;

    let mut prims = Vec::new();
    // Enclosure (same 0..555 box as the Cornell scene).
    quad(
        &mut prims,
        Vec3::new(555.0, 0.0, 0.0),
        Vec3::new(555.0, 555.0, 0.0),
        Vec3::new(555.0, 555.0, 555.0),
        Vec3::new(555.0, 0.0, 555.0),
        GREEN,
    );
    quad(
        &mut prims,
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 555.0),
        Vec3::new(0.0, 555.0, 555.0),
        Vec3::new(0.0, 555.0, 0.0),
        RED,
    );
    quad(
        &mut prims,
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(555.0, 0.0, 0.0),
        Vec3::new(555.0, 0.0, 555.0),
        Vec3::new(0.0, 0.0, 555.0),
        WHITE,
    );
    quad(
        &mut prims,
        Vec3::new(0.0, 555.0, 0.0),
        Vec3::new(0.0, 555.0, 555.0),
        Vec3::new(555.0, 555.0, 555.0),
        Vec3::new(555.0, 555.0, 0.0),
        WHITE,
    );
    quad(
        &mut prims,
        Vec3::new(0.0, 0.0, 555.0),
        Vec3::new(555.0, 0.0, 555.0),
        Vec3::new(555.0, 555.0, 555.0),
        Vec3::new(0.0, 555.0, 555.0),
        WHITE,
    );
    // A larger ceiling light for the busier scene.
    quad(
        &mut prims,
        Vec3::new(193.0, 554.0, 207.0),
        Vec3::new(363.0, 554.0, 207.0),
        Vec3::new(363.0, 554.0, 352.0),
        Vec3::new(193.0, 554.0, 352.0),
        LIGHT,
    );

    // Back row: three large feature spheres.
    let big = [
        (Vec3::new(140.0, 90.0, 260.0), GLASS),
        (Vec3::new(300.0, 90.0, 160.0), MIRROR),
        (Vec3::new(430.0, 90.0, 300.0), GOLD),
    ];
    for (c, m) in big {
        prims.push(Primitive::Sphere(Sphere::new(c, 90.0, m)));
    }
    // Front row: three smaller spheres.
    let small = [
        (Vec3::new(110.0, 55.0, 430.0), TEAL),
        (Vec3::new(255.0, 55.0, 450.0), ORANGE),
        (Vec3::new(410.0, 55.0, 440.0), BRUSHED),
    ];
    for (c, m) in small {
        prims.push(Primitive::Sphere(Sphere::new(c, 55.0, m)));
    }

    let camera = CameraConfig {
        look_from: Vec3::new(278.0, 278.0, -800.0),
        look_at: Vec3::new(278.0, 278.0, 0.0),
        vup: Vec3::new(0.0, 1.0, 0.0),
        vfov: 40.0,
        aperture: 0.0,
        focus_dist: None,
    };

    Scene::new(materials, prims, camera, Background::Solid(Color::ZERO))
}

/// A simple outdoor scene: a large ground sphere with diffuse, metal, and glass
/// spheres under a sky gradient. Fast to converge; good for smoke tests.
pub fn spheres() -> Scene {
    let ground = Material::Lambertian {
        albedo: Color::new(0.5, 0.5, 0.5).into(),
        normal: None,
    };
    let diffuse = Material::Lambertian {
        albedo: Color::new(0.7, 0.3, 0.3).into(),
        normal: None,
    };
    let glass = Material::Dielectric {
        ior: 1.5,
        absorption: Color::ZERO,
        roughness: 0.0,
        dispersion: 0.0,
    };
    let metal = Material::Metal {
        albedo: Color::new(0.8, 0.6, 0.2).into(),
        roughness: 0.25,
        normal: None,
    };

    let materials = vec![ground, diffuse, glass, metal];
    let prims = vec![
        Primitive::Sphere(Sphere::new(Vec3::new(0.0, -1000.0, 0.0), 1000.0, 0)),
        Primitive::Sphere(Sphere::new(Vec3::new(-2.2, 1.0, 0.0), 1.0, 1)),
        Primitive::Sphere(Sphere::new(Vec3::new(0.0, 1.0, 0.0), 1.0, 2)),
        Primitive::Sphere(Sphere::new(Vec3::new(2.2, 1.0, 0.0), 1.0, 3)),
    ];

    let camera = CameraConfig {
        look_from: Vec3::new(0.0, 1.6, 6.0),
        look_at: Vec3::new(0.0, 0.8, 0.0),
        vup: Vec3::new(0.0, 1.0, 0.0),
        vfov: 40.0,
        aperture: 0.04,
        focus_dist: Some(6.0),
    };

    Scene::new(materials, prims, camera, Background::default())
}

/// The classic "Ray Tracing in One Weekend" final scene: a large ground sphere
/// strewn with hundreds of small spheres of randomized materials (diffuse,
/// metal, glass) plus three big feature spheres, under a sky gradient. A good
/// stress test for the BVH and a busy mix of every material. Generated from a
/// fixed seed, so it is fully reproducible.
pub fn rtweekend() -> Scene {
    let mut rng = SmallRng::seed_from_u64(2024);
    let mut materials: Vec<Material> = vec![Material::Lambertian {
        albedo: Color::new(0.5, 0.5, 0.5).into(),
        normal: None,
    }];
    let mut prims: Vec<Primitive> =
        vec![Sphere::new(Vec3::new(0.0, -1000.0, 0.0), 1000.0, 0).into()];

    let mut push = |m: Material, center: Vec3, radius: Float, prims: &mut Vec<Primitive>| {
        materials.push(m);
        prims.push(Sphere::new(center, radius, materials.len() - 1).into());
    };

    for a in -11..11 {
        for b in -11..11 {
            let choose: Float = rng.gen();
            let center = Vec3::new(
                a as Float + 0.9 * rng.gen::<Float>(),
                0.2,
                b as Float + 0.9 * rng.gen::<Float>(),
            );
            if (center - Vec3::new(4.0, 0.2, 0.0)).length() <= 0.9 {
                continue; // keep clear of the big glass sphere
            }
            let m = if choose < 0.8 {
                let a = Color::new(
                    rng.gen::<Float>() * rng.gen::<Float>(),
                    rng.gen::<Float>() * rng.gen::<Float>(),
                    rng.gen::<Float>() * rng.gen::<Float>(),
                );
                Material::Lambertian {
                    albedo: a.into(),
                    normal: None,
                }
            } else if choose < 0.95 {
                let a = Color::new(
                    0.5 + 0.5 * rng.gen::<Float>(),
                    0.5 + 0.5 * rng.gen::<Float>(),
                    0.5 + 0.5 * rng.gen::<Float>(),
                );
                Material::Metal {
                    albedo: a.into(),
                    roughness: 0.5 * rng.gen::<Float>(),
                    normal: None,
                }
            } else {
                Material::Dielectric {
                    ior: 1.5,
                    absorption: Color::ZERO,
                    roughness: 0.0,
                    dispersion: 0.0,
                }
            };
            push(m, center, 0.2, &mut prims);
        }
    }

    push(
        Material::Dielectric {
            ior: 1.5,
            absorption: Color::ZERO,
            roughness: 0.0,
            dispersion: 0.0,
        },
        Vec3::new(0.0, 1.0, 0.0),
        1.0,
        &mut prims,
    );
    push(
        Material::Lambertian {
            albedo: Color::new(0.4, 0.2, 0.1).into(),
            normal: None,
        },
        Vec3::new(-4.0, 1.0, 0.0),
        1.0,
        &mut prims,
    );
    push(
        Material::Metal {
            albedo: Color::new(0.7, 0.6, 0.5).into(),
            roughness: 0.0,
            normal: None,
        },
        Vec3::new(4.0, 1.0, 0.0),
        1.0,
        &mut prims,
    );

    let camera = CameraConfig {
        look_from: Vec3::new(13.0, 2.0, 3.0),
        look_at: Vec3::new(0.0, 0.0, 0.0),
        vup: Vec3::new(0.0, 1.0, 0.0),
        vfov: 20.0,
        aperture: 0.1,
        focus_dist: Some(10.0),
    };

    Scene::new(
        materials,
        prims,
        camera,
        Background::Gradient {
            bottom: Color::ONE,
            top: Color::new(0.5, 0.7, 1.0),
        },
    )
}

/// An area-lit "material studio": a grid of spheres sweeping GGX roughness
/// left-to-right across three metal tints, with a row of glass and a row of
/// rainbow diffuse in front, on a neutral floor under a large soft light. Shows
/// the full BRDF range and exercises next-event estimation.
pub fn studio() -> Scene {
    const COLS: usize = 9;
    const SPACING: Float = 1.15;
    const RADIUS: Float = 0.45;

    let mut materials: Vec<Material> = Vec::new();
    let mut prims: Vec<Primitive> = Vec::new();

    // Floor and a big overhead area light.
    materials.push(Material::Lambertian {
        albedo: Color::splat(0.55).into(),
        normal: None,
    });
    let floor = 0;
    quad(
        &mut prims,
        Vec3::new(-25.0, 0.0, -25.0),
        Vec3::new(25.0, 0.0, -25.0),
        Vec3::new(25.0, 0.0, 25.0),
        Vec3::new(-25.0, 0.0, 25.0),
        floor,
    );
    materials.push(Material::Emissive {
        emit: Color::splat(7.0),
    });
    let light = materials.len() - 1;
    quad(
        &mut prims,
        Vec3::new(-6.0, 9.0, -4.0),
        Vec3::new(6.0, 9.0, -4.0),
        Vec3::new(6.0, 9.0, 4.0),
        Vec3::new(-6.0, 9.0, 4.0),
        light,
    );

    let x_of = |c: usize| -((COLS - 1) as Float) * SPACING / 2.0 + c as Float * SPACING;
    let mut add_sphere = |m: Material, x: Float, z: Float, prims: &mut Vec<Primitive>| {
        materials.push(m);
        prims.push(Sphere::new(Vec3::new(x, RADIUS, z), RADIUS, materials.len() - 1).into());
    };

    // Three metal rows (gold, silver, copper), roughness 0 -> 1 across columns.
    let tints = [
        Color::new(1.0, 0.78, 0.34),
        Color::new(0.95, 0.95, 0.97),
        Color::new(0.95, 0.64, 0.54),
    ];
    for (ri, &tint) in tints.iter().enumerate() {
        let z = ri as Float * SPACING;
        for c in 0..COLS {
            let roughness = c as Float / (COLS - 1) as Float;
            add_sphere(
                Material::Metal {
                    albedo: tint.into(),
                    roughness,
                    normal: None,
                },
                x_of(c),
                z,
                &mut prims,
            );
        }
    }
    // A glass row and a rainbow diffuse row, in front of the metals.
    for c in 0..COLS {
        add_sphere(
            Material::Dielectric {
                ior: 1.5,
                absorption: Color::ZERO,
                roughness: 0.0,
                dispersion: 0.0,
            },
            x_of(c),
            -SPACING,
            &mut prims,
        );
    }
    for c in 0..COLS {
        let albedo = hsv(c as Float / COLS as Float, 0.7, 0.9);
        add_sphere(
            Material::Lambertian {
                albedo: albedo.into(),
                normal: None,
            },
            x_of(c),
            -2.0 * SPACING,
            &mut prims,
        );
    }

    let camera = CameraConfig {
        look_from: Vec3::new(0.0, 6.5, -11.0),
        look_at: Vec3::new(0.0, 0.3, 0.4),
        vup: Vec3::new(0.0, 1.0, 0.0),
        vfov: 38.0,
        aperture: 0.0,
        focus_dist: None,
    };

    Scene::new(
        materials,
        prims,
        camera,
        Background::Gradient {
            bottom: Color::new(0.15, 0.16, 0.18),
            top: Color::new(0.35, 0.4, 0.5),
        },
    )
}

/// Spheres on a ground plane lit by a procedural sky **environment map** (a
/// horizon→zenith gradient with a bright sun). Demonstrates importance-sampled
/// image-based lighting and the soft directional shadows a sun casts — with no
/// external HDR asset.
pub fn sky() -> Scene {
    let ground = Material::Lambertian {
        albedo: Color::new(0.6, 0.6, 0.62).into(),
        normal: None,
    };
    let red = Material::Lambertian {
        albedo: Color::new(0.8, 0.25, 0.2).into(),
        normal: None,
    };
    let gold = Material::Metal {
        albedo: Color::new(1.0, 0.78, 0.34).into(),
        roughness: 0.12,
        normal: None,
    };
    let glass = Material::Dielectric {
        ior: 1.5,
        absorption: Color::ZERO,
        roughness: 0.0,
        dispersion: 0.0,
    };

    let materials = vec![ground, red, gold, glass];
    let prims = vec![
        Primitive::Sphere(Sphere::new(Vec3::new(0.0, -1000.0, 0.0), 1000.0, 0)),
        Primitive::Sphere(Sphere::new(Vec3::new(-2.2, 1.0, 0.0), 1.0, 1)),
        Primitive::Sphere(Sphere::new(Vec3::new(0.0, 1.0, 0.0), 1.0, 2)),
        Primitive::Sphere(Sphere::new(Vec3::new(2.2, 1.0, 0.0), 1.0, 3)),
    ];

    let camera = CameraConfig {
        look_from: Vec3::new(0.0, 1.6, 7.0),
        look_at: Vec3::new(0.0, 0.7, 0.0),
        vup: Vec3::new(0.0, 1.0, 0.0),
        vfov: 38.0,
        aperture: 0.0,
        focus_dist: None,
    };

    let mut scene = Scene::new(materials, prims, camera, Background::default());
    scene.set_environment(EnvMap::sky(
        1024,
        512,
        Vec3::new(0.3, 0.6, 0.6),     // sun direction (up, toward camera)
        Color::splat(25.0),           // bright sun
        0.05,                         // ~3° angular radius (soft shadows)
        Color::new(0.85, 0.88, 0.95), // horizon
        Color::new(0.25, 0.45, 0.85), // zenith
    ));
    scene
}

/// A checkerboard floor with checker/metal/glass spheres, lit by a procedural
/// sky. Demonstrates texture mapping (procedural textures + interpolated UVs).
pub fn textured() -> Scene {
    let floor = Material::Lambertian {
        albedo: Texture::Checker {
            even: Color::splat(0.9),
            odd: Color::new(0.12, 0.12, 0.15),
            scale: 1.0, // UVs are world units below, so 1 check per unit
        },
        normal: None,
    };
    let ball = Material::Lambertian {
        albedo: Texture::Checker {
            even: Color::new(0.85, 0.2, 0.2),
            odd: Color::new(0.95, 0.85, 0.2),
            scale: 10.0,
        },
        normal: None,
    };
    let metal = Material::Metal {
        albedo: Color::new(0.9, 0.9, 0.95).into(),
        roughness: 0.05,
        normal: None,
    };
    let glass = Material::Dielectric {
        ior: 1.5,
        absorption: Color::ZERO,
        roughness: 0.0,
        dispersion: 0.0,
    };

    let materials = vec![floor, ball, metal, glass];
    let mut prims = Vec::new();
    quad_uv(
        &mut prims,
        Vec3::new(-20.0, 0.0, -20.0),
        Vec3::new(20.0, 0.0, -20.0),
        Vec3::new(20.0, 0.0, 20.0),
        Vec3::new(-20.0, 0.0, 20.0),
        [-20.0, -20.0],
        [20.0, -20.0],
        [20.0, 20.0],
        [-20.0, 20.0],
        0,
    );
    prims.push(Sphere::new(Vec3::new(-2.2, 1.0, 0.0), 1.0, 1).into());
    prims.push(Sphere::new(Vec3::new(0.0, 1.0, 0.0), 1.0, 2).into());
    prims.push(Sphere::new(Vec3::new(2.2, 1.0, 0.0), 1.0, 3).into());

    let camera = CameraConfig {
        look_from: Vec3::new(0.0, 2.0, 7.5),
        look_at: Vec3::new(0.0, 0.8, 0.0),
        vup: Vec3::new(0.0, 1.0, 0.0),
        vfov: 40.0,
        aperture: 0.0,
        focus_dist: None,
    };

    let mut scene = Scene::new(materials, prims, camera, Background::default());
    scene.set_environment(EnvMap::sky(
        1024,
        512,
        Vec3::new(0.4, 0.7, 0.4),
        Color::splat(22.0),
        0.06,
        Color::new(0.85, 0.88, 0.95),
        Color::new(0.25, 0.45, 0.85),
    ));
    scene
}

/// HSV (`h, s, v` in `[0, 1]`) to linear RGB, for the rainbow diffuse row.
fn hsv(h: Float, s: Float, v: Float) -> Color {
    let h6 = h.fract() * 6.0;
    let i = h6.floor() as i32;
    let f = h6 - i as Float;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i.rem_euclid(6) {
        0 => Color::new(v, t, p),
        1 => Color::new(q, v, p),
        2 => Color::new(p, v, t),
        3 => Color::new(p, q, v),
        4 => Color::new(t, p, v),
        _ => Color::new(v, p, q),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_scenes_build() {
        assert!(cornell_box().primitive_count() > 0);
        assert!(spheres().primitive_count() > 0);
        // The showcase has an area light, so it must register a light source.
        let sc = showcase();
        assert!(sc.primitive_count() > 0);
        assert!(sc.num_lights() > 0);
        // The bigger scenes.
        assert!(rtweekend().primitive_count() > 100);
        let studio = studio();
        assert!(studio.primitive_count() > 0);
        assert!(studio.num_lights() > 0);
        // The sky scene carries an environment map.
        assert!(sky().environment().is_some());
        // The textured scene builds (checker floor + spheres).
        assert!(textured().primitive_count() > 0);
    }
}
