//! Loading textures and sounds from the `assets/` folder next to the game, or from
//! inside the executable when built with `--features embed`.

use crate::audio::Audio;
use macroquad::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

/// Finds the assets folder: next to the executable, or in the current directory
/// (so `cargo run` works from the project folder).
pub fn asset_dir() -> PathBuf {
    let mut cands = vec![PathBuf::from("assets")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            cands.insert(0, dir.join("assets"));
            cands.push(dir.join("../../assets")); // target/release -> project
        }
    }
    cands.into_iter().find(|p| p.join("tuning.json").exists()).unwrap_or_else(|| PathBuf::from("assets"))
}

// The packed files: empty unless built with `--features embed` (see build.rs).
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded.rs"));
}

/// Is this a single-file build with the assets packed inside?
pub fn embedded() -> bool {
    !embedded::FILES.is_empty()
}

/// An asset's bytes, or None if it's missing. `rel` is relative to assets/, e.g. "music/x.xm".
pub fn try_read(rel: &str) -> Option<Vec<u8>> {
    if embedded() {
        return embedded::FILES.iter().find(|(name, _)| *name == rel).map(|(_, b)| b.to_vec());
    }
    std::fs::read(asset_dir().join(rel)).ok()
}

pub fn read(rel: &str) -> Vec<u8> {
    try_read(rel).unwrap_or_else(|| panic!("can't read asset {rel} (looked in {})", asset_dir().display()))
}

pub fn read_text(rel: &str) -> String {
    String::from_utf8_lossy(&read(rel)).into_owned()
}

pub fn texture(rel: &str) -> Texture2D {
    let t = Texture2D::from_file_with_format(&read(rel), Some(ImageFormat::Png));
    t.set_filter(FilterMode::Nearest);
    t
}

/// A sprite sheet: a grid of equal frames. `fw`/`fh` are frame sizes in sheet pixels,
/// which are 1080p screen pixels, so an art pixel is 2 sheet pixels.
pub struct Sheet {
    pub tex: Texture2D,
    pub fw: f32,
    pub fh: f32,
}

impl Sheet {
    pub fn load(rel: &str, fw: f32, fh: f32) -> Sheet {
        Sheet { tex: texture(rel), fw, fh }
    }
    pub fn from_tex(tex: Texture2D, fw: f32, fh: f32) -> Sheet {
        Sheet { tex, fw, fh }
    }
    pub fn frames(&self) -> usize {
        (self.tex.width() / self.fw) as usize
    }
    /// Size of one frame in art px at the given scale.
    pub fn size(&self, scale: f32) -> Vec2 {
        vec2(self.fw * 0.5 * scale, self.fh * 0.5 * scale)
    }
    /// Draws a frame centred on (x, y) in art px; `rot` in radians (clockwise on screen).
    pub fn draw(&self, row: usize, frame: usize, x: f32, y: f32, scale: f32, rot: f32, flip: bool, color: Color) {
        let s = self.size(scale);
        draw_texture_ex(
            &self.tex,
            x - s.x / 2.0,
            y - s.y / 2.0,
            color,
            DrawTextureParams {
                dest_size: Some(s),
                source: Some(Rect::new(frame as f32 * self.fw, row as f32 * self.fh, self.fw, self.fh)),
                rotation: rot,
                flip_x: flip,
                ..Default::default()
            },
        );
    }
    /// Draw with a custom size (stretch), top-left anchored at (x,y), rotated about `pivot`.
    pub fn draw_stretch(&self, row: usize, frame: usize, x: f32, y: f32, w: f32, h: f32, rot: f32, pivot: Vec2, color: Color) {
        draw_texture_ex(
            &self.tex,
            x,
            y,
            color,
            DrawTextureParams {
                dest_size: Some(vec2(w, h)),
                source: Some(Rect::new(frame as f32 * self.fw, row as f32 * self.fh, self.fw, self.fh)),
                rotation: rot,
                pivot: Some(pivot),
                ..Default::default()
            },
        );
    }
}

/// All the sprite sheets the game uses. Sizes come from sprites/px-meta.json.
pub struct Sprites {
    pub kx2: [Sheet; 2],
    pub shield: [Sheet; 2],
    pub bullet: [Sheet; 2],
    pub torpedo: [Sheet; 2],
    pub laser: [Sheet; 2],
    pub laser_head: [Sheet; 2],
    pub pickups: [Sheet; 2],
    pub hull_digits: Texture2D,
    pub muzzle: Sheet,
    pub explosion: Sheet,
    pub ground_blast: Sheet,
    pub spark: Sheet,
    pub flak: Sheet,
    pub e_bullet: Sheet,
    pub shell: Sheet,
    pub e_laser: Sheet,
    pub rocket: Sheet,
    pub tank: Sheet,
    pub truck: Sheet,
    pub tower: Sheet,
    pub drone: Sheet,
    pub pad: Sheet,
    pub fighter: Sheet,
    pub whirler: Sheet,
    pub balloon: Sheet,
    pub ex: Extras,
}

/// The sprites drawn from Glen's picks on the concept sheet (sprites/extras/).
/// Variants: fuel A (shoot), B (airstrip), C (crane); radar A/B; turrets A/B/C with a
/// separate gun barrel; mines A/C; rocks A/B/C; pilots A/B.
pub struct Extras {
    pub fuel: [Sheet; 3],
    pub flak: Sheet,
    pub radar: [Sheet; 2],
    pub turret: [Sheet; 3],
    pub turret_gun: [Sheet; 3],
    /// gun mount on the turret body and the barrel's pivot, both in sheet px from the top-left
    pub turret_mount: [Vec2; 3],
    pub turret_pivot: [Vec2; 3],
    pub mine: [Sheet; 2],
    pub rock: [Sheet; 3],
    pub pilot: [Sheet; 2],
    pub cache: Sheet,
    pub chopper: Sheet,
}

impl Extras {
    fn load() -> Extras {
        let meta: serde_json::Value = serde_json::from_str(&read_text("sprites/extras/extras-meta.json")).unwrap_or_default();
        let s = |n: &str| {
            let m = &meta[n];
            let fw = m["frame_px"][0].as_f64().unwrap_or(64.0) as f32;
            let fh = m["frame_px"][1].as_f64().unwrap_or(64.0) as f32;
            Sheet::load(&format!("sprites/extras/{n}-sheet.png"), fw, fh)
        };
        let v2 = |n: &str, k: &str| {
            let a = &meta[n][k];
            vec2(a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32)
        };
        Extras {
            fuel: [s("fuel-a"), s("fuel-b"), s("fuel-c")],
            flak: s("flak"),
            radar: [s("radar-a"), s("radar-b")],
            turret: [s("turret-a"), s("turret-b"), s("turret-c")],
            turret_gun: [s("turret-a-gun"), s("turret-b-gun"), s("turret-c-gun")],
            turret_mount: [v2("turret-a", "gun_mount_px"), v2("turret-b", "gun_mount_px"), v2("turret-c", "gun_mount_px")],
            turret_pivot: [v2("turret-a-gun", "pivot_px"), v2("turret-b-gun", "pivot_px"), v2("turret-c-gun", "pivot_px")],
            mine: [s("mine-a"), s("mine-c")],
            rock: [s("rock-a"), s("rock-b"), s("rock-c")],
            pilot: [s("pilot-a"), s("pilot-b")],
            cache: s("cache"),
            chopper: s("chopper"),
        }
    }
}

impl Sprites {
    pub fn load() -> Sprites {
        let s = |n: &str, w: f32, h: f32| Sheet::load(&format!("sprites/{n}.png"), w, h);
        Sprites {
            kx2: [s("kx2-p1-px-sheet", 400.0, 240.0), s("kx2-p2-px-sheet", 400.0, 240.0)],
            shield: [s("shield-p1-sheet", 400.0, 240.0), s("shield-p2-sheet", 400.0, 240.0)],
            bullet: [s("bullet-p1-sheet", 40.0, 14.0), s("bullet-p2-sheet", 40.0, 14.0)],
            torpedo: [s("torpedo-p1-sheet", 74.0, 74.0), s("torpedo-p2-sheet", 74.0, 74.0)],
            laser: [s("laser-p1-sheet", 80.0, 20.0), s("laser-p2-sheet", 80.0, 20.0)],
            laser_head: [s("laser-head-p1-sheet", 30.0, 30.0), s("laser-head-p2-sheet", 30.0, 30.0)],
            pickups: [s("pickups-p1-sheet", 40.0, 40.0), s("pickups-p2-sheet", 40.0, 40.0)],
            hull_digits: texture("sprites/hull-digits.png"),
            muzzle: s("muzzle-sheet", 30.0, 30.0),
            explosion: s("explosion-sheet", 110.0, 110.0),
            ground_blast: s("ground-blast-sheet", 110.0, 110.0),
            spark: s("spark-sheet", 24.0, 24.0),
            flak: s("flak-burst-sheet", 70.0, 70.0),
            e_bullet: s("bullet-sheet", 34.0, 20.0),
            shell: s("shell-sheet", 44.0, 20.0),
            e_laser: s("enemy-laser-sheet", 80.0, 20.0),
            rocket: s("rocket-sheet", 54.0, 14.0),
            tank: s("tank-sheet", 114.0, 80.0),
            truck: s("rocket-truck-sheet", 124.0, 90.0),
            tower: s("gun-tower-sheet", 100.0, 134.0),
            drone: s("drone-sheet", 74.0, 54.0),
            pad: s("drone-pad-sheet", 100.0, 30.0),
            fighter: s("fighter-sheet", 110.0, 64.0),
            whirler: s("whirler-sheet", 64.0, 64.0),
            balloon: s("balloon-sheet", 80.0, 114.0),
            ex: Extras::load(),
        }
    }
}

/// Sound effects by name (file name without .wav), played through our own mixer
/// (see audio.rs). Each name plays at most once per frame.
pub struct Sfx {
    pub audio: Audio,
    clips: HashMap<String, usize>,
    played: Vec<String>,
    pub volume: f32,
}

impl Sfx {
    pub fn load() -> Sfx {
        let audio = Audio::start();
        let names = [
            "bomb-drop", "bomb-hit", "cannon", "checkpoint", "door", "enemy-laser", "enemy-shot", "explode-big",
            "explode-small", "extra-life", "fighter-pass", "flak-burst", "fuel-low", "fuel-up", "laser", "pickup",
            "plasma-up", "player-die", "rocket-launch", "rotor-loop", "shield-down", "shield-hit", "shield-on",
            "swarm", "tank-cannon", "tank-tracks", "winch",
        ];
        let mut clips = HashMap::new();
        for n in names {
            if let Some(bytes) = try_read(&format!("sfx/{n}.wav")) {
                if let Some(c) = audio.load_wav(&bytes) {
                    clips.insert(n.to_string(), c);
                }
            }
        }
        Sfx { audio, clips, played: Vec::new(), volume: 0.8 }
    }

    pub fn play(&mut self, name: &str, vol: f32) {
        if self.volume <= 0.0 || self.played.iter().any(|p| p == name) {
            return;
        }
        if let Some(&c) = self.clips.get(name) {
            self.audio.play(c, vol * self.volume, false);
            self.played.push(name.to_string());
        }
    }

    /// Starts a looping sound; returns a handle for `stop`.
    pub fn play_loop(&mut self, name: &str, vol: f32) -> Option<u64> {
        self.clips.get(name).map(|&c| self.audio.play(c, vol, true))
    }

    pub fn stop(&mut self, handle: u64) {
        self.audio.stop(handle);
    }

    /// Call once per frame.
    pub fn end_frame(&mut self) {
        self.played.clear();
    }
}
