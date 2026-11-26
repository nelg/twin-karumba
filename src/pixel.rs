//! Small pixel-art helpers:
//! - `Canvas`: paints placeholder sprites in code (art px) for enemies that don't have
//!   art yet (fuel depot, flak gun, radar, ceiling turret, mine, rocks, pilot, supply
//!   crate, the Base Core boss), with the same dark rim as the real sprites.
//! - a soft glow texture for additive blending,
//! - the 5x5 HUD font from hud/src/hudfont.py.

use macroquad::prelude::*;

pub type Rgba = [u8; 4];
pub const OUTLINE: Rgba = [14, 12, 26, 255];

pub const fn rgb(v: u32) -> Rgba {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    px: Vec<Rgba>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Canvas {
        Canvas { w, h, px: vec![[0, 0, 0, 0]; (w * h) as usize] }
    }
    pub fn set(&mut self, x: i32, y: i32, c: Rgba) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = c;
        }
    }
    pub fn get(&self, x: i32, y: i32) -> Rgba {
        if x >= 0 && y >= 0 && x < self.w && y < self.h { self.px[(y * self.w + x) as usize] } else { [0, 0, 0, 0] }
    }
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, c);
            }
        }
    }
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        for y in (cy - r - 1.0) as i32..=(cy + r + 1.0) as i32 {
            for x in (cx - r - 1.0) as i32..=(cx + r + 1.0) as i32 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r * r {
                    self.set(x, y, c);
                }
            }
        }
    }
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgba) {
        let n = ((x1 - x0).abs().max((y1 - y0).abs()) as i32).max(1);
        for i in 0..=n {
            let t = i as f32 / n as f32;
            self.set((x0 + (x1 - x0) * t) as i32, (y0 + (y1 - y0) * t) as i32, c);
        }
    }
    /// Adds the 1 px dark rim around everything painted so far.
    pub fn outline(&mut self) {
        let src = self.px.clone();
        for y in 0..self.h {
            for x in 0..self.w {
                if src[(y * self.w + x) as usize][3] == 0 {
                    let near = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                        let (nx, ny) = (x + dx, y + dy);
                        nx >= 0 && ny >= 0 && nx < self.w && ny < self.h && src[(ny * self.w + nx) as usize][3] > 0
                    });
                    if near {
                        self.px[(y * self.w + x) as usize] = OUTLINE;
                    }
                }
            }
        }
    }
    pub fn to_texture(&self) -> Texture2D {
        let bytes: Vec<u8> = self.px.iter().flatten().copied().collect();
        let t = Texture2D::from_rgba8(self.w as u16, self.h as u16, &bytes);
        t.set_filter(FilterMode::Nearest);
        t
    }
    /// Paste another canvas at (x, y) (only opaque pixels).
    pub fn paste(&mut self, o: &Canvas, x: i32, y: i32) {
        for yy in 0..o.h {
            for xx in 0..o.w {
                let c = o.get(xx, yy);
                if c[3] > 0 {
                    self.set(x + xx, y + yy, c);
                }
            }
        }
    }
}

/// A frame strip built from several canvases of the same size; drawn like a Sheet
/// but at 1 texel per art px (so pass size in art px when drawing).
pub struct Strip {
    pub tex: Texture2D,
    pub w: f32,
    pub h: f32,
}

impl Strip {
    pub fn new(frames: &[Canvas]) -> Strip {
        let (w, h) = (frames[0].w, frames[0].h);
        let mut all = Canvas::new(w * frames.len() as i32, h);
        for (i, f) in frames.iter().enumerate() {
            all.paste(f, i as i32 * w, 0);
        }
        Strip { tex: all.to_texture(), w: w as f32, h: h as f32 }
    }
    /// Draw frame centred at (x, y) art px.
    pub fn draw(&self, frame: usize, x: f32, y: f32, rot: f32, flip: bool, color: Color) {
        draw_texture_ex(
            &self.tex,
            (x - self.w / 2.0).round(),
            (y - self.h / 2.0).round(),
            color,
            DrawTextureParams {
                dest_size: Some(vec2(self.w, self.h)),
                source: Some(Rect::new(frame as f32 * self.w, 0.0, self.w, self.h)),
                rotation: rot,
                flip_x: flip,
                ..Default::default()
            },
        );
    }
}

/// Soft round glow, white, for additive blending (tint it with the draw colour).
pub fn glow_texture() -> Texture2D {
    let n = 64;
    let mut c = Canvas::new(n, n);
    for y in 0..n {
        for x in 0..n {
            let d = (((x as f32 + 0.5 - 32.0).powi(2) + (y as f32 + 0.5 - 32.0).powi(2)).sqrt() / 32.0).min(1.0);
            let a = (1.0 - d).powf(2.2);
            c.set(x, y, [255, 255, 255, (a * 255.0) as u8]);
        }
    }
    let t = c.to_texture();
    t.set_filter(FilterMode::Linear);
    t
}

// ---------------------------------------------------------------- placeholder art

/// Shaded horizontal band helper: lit top row, mid, dark bottom row.
fn slab(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, hi: Rgba, mid: Rgba, lo: Rgba) {
    c.rect(x, y, w, h, mid);
    c.rect(x, y, w, 1, hi);
    c.rect(x, y + h - 1, w, 1, lo);
}

pub struct Placeholders {
    pub fuel: Strip,
    pub flak: Strip,
    pub radar: Strip,
    pub turret: Strip,
    pub mine: Strip,
    pub rock: Strip,
    pub pilot: Strip,
    pub cache: Strip,
    pub boss: Strip,
    pub boss_pod: Strip,
    pub boss_core: Strip,
}

pub fn make_placeholders() -> Placeholders {
    // Fuel depot: striped tank on legs with FUEL lettering.
    let fuel = {
        let mut c = Canvas::new(40, 32);
        c.rect(6, 24, 3, 7, rgb(0x5a5a7a));
        c.rect(31, 24, 3, 7, rgb(0x5a5a7a));
        c.rect(3, 29, 34, 3, rgb(0x6a6a8a));
        c.circle(20.0, 15.0, 11.0, rgb(0xd8d8e8));
        c.rect(4, 6, 32, 18, rgb(0xd8d8e8));
        c.circle(5.0, 15.0, 7.0, rgb(0xc0c0d4));
        c.circle(35.0, 15.0, 7.0, rgb(0xb0b0c8));
        c.rect(3, 12, 34, 7, rgb(0xff3a5a));
        c.rect(3, 12, 34, 1, rgb(0xff8aa0));
        c.rect(8, 6, 24, 2, rgb(0xffffff));
        text_canvas(&mut c, 10, 13, "FUEL", rgb(0xfff0b0));
        c.outline();
        c
    };
    // Flak gun: armoured base, twin barrels pointing up; frame 1 = muzzle flash.
    let flak = |fire: bool| {
        let mut c = Canvas::new(32, 30);
        c.rect(10, 2, 3, 16, rgb(0x8a8aa0));
        c.rect(17, 2, 3, 16, rgb(0x8a8aa0));
        c.rect(10, 2, 1, 16, rgb(0xd0d0e0));
        c.rect(17, 2, 1, 16, rgb(0xd0d0e0));
        if fire {
            c.circle(11.5, 1.5, 3.0, rgb(0xffe070));
            c.circle(18.5, 1.5, 3.0, rgb(0xffe070));
        }
        c.circle(15.5, 18.0, 8.0, rgb(0xff8a3a));
        slab(&mut c, 3, 20, 26, 9, rgb(0xffc07a), rgb(0xd06a2a), rgb(0x7a3412));
        c.rect(6, 23, 3, 3, rgb(0x2a1a2a));
        c.rect(23, 23, 3, 3, rgb(0x2a1a2a));
        c.outline();
        c
    };
    // Radar dish: post plus a dish that turns (4 frames).
    let radar = |f: i32| {
        let mut c = Canvas::new(32, 36);
        slab(&mut c, 6, 28, 20, 7, rgb(0xc0e8ff), rgb(0x5a8ab0), rgb(0x2a4a6a));
        c.rect(14, 14, 4, 14, rgb(0x8aa0c0));
        let w = [12.0, 8.0, 3.0, 8.0][f as usize];
        let shade = [rgb(0xe8f4ff), rgb(0xb0d0f0), rgb(0x90b0d0), rgb(0xb0d0f0)][f as usize];
        for i in 0..7 {
            let t = i as f32 / 6.0;
            let hw = w * (1.0 - (t - 0.5).abs() * 0.6);
            c.rect((16.0 - hw) as i32, 3 + i * 2, (hw * 2.0) as i32, 2, shade);
        }
        c.line(16.0, 9.0, 16.0 + w * 0.7, 2.0, rgb(0xff5a5a));
        c.circle(16.0 + w * 0.7, 2.0, 1.5, rgb(0xff5a5a));
        c.outline();
        c
    };
    // Ceiling turret: hangs from the roof; frame 1 = charging glow.
    let turret = |glow: bool| {
        let mut c = Canvas::new(28, 20);
        slab(&mut c, 2, 0, 24, 5, rgb(0x9aa4c4), rgb(0x5a6488), rgb(0x2a3050));
        c.circle(14.0, 8.0, 8.0, rgb(0x3ad8a0));
        c.circle(12.0, 6.0, 3.0, rgb(0xa0ffe0));
        c.rect(12, 12, 4, 6, rgb(0x6a7494));
        c.circle(14.0, 17.0, 2.5, if glow { rgb(0xffffff) } else { rgb(0xff3a5a) });
        c.outline();
        c
    };
    let mine = |f: i32| {
        let mut c = Canvas::new(20, 20);
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::PI / 4.0 + f as f32 * 0.2;
            c.line(10.0, 10.0, 10.0 + a.cos() * 9.0, 10.0 + a.sin() * 9.0, rgb(0x9a9ab4));
        }
        c.circle(10.0, 10.0, 6.0, rgb(0x4a4a64));
        c.circle(8.5, 8.5, 2.0, rgb(0x9a9ab4));
        c.circle(10.0, 10.0, 1.6, if f % 2 == 0 { rgb(0xff3a3a) } else { rgb(0x7a1a1a) });
        c.outline();
        c
    };
    let rock = {
        let mut c = Canvas::new(20, 18);
        c.circle(10.0, 9.0, 8.0, rgb(0x8a7a90));
        c.circle(7.0, 7.0, 4.0, rgb(0xb0a0b8));
        c.circle(13.0, 12.0, 4.0, rgb(0x5a4a64));
        c.outline();
        c
    };
    let pilot = |f: i32| {
        let mut c = Canvas::new(12, 18);
        c.circle(6.0, 3.5, 2.5, rgb(0xffd0a0));
        c.rect(4, 6, 5, 7, rgb(0xff9a2a));
        c.rect(4, 13, 2, 4, rgb(0x3a4a7a));
        c.rect(7, 13, 2, 4, rgb(0x3a4a7a));
        if f == 0 {
            c.line(8.0, 7.0, 11.0, 1.0, rgb(0xff9a2a));
        } else {
            c.line(8.0, 7.0, 11.0, 4.0, rgb(0xff9a2a));
        }
        c.line(4.0, 7.0, 2.0, 11.0, rgb(0xff9a2a));
        c.outline();
        c
    };
    let cache = {
        let mut c = Canvas::new(20, 16);
        slab(&mut c, 1, 2, 18, 13, rgb(0xd8b070), rgb(0xa07a40), rgb(0x5a3a1a));
        c.rect(1, 7, 18, 1, rgb(0x5a3a1a));
        c.rect(8, 4, 4, 9, rgb(0x3aff9a));
        c.rect(5, 7, 10, 3, rgb(0x3aff9a));
        c.outline();
        c
    };
    // Base Core: a 160x280 armoured fortress face with a slot for the core.
    let boss = {
        let mut c = Canvas::new(480, 284);
        c.rect(30, 0, 450, 284, rgb(0x3a4060));
        for y in (0..284).step_by(24) {
            c.rect(30, y, 450, 1, rgb(0x262a44));
        }
        for x in (30..480).step_by(32) {
            c.rect(x, 0, 1, 284, rgb(0x262a44));
        }
        // stepped armour plates at the front
        slab(&mut c, 10, 20, 40, 244, rgb(0xb0b8d8), rgb(0x6a7498), rgb(0x30365a));
        slab(&mut c, 0, 60, 20, 164, rgb(0xd0d8f0), rgb(0x8a94b8), rgb(0x3a4064));
        // hazard stripes
        for y in (24..260).step_by(8) {
            c.rect(44, y, 4, 4, rgb(0xffd24a));
        }
        // core slot
        c.rect(4, 118, 30, 48, rgb(0x0e0c1a));
        // red lights
        for y in [40, 240] {
            c.circle(30.0, y as f32, 4.0, rgb(0xff3a5a));
            c.circle(29.0, y as f32 - 1.0, 1.5, rgb(0xffc0c8));
        }
        c.outline();
        c
    };
    let boss_pod = |fire: bool| {
        let mut c = Canvas::new(40, 28);
        c.rect(0, 10, 18, 3, rgb(0xa0a4c0));
        c.rect(0, 16, 18, 3, rgb(0xa0a4c0));
        if fire {
            c.circle(2.0, 11.5, 4.0, rgb(0xffe070));
            c.circle(2.0, 17.5, 4.0, rgb(0xffe070));
        }
        c.circle(26.0, 14.0, 12.0, rgb(0xd04a7a));
        c.circle(22.0, 10.0, 5.0, rgb(0xff9ac0));
        c.circle(26.0, 14.0, 4.0, rgb(0x4a1a3a));
        c.outline();
        c
    };
    let boss_core = |open: bool, f: i32| {
        let mut c = Canvas::new(30, 48);
        if open {
            let r = 12.0 + (f % 2) as f32;
            c.circle(15.0, 24.0, r, rgb(0xff5a2a));
            c.circle(15.0, 24.0, r - 4.0, rgb(0xffd04a));
            c.circle(13.0, 21.0, 3.0, rgb(0xffffff));
        } else {
            for i in 0..6 {
                slab(&mut c, 0, i * 8, 30, 8, rgb(0xd0d8f0), rgb(0x8a94b8), rgb(0x3a4064));
            }
        }
        c.outline();
        c
    };
    Placeholders {
        fuel: Strip::new(&[fuel]),
        flak: Strip::new(&[flak(false), flak(true)]),
        radar: Strip::new(&[radar(0), radar(1), radar(2), radar(3)]),
        turret: Strip::new(&[turret(false), turret(true)]),
        mine: Strip::new(&[mine(0), mine(1)]),
        rock: Strip::new(&[rock]),
        pilot: Strip::new(&[pilot(0), pilot(1)]),
        cache: Strip::new(&[cache]),
        boss: Strip::new(&[boss]),
        boss_pod: Strip::new(&[boss_pod(false), boss_pod(true)]),
        boss_core: Strip::new(&[boss_core(false, 0), boss_core(true, 0), boss_core(true, 1)]),
    }
}

// ---------------------------------------------------------------- HUD font

/// 5-row glyphs from hud/src/hudfont.py. '#' = lit.
fn glyph(c: char) -> &'static [&'static str] {
    match c {
        'A' => &[".#.", "#.#", "###", "#.#", "#.#"],
        'B' => &["##.", "#.#", "##.", "#.#", "##."],
        'C' => &[".##", "#..", "#..", "#..", ".##"],
        'D' => &["##.", "#.#", "#.#", "#.#", "##."],
        'E' => &["###", "#..", "##.", "#..", "###"],
        'F' => &["###", "#..", "##.", "#..", "#.."],
        'G' => &[".##", "#..", "#.#", "#.#", ".##"],
        'H' => &["#.#", "#.#", "###", "#.#", "#.#"],
        'I' => &["###", ".#.", ".#.", ".#.", "###"],
        'J' => &["..#", "..#", "..#", "#.#", ".#."],
        'K' => &["#.#", "#.#", "##.", "#.#", "#.#"],
        'L' => &["#..", "#..", "#..", "#..", "###"],
        'M' => &["#...#", "##.##", "#.#.#", "#...#", "#...#"],
        'N' => &["#..#", "##.#", "#.##", "#..#", "#..#"],
        'O' => &[".#.", "#.#", "#.#", "#.#", ".#."],
        'P' => &["##.", "#.#", "##.", "#..", "#.."],
        'Q' => &[".#.", "#.#", "#.#", "##.", ".##"],
        'R' => &["##.", "#.#", "##.", "#.#", "#.#"],
        'S' => &[".##", "#..", ".#.", "..#", "##."],
        'T' => &["###", ".#.", ".#.", ".#.", ".#."],
        'U' => &["#.#", "#.#", "#.#", "#.#", "###"],
        'V' => &["#.#", "#.#", "#.#", "#.#", ".#."],
        'W' => &["#...#", "#...#", "#.#.#", "#.#.#", ".#.#."],
        'X' => &["#.#", "#.#", ".#.", "#.#", "#.#"],
        'Y' => &["#.#", "#.#", ".#.", ".#.", ".#."],
        'Z' => &["###", "..#", ".#.", "#..", "###"],
        '0' => &["###", "#.#", "#.#", "#.#", "###"],
        '1' => &[".#.", "##.", ".#.", ".#.", "###"],
        '2' => &["##.", "..#", ".#.", "#..", "###"],
        '3' => &["##.", "..#", ".#.", "..#", "##."],
        '4' => &["#.#", "#.#", "###", "..#", "..#"],
        '5' => &["###", "#..", "##.", "..#", "##."],
        '6' => &[".##", "#..", "###", "#.#", "###"],
        '7' => &["###", "..#", ".#.", ".#.", ".#."],
        '8' => &["###", "#.#", "###", "#.#", "###"],
        '9' => &["###", "#.#", "###", "..#", "##."],
        '-' => &["...", "...", "###", "...", "..."],
        '.' => &[".", ".", ".", ".", "#"],
        ':' => &[".", "#", ".", "#", "."],
        '!' => &["#", "#", "#", ".", "#"],
        '+' => &["...", ".#.", "###", ".#.", "..."],
        '/' => &["..#", "..#", ".#.", "#..", "#.."],
        ',' => &["..", "..", "..", ".#", "#."],
        '%' => &["#.#", "..#", ".#.", "#..", "#.#"],
        '(' => &[".#", "#.", "#.", "#.", ".#"],
        ')' => &["#.", ".#", ".#", ".#", "#."],
        _ => &["..", "..", "..", "..", ".."],
    }
}

/// Width of text in art px at scale s.
pub fn text_width(text: &str, s: f32) -> f32 {
    let cols: usize = text.chars().map(|c| glyph(c.to_ascii_uppercase())[0].len()).sum();
    (cols + text.chars().count()).saturating_sub(1) as f32 * s
}

/// Draws pixel-font text with its top-left at (x, y), art px. Optional drop shadow.
pub fn draw_text_px(text: &str, x: f32, y: f32, s: f32, color: Color, shadow: Option<Color>) {
    if let Some(sc) = shadow {
        draw_text_px(text, x + s.max(1.0), y + s.max(1.0), s, sc, None);
    }
    let mut cx = x;
    for ch in text.chars() {
        let g = glyph(ch.to_ascii_uppercase());
        for (gy, row) in g.iter().enumerate() {
            for (gx, b) in row.bytes().enumerate() {
                if b == b'#' {
                    draw_rectangle(cx + gx as f32 * s, y + gy as f32 * s, s, s, color);
                }
            }
        }
        cx += (g[0].len() + 1) as f32 * s;
    }
}

pub fn draw_text_px_centered(text: &str, cx: f32, y: f32, s: f32, color: Color, shadow: Option<Color>) {
    draw_text_px(text, (cx - text_width(text, s) / 2.0).round(), y, s, color, shadow);
}

/// Font into a canvas (for placeholder sprites).
fn text_canvas(c: &mut Canvas, x: i32, y: i32, text: &str, col: Rgba) {
    let mut cx = x;
    for ch in text.chars() {
        let g = glyph(ch);
        for (gy, row) in g.iter().enumerate() {
            for (gx, b) in row.bytes().enumerate() {
                if b == b'#' {
                    c.set(cx + gx as i32, y + gy as i32, col);
                }
            }
        }
        cx += g[0].len() as i32 + 1;
    }
}
