//! Effects: layered explosions, sparks, smoke, glow flashes and screen shake.
//! Explosions follow the brief: several sprites per blast with different sizes, delays
//! and fade times, then sparks, smoke and debris; big ones shake the screen.

use macroquad::prelude::*;
use macroquad::rand::gen_range;

#[derive(Clone, Copy, PartialEq)]
pub enum AnimKind {
    Explosion,
    GroundBlast,
    Flak,
    Muzzle(usize), // row
    Spark,
}

pub struct Anim {
    pub kind: AnimKind,
    pub x: f32,
    pub y: f32,
    pub t: f32, // negative = waiting (delay)
    pub frame_time: f32,
    pub scale: f32,
    pub rot: f32,
}

pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub life: f32,
    pub max: f32,
    pub size: f32,
    pub color: Color,
    pub glow: bool,  // additive
    pub gravity: f32,
}

/// Additive glow flash (soft light that fades out).
pub struct Flash {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub life: f32,
    pub max: f32,
    pub color: Color,
}

#[derive(Default)]
pub struct Fx {
    pub anims: Vec<Anim>,
    pub parts: Vec<Particle>,
    pub flashes: Vec<Flash>,
    pub shake: f32,
}

impl Fx {
    pub fn update(&mut self, dt: f32) {
        for a in &mut self.anims {
            a.t += dt;
        }
        // `retain` drops finished animations (8 frames for most sheets)
        self.anims.retain(|a| {
            let n = match a.kind {
                AnimKind::Flak => 6.0,
                AnimKind::Muzzle(_) | AnimKind::Spark => 3.0,
                _ => 8.0,
            };
            a.t < a.frame_time * n
        });
        for p in &mut self.parts {
            p.vy += p.gravity * dt;
            p.vx *= 1.0 - 1.5 * dt;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.life -= dt;
        }
        self.parts.retain(|p| p.life > 0.0);
        for f in &mut self.flashes {
            f.life -= dt;
        }
        self.flashes.retain(|f| f.life > 0.0);
        self.shake = (self.shake - dt * 18.0).max(0.0);
    }

    pub fn anim(&mut self, kind: AnimKind, x: f32, y: f32, delay: f32, frame_time: f32, scale: f32) {
        self.anims.push(Anim { kind, x, y, t: -delay, frame_time, scale, rot: 0.0 });
    }

    pub fn flash(&mut self, x: f32, y: f32, r: f32, life: f32, color: Color) {
        self.flashes.push(Flash { x, y, r, life, max: life, color });
    }

    pub fn sparks(&mut self, x: f32, y: f32, n: usize, speed: f32, color: Color) {
        for _ in 0..n {
            let a = gen_range(0.0, std::f32::consts::TAU);
            let s = gen_range(0.3, 1.0) * speed;
            let life = gen_range(0.2, 0.6);
            self.parts.push(Particle {
                x,
                y,
                vx: a.cos() * s,
                vy: a.sin() * s,
                life,
                max: life,
                size: gen_range(1.0, 2.5),
                color,
                glow: true,
                gravity: 300.0,
            });
        }
    }

    /// Layered explosion: `size` 1.0 = enemy, 2.0+ = big.
    pub fn explode(&mut self, x: f32, y: f32, size: f32) {
        let s = size;
        self.anim(AnimKind::Explosion, x, y, 0.0, 0.05, 0.9 * s);
        for k in 0..(2 + (s * 1.5) as usize) {
            let (ox, oy) = (gen_range(-14.0, 14.0) * s, gen_range(-12.0, 12.0) * s);
            self.anim(AnimKind::Explosion, x + ox, y + oy, 0.05 + k as f32 * 0.06, gen_range(0.045, 0.07), gen_range(0.5, 0.8) * s);
        }
        self.flash(x, y, 50.0 * s, 0.35, Color::new(1.0, 0.7, 0.3, 0.9));
        self.sparks(x, y, (10.0 * s) as usize, 260.0 * s.sqrt(), Color::new(1.0, 0.8, 0.4, 1.0));
        // smoke puffs
        for _ in 0..(4.0 * s) as usize {
            let life = gen_range(0.6, 1.2);
            self.parts.push(Particle {
                x: x + gen_range(-10.0, 10.0) * s,
                y: y + gen_range(-8.0, 8.0) * s,
                vx: gen_range(-30.0, 30.0),
                vy: gen_range(-50.0, -15.0),
                life,
                max: life,
                size: gen_range(4.0, 8.0) * s.sqrt(),
                color: Color::new(0.25, 0.2, 0.3, 0.6),
                glow: false,
                gravity: -10.0,
            });
        }
        // chunky debris
        for _ in 0..(5.0 * s) as usize {
            let a: f32 = gen_range(-2.8, -0.3);
            let sp = gen_range(120.0, 300.0);
            let life = gen_range(0.5, 1.0);
            self.parts.push(Particle {
                x,
                y,
                vx: a.cos() * sp,
                vy: a.sin() * sp,
                life,
                max: life,
                size: 2.0,
                color: Color::new(0.35, 0.3, 0.4, 1.0),
                glow: false,
                gravity: 700.0,
            });
        }
        if s >= 1.5 {
            self.shake = self.shake.max(4.0 * s);
        }
    }

    /// Normal-blend particles (smoke, debris).
    pub fn draw_solid(&self) {
        for p in self.parts.iter().filter(|p| !p.glow) {
            let k = p.life / p.max;
            let mut c = p.color;
            c.a *= k.min(1.0);
            let s = (p.size * (if p.gravity < 0.0 { 1.0 + (1.0 - k) } else { 1.0 })).round().max(1.0);
            draw_rectangle((p.x - s / 2.0).round(), (p.y - s / 2.0).round(), s, s, c);
        }
    }

    /// Additive particles and flashes (call with the additive material active).
    pub fn draw_glow(&self, glow: &Texture2D) {
        for f in &self.flashes {
            let k = f.life / f.max;
            let mut c = f.color;
            c.a *= k;
            let r = f.r * (1.2 - 0.4 * k);
            draw_texture_ex(glow, f.x - r, f.y - r, c, DrawTextureParams { dest_size: Some(vec2(r * 2.0, r * 2.0)), ..Default::default() });
        }
        for p in self.parts.iter().filter(|p| p.glow) {
            let k = p.life / p.max;
            let mut c = p.color;
            c.a *= k;
            let s = p.size.round().max(1.0);
            draw_rectangle((p.x - s / 2.0).round(), (p.y - s / 2.0).round(), s, s, c);
            let r = s * 3.0;
            c.a *= 0.35;
            draw_texture_ex(glow, p.x - r, p.y - r, c, DrawTextureParams { dest_size: Some(vec2(r * 2.0, r * 2.0)), ..Default::default() });
        }
    }
}
