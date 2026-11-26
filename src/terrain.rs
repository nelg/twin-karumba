//! Land drawing: bakes the level's terrain into pixel-art textures, a chunk at a time,
//! and draws the parallax sky layers.
//!
//! Works in ART pixels: each chunk texture is 1 texel per art px and is drawn with
//! nearest filtering, so every art pixel becomes a crisp 2x2 block at 1080p.
//! Colours follow `land/src/terrain_gen.py` (rim highlight, rim, textured body).

use crate::level::{Level, PALETTES, SCREEN_H, SCREEN_W, TILE};
use macroquad::prelude::*;
use std::collections::HashMap;

pub const CHUNK_W: usize = 512;
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Rim highlight, rim, body light..dark, per palette (from terrain_gen.py).
struct Pal {
    rim: [[u8; 3]; 2],
    body: [[u8; 3]; 4],
}

fn hex(s: &str) -> [u8; 3] {
    let v = u32::from_str_radix(&s[1..], 16).unwrap_or(0);
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn pal(i: usize) -> Pal {
    let (rim, body): ([&str; 2], [&str; 4]) = match i {
        0 => (["#e4ff8a", "#9ad83a"], ["#7a8a3a", "#5a6a2a", "#3e4a1e", "#262e14"]),
        1 => (["#ffe0f4", "#ff8ad0"], ["#d0589e", "#a63c7c", "#78245a", "#4a1238"]),
        2 => (["#d8f4ff", "#6ac8ff"], ["#3a86d8", "#2a60aa", "#1c3e7a", "#10224a"]),
        3 => (["#fff0b0", "#ffb840"], ["#e07a22", "#b0521a", "#7a3412", "#4a1c0a"]),
        4 => (["#f0dcff", "#c08aff"], ["#8a5ad8", "#6a3cb0", "#482480", "#2a1250"]),
        _ => (["#dcfff4", "#5ae8c0"], ["#22b090", "#188470", "#0e5a4e", "#06362e"]),
    };
    Pal { rim: [hex(rim[0]), hex(rim[1])], body: [hex(body[0]), hex(body[1]), hex(body[2]), hex(body[3])] }
}

/// A CPU-side RGB image (used for the seamless 64x64 land textures).
pub struct RgbImg {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl RgbImg {
    pub fn from_png(bytes: &[u8]) -> RgbImg {
        let im = Image::from_file_with_format(bytes, Some(ImageFormat::Png)).expect("png");
        RgbImg { w: im.width as usize, h: im.height as usize, px: im.bytes }
    }
    fn at(&self, x: usize, y: usize) -> [u8; 3] {
        let i = ((y % self.h) * self.w + (x % self.w)) * 4;
        [self.px[i], self.px[i + 1], self.px[i + 2]]
    }
}

/// Cheap integer hash for per-pixel randomness that never changes between frames.
fn hash(x: u32, y: u32) -> u32 {
    let mut h = x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^ (h >> 16)
}

pub struct Terrain {
    /// textures[palette][variant] (variant 0 ground, 1 cave roof, 2 mountain)
    tex: Vec<Vec<RgbImg>>,
    chunks: HashMap<usize, Texture2D>,
    /// parallax layers: sky, far, mid per palette
    pub bg: Vec<[Texture2D; 3]>,
}

impl Terrain {
    pub fn new(tex: Vec<Vec<RgbImg>>, bg: Vec<[Texture2D; 3]>) -> Terrain {
        Terrain { tex, chunks: HashMap::new(), bg }
    }

    /// Palette for an art pixel, with an ordered-dither blend across ~40 px at section changes.
    fn pal_at(level: &Level, x: usize, y: usize) -> usize {
        let j = (BAYER[y % 4][x % 4] as f32 / 16.0 - 0.5) * 40.0;
        let col = ((x as f32 + j).max(0.0) / TILE) as usize;
        level.palette_at_col(col)
    }

    /// Bake one chunk of land into RGBA texels.
    fn bake(&self, level: &Level, chunk: usize) -> Texture2D {
        let w = CHUNK_W;
        let h = SCREEN_H as usize;
        let mut px = vec![0u8; w * h * 4];
        let pals: Vec<Pal> = (0..6).map(pal).collect();
        let x0 = chunk * CHUNK_W;
        let lw = level.ground_y.len();
        for lx in 0..w {
            let x = x0 + lx;
            if x >= lw {
                break;
            }
            let colinfo = level.col_at(x as f32);
            let gy = level.ground_y[x] as i32;
            let cy = level.ceil_y[x] as i32;
            let mut put = |y: i32, c: [u8; 3]| {
                if y >= 0 && (y as usize) < h {
                    let i = (y as usize * w + lx) * 4;
                    px[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
                }
            };
            // ---- ground ----
            let metal = colinfo.gtype == '=';
            let variant = if colinfo.gtype == '▲' || colinfo.gtype == '█' { 2 } else { 0 };
            for y in gy.max(0)..h as i32 {
                let d = y - gy;
                let p = Self::pal_at(level, x, y as usize);
                let c = if metal {
                    metal_px(x, d)
                } else if d == 0 {
                    pals[p].rim[0]
                } else if d <= 2 {
                    pals[p].rim[1]
                } else {
                    let mut c = self.tex[p][variant].at(x, y as usize);
                    if d < 7 {
                        // slightly lighter just under the rim
                        for k in 0..3 {
                            c[k] = c[k].saturating_add(((7 - d) * 4) as u8);
                        }
                    }
                    c
                };
                put(y, c);
            }
            // snow / crystal caps on the big peaks
            if colinfo.gtype == '▲' && gy < 250 {
                let n = if gy < 200 { 4 } else { 2 };
                for d in 0..n {
                    if d < 2 || (x as i32 + gy + d) % 3 != 0 {
                        put(gy + d, [236, 244, 255]);
                    }
                }
            }
            // grass tufts above the rim (not on metal or cliffs)
            if !metal && colinfo.gtype != '█' {
                let tuft = [0, 0, 1, 2, 3][(hash(x as u32, 7) % 5) as usize];
                let p = level.palette_at_col((x as f32 / TILE) as usize);
                for k in 0..tuft {
                    if (x * 7 + k) % 3 != 0 {
                        put(gy - 1 - k as i32, pals[p].rim[if k > 0 { 1 } else { 0 }]);
                    }
                }
            }
            // ---- cave roof ----
            if cy > 0 {
                let metal_roof = colinfo.ctype == '=';
                for y in 0..cy.min(h as i32) {
                    let d = cy - 1 - y;
                    let p = Self::pal_at(level, x, y as usize);
                    let c = if metal_roof {
                        if d < 2 {
                            [150, 156, 178]
                        } else if x % 32 == 0 || y % 24 == 0 {
                            [70, 76, 98]
                        } else {
                            [96, 104, 128]
                        }
                    } else if d == 0 {
                        pals[p].rim[1]
                    } else if d <= 2 {
                        pals[p].body[0]
                    } else {
                        self.tex[p][1].at(x, y as usize)
                    };
                    put(y, c);
                }
                if !metal_roof {
                    // drips / small stalactites
                    let p = level.palette_at_col((x as f32 / TILE) as usize);
                    let drip = [0, 0, 1, 2, 3][(hash(x as u32, 9) % 5) as usize] + if x % 5 == 0 { 1 } else { 0 };
                    for k in 0..drip {
                        if cy + k < gy {
                            put(cy + k, pals[p].body[0]);
                        }
                    }
                }
            }
        }
        let t = Texture2D::from_rgba8(w as u16, h as u16, &px);
        t.set_filter(FilterMode::Nearest);
        t
    }

    /// Make sure the chunks covering [x0, x1] exist, and drop ones far behind.
    pub fn prepare(&mut self, level: &Level, x0: f32, x1: f32) {
        let a = (x0.max(0.0) as usize) / CHUNK_W;
        let b = (x1.max(0.0) as usize) / CHUNK_W + 1; // one chunk of look-ahead
        for c in a..=b {
            if c * CHUNK_W < level.ground_y.len() && !self.chunks.contains_key(&c) {
                let t = self.bake(level, c);
                self.chunks.insert(c, t);
            }
        }
        // `retain` keeps only entries for which the closure returns true
        self.chunks.retain(|&c, _| c + 3 >= a && c <= b + 3);
    }

    pub fn draw_land(&self, cam_x: f32) {
        let a = (cam_x.max(0.0) as usize) / CHUNK_W;
        let b = ((cam_x + SCREEN_W) as usize) / CHUNK_W;
        for c in a..=b {
            if let Some(t) = self.chunks.get(&c) {
                draw_texture_ex(
                    t,
                    (c * CHUNK_W) as f32,
                    0.0,
                    WHITE,
                    DrawTextureParams { dest_size: Some(vec2(CHUNK_W as f32, SCREEN_H)), ..Default::default() },
                );
            }
        }
    }

    /// Draws the three parallax layers (sky, far hills, mid hills), crossfading the
    /// palette over 200 px after each section colour change. Drawn in world space.
    pub fn draw_sky(&self, level: &Level, cam_x: f32) {
        let centre = cam_x + SCREEN_W / 2.0;
        let col = ((centre / TILE) as usize).min(level.pal.len() - 1);
        let cur = level.pal[col];
        // find where this palette started and what came before
        let mut start = col;
        while start > 0 && level.pal[start - 1] == cur {
            start -= 1;
        }
        let prev = if start > 0 { Some(level.pal[start - 1]) } else { None };
        let t = ((centre - start as f32 * TILE) / 200.0).clamp(0.0, 1.0);
        for (layer, speed) in [(0usize, 0.1f32), (1, 0.25), (2, 0.5)] {
            let mut draws: Vec<(usize, f32)> = Vec::new();
            match prev {
                Some(p) if t < 1.0 => {
                    draws.push((p, 1.0));
                    draws.push((cur, t));
                }
                _ => draws.push((cur, 1.0)),
            }
            for (p, alpha) in draws {
                let tex = &self.bg[p][layer];
                let off = (cam_x * speed) % SCREEN_W;
                for k in 0..2 {
                    let x = cam_x - off + k as f32 * SCREEN_W;
                    draw_texture_ex(
                        tex,
                        x.floor(),
                        0.0,
                        Color::new(1.0, 1.0, 1.0, alpha),
                        DrawTextureParams { dest_size: Some(vec2(SCREEN_W + 0.5, SCREEN_H)), ..Default::default() },
                    );
                }
            }
        }
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
    }
}

/// Metal deck plating (hazard stripe, rivets, panel lines), from level_render.py.
fn metal_px(x: usize, d: i32) -> [u8; 3] {
    let y = d as usize;
    if d < 2 {
        return [190, 196, 214];
    }
    if d < 5 {
        return if ((x + y) / 4) % 2 == 1 { [255, 210, 74] } else { [30, 32, 44] };
    }
    if x % 32 == 0 || y % 24 == 0 {
        return [54, 58, 76];
    }
    if x % 32 == 3 && y % 24 == 3 {
        return [210, 214, 230];
    }
    if (y / 3) % 2 == 1 { [118, 126, 150] } else { [96, 104, 128] }
}

/// Load names helper for main: tex file name for palette p, variant v.
pub fn tex_name(p: usize, v: usize) -> String {
    format!("land/tex-{}-{}.png", PALETTES[p], v)
}
