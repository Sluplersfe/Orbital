//! Procedural textures. There are no art assets yet, so planets, dials and the navball are
//! generated at startup from value noise.
use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::math::Vec3;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::f32::consts::PI;

fn hash(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ (z as u32).wrapping_mul(0xcb1a_b31f) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_215.0
}

/// Smooth value noise in [0, 1].
pub fn noise(p: Vec3, seed: u32) -> f32 {
    let (fx, fy, fz) = (p.x.floor(), p.y.floor(), p.z.floor());
    let (ix, iy, iz) = (fx as i32, fy as i32, fz as i32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (u, v, w) = (s(p.x - fx), s(p.y - fy), s(p.z - fz));
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dx, dy, dz| hash(ix + dx, iy + dy, iz + dz, seed);
    l(
        l(l(c(0, 0, 0), c(1, 0, 0), u), l(c(0, 1, 0), c(1, 1, 0), u), v),
        l(l(c(0, 0, 1), c(1, 0, 1), u), l(c(0, 1, 1), c(1, 1, 1), u), v),
        w,
    )
}

/// Fractal noise in roughly [0, 1].
pub fn fbm(p: Vec3, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * noise(p * freq, seed + o);
        norm += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / norm
}

fn image(w: u32, h: u32, data: Vec<u8>, srgb: bool) -> Image {
    let fmt = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
    let mut img = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, data, fmt, RenderAssetUsages::RENDER_WORLD);
    img.sampler = ImageSampler::linear();
    img
}

/// Equirectangular map: `f` gets the unit direction (pole along +Z and longitude from +X toward
/// +Y, matching Bevy's UV sphere)
/// and the latitude in radians, and returns RGBA.
fn sphere_map(w: u32, h: u32, f: impl Fn(Vec3, f32) -> [f32; 4]) -> Vec<u8> {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for j in 0..h {
        let lat = PI / 2.0 - (j as f32 + 0.5) / h as f32 * PI;
        for i in 0..w {
            let lon = (i as f32 + 0.5) / w as f32 * 2.0 * PI;
            let d = Vec3::new(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin());
            for c in f(d, lat) {
                data.push((c.clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
    }
    data
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Earth: oceans, continents with forest, grassland and desert, and polar ice.
pub fn earth() -> Image {
    let (w, h) = (1024, 512);
    image(w, h, sphere_map(w, h, |d, lat| {
        // The launch site (longitude 0 on the equator) is always on land.
        let n = fbm(d * 2.2, 6, 7) + 0.3 * (-(d - Vec3::X).length_squared() / 0.04).exp();
        let land = n > 0.52;
        let alat = lat.abs();
        let c = if alat > 1.22 + 0.1 * noise(d * 6.0, 3) {
            [0.92, 0.94, 0.97]
        } else if land {
            let dry = fbm(d * 3.5 + Vec3::splat(9.0), 4, 11);
            let desert = (1.0 - (alat - 0.42).abs() * 4.0).max(0.0) * 0.8 + dry * 0.5 - 0.35;
            let green = mix([0.16, 0.32, 0.12], [0.33, 0.42, 0.2], dry);
            let mut c = mix(green, [0.72, 0.6, 0.4], desert * 1.6);
            let high = ((n - 0.62) * 6.0).clamp(0.0, 1.0);
            c = mix(c, [0.45, 0.4, 0.36], high);
            mix(c, [0.85, 0.87, 0.9], ((alat - 1.0) * 5.0).clamp(0.0, 1.0))
        } else {
            let depth = ((0.52 - n) * 5.0).clamp(0.0, 1.0);
            mix([0.08, 0.3, 0.5], [0.02, 0.08, 0.25], depth)
        };
        [c[0], c[1], c[2], 1.0]
    }), true)
}

/// Cloud layer: white with alpha from noise.
pub fn clouds() -> Image {
    let (w, h) = (1024, 512);
    image(w, h, sphere_map(w, h, |d, lat| {
        let band = 0.75 + 0.25 * (lat * 3.0).cos().abs();
        let n = fbm(d * 3.0 + Vec3::new(4.0, 1.0, 2.0), 6, 23) * band;
        let a = ((n - 0.43) * 3.2).clamp(0.0, 0.9);
        [1.0, 1.0, 1.0, a]
    }), true)
}

/// A rocky, cratered body tinted with `tint`.
pub fn rock(tint: [f32; 3], seed: u32) -> Image {
    let (w, h) = (512, 256);
    image(w, h, sphere_map(w, h, |d, _| {
        let maria = fbm(d * 1.5, 4, seed);
        let detail = fbm(d * 12.0, 4, seed + 50);
        let craters = (noise(d * 24.0, seed + 90) - 0.5).abs();
        let v = 0.55 + 0.35 * detail - if maria > 0.55 { 0.22 } else { 0.0 } - (0.12 - craters).max(0.0) * 1.5;
        [tint[0] * v, tint[1] * v, tint[2] * v, 1.0]
    }), true)
}

/// Banded gas giant.
pub fn gas(a: [f32; 3], b: [f32; 3], seed: u32) -> Image {
    let (w, h) = (512, 256);
    image(w, h, sphere_map(w, h, |d, lat| {
        let t = (lat * 9.0 + 2.0 * fbm(d * 3.0, 4, seed)).sin() * 0.5 + 0.5;
        let c = mix(a, b, t);
        [c[0], c[1], c[2], 1.0]
    }), true)
}

/// Navball: sky (blue) above the horizon, ground (brown) below, with pitch ladder lines every
/// 30 degrees and a heading line at the east meridian.
pub fn navball() -> Image {
    let (w, h) = (512, 256);
    image(w, h, sphere_map(w, h, |d, lat| {
        let deg = lat.to_degrees();
        let mut c = if deg >= 0.0 { [0.2, 0.55, 0.9] } else { [0.62, 0.38, 0.15] };
        if deg.abs() < 0.8 {
            c = [0.95, 0.95, 0.95];
        } else if (deg.abs() % 30.0) < 0.6 {
            c = mix(c, [1.0, 1.0, 1.0], 0.7);
        }
        // East meridian: direction +X at the equator.
        let lon = d.y.atan2(d.x).to_degrees();
        if lon.abs() < 0.9 || (lon.abs() - 90.0).abs() < 0.5 || (lon.abs() - 180.0).abs() < 0.5 {
            c = mix(c, [1.0, 1.0, 1.0], 0.6);
        }
        [c[0], c[1], c[2], 1.0]
    }), true)
}

/// Gauge face: dark dial with major and minor ticks around 270 degrees.
pub fn dial() -> Image {
    let n = 256u32;
    let mut data = Vec::with_capacity((n * n * 4) as usize);
    for j in 0..n {
        for i in 0..n {
            let x = (i as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let y = 1.0 - (j as f32 + 0.5) / n as f32 * 2.0;
            let r = (x * x + y * y).sqrt();
            // Angle from straight up, clockwise, so the sweep runs -135..135 degrees with the
            // gap at the bottom.
            let a = x.atan2(y).to_degrees();
            let mut c = [0.06, 0.07, 0.08];
            if (0.78..0.9).contains(&r) && a.abs() <= 136.0 {
                let major = ((a + 135.0) % 27.0).min(27.0 - (a + 135.0) % 27.0) < 1.6;
                let minor = ((a + 135.0) % 5.4).min(5.4 - (a + 135.0) % 5.4) < 0.7 && r > 0.83;
                if major || minor {
                    c = [0.92, 0.92, 0.88];
                }
            }
            if (0.94..1.0).contains(&r) {
                c = [0.3, 0.32, 0.35];
            }
            for v in c {
                data.push((v * 255.0) as u8);
            }
            data.push(255);
        }
    }
    image(n, n, data, true)
}

/// Screen: dark glass with a faint grid.
pub fn screen(tint: [f32; 3]) -> Image {
    let (w, h) = (256u32, 160u32);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for j in 0..h {
        for i in 0..w {
            let grid = i % 32 == 0 || j % 32 == 0;
            let v = if grid { 0.35 } else { 0.08 };
            for c in tint {
                data.push((c * v * 255.0) as u8);
            }
            data.push(255);
        }
    }
    image(w, h, data, true)
}
