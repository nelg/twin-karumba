#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")] // no console window on Windows
//! Twin Karumba: Rust + macroquad prototype of level 1.
//!
//! Layout of the code:
//! - `level.rs`   reads levels/level1.txt (the shared ASCII format)
//! - `tuning.rs`  reads tuning.json into a typed `Cfg`
//! - `terrain.rs` bakes the land into pixel-art textures and draws the parallax sky
//! - `game.rs`    players, enemies, weapons, pickups, boss, level flow
//! - `fx.rs`      explosions and particles, `hud.rs` the HUD bar
//! - `pixel.rs`   placeholder sprites painted in code + the 5x5 HUD font
//! - `assets.rs`  file loading, sprite sheets, sound effects
//!
//! Everything is drawn in art pixels (960x540) into a 1920x1080 render target, so
//! each art pixel is a 2x2 block, then scaled to the window (optionally through a
//! CRT shader).

mod assets;
mod audio;
mod fx;
mod game;
mod hud;
mod level;
mod music;
mod pixel;
mod terrain;
mod tuning;
mod xm;

use assets::{Sfx, Sprites};
use game::{Game, Input, Phase};
use level::{Level, SCREEN_H, SCREEN_W};
use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation};
use macroquad::prelude::*;
use pixel::{draw_text_px, draw_text_px_centered, Placeholders};
use terrain::{RgbImg, Terrain};

fn conf() -> Conf {
    Conf {
        window_title: "Twin Karumba".to_owned(),
        window_width: 1280,
        window_height: 720,
        window_resizable: true,
        high_dpi: false,
        // opens full screen unless TK_WINDOWED is set (F11 toggles)
        fullscreen: std::env::var("TK_WINDOWED").is_err(),
        ..Default::default()
    }
}

const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying lowp vec2 uv;
varying lowp vec4 color;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
}"#;

const ADD_FRAGMENT: &str = r#"#version 100
varying lowp vec2 uv;
varying lowp vec4 color;
uniform sampler2D Texture;
void main() {
    gl_FragColor = color * texture2D(Texture, uv);
}"#;

/// Final blit: optional CRT look (curve, scanlines, glow, vignette).
const CRT_FRAGMENT: &str = r#"#version 100
precision mediump float;
varying lowp vec2 uv;
varying lowp vec4 color;
uniform sampler2D Texture;
uniform float crt;
void main() {
    vec2 q = uv;
    if (crt < 0.5) {
        gl_FragColor = vec4(texture2D(Texture, q).rgb, 1.0);
        return;
    }
    vec2 c = q * 2.0 - 1.0;
    c *= 1.0 + 0.035 * vec2(c.y * c.y, c.x * c.x);
    q = c * 0.5 + 0.5;
    if (q.x < 0.0 || q.x > 1.0 || q.y < 0.0 || q.y > 1.0) {
        gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0);
        return;
    }
    vec3 col = texture2D(Texture, q).rgb;
    // cheap phosphor glow: blur a few taps and add the bright part
    vec2 px = vec2(1.0 / 960.0, 1.0 / 540.0);
    vec3 b = texture2D(Texture, q + vec2(px.x * 2.0, 0.0)).rgb + texture2D(Texture, q - vec2(px.x * 2.0, 0.0)).rgb
           + texture2D(Texture, q + vec2(0.0, px.y * 2.0)).rgb + texture2D(Texture, q - vec2(0.0, px.y * 2.0)).rgb;
    col += max(b * 0.25 - 0.45, 0.0) * 0.8;
    float scan = 0.78 + 0.22 * sin(q.y * 540.0 * 6.28318);
    col *= scan;
    // aperture grille
    float m = mod(gl_FragCoord.x, 3.0);
    col *= vec3(m < 1.0 ? 1.08 : 0.94, m >= 1.0 && m < 2.0 ? 1.08 : 0.94, m >= 2.0 ? 1.08 : 0.94);
    col *= 1.0 - 0.35 * dot(c * 0.7, c * 0.7);
    gl_FragColor = vec4(col * 1.12, 1.0);
}"#;

enum Screen {
    Title { sel: usize, t: f32 },
    Play { game: Game, paused: bool },
}

/// Keyboard and mouse mapping. In 1-player mode every scheme flies P1.
fn read_inputs(two: bool, mouse_mode: bool, mouse_pos: Vec2) -> [Input; 2] {
    let axis = |neg: &[KeyCode], pos: &[KeyCode]| {
        let n = neg.iter().any(|&k| is_key_down(k));
        let p = pos.iter().any(|&k| is_key_down(k));
        (p as i32 - n as i32) as f32
    };
    let any = |ks: &[KeyCode]| ks.iter().any(|&k| is_key_down(k));
    // P1: WASD, fire Space/F, bomb LeftShift/G  (+ arrows, Z, X when playing alone)
    // P2: arrows, fire RightCtrl/Enter/K, bomb RightShift/L
    let mut p1 = Input {
        mx: axis(&[KeyCode::A], &[KeyCode::D]),
        my: axis(&[KeyCode::W], &[KeyCode::S]),
        fire: any(&[KeyCode::Space, KeyCode::F]),
        bomb: any(&[KeyCode::LeftShift, KeyCode::G]),
        crane: is_key_pressed(KeyCode::E),
        mouse: None,
    };
    let p2 = Input {
        mx: axis(&[KeyCode::Left], &[KeyCode::Right]),
        my: axis(&[KeyCode::Up], &[KeyCode::Down]),
        fire: any(&[KeyCode::RightControl, KeyCode::Enter, KeyCode::K, KeyCode::KpEnter]),
        bomb: any(&[KeyCode::RightShift, KeyCode::L, KeyCode::Kp0]),
        crane: is_key_pressed(KeyCode::Slash) || is_key_pressed(KeyCode::RightAlt) || is_key_pressed(KeyCode::KpDecimal),
        mouse: None,
    };
    if !two {
        p1.mx = (p1.mx + p2.mx).clamp(-1.0, 1.0);
        p1.my = (p1.my + p2.my).clamp(-1.0, 1.0);
        p1.fire |= p2.fire || is_key_down(KeyCode::Z) || is_key_down(KeyCode::LeftControl);
        p1.bomb |= p2.bomb || is_key_down(KeyCode::X) || is_key_down(KeyCode::C);
        p1.crane |= p2.crane || is_key_pressed(KeyCode::H);
    }
    if mouse_mode {
        p1.mouse = Some(mouse_pos);
        p1.fire |= is_mouse_button_down(MouseButton::Left);
        p1.bomb |= is_mouse_button_down(MouseButton::Right);
        p1.crane |= is_mouse_button_pressed(MouseButton::Middle);
    }
    if two { [p1, p2] } else { [p1, Input::default()] }
}

/// Simple autopilot used for automated screenshots/testing (TK_AUTOPILOT=1).
fn autopilot(g: &Game, level: &Level) -> Input {
    let p = &g.players[0];
    if !p.alive {
        return Input::default();
    }
    let look = p.x + 140.0;
    let mut lo = f32::MAX;
    let mut hi: f32 = 0.0;
    let mut x = p.x - 70.0;
    while x < look {
        lo = lo.min(level.ground(x));
        hi = hi.max(level.ceil(x).max(game::HUD_H));
        x += 6.0;
    }
    let mut target = ((lo + hi) / 2.0).min(lo - 70.0).max(hi + 50.0);
    // dip down to the crane height over a pilot, cache or crane fuel tank just ahead
    if let Some(s) = g.sites.iter().find(|s| !s.done && s.x > p.x - 20.0 && s.x < p.x + 160.0) {
        target = s.y - 60.0;
    }
    let my = ((target - p.y) / 30.0).clamp(-1.0, 1.0);
    let mx = ((g.cam_x + 330.0 - p.x) / 60.0).clamp(-1.0, 1.0);
    Input { mx, my, fire: true, bomb: (g.time * 2.0) as i32 % 2 == 0, crane: true, mouse: None }
}

fn hiscore_path() -> std::path::PathBuf {
    // a single-file build keeps the high score next to the executable
    if assets::embedded() {
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf())) {
            return dir.join("hiscore.txt");
        }
    }
    assets::asset_dir().join("..").join("hiscore.txt")
}

#[macroquad::main(conf)]
async fn main() {
    // test: TK_RENDER_XM=song.xm:out.raw:seconds writes the song as raw 16-bit stereo and quits
    if let Ok(spec) = std::env::var("TK_RENDER_XM") {
        let parts: Vec<&str> = spec.split(':').collect();
        let module = xm::Module::parse(&std::fs::read(parts[0]).expect("read xm")).expect("parse xm");
        let mut p = xm::Player::new(std::sync::Arc::new(module), audio::RATE);
        let secs: f32 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(30.0);
        // TK_RENDER_MUTE=6,7,8,9 silences those channels (e.g. the extra in-game leads)
        for ch in std::env::var("TK_RENDER_MUTE").unwrap_or_default().split(',').filter_map(|c| c.trim().parse().ok()) {
            p.snap_channel_gain(ch, 0.0);
        }
        let mut buf = vec![0f32; (secs * audio::RATE as f32) as usize * 2];
        p.mix(&mut buf);
        let bytes: Vec<u8> = buf.iter().flat_map(|x| (((x * 0.5).tanh() * 32000.0) as i16).to_le_bytes()).collect();
        std::fs::write(parts[1], bytes).expect("write");
        return;
    }
    // ---------------- load ----------------
    let tuning = tuning::Tuning::parse(&assets::read_text("tuning.json"));
    let cfg = tuning::Cfg::from(&tuning);
    let level = Level::parse(&assets::read_text("levels/level1.txt"));
    let sprites = Sprites::load();
    // title logo: "chrome" (default) or "sunset", set by title.logo in tuning.json
    let logo = {
        let style = if tuning.s("title.logo", "chrome") == "sunset" { "sunset" } else { "chrome" };
        Logo {
            tex: assets::texture(&format!("sprites/logo-{style}.png")),
            flash: assets::texture(&format!("sprites/logo-{style}-flash.png")),
        }
    };
    let ph: Placeholders = pixel::make_placeholders();
    let glow = pixel::glow_texture();
    let mut land_tex = Vec::new();
    let mut bgs = Vec::new();
    for (p, name) in level::PALETTES.iter().enumerate() {
        land_tex.push((0..3).map(|v| RgbImg::from_png(&assets::read(&terrain::tex_name(p, v)))).collect());
        let l = |k: &str| assets::texture(&format!("land/bg-{name}-{k}.png"));
        bgs.push([l("sky"), l("far"), l("mid")]);
    }
    let mut terrain = Terrain::new(land_tex, bgs);
    let mut sfx = Sfx::load();
    eprintln!("sound output: {}", sfx.audio.backend);
    let mut music = music::Director::load(
        &sfx.audio,
        &tuning.s("music.p2_lead", "flute"),
        &tuning.s("music.level2_song", "fifth-rotor"),
    );
    music.title(&sfx.audio);
    let mut music_on = std::env::var("TK_MUTE").is_err();
    sfx.audio.set_master(if music_on { 1.0 } else { 0.0 });

    let add = load_material(
        ShaderSource::Glsl { vertex: VERTEX, fragment: ADD_FRAGMENT },
        MaterialParams {
            pipeline_params: PipelineParams {
                color_blend: Some(BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::SourceAlpha), BlendFactor::One)),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .expect("additive shader");
    let crt = load_material(
        ShaderSource::Glsl { vertex: VERTEX, fragment: CRT_FRAGMENT },
        MaterialParams { uniforms: vec![UniformDesc::new("crt", UniformType::Float1)], ..Default::default() },
    )
    .expect("crt shader");

    let rt = render_target(1920, 1080);
    rt.texture.set_filter(FilterMode::Nearest);

    let mut hiscore: i32 = std::fs::read_to_string(hiscore_path()).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(10000);
    let mut screen = Screen::Title { sel: 0, t: 0.0 };
    // CRT mode is on by default; TK_CRT=0 starts without it (V or F2 toggles)
    let mut crt_on = std::env::var("TK_CRT").map_or(true, |v| v != "0");
    let mut full = std::env::var("TK_WINDOWED").is_err();
    let mut mouse_mode = false;
    let mut title_cam = 0.0f32;
    let mut rotor: Option<u64> = None;

    // test hooks: TK_AUTOPILOT, TK_SHOTS="t:path,t:path", TK_QUIT=seconds, TK_PLAY=1|2
    let autopilot_on = std::env::var("TK_AUTOPILOT").is_ok();
    let mut shots: Vec<(f32, String)> = std::env::var("TK_SHOTS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| s.split_once(':').and_then(|(t, p)| t.parse().ok().map(|t| (t, p.to_string()))))
        .collect();
    let quit_at: Option<f32> = std::env::var("TK_QUIT").ok().and_then(|s| s.parse().ok());
    if let Ok(n) = std::env::var("TK_PLAY") {
        screen = Screen::Play { game: Game::new(cfg.clone(), &level, n == "2", hiscore), paused: false };
    }
    let fixed_dt = std::env::var("TK_FIXED_DT").is_ok();
    let mut clock = 0.0f32;

    loop {
        let dt = if fixed_dt { 1.0 / 60.0 } else { get_frame_time().min(1.0 / 30.0) };
        clock += dt;

        // ---------- global keys ----------
        if is_key_pressed(KeyCode::F11) || (is_key_pressed(KeyCode::Enter) && is_key_down(KeyCode::LeftAlt)) {
            full = !full;
            set_fullscreen(full);
        }
        if is_key_pressed(KeyCode::F2) || is_key_pressed(KeyCode::V) {
            crt_on = !crt_on;
        }
        if is_key_pressed(KeyCode::M) {
            mouse_mode = !mouse_mode;
        }
        if is_key_pressed(KeyCode::N) {
            music_on = !music_on; // N = all sound on/off
            sfx.audio.set_master(if music_on { 1.0 } else { 0.0 });
        }

        // mouse position in art px
        let (sw, sh) = (screen_width(), screen_height());
        let scale = (sw / SCREEN_W).min(sh / SCREEN_H);
        let (ox, oy) = ((sw - SCREEN_W * scale) / 2.0, (sh - SCREEN_H * scale) / 2.0);
        let (mx, my) = mouse_position();
        let mouse_art = vec2((mx - ox) / scale, (my - oy) / scale);

        // ---------- update ----------
        let mut next: Option<Screen> = None;
        match &mut screen {
            Screen::Title { sel, t } => {
                *t += dt;
                title_cam += 45.0 * dt;
                if is_key_pressed(KeyCode::Up) || is_key_pressed(KeyCode::W) {
                    *sel = 0;
                }
                if is_key_pressed(KeyCode::Down) || is_key_pressed(KeyCode::S) {
                    *sel = 1;
                }
                if is_key_pressed(KeyCode::Key1) {
                    *sel = 0;
                }
                if is_key_pressed(KeyCode::Key2) {
                    *sel = 1;
                }
                let start = [KeyCode::Space, KeyCode::Enter, KeyCode::Z, KeyCode::F, KeyCode::Key1, KeyCode::Key2, KeyCode::KpEnter, KeyCode::RightControl]
                    .iter()
                    .any(|&k| is_key_pressed(k))
                    || is_mouse_button_pressed(MouseButton::Left);
                if start && *t > 0.4 {
                    sfx.play("checkpoint", 0.7);
                    next = Some(Screen::Play { game: Game::new(cfg.clone(), &level, *sel == 1, hiscore), paused: false });
                }
                if is_key_pressed(KeyCode::Escape) {
                    break;
                }
            }
            Screen::Play { game, paused } => {
                if is_key_pressed(KeyCode::P) || is_key_pressed(KeyCode::Pause) {
                    *paused = !*paused;
                }
                if is_key_pressed(KeyCode::Escape) {
                    next = Some(Screen::Title { sel: 0, t: 0.0 });
                }
                // debug: F3 swaps to level 2's music (Fifth Rotor) and back
                if is_key_pressed(KeyCode::F3) {
                    music.level = if music.level == 1 { 2 } else { 1 };
                }
                // debug: 1-9 (top row or keypad) jump to that section
                let digits = [
                    (KeyCode::Key1, KeyCode::Kp1), (KeyCode::Key2, KeyCode::Kp2), (KeyCode::Key3, KeyCode::Kp3),
                    (KeyCode::Key4, KeyCode::Kp4), (KeyCode::Key5, KeyCode::Kp5), (KeyCode::Key6, KeyCode::Kp6),
                    (KeyCode::Key7, KeyCode::Kp7), (KeyCode::Key8, KeyCode::Kp8), (KeyCode::Key9, KeyCode::Kp9),
                ];
                for (i, &(a, b)) in digits.iter().enumerate() {
                    if is_key_pressed(a) || is_key_pressed(b) {
                        game.jump_to_section(i + 1, &level);
                    }
                }
                if !*paused {
                    let mut inputs = read_inputs(game.players[1].joined, mouse_mode, mouse_art);
                    if autopilot_on {
                        inputs[0] = autopilot(game, &level);
                    }
                    // P2 drops in by pressing their fire key
                    if !game.players[1].joined
                        && [KeyCode::RightControl, KeyCode::Enter, KeyCode::K, KeyCode::KpEnter].iter().any(|&k| is_key_pressed(k))
                    {
                        game.join_p2(&level);
                    }
                    game.update(&level, inputs, dt);
                    game.play_sounds(&mut sfx);
                    hiscore = hiscore.max(game.hiscore);
                    match game.phase {
                        Phase::Clear(t) if t > 9.0 => next = Some(Screen::Title { sel: 0, t: 0.0 }),
                        Phase::Over(t) if t > 4.0 => next = Some(Screen::Title { sel: 0, t: 0.0 }),
                        _ => {}
                    }
                }
            }
        }
        if let Some(n) = next {
            if matches!(n, Screen::Title { .. }) {
                let _ = std::fs::write(hiscore_path(), hiscore.to_string());
                music.title(&sfx.audio);
            }
            screen = n;
        }
        if let Screen::Play { game, paused } = &screen {
            music.update(&sfx.audio, game, &level, *paused);
        }
        // rotor hum while flying
        let flying = matches!(&screen, Screen::Play { paused: false, .. });
        if flying && rotor.is_none() {
            rotor = sfx.play_loop("rotor-loop", 0.12);
        } else if !flying {
            if let Some(r) = rotor.take() {
                sfx.stop(r);
            }
        }
        sfx.end_frame();

        // ---------- draw into the 1920x1080 target, in art px ----------
        let cam_x = match &screen {
            Screen::Play { game, .. } => game.cam_x,
            Screen::Title { .. } => title_cam % (level.width - SCREEN_W * 2.0),
        };
        let shake = match &screen {
            Screen::Play { game, .. } => game.fx.shake,
            _ => 0.0,
        };
        let (sx, sy) = if shake > 0.0 { (rand::gen_range(-shake, shake) * 0.5, rand::gen_range(-shake, shake) * 0.5) } else { (0.0, 0.0) };
        // snap the camera to half art pixels (= whole screen pixels at 1080p)
        let cam_snap = (cam_x * 2.0).round() / 2.0;
        terrain.prepare(&level, cam_snap - 64.0, cam_snap + SCREEN_W + 64.0);
        let mut world_cam = Camera2D::from_display_rect(Rect::new(cam_snap + sx, sy, SCREEN_W, SCREEN_H));
        world_cam.render_target = Some(rt.clone());
        world_cam.zoom.y = -world_cam.zoom.y; // render targets are stored bottom-up
        set_camera(&world_cam);
        clear_background(BLACK);
        terrain.draw_sky(&level, cam_snap);
        terrain.draw_land(cam_snap);
        match &screen {
            Screen::Play { game, .. } => game.draw_world(&level, &sprites, &ph, &glow, &add),
            Screen::Title { t, .. } => {
                // two KX-2s hovering over the scrolling land
                for i in 0..2 {
                    let x = cam_snap + 200.0 + i as f32 * 90.0 + (t * 0.7 + i as f32).sin() * 20.0;
                    let y = 330.0 + i as f32 * 60.0 + (t * 1.3 + i as f32 * 2.0).sin() * 8.0;
                    sprites.kx2[i].draw(0, ((t * 24.0) as usize) % 8, x, y, 0.8, 0.03 * (t + i as f32).sin(), false, WHITE);
                }
            }
        }

        // ---------- screen-space overlay (HUD, text) ----------
        let mut ui_cam = Camera2D::from_display_rect(Rect::new(0.0, 0.0, SCREEN_W, SCREEN_H));
        ui_cam.render_target = Some(rt.clone());
        ui_cam.zoom.y = -ui_cam.zoom.y;
        set_camera(&ui_cam);
        match &screen {
            Screen::Play { game, paused } => {
                hud::draw(game, &sprites, clock);
                game.draw_message();
                if *paused {
                    draw_rectangle(0.0, 0.0, SCREEN_W, SCREEN_H, Color::new(0.0, 0.0, 0.05, 0.5));
                    draw_text_px_centered("PAUSED", SCREEN_W / 2.0, 220.0, 6.0, WHITE, Some(BLACK));
                    draw_text_px_centered("P RESUME   ESC TITLE   V CRT   M MOUSE   N SOUND", SCREEN_W / 2.0, 290.0, 2.0, WHITE, None);
                }
            }
            Screen::Title { sel, t } => draw_title(*sel, *t, hiscore, &sprites, &logo, mouse_mode, crt_on),
        }

        // ---------- present ----------
        set_default_camera();
        clear_background(BLACK);
        gl_use_material(&crt);
        crt.set_uniform("crt", if crt_on { 1.0f32 } else { 0.0f32 });
        draw_texture_ex(&rt.texture, ox, oy, WHITE, DrawTextureParams { dest_size: Some(vec2(SCREEN_W * scale, SCREEN_H * scale)), ..Default::default() });
        gl_use_default_material();

        // automated screenshots
        if let Some(i) = shots.iter().position(|(t, _)| clock >= *t) {
            let (_, path) = shots.remove(i);
            // render target rows are stored bottom-up; flip and make opaque for the PNG
            let mut img = rt.texture.get_texture_data();
            let (w, h) = (img.width as usize, img.height as usize);
            let mut out = img.bytes.clone();
            for y in 0..h {
                let src = &img.bytes[(h - 1 - y) * w * 4..(h - y) * w * 4];
                out[y * w * 4..(y + 1) * w * 4].copy_from_slice(src);
            }
            for a in out.iter_mut().skip(3).step_by(4) {
                *a = 255;
            }
            img.bytes = out;
            img.export_png(&path);
        }
        if quit_at.is_some_and(|q| clock >= q) {
            break;
        }
        next_frame().await;
    }
    let _ = std::fs::write(hiscore_path(), hiscore.to_string());
}

/// The title logo picture plus a white copy for the light sweep.
struct Logo {
    tex: Texture2D,
    flash: Texture2D,
}

fn draw_title(sel: usize, t: f32, hiscore: i32, sp: &Sprites, logo: &Logo, mouse_mode: bool, crt_on: bool) {
    let cx = SCREEN_W / 2.0;
    draw_rectangle(0.0, 0.0, SCREEN_W, SCREEN_H, Color::new(0.02, 0.01, 0.06, 0.35));
    draw_text_px_centered(&format!("HI {:07}", hiscore), cx, 30.0, 2.0, Color::new(1.0, 0.9, 0.6, 1.0), None);
    // logo picture (title.logo in tuning.json: chrome or sunset) with a light sweep
    let (lw, lh) = (logo.tex.width(), logo.tex.height());
    let (lx, ly) = ((cx - lw / 2.0).round(), (98.0 - lh / 2.0).round());
    draw_texture(&logo.tex, lx, ly, WHITE);
    // the sweep: thin slices of the white copy, brightest in the middle of the band
    let sweep = ((t * 0.5) % 1.5) * (lw + 120.0) - 60.0;
    let mut sx = (sweep - 40.0).max(0.0);
    while sx < (sweep + 40.0).min(lw) {
        let a = (1.0 - (sx - sweep).abs() / 40.0) * 0.55;
        let src = Rect::new(sx, 0.0, 2.0, lh);
        let params = DrawTextureParams { source: Some(src), dest_size: Some(vec2(2.0, lh)), ..Default::default() };
        draw_texture_ex(&logo.flash, lx + sx, ly, Color::new(1.0, 1.0, 1.0, a), params);
        sx += 2.0;
    }
    draw_text_px_centered("TANDEM ROTOR ASSAULT", cx, 135.0, 2.0, Color::new(1.0, 0.82, 0.3, 1.0), Some(Color::new(0.1, 0.03, 0.1, 1.0)));
    // menu with a torpedo cursor
    for (i, item) in ["1 PLAYER", "2 PLAYERS"].iter().enumerate() {
        let y = 190.0 + i as f32 * 30.0;
        let c = if i == sel { WHITE } else { Color::new(0.6, 0.6, 0.75, 1.0) };
        draw_text_px(item, cx - 50.0, y, 3.0, c, Some(Color::new(0.05, 0.03, 0.12, 1.0)));
        if i == sel {
            sp.torpedo[0].draw(0, 0, cx - 80.0 + (t * 6.0).sin() * 3.0, y + 7.0, 0.8, 0.0, false, WHITE);
        }
    }
    if ((t * 2.0) as i32) % 2 == 0 {
        draw_text_px_centered("PRESS FIRE", cx, 266.0, 2.0, Color::new(1.0, 1.0, 0.6, 1.0), None);
    }
    let hint = [
        "1P  ARROWS OR WASD  FIRE Z SPACE  BOMB X SHIFT  (HOLD BOTH)",
        "2P  P1 WASD SPACE LSHIFT   P2 ARROWS RCTRL RSHIFT   P2 CAN JOIN ANY TIME",
        &format!("M MOUSE {}   V CRT {}   N SOUND   P PAUSE   F11 FULL SCREEN   ESC QUIT", if mouse_mode { "ON" } else { "OFF" }, if crt_on { "ON" } else { "OFF" }),
    ];
    for (i, h) in hint.iter().enumerate() {
        draw_text_px_centered(h, cx, 486.0 + i as f32 * 12.0, 1.0, Color::new(0.8, 0.85, 1.0, 1.0), Some(Color::new(0.02, 0.02, 0.06, 1.0)));
    }
}
