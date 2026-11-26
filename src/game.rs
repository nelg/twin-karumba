//! The game world: players, enemies, shots, bombs, pickups and level flow.
//!
//! Coordinates are WORLD art pixels: x runs along the level (0 .. ~18900), y is
//! 0 at the top of the screen and 540 at the bottom. The camera's left edge is
//! `cam_x`, which the forced scroll pushes right.
//!
//! Rust note: the world owns everything in `Vec`s. When one thing needs to change
//! another (a bullet killing an enemy), we loop with indices and collect "events"
//! (kills, sounds) to apply afterwards. That keeps the borrow checker happy without
//! needing shared pointers.

use crate::assets::{Sfx, Sprites};
use crate::fx::{AnimKind, Fx};
use crate::level::{Level, SCREEN_H, SCREEN_W, TILE};
use crate::pixel::{draw_text_px_centered, Placeholders};
use crate::tuning::Cfg;
use macroquad::prelude::*;
use macroquad::rand::gen_range;
use std::f32::consts::PI;

pub const HUD_H: f32 = 23.0;

// Points on the KX-2 relative to the frame centre, in art px at scale 1.0
// (measured from kx2-p1-px-sheet frame 0; multiply by cfg.scale).
const GUN_TIP: Vec2 = vec2(65.0, 17.0);
const BOMB_MOUNT: Vec2 = vec2(-9.5, 24.0); // px-meta kx2.bomb_mount.torpedo_centre
const BELLY: Vec2 = vec2(-10.0, 18.0);
const HITBOX_CENTRE: Vec2 = vec2(14.0, -3.0);
const LIVES_DIGIT: Vec2 = vec2(32.5, -10.5); // px-meta kx2.lives_digit

// ------------------------------------------------------------------ input

#[derive(Default, Clone, Copy)]
pub struct Input {
    pub mx: f32, // -1..1
    pub my: f32,
    pub fire: bool,
    pub bomb: bool,
    /// Crane button pressed this frame (lowers the hook over a pilot or ground item).
    pub crane: bool,
    /// Mouse mode: the heli chases this point (art px, screen space).
    pub mouse: Option<Vec2>,
}

// ------------------------------------------------------------------ players

#[derive(Clone, Copy, PartialEq)]
pub enum HoistKind {
    Pilot,
    Cache,
    Fuel, // barrel stack (fuel depot option C): lifted with the crane
}

pub struct Site {
    pub kind: HoistKind,
    pub x: f32,
    pub y: f32, // ground y
    pub done: bool,
    pub lift: f32, // 0..1 how far the pilot/crate has been lifted
    pub var: usize, // art variant (pilot A/B)
}

pub struct Player {
    pub id: usize,
    pub joined: bool,
    pub out: bool, // no lives left
    pub alive: bool,
    pub x: f32,
    pub y: f32,
    pub lives: i32,
    pub score: i32,
    pub next_life: i32,
    pub fuel: f32,
    pub shield: f32,
    pub shield_flash: f32,
    pub laser: bool,
    pub falling: bool,
    pub fall_vy: f32,
    pub dead_t: f32,
    pub invuln: f32,
    pub tilt: f32, // degrees, + = nose down
    pub move_x: f32,
    pub fire_cd: f32,
    pub reload_t: f32, // time since the last bomb left the clamps
    pub muzzle_t: f32,
    pub alarm_t: f32,
    pub laser_target: Option<u32>,
    pub laser_dwell: f32,
    pub laser_end: Option<Vec2>,
    pub laser_snd: f32,
    pub hoist: Option<(usize, f32)>, // (site index, time)
    pub alpha: f32,
    pub anim: f32,
}

impl Player {
    pub fn new(id: usize, cfg: &Cfg) -> Player {
        Player {
            id,
            joined: false,
            out: false,
            alive: false,
            x: 0.0,
            y: 0.0,
            lives: cfg.lives,
            score: 0,
            next_life: cfg.life_first,
            fuel: 1.0,
            shield: 0.0,
            shield_flash: 0.0,
            laser: false,
            falling: false,
            fall_vy: 0.0,
            dead_t: 0.0,
            invuln: 0.0,
            tilt: 0.0,
            move_x: 0.0,
            fire_cd: 0.0,
            reload_t: 10.0,
            muzzle_t: 0.0,
            alarm_t: 0.0,
            laser_target: None,
            laser_dwell: 0.0,
            laser_end: None,
            laser_snd: 0.0,
            hoist: None,
            alpha: 1.0,
            anim: 0.0,
        }
    }

    /// A local point on the ship (art px at scale 1) rotated by the tilt, in world px.
    pub fn local(&self, p: Vec2, scale: f32) -> Vec2 {
        let a = self.tilt.to_radians();
        let (s, c) = a.sin_cos();
        let q = p * scale;
        vec2(self.x + q.x * c - q.y * s, self.y + q.x * s + q.y * c)
    }

    /// Hitbox as (centre, half size).
    pub fn hitbox(&self, cfg: &Cfg) -> (Vec2, Vec2) {
        (self.local(HITBOX_CENTRE, cfg.scale), vec2(cfg.hitbox_half.0, cfg.hitbox_half.1))
    }
}

// ------------------------------------------------------------------ projectiles

pub struct Bullet {
    pub owner: usize,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub dead: bool,
}

pub struct Bomb {
    pub owner: usize,
    pub x0: f32,
    pub y0: f32,
    pub vx: f32,
    pub g: f32,
    pub t: f32,
    pub fall_t: f32,
    pub phase: f32,
    pub back: bool,
    pub x: f32,
    pub y: f32,
    pub pitch: f32,
    pub dead: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ShotKind {
    Bullet,
    Shell,
    Rocket,
    Flak { burst_y: f32 },
    FlakBurst,
}

pub struct Shot {
    pub kind: ShotKind,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub accel: f32,
    pub t: f32,
    pub dead: bool,
    pub hit_mask: [bool; 2], // flak bursts hurt each player once
}

#[derive(Clone, Copy, PartialEq)]
pub enum PickKind {
    Shield = 0,
    Fuel = 1,
    Life = 2,
    Laser = 3,
}

pub struct Pickup {
    pub owner: usize,
    pub kind: PickKind,
    pub x: f32,
    pub y: f32,
    pub t: f32,
    pub dead: bool,
}

// ------------------------------------------------------------------ enemies

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Fuel,
    Rocket,
    Tank,
    Flak,
    Turret,
    Whirler,
    Chopper,
    Truck,
    Tower,
    Pad,
    Drone,
    Fighter,
    Balloon,
    Mine,
    Radar,
    Rock,
}

impl Kind {
    /// Half width / height of the hitbox in art px (from enemies.json sizes).
    fn half(self) -> (f32, f32) {
        match self {
            Kind::Fuel => (20.0, 15.0),
            Kind::Rocket => (4.0, 11.0),
            Kind::Tank => (23.0, 15.0),
            Kind::Flak => (16.0, 13.0),
            Kind::Turret => (14.0, 10.0),
            Kind::Whirler => (11.0, 11.0),
            Kind::Chopper => (30.0, 11.0),
            Kind::Truck => (25.0, 17.0),
            Kind::Tower => (18.0, 27.0),
            Kind::Pad => (20.0, 6.0),
            Kind::Drone => (14.0, 9.0),
            Kind::Fighter => (21.0, 9.0),
            Kind::Balloon => (15.0, 22.0),
            Kind::Mine => (9.0, 9.0),
            Kind::Radar => (14.0, 18.0),
            Kind::Rock => (9.0, 11.0),
        }
    }
    fn score(self) -> i32 {
        match self {
            Kind::Fuel => 150,
            Kind::Rocket => 80,
            Kind::Tank => 200,
            Kind::Flak => 300,
            Kind::Turret => 200,
            Kind::Whirler => 100,
            Kind::Chopper => 400,
            Kind::Truck => 250,
            Kind::Tower => 300,
            Kind::Pad => 150,
            Kind::Drone => 150,
            Kind::Fighter => 500,
            Kind::Balloon => 200,
            Kind::Mine => 100,
            Kind::Radar => 1000,
            Kind::Rock => 0,
        }
    }
    fn ground(self) -> bool {
        matches!(self, Kind::Fuel | Kind::Tank | Kind::Flak | Kind::Truck | Kind::Tower | Kind::Pad | Kind::Radar)
            || self == Kind::Rocket
    }
    fn ram_cost(self, cfg: &Cfg) -> f32 {
        match self {
            Kind::Balloon => cfg.ram_balloon,
            Kind::Drone => cfg.ram_drone,
            Kind::Fighter => cfg.ram_fighter,
            Kind::Whirler => cfg.ram_whirler,
            _ => cfg.shield_shot,
        }
    }
}

pub struct Enemy {
    pub id: u32,
    pub kind: Kind,
    pub x: f32,
    pub y: f32,
    pub ox: f32, // home position
    pub oy: f32,
    pub vx: f32,
    pub vy: f32,
    pub t: f32,
    pub cd: f32,
    pub cd2: f32,
    pub state: u8,
    pub n: i32,
    pub ang: f32,
    pub aim: f32,
    pub flip: bool,
    pub dead: bool,
    pub group: usize,
    pub member: usize,
    pub range: (f32, f32),
    pub elev: usize,
    pub target: usize,
    pub retarget: f32,
    pub fire_t: f32,
    pub dist: f32,
    pub var: usize, // art variant (see assets::Extras)
    pub hits: i32,  // bullet hits taken so far (armoured ground targets need cfg.armour_hits)
}

impl Enemy {
    fn new(id: u32, kind: Kind, x: f32, y: f32) -> Enemy {
        Enemy {
            id,
            kind,
            x,
            y,
            ox: x,
            oy: y,
            vx: 0.0,
            vy: 0.0,
            t: 0.0,
            cd: gen_range(0.3, 1.2),
            cd2: 0.0,
            state: 0,
            n: 0,
            ang: 0.0,
            aim: 0.0,
            flip: false,
            dead: false,
            group: 0,
            member: 0,
            range: (x, x),
            elev: 0,
            target: 0,
            retarget: 0.0,
            fire_t: 0.0,
            dist: 0.0,
            var: 0,
            hits: 0,
        }
    }
    /// Armoured targets take `cfg.armour_hits` bullets; everything else dies to one.
    /// (Bombs still kill anything in one go.)
    fn armoured(&self) -> bool {
        match self.kind {
            Kind::Tank | Kind::Truck | Kind::Turret | Kind::Tower | Kind::Flak | Kind::Radar | Kind::Pad | Kind::Rock => true,
            Kind::Rocket => self.state == 0, // still sitting on its pad
            _ => false,
        }
    }
    fn contains(&self, p: Vec2, pad: f32) -> bool {
        let (hw, hh) = self.kind.half();
        (p.x - self.x).abs() <= hw + pad && (p.y - self.y).abs() <= hh + pad
    }
}

pub struct Swarm {
    pub cx: f32,
    pub cy: f32,
    pub ut: f32,
    pub unfurled: bool,
    pub target: usize,
}

// ------------------------------------------------------------------ boss

pub struct Boss {
    pub x: f32,  // left edge of the fortress
    pub cy: f32, // vertical centre
    pub pods: [i32; 2],
    pub pod_fire: [f32; 2],
    pub core: i32,
    pub t: f32,
    pub active: bool,
    pub dying: f32, // > 0 while the chain of explosions runs
    pub dead: bool,
    pub hit_flash: f32,
}

impl Boss {
    fn pod_pos(&self, i: usize) -> Vec2 {
        vec2(self.x - 14.0, self.cy + if i == 0 { -104.0 } else { 104.0 })
    }
    fn core_pos(&self) -> Vec2 {
        vec2(self.x + 19.0, self.cy)
    }
    fn core_open(&self) -> bool {
        self.t % 5.0 > 3.0
    }
}

// ------------------------------------------------------------------ game

#[derive(Clone, Copy, PartialEq)]
pub enum Phase {
    Playing,
    Clear(f32),
    Over(f32),
}

pub struct Game {
    pub cfg: Cfg,
    pub players: [Player; 2],
    pub bullets: Vec<Bullet>,
    pub bombs: Vec<Bomb>,
    pub shots: Vec<Shot>,
    pub enemies: Vec<Enemy>,
    pub swarms: Vec<Swarm>,
    pub pickups: Vec<Pickup>,
    pub sites: Vec<Site>,
    pub boss: Option<Boss>,
    pub fx: Fx,
    pub cam_x: f32,
    pub scroll_mul: f32,
    pub spawned: Vec<bool>,
    pub swarm_of: Vec<Option<usize>>, // thing index -> whirler group number
    pub checkpoint: usize,             // section index
    pub sections: Vec<(usize, u32)>,
    pub time: f32,
    pub radar_off: Vec<(f32, f32)>, // (x, until)
    pub phase: Phase,
    pub next_id: u32,
    pub message: Option<(String, f32)>,
    pub lasers: Vec<(Vec2, Vec2, f32)>, // enemy laser beams this frame (from, to, alpha)
    pub telegraphs: Vec<Vec2>,
    pub sfx_queue: Vec<(&'static str, f32)>,
    pub hiscore: i32,
    pub god: bool,
}

impl Game {
    pub fn new(cfg: Cfg, level: &Level, two_players: bool, hiscore: i32) -> Game {
        let sections = level.sections();
        // Group whirler letters that sit close together into swarms.
        let mut swarm_of = vec![None; level.things.len()];
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for (i, t) in level.things.iter().enumerate() {
            if t.ch != 'S' {
                continue;
            }
            let found = groups.iter().position(|g| {
                g.iter().any(|&j| {
                    let o = &level.things[j];
                    (o.col as i32 - t.col as i32).abs() <= 4 && (o.row as i32 - t.row as i32).abs() <= 3
                })
            });
            match found {
                Some(g) => groups[g].push(i),
                None => groups.push(vec![i]),
            }
        }
        for (g, members) in groups.iter().enumerate() {
            for &i in members {
                swarm_of[i] = Some(g);
            }
        }
        let mut sites: Vec<Site> = level
            .things
            .iter()
            .filter(|t| t.ch == '@' || t.ch == '+')
            .map(|t| Site {
                kind: if t.ch == '@' { HoistKind::Pilot } else { HoistKind::Cache },
                x: t.x(),
                y: level.ground(t.x()),
                done: false,
                lift: 0.0,
                var: t.col % 2,
            })
            .collect();
        // Fuel depots: B (gas globe) on airstrip metal, otherwise A (shoot it); every third
        // depot off the airstrip is C, a barrel stack you lift with the crane.
        let fuel_kinds = fuel_variants(level);
        for (i, t) in level.things.iter().enumerate() {
            if fuel_kinds[i] == Some(2) {
                sites.push(Site { kind: HoistKind::Fuel, x: t.x(), y: level.ground(t.x()), done: false, lift: 0.0, var: 0 });
            }
        }
        sites.sort_by(|a, b| a.x.total_cmp(&b.x));
        let mut g = Game {
            players: [Player::new(0, &cfg), Player::new(1, &cfg)],
            cfg,
            bullets: Vec::new(),
            bombs: Vec::new(),
            shots: Vec::new(),
            enemies: Vec::new(),
            swarms: Vec::new(),
            pickups: Vec::new(),
            sites,
            boss: None,
            fx: Fx::default(),
            cam_x: 0.0,
            scroll_mul: 1.0,
            spawned: vec![false; level.things.len()],
            swarm_of,
            checkpoint: 0,
            sections,
            time: 0.0,
            radar_off: Vec::new(),
            phase: Phase::Playing,
            next_id: 1,
            message: None,
            lasers: Vec::new(),
            telegraphs: Vec::new(),
            sfx_queue: Vec::new(),
            hiscore,
            god: false,
        };
        g.players[0].joined = true;
        g.players[1].joined = two_players;
        // Optional debug start column (TK_START_SECTION=1..5) for testing.
        if let Ok(s) = std::env::var("TK_START_SECTION") {
            if let Ok(n) = s.parse::<usize>() {
                g.checkpoint = n.saturating_sub(1).min(g.sections.len() - 1);
            }
        }
        g.god = std::env::var("TK_GOD").is_ok();
        g.restart_at_checkpoint(level);
        g.players[0].laser = std::env::var("TK_LASER").is_ok(); // test: start with the laser
        g
    }

    fn sfx(&mut self, name: &'static str, vol: f32) {
        self.sfx_queue.push((name, vol));
    }

    pub fn play_sounds(&mut self, sfx: &mut Sfx) {
        for (n, v) in self.sfx_queue.drain(..) {
            sfx.play(n, v);
        }
    }

    fn checkpoint_cam(&self, level: &Level) -> f32 {
        let col = self.sections.get(self.checkpoint).map(|s| s.0).unwrap_or(0);
        (col as f32 * TILE - 60.0).clamp(0.0, level.width - SCREEN_W)
    }

    /// Clears the screen and puts the players back at the current checkpoint.
    fn restart_at_checkpoint(&mut self, level: &Level) {
        self.cam_x = self.checkpoint_cam(level);
        self.enemies.clear();
        self.swarms.clear();
        self.shots.clear();
        self.bullets.clear();
        self.bombs.clear();
        self.pickups.clear();
        self.boss = None;
        for (i, t) in level.things.iter().enumerate() {
            self.spawned[i] = t.x() < self.cam_x + 8.0;
        }
        for i in 0..2 {
            if self.players[i].joined && !self.players[i].out {
                self.respawn(i, level, 130.0 + 40.0 * i as f32);
            }
        }
        let n = self.sections.get(self.checkpoint).map(|s| s.1).unwrap_or(1);
        self.message = Some((format!("SECTION {n}"), 2.0));
    }

    fn safe_y(&self, level: &Level, x: f32) -> f32 {
        let top = level.ceil(x).max(HUD_H);
        let bot = level.ground(x);
        let mut y = (top + bot) / 2.0;
        // in open country fly a bit above the middle
        if level.ceil(x) <= 0.0 {
            y = y.min(bot - 120.0).max(top + 60.0);
        }
        y
    }

    fn respawn(&mut self, i: usize, level: &Level, sx: f32) {
        let x = self.cam_x + sx;
        let y = self.safe_y(level, x);
        let invuln = self.cfg.invuln_s;
        let p = &mut self.players[i];
        p.alive = true;
        p.falling = false;
        p.x = x;
        p.y = y;
        p.fuel = 1.0;
        p.invuln = invuln;
        p.tilt = 0.0;
        p.hoist = None;
        p.reload_t = 10.0;
        p.laser_target = None;
    }

    /// Debug: number keys 1-9 jump straight to that section (stage). Players who are
    /// out of lives come back so you can keep testing.
    pub fn jump_to_section(&mut self, n: usize, level: &Level) {
        if n == 0 || n > self.sections.len() {
            return;
        }
        self.checkpoint = n - 1;
        self.phase = Phase::Playing;
        for p in self.players.iter_mut() {
            if p.joined {
                p.out = false;
                p.lives = p.lives.max(1);
            }
        }
        self.restart_at_checkpoint(level);
    }

    pub fn join_p2(&mut self, level: &Level) {
        if !self.players[1].joined {
            self.players[1] = Player::new(1, &self.cfg);
            self.players[1].joined = true;
            self.respawn(1, level, 120.0);
            self.sfx("checkpoint", 0.6);
        }
    }

    fn current_section(&self) -> usize {
        let centre = self.cam_x + SCREEN_W / 2.0;
        self.sections.iter().rposition(|s| s.0 as f32 * TILE <= centre).unwrap_or(0)
    }

    pub fn section_info(&self) -> (usize, usize) {
        (self.current_section(), self.sections.len())
    }

    fn nearest_player(&self, x: f32, y: f32) -> Option<usize> {
        let mut best = None;
        let mut bd = f32::MAX;
        for p in &self.players {
            if p.alive && !p.falling {
                let d = (p.x - x).powi(2) + (p.y - y).powi(2);
                if d < bd {
                    bd = d;
                    best = Some(p.id);
                }
            }
        }
        best
    }

    fn new_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    // ============================================================== update

    pub fn update(&mut self, level: &Level, inputs: [Input; 2], dt: f32) {
        self.time += dt;
        self.lasers.clear();
        self.telegraphs.clear();
        if let Some((_, t)) = &mut self.message {
            *t -= dt;
            if *t <= 0.0 {
                self.message = None;
            }
        }
        match self.phase {
            Phase::Clear(t) => {
                self.phase = Phase::Clear(t + dt);
                self.fx.update(dt);
                for i in 0..2 {
                    let p = &mut self.players[i];
                    if p.alive {
                        p.x += 120.0 * dt; // fly off into the hangar
                        p.anim += dt;
                    }
                }
                return;
            }
            Phase::Over(t) => {
                self.phase = Phase::Over(t + dt);
                self.fx.update(dt);
                return;
            }
            Phase::Playing => {}
        }

        // ---- forced scroll ----
        let any_back = self.players.iter().zip(inputs.iter()).any(|(p, i)| p.alive && i.mx < -0.3);
        let target = if any_back { self.cfg.scroll_slow_back } else { 1.0 };
        let rate = (1.0 - self.cfg.scroll_slow_back) / self.cfg.scroll_recover_s.max(0.01);
        self.scroll_mul += (target - self.scroll_mul).clamp(-rate * dt, rate * dt);
        let hoisting = self.players.iter().any(|p| p.hoist.is_some());
        let mut speed = level.scroll * TILE * level.scroll_factor(self.cam_x + SCREEN_W / 2.0, self.cfg.cave_factor) * self.scroll_mul;
        if hoisting {
            speed = 0.0;
        }
        let max_cam = level.width - SCREEN_W;
        let old = self.cam_x;
        self.cam_x = (self.cam_x + speed * dt).min(max_cam);
        let scroll_dx = self.cam_x - old;

        // ---- checkpoints ----
        let sec = self.current_section();
        if sec > self.checkpoint {
            self.checkpoint = sec;
            let n = self.sections[sec].1;
            self.message = Some((format!("SECTION {n}"), 2.0));
            self.sfx("checkpoint", 0.7);
        }

        // ---- players ----
        for i in 0..2 {
            self.update_player(level, i, inputs[i], scroll_dx, dt);
        }

        self.spawn(level);
        self.update_swarms(dt);
        self.update_enemies(level, dt);
        self.update_boss(level, dt);
        self.update_bullets(level, dt);
        self.update_bombs(level, dt);
        self.update_shots(level, dt);
        self.update_pickups(dt);
        self.collide_players(level);
        self.fx.update(dt);
        self.deaths(level, dt);

        // remove what scrolled away
        let cam = self.cam_x;
        self.enemies.retain(|e| !e.dead && e.x > cam - 200.0 && e.x < cam + SCREEN_W + 900.0 && e.y < 700.0 && e.y > -300.0);
        self.bullets.retain(|b| !b.dead);
        self.bombs.retain(|b| !b.dead);
        self.shots.retain(|s| !s.dead);
        self.pickups.retain(|p| !p.dead);
        self.radar_off.retain(|r| r.1 > self.time);
        for p in &self.players {
            self.hiscore = self.hiscore.max(p.score);
        }
    }

    fn update_player(&mut self, level: &Level, i: usize, inp: Input, scroll_dx: f32, dt: f32) {
        let cfg = self.cfg.clone();
        let cam = self.cam_x;
        let boss_limit = self.boss.as_ref().map(|b| b.x - 80.0).unwrap_or(f32::MAX);
        let p = &mut self.players[i];
        if !p.joined || !p.alive {
            return;
        }
        p.anim += dt;
        p.invuln = (p.invuln - dt).max(0.0);
        p.shield_flash = (p.shield_flash - dt).max(0.0);
        p.fire_cd -= dt;
        p.reload_t += dt;
        p.muzzle_t -= dt;
        p.x += scroll_dx;

        if p.falling {
            // out of fuel: drop like a stone, nose down
            p.fall_vy += 500.0 * dt;
            p.y += p.fall_vy * dt;
            p.tilt = (p.tilt + 60.0 * dt).min(40.0);
            return;
        }

        // input -> movement (direct, no inertia)
        let (mut mx, mut my) = (inp.mx, inp.my);
        if let Some(m) = inp.mouse {
            let d = vec2(m.x + cam - p.x, m.y - p.y);
            let len = d.length();
            if len > 4.0 {
                let sp = (len * 6.0).min(cfg.mouse_speed);
                mx = d.x / len * sp / cfg.speed_x;
                my = d.y / len * sp / cfg.speed_y;
            } else {
                mx = 0.0;
                my = 0.0;
            }
        }
        if p.hoist.is_some() {
            mx *= 0.4;
            my *= 0.4;
        }
        p.x += mx * cfg.speed_x * dt;
        p.y += my * cfg.speed_y * dt;
        p.move_x = mx;
        let half_w = 64.0 * cfg.scale;
        let fwd = (cam + SCREEN_W * cfg.forward_limit).min(boss_limit);
        p.x = p.x.clamp(cam + half_w, fwd.max(cam + half_w));
        p.y = p.y.clamp(HUD_H + 26.0 * cfg.scale, SCREEN_H - 10.0);
        // visual tilt eases in and out
        let target = mx.clamp(-1.0, 1.0) * cfg.tilt_deg;
        p.tilt += (target - p.tilt) * (dt * 10.0).min(1.0);

        // fuel never stops falling
        p.fuel -= dt / cfg.fuel_tank_s;
        if p.fuel < cfg.fuel_low {
            p.alarm_t -= dt;
            if p.alarm_t <= 0.0 {
                p.alarm_t = 1.0;
                self.sfx_queue.push(("fuel-low", 0.5));
            }
        }
        if p.fuel <= 0.0 && !self.god {
            p.fuel = 0.0;
            p.falling = true;
            p.fall_vy = 0.0;
            p.hoist = None;
            self.sfx_queue.push(("shield-down", 0.6));
            return;
        }
        if self.god {
            p.fuel = p.fuel.max(0.3);
        }

        // ---- cannon / laser ----
        let tip = p.local(GUN_TIP, cfg.scale);
        p.laser_end = None;
        if inp.fire && p.laser {
            // laser: instant beam straight ahead to the first target or terrain
            let mut end_x = cam + SCREEN_W;
            let mut x = tip.x;
            while x < end_x {
                if level.solid(x, tip.y) {
                    end_x = x;
                    break;
                }
                x += 3.0;
            }
            let mut hit: Option<(usize, f32)> = None;
            for (k, e) in self.enemies.iter().enumerate() {
                if e.dead {
                    continue;
                }
                let (hw, hh) = e.kind.half();
                if (e.y - tip.y).abs() <= hh + 2.0 && e.x + hw > tip.x && e.x - hw < end_x {
                    let ex = (e.x - hw).max(tip.x);
                    if hit.map_or(true, |h| ex < h.1) {
                        hit = Some((k, ex));
                    }
                }
            }
            let p = &mut self.players[i];
            if let Some((k, ex)) = hit {
                end_x = ex;
                let e = &self.enemies[k];
                if p.laser_target == Some(e.id) {
                    p.laser_dwell += dt;
                } else {
                    p.laser_target = Some(e.id);
                    p.laser_dwell = 0.0;
                }
                if p.laser_dwell >= cfg.laser_dwell || e.kind == Kind::Balloon {
                    p.laser_target = None;
                    self.kill_enemy(k, Some(i), level);
                }
            } else {
                p.laser_target = None;
            }
            // laser can also burn the boss core when open
            let p = &mut self.players[i];
            p.laser_end = Some(vec2(end_x, tip.y));
            p.laser_snd -= dt;
            if p.laser_snd <= 0.0 {
                p.laser_snd = 0.25;
                self.sfx_queue.push(("laser", 0.35));
            }
        } else if inp.fire && p.fire_cd <= 0.0 {
            let mine = self.bullets.iter().filter(|b| b.owner == i).count();
            if mine < cfg.max_bullets {
                let a = p.tilt.to_radians();
                self.bullets.push(Bullet { owner: i, x: tip.x, y: tip.y, vx: a.cos() * cfg.bullet_speed, vy: a.sin() * cfg.bullet_speed, dead: false });
                p.fire_cd = cfg.fire_interval;
                p.muzzle_t = 0.05;
                self.sfx_queue.push(("cannon", 0.3));
            }
        }

        // ---- bombs ----
        let p = &mut self.players[i];
        let mount_ready = p.reload_t >= cfg.reload_delay + cfg.reload_time;
        if inp.bomb && mount_ready && self.bombs.iter().filter(|b| b.owner == i).count() < cfg.bomb_max {
            let m = p.local(BOMB_MOUNT, cfg.scale);
            let v = mx.clamp(-1.0, 1.0);
            let theta = cfg.bomb_angle_stopped + (cfg.bomb_angle_forward - cfg.bomb_angle_stopped) * v.max(0.0)
                + (cfg.bomb_angle_backward - cfg.bomb_angle_stopped) * (-v).max(0.0)
                + gen_range(-cfg.bomb_jitter_angle, cfg.bomb_jitter_angle);
            let g = cfg.bomb_gravity * (1.0 + gen_range(-cfg.bomb_jitter_gravity, cfg.bomb_jitter_gravity));
            // landing point: march along the line at theta from vertical until it meets ground
            let (s, c) = theta.to_radians().sin_cos();
            let mut d = 0.0;
            let mut land = vec2(m.x, level.ground(m.x));
            while d < 900.0 {
                let q = vec2(m.x + s * d, m.y + c * d);
                if q.y >= level.ground(q.x) {
                    land = q;
                    break;
                }
                d += 4.0;
            }
            let h = (land.y - m.y).max(10.0);
            let fall_t = (2.0 * h / g).sqrt();
            self.bombs.push(Bomb {
                owner: i,
                x0: m.x,
                y0: m.y,
                vx: (land.x - m.x) / fall_t,
                g,
                t: 0.0,
                fall_t,
                phase: gen_range(0.0, PI * 2.0),
                back: land.x < m.x,
                x: m.x,
                y: m.y,
                pitch: 0.0,
                dead: false,
            });
            p.reload_t = 0.0;
            self.sfx_queue.push(("bomb-drop", 0.5));
        }

        // ---- hoist (rescue pilots / lift supply caches) ----
        let p = &mut self.players[i];
        let belly = p.local(BELLY, cfg.scale);
        match p.hoist {
            Some((s, t)) => {
                let site = &mut self.sites[s];
                let t = t + dt;
                site.lift = (t / cfg.hoist_time).min(1.0);
                if (belly.x - site.x).abs() > 40.0 || site.y - belly.y > cfg.hoist_height + 20.0 {
                    p.hoist = None; // flew away: cancel
                    site.lift = 0.0;
                } else if t >= cfg.hoist_time {
                    site.done = true;
                    p.hoist = None;
                    p.score += 1000;
                    match site.kind {
                        HoistKind::Pilot => {
                            p.lives += 1;
                            self.message = Some(("PILOT RESCUED  +1 LIFE".into(), 2.0));
                            self.sfx_queue.push(("extra-life", 0.8));
                        }
                        HoistKind::Cache => {
                            p.fuel = 1.0;
                            p.shield = 1.0;
                            self.message = Some(("SUPPLIES  FUEL + SHIELD".into(), 2.0));
                            self.sfx_queue.push(("fuel-up", 0.7));
                            self.sfx_queue.push(("shield-on", 0.7));
                        }
                        HoistKind::Fuel => {
                            p.fuel = 1.0;
                            self.message = Some(("FUEL LIFTED  TANK FULL".into(), 2.0));
                            self.sfx_queue.push(("fuel-up", 0.8));
                        }
                    }
                } else {
                    p.hoist = Some((s, t));
                }
            }
            None => {
                for (k, site) in self.sites.iter().enumerate() {
                    if inp.crane && !site.done && (belly.x - site.x).abs() < 30.0 && belly.y < site.y && site.y - belly.y < cfg.hoist_height {
                        p.hoist = Some((k, 0.0));
                        self.sfx_queue.push(("door", 0.6));
                        self.sfx_queue.push(("winch", 0.6));
                        break;
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------- spawning

    fn spawn(&mut self, level: &Level) {
        let edge = self.cam_x + SCREEN_W + 64.0;
        for i in 0..level.things.len() {
            if self.spawned[i] {
                continue;
            }
            let t = &level.things[i];
            if t.x() > edge {
                break; // things are sorted by column
            }
            self.spawned[i] = true;
            let (x, y) = (t.x(), t.y());
            let id = self.new_id();
            let on_ground = |k: Kind| {
                let mut e = Enemy::new(id, k, x, level.ground(x) - k.half().1);
                e.oy = e.y;
                e
            };
            let e = match t.ch {
                'F' => match fuel_variants(level)[i] {
                    Some(2) => None, // crane pickup: handled as a hoist site
                    v => {
                        let mut e = on_ground(Kind::Fuel);
                        e.var = v.unwrap_or(0);
                        Some(e)
                    }
                },
                'R' => Some(on_ground(Kind::Rocket)),
                'G' => Some(on_ground(Kind::Flak)),
                'D' => {
                    // B (bar antenna on a hut) where the base is busy, A (dish) elsewhere
                    let busy = level.things.iter().filter(|o| (o.col as i32 - t.col as i32).abs() <= 12 && "TVWGLFR".contains(o.ch)).count();
                    let mut e = on_ground(Kind::Radar);
                    e.var = if busy >= 4 { 1 } else { 0 };
                    Some(e)
                }
                'W' => Some(on_ground(Kind::Tower)),
                'L' => Some(on_ground(Kind::Pad)),
                'T' | 'V' => {
                    let k = if t.ch == 'T' { Kind::Tank } else { Kind::Truck };
                    let mut e = on_ground(k);
                    e.range = self.drive_range(level, x);
                    Some(e)
                }
                'C' => {
                    let mut e = Enemy::new(id, Kind::Turret, x, level.ceil(x).max(TILE) + 10.0);
                    e.var = gen_range(0, 3);
                    e.aim = 2.2; // barrel pointing down and back
                    Some(e)
                }
                'K' => {
                    let mut e = Enemy::new(id, Kind::Rock, x, level.ceil(x).max(TILE) + 11.0);
                    e.var = gen_range(0, 3);
                    Some(e)
                }
                'M' => {
                    let mut e = Enemy::new(id, Kind::Mine, x, y);
                    e.var = gen_range(0, 2);
                    Some(e)
                }
                'O' => Some(Enemy::new(id, Kind::Balloon, x, y)),
                'J' => {
                    let mut e = Enemy::new(id, Kind::Fighter, x, y);
                    e.ang = PI; // heading left
                    self.sfx_queue.push(("fighter-pass", 0.5));
                    Some(e)
                }
                'H' => Some(Enemy::new(id, Kind::Chopper, x, y)),
                'S' => {
                    // spawn the whole swarm at once
                    if let Some(g) = self.swarm_of[i] {
                        let members: Vec<usize> = (0..level.things.len()).filter(|&j| self.swarm_of[j] == Some(g)).collect();
                        let cx = members.iter().map(|&j| level.things[j].x()).sum::<f32>() / members.len() as f32;
                        let cy = members.iter().map(|&j| level.things[j].y()).sum::<f32>() / members.len() as f32;
                        let si = self.swarms.len();
                        self.swarms.push(Swarm { cx, cy, ut: 0.0, unfurled: false, target: 0 });
                        for (m, &j) in members.iter().enumerate() {
                            self.spawned[j] = true;
                            let th = &level.things[j];
                            let id = self.new_id();
                            let mut e = Enemy::new(id, Kind::Whirler, th.x(), th.y());
                            e.ox = th.x() - cx;
                            e.oy = th.y() - cy;
                            e.group = si;
                            e.member = m;
                            e.flip = g % 2 == 1;
                            self.enemies.push(e);
                        }
                        self.sfx_queue.push(("swarm", 0.5));
                    }
                    None
                }
                'B' => {
                    let ground = level.ground(x);
                    let top = level.ceil(x).max(TILE);
                    self.boss = Some(Boss {
                        x: x - 75.0,
                        cy: (ground + top) / 2.0,
                        pods: [self.cfg.boss_pod_hp; 2],
                        pod_fire: [0.0; 2],
                        core: self.cfg.boss_core_hp,
                        t: 0.0,
                        active: false,
                        dying: 0.0,
                        dead: false,
                        hit_flash: 0.0,
                    });
                    None
                }
                _ => None,
            };
            if let Some(e) = e {
                self.enemies.push(e);
            }
        }
    }

    /// How far a tank/truck can drive from x: flat-ish ground, no cliffs or steep slopes.
    fn drive_range(&self, level: &Level, x: f32) -> (f32, f32) {
        let ok = |xx: f32| level.slope_deg(xx).abs() <= self.cfg.tank_slope && level.col_at(xx).gtype != '█';
        let mut a = x;
        while a > x - 260.0 && ok(a - 4.0) && (level.ground(a - 4.0) - level.ground(a)).abs() < 3.0 {
            a -= 4.0;
        }
        let mut b = x;
        while b < x + 260.0 && ok(b + 4.0) && (level.ground(b + 4.0) - level.ground(b)).abs() < 3.0 {
            b += 4.0;
        }
        (a + 20.0, (b - 20.0).max(a + 20.0))
    }

    // ---------------------------------------------------------- enemies

    fn update_swarms(&mut self, dt: f32) {
        let cfg = &self.cfg;
        let cam = self.cam_x;
        let players: Vec<(f32, f32, bool)> = self.players.iter().map(|p| (p.x, p.y, p.alive)).collect();
        for s in &mut self.swarms {
            if !s.unfurled {
                s.cx -= cfg.wh_enter_speed * dt;
                let near = players.iter().any(|&(x, y, a)| a && ((x - s.cx).powi(2) + (y - s.cy).powi(2)).sqrt() < 300.0);
                if s.cx - cam < cfg.wh_unfurl_frac * SCREEN_W || near {
                    s.unfurled = true;
                    // target the nearest player
                    let mut bd = f32::MAX;
                    for (k, &(x, y, a)) in players.iter().enumerate() {
                        let d = (x - s.cx).powi(2) + (y - s.cy).powi(2);
                        if a && d < bd {
                            bd = d;
                            s.target = k;
                        }
                    }
                }
            } else {
                s.ut += dt;
                s.cx -= cfg.wh_enter_speed * 0.35 * dt;
                let ty = players[s.target].1;
                s.cy += (ty - s.cy).clamp(-cfg.wh_drift * dt, cfg.wh_drift * dt);
            }
        }
    }

    fn update_enemies(&mut self, level: &Level, dt: f32) {
        let cfg = self.cfg.clone();
        let cam = self.cam_x;
        let flak_off: Vec<(f32, f32)> = self.radar_off.clone();
        let time = self.time;
        let mut new_shots: Vec<Shot> = Vec::new();
        let mut new_enemies: Vec<Enemy> = Vec::new();
        let mut kills: Vec<usize> = Vec::new();
        let mut sounds: Vec<(&'static str, f32)> = Vec::new();
        let mut lasers: Vec<(Vec2, Vec2, f32)> = Vec::new();
        let mut telegraphs: Vec<Vec2> = Vec::new();
        let nearest: Vec<Option<usize>> = self.enemies.iter().map(|e| self.nearest_player(e.x, e.y)).collect();
        let pl: Vec<(f32, f32, bool)> = self.players.iter().map(|p| (p.x, p.y, p.alive && !p.falling)).collect();
        let mut next_id = self.next_id;

        for (k, e) in self.enemies.iter_mut().enumerate() {
            if e.dead {
                continue;
            }
            e.t += dt;
            e.cd -= dt;
            e.fire_t -= dt;
            e.retarget -= dt;
            if e.retarget <= 0.0 {
                e.retarget = cfg.retarget_s;
                if let Some(n) = nearest[k] {
                    e.target = n;
                }
            }
            let (px, py, palive) = pl[e.target];
            let dx = px - e.x;
            let dy = py - e.y;
            let on_screen = e.x < cam + SCREEN_W + 20.0 && e.x > cam - 20.0;
            // helper: aimed shot at the player's current position (no lead)
            let aimed = |x: f32, y: f32, speed: f32, kind: ShotKind| {
                let d = vec2(px - x, py - y).normalize_or_zero() * speed;
                Shot { kind, x, y, vx: d.x, vy: d.y, accel: 0.0, t: 0.0, dead: false, hit_mask: [false; 2] }
            };
            match e.kind {
                Kind::Fuel | Kind::Radar => {}
                Kind::Rocket => {
                    if e.state == 0 {
                        let ahead = e.x - px;
                        if palive && ahead < cfg.rocket_trigger && ahead > -40.0 {
                            e.state = 1;
                            e.vy = -cfg.rocket_speed;
                            sounds.push(("rocket-launch", 0.5));
                        }
                    } else {
                        e.vy -= cfg.rocket_accel * dt;
                        e.y += e.vy * dt;
                        if e.y < -60.0 || (level.ceil(e.x) > 0.0 && e.y - 11.0 <= level.ceil(e.x)) {
                            kills.push(k);
                        }
                    }
                }
                Kind::Tank | Kind::Truck => {
                    let is_tank = e.kind == Kind::Tank;
                    e.flip = dx > 0.0; // face the player
                    let slope = level.slope_deg(e.x);
                    // effective barrel elevations given the ground tilt
                    let tilt = if e.flip { slope } else { -slope };
                    let elevs: Vec<f32> = if is_tank { cfg.tank_elev.clone() } else { vec![cfg.truck_elev] };
                    let a = (-(dy)).atan2(dx.abs()).to_degrees(); // angle up to the player
                    let mut best = 0;
                    for (i, el) in elevs.iter().enumerate() {
                        if (a - (el + tilt)).abs() < (a - (elevs[best] + tilt)).abs() {
                            best = i;
                        }
                    }
                    e.elev = best;
                    let eff = elevs[best] + tilt;
                    let diff = a - eff;
                    let in_range = dx.abs() < 650.0 && dy < 0.0 && palive && on_screen;
                    if in_range && diff.abs() < cfg.tank_tol && e.cd <= 0.0 {
                        let dir = vec2(if e.flip { 1.0 } else { -1.0 } * eff.to_radians().cos(), -eff.to_radians().sin());
                        let tip = vec2(e.x, e.y - 8.0) + dir * 22.0;
                        if is_tank {
                            new_shots.push(Shot { kind: ShotKind::Shell, x: tip.x, y: tip.y, vx: dir.x * cfg.e_shell_speed * 0.55, vy: dir.y * cfg.e_shell_speed * 0.55, accel: 0.0, t: 0.0, dead: false, hit_mask: [false; 2] });
                            e.cd = cfg.tank_cooldown;
                            e.fire_t = 0.15;
                            sounds.push(("tank-cannon", 0.5));
                        } else {
                            e.n = cfg.truck_salvo;
                            e.cd2 = 0.0;
                            e.aim = eff;
                            e.cd = cfg.truck_cooldown;
                        }
                    } else if in_range {
                        // drive to get the angle right: steeper than the barrel -> back off
                        let toward = dx.signum();
                        let dir = if diff > 0.0 { -toward } else { toward };
                        let sp = if is_tank { cfg.tank_speed } else { cfg.truck_speed };
                        let nx = (e.x + dir * sp * dt).clamp(e.range.0, e.range.1);
                        e.dist += (nx - e.x).abs();
                        e.x = nx;
                    }
                    // truck salvo
                    if e.n > 0 {
                        e.cd2 -= dt;
                        if e.cd2 <= 0.0 {
                            e.n -= 1;
                            e.cd2 = cfg.truck_gap;
                            let dir = vec2(if e.flip { 1.0 } else { -1.0 } * e.aim.to_radians().cos(), -e.aim.to_radians().sin());
                            let tip = vec2(e.x, e.y - 10.0) + dir * 20.0;
                            new_shots.push(Shot { kind: ShotKind::Rocket, x: tip.x, y: tip.y, vx: dir.x * cfg.e_rocket_speed, vy: dir.y * cfg.e_rocket_speed, accel: cfg.e_rocket_accel, t: 0.0, dead: false, hit_mask: [false; 2] });
                            sounds.push(("rocket-launch", 0.4));
                        }
                    }
                    e.y = level.ground(e.x) - e.kind.half().1;
                    e.ang = -slope.to_radians();
                }
                Kind::Tower => {
                    // angle from pointing left (0) through up (90) to right (180)
                    let want = (-(dy)).atan2(-dx).to_degrees();
                    let want = if want < 0.0 { if dx < 0.0 { 0.0 } else { 180.0 } } else { want };
                    let step = cfg.tower_turn * dt;
                    e.aim += (want - e.aim).clamp(-step, step);
                    let dist = (dx * dx + dy * dy).sqrt();
                    if e.n > 0 {
                        e.cd2 -= dt;
                        if e.cd2 <= 0.0 {
                            e.n -= 1;
                            e.cd2 = 0.12;
                            e.fire_t = 0.1;
                            let a = e.aim.to_radians();
                            let tip = vec2(e.x - a.cos() * 16.0, e.y - 18.0 - a.sin() * 12.0);
                            new_shots.push(aimed(tip.x, tip.y, cfg.e_bullet_speed * 0.5, ShotKind::Bullet));
                            sounds.push(("enemy-shot", 0.35));
                        }
                    } else if palive && on_screen && e.cd <= 0.0 && dist < cfg.tower_range && (want - e.aim).abs() < 10.0 && line_of_sight(level, vec2(e.x, e.y - 20.0), vec2(px, py)) {
                        e.n = cfg.tower_burst;
                        e.cd = cfg.tower_cooldown;
                    }
                }
                Kind::Flak => {
                    let disabled = flak_off.iter().any(|r| (r.0 - e.x).abs() < cfg.radar_range && r.1 > time);
                    if palive && on_screen && !disabled && e.cd <= 0.0 && dy < -40.0 && dx.abs() < 700.0 {
                        e.cd = cfg.flak_cooldown;
                        e.fire_t = 0.12;
                        let mut s = aimed(e.x, e.y - 14.0, cfg.e_shell_speed * 0.6, ShotKind::Flak { burst_y: py });
                        s.vx += 0.0;
                        new_shots.push(s);
                        sounds.push(("tank-cannon", 0.3));
                    }
                }
                Kind::Pad => {
                    let ahead = e.x - px;
                    if palive && e.n < cfg.drone_launches && e.cd <= 0.0 && ahead < cfg.drone_trigger && ahead > -100.0 {
                        e.n += 1;
                        e.cd = 1.5;
                        next_id += 1;
                        let mut d = Enemy::new(next_id, Kind::Drone, e.x, e.y - 14.0);
                        d.target = e.target;
                        new_enemies.push(d);
                        sounds.push(("rocket-launch", 0.3));
                    }
                }
                Kind::Drone => {
                    if e.state == 0 {
                        e.y -= cfg.drone_climb * dt;
                        if (e.y - py).abs() < cfg.drone_tol || e.y < py || e.y < HUD_H + 20.0 {
                            e.state = 1;
                            e.vx = if dx < 0.0 { -cfg.drone_speed } else { cfg.drone_speed };
                            e.flip = dx > 0.0;
                        }
                    } else {
                        e.x += e.vx * dt; // flies level, no more steering
                    }
                    if level.solid(e.x + e.vx.signum() * 14.0, e.y) && e.state == 1 {
                        kills.push(k);
                    }
                }
                Kind::Fighter => {
                    // heading `ang`: PI = flying left
                    if e.state == 0 {
                        let want = (dy).atan2(dx);
                        let mut diff = want - e.ang;
                        while diff > PI {
                            diff -= 2.0 * PI;
                        }
                        while diff < -PI {
                            diff += 2.0 * PI;
                        }
                        let step = cfg.fighter_turn.to_radians() * dt;
                        if palive && dx < 0.0 {
                            e.ang += diff.clamp(-step, step);
                        }
                        let dist = (dx * dx + dy * dy).sqrt();
                        if palive && dx < 0.0 && dist < cfg.fighter_range.1 && dist > cfg.fighter_range.0 * 0.5 && e.n == 0 {
                            e.n = cfg.fighter_burst;
                            e.cd2 = 0.0;
                        }
                        if e.n > 0 {
                            e.cd2 -= dt;
                            if e.cd2 <= 0.0 {
                                e.n -= 1;
                                e.cd2 = cfg.fighter_gap;
                                new_shots.push(aimed(e.x - 20.0, e.y + 2.0, cfg.e_bullet_speed * 0.5, ShotKind::Bullet));
                                sounds.push(("enemy-shot", 0.35));
                                if e.n == 0 {
                                    e.state = 1; // pull past the player and leave left
                                    e.aim = if dy > 0.0 { -1.0 } else { 1.0 };
                                }
                            }
                        }
                        if dx > -60.0 && e.state == 0 && dist < 160.0 {
                            e.state = 1;
                            e.aim = if dy > 0.0 { -1.0 } else { 1.0 };
                        }
                    } else {
                        let want = PI - e.aim * 0.45;
                        let step = cfg.fighter_turn.to_radians() * 1.5 * dt;
                        let mut diff = want - e.ang;
                        while diff > PI {
                            diff -= 2.0 * PI;
                        }
                        while diff < -PI {
                            diff += 2.0 * PI;
                        }
                        e.ang += diff.clamp(-step, step);
                    }
                    let sp = cfg.fighter_speed * if e.state == 1 { 1.3 } else { 1.0 };
                    e.x += e.ang.cos() * sp * dt;
                    e.y += e.ang.sin() * sp * dt;
                    if level.solid(e.x, e.y) {
                        kills.push(k);
                    }
                }
                Kind::Chopper => {
                    // hover ahead of the player, bob, match height slowly, fire straight back
                    if e.state == 0 {
                        let hold_x = (px + cfg.chop_hold).min(cam + SCREEN_W - 50.0);
                        e.x += (hold_x - e.x).clamp(-260.0 * dt, 260.0 * dt);
                        e.oy += (py - e.oy).clamp(-cfg.chop_follow * dt, cfg.chop_follow * dt);
                        let gy = level.ground(e.x) - 30.0;
                        let cy = level.ceil(e.x).max(HUD_H) + 20.0;
                        e.oy = e.oy.clamp(cy, gy);
                        e.y = e.oy + (e.t * 2.0).sin() * cfg.chop_bob * 0.5;
                        if e.cd <= 0.0 && palive && on_screen {
                            e.cd = cfg.chop_fire;
                            new_shots.push(Shot { kind: ShotKind::Bullet, x: e.x - 30.0, y: e.y + 4.0, vx: -cfg.e_bullet_speed * 0.5, vy: 0.0, accel: 0.0, t: 0.0, dead: false, hit_mask: [false; 2] });
                            sounds.push(("enemy-shot", 0.35));
                        }
                        if e.t > cfg.chop_stay {
                            e.state = 1;
                        }
                    } else {
                        e.x += 200.0 * dt;
                        e.y -= 120.0 * dt;
                        if e.y < -40.0 {
                            e.dead = true;
                        }
                    }
                }
                Kind::Whirler => {} // moved by its swarm below
                Kind::Balloon => {
                    e.y = e.oy + (e.t * 2.0 * PI / cfg.balloon_period).sin() * cfg.balloon_bob;
                    e.x = e.ox + (e.t * 1.3).sin() * 4.0;
                }
                Kind::Mine => {
                    if palive {
                        let d = vec2(dx, dy).normalize_or_zero() * cfg.mine_drift * dt;
                        if !level.solid(e.x + d.x * 4.0, e.y + d.y * 4.0) {
                            e.x += d.x;
                            e.y += d.y;
                        }
                    }
                }
                Kind::Rock => {
                    if e.state == 0 {
                        if pl.iter().any(|&(x, y, a)| a && y > e.y && x + cfg.rocks_trigger >= e.x - 30.0 && x < e.x + 40.0) {
                            e.state = 1;
                        }
                    } else {
                        e.vy += cfg.rocks_gravity * dt;
                        e.y += e.vy * dt;
                        if e.y + 8.0 >= level.ground(e.x) {
                            kills.push(k);
                        }
                    }
                }
                Kind::Turret => {
                    // 0 idle, 1 telegraph (glow), 2 laser on
                    let ang = (-(dy)).atan2(dx).to_degrees().rem_euclid(360.0);
                    let gun = turret_gun_len(e.var);
                    let mount = vec2(e.x, e.y + 1.0);
                    if e.state == 0 && palive {
                        // the barrel follows the player inside its downward arc
                        let want = dy.atan2(dx).clamp(0.35, PI - 0.35);
                        let step = 2.5 * dt;
                        e.aim += (want - e.aim).clamp(-step, step);
                    }
                    match e.state {
                        0 => {
                            let in_arc = ang >= cfg.turret_arc.0 && ang <= cfg.turret_arc.1;
                            if palive && on_screen && e.cd <= 0.0 && in_arc && dy.abs() + dx.abs() < 700.0 {
                                e.state = 1;
                                e.cd2 = cfg.turret_telegraph;
                                e.aim = dy.atan2(dx).clamp(0.35, PI - 0.35); // locked now
                            }
                        }
                        1 => {
                            telegraphs.push(mount + vec2(e.aim.cos(), e.aim.sin()) * gun);
                            e.cd2 -= dt;
                            if e.cd2 <= 0.0 {
                                e.state = 2;
                                e.cd2 = cfg.turret_on;
                                sounds.push(("enemy-laser", 0.5));
                            }
                        }
                        _ => {
                            let dir = vec2(e.aim.cos(), e.aim.sin());
                            let from = mount + dir * gun;
                            let mut l = 0.0;
                            while l < 900.0 && !level.solid(from.x + dir.x * l, from.y + dir.y * l) {
                                l += 4.0;
                            }
                            lasers.push((from, from + dir * l, (e.cd2 / cfg.turret_on).clamp(0.0, 1.0)));
                            e.cd2 -= dt;
                            if e.cd2 <= 0.0 {
                                e.state = 0;
                                e.cd = cfg.turret_cooldown;
                            }
                        }
                    }
                }
            }
        }
        // whirlers follow their swarm
        for e in self.enemies.iter_mut().filter(|e| e.kind == Kind::Whirler && !e.dead) {
            let s = &self.swarms[e.group];
            if !s.unfurled {
                e.x = s.cx + e.ox;
                e.y = s.cy + e.oy;
            } else {
                let a = e.member as f32 * cfg.wh_phase_step + cfg.wh_omega * s.ut;
                let r = cfg.wh_r0 + cfg.wh_growth * s.ut + e.member as f32 * 6.0;
                e.x = s.cx + a.cos() * r;
                e.y = s.cy + a.sin() * r * 0.8;
                if s.ut > 10.0 {
                    e.dead = true;
                }
            }
        }
        self.next_id = next_id;
        self.shots.extend(new_shots);
        self.enemies.extend(new_enemies);
        self.sfx_queue.extend(sounds);
        self.lasers = lasers;
        self.telegraphs = telegraphs;
        for k in kills {
            if !self.enemies[k].dead {
                self.kill_enemy(k, None, level);
            }
        }
    }

    fn update_boss(&mut self, level: &Level, dt: f32) {
        let cam = self.cam_x;
        let max_cam = level.width - SCREEN_W;
        let target = match &self.boss {
            Some(b) => self.nearest_player(b.x, b.cy),
            None => return,
        };
        let Some(b) = self.boss.as_mut() else { return };
        if b.dead {
            return;
        }
        b.hit_flash -= dt;
        if !b.active {
            if cam >= max_cam - 1.0 {
                b.active = true;
                self.message = Some(("BASE CORE".into(), 2.0));
            }
            return;
        }
        if b.dying > 0.0 {
            b.dying -= dt;
            if gen_range(0.0, 1.0) < 0.3 {
                let (x, y) = (b.x + gen_range(-10.0, 150.0), b.cy + gen_range(-130.0, 130.0));
                self.fx.explode(x, y, gen_range(1.0, 2.2));
                self.sfx_queue.push(("explode-big", 0.6));
            }
            if b.dying <= 0.0 {
                b.dead = true;
                self.fx.explode(b.x + 40.0, b.cy, 4.0);
                self.phase = Phase::Clear(0.0);
                for p in self.players.iter_mut() {
                    if p.joined && p.alive {
                        // fuel bonus at the end of the level
                        p.score += (p.fuel * 100.0).round() as i32 * 50;
                    }
                }
                self.message = None;
                self.sfx_queue.push(("extra-life", 0.8));
            }
            return;
        }
        b.t += dt;
        for i in 0..2 {
            b.pod_fire[i] -= dt;
            if b.pods[i] <= 0 {
                continue;
            }
            // volleys while the core is closed: 3 aimed bullets every ~0.9 s
            let ph = b.t % 5.0;
            if ph < 3.0 && ((b.t * 10.0) as i32 + i as i32 * 4) % 9 == 0 && b.pod_fire[i] <= -0.05 {
                if let Some(t) = target {
                    let p = &self.players[t];
                    let from = b.pod_pos(i) - vec2(18.0, 0.0);
                    let d = vec2(p.x - from.x, p.y - from.y).normalize_or_zero() * self.cfg.e_bullet_speed * 0.45;
                    self.shots.push(Shot { kind: ShotKind::Bullet, x: from.x, y: from.y, vx: d.x, vy: d.y, accel: 0.0, t: 0.0, dead: false, hit_mask: [false; 2] });
                    b.pod_fire[i] = 0.12;
                    self.sfx_queue.push(("enemy-shot", 0.35));
                }
            }
        }
    }

    fn damage_boss(&mut self, p: Vec2, amount: i32, by: usize) -> bool {
        let Some(b) = self.boss.as_mut() else { return false };
        if b.dead || !b.active || b.dying > 0.0 {
            return false;
        }
        for i in 0..2 {
            if b.pods[i] > 0 && p.distance(b.pod_pos(i)) < 16.0 {
                b.pods[i] -= amount;
                b.hit_flash = 0.08;
                if b.pods[i] <= 0 {
                    let pp = b.pod_pos(i);
                    self.fx.explode(pp.x, pp.y, 1.6);
                    self.sfx_queue.push(("explode-big", 0.6));
                    self.players[by].score += 2000;
                } else {
                    self.fx.sparks(p.x, p.y, 4, 120.0, Color::new(1.0, 0.9, 0.5, 1.0));
                }
                return true;
            }
        }
        let c = b.core_pos();
        if (p.x - c.x).abs() < 16.0 && (p.y - c.y).abs() < 24.0 {
            if b.core_open() {
                b.core -= amount;
                b.hit_flash = 0.08;
                self.fx.sparks(p.x, p.y, 6, 160.0, Color::new(1.0, 0.6, 0.3, 1.0));
                if b.core <= 0 {
                    b.dying = 2.5;
                    self.players[by].score += 10000;
                    self.sfx_queue.push(("explode-big", 1.0));
                }
            } else {
                self.fx.sparks(p.x, p.y, 3, 100.0, Color::new(0.8, 0.8, 1.0, 1.0));
            }
            return true;
        }
        if p.x > b.x && (p.y - b.cy).abs() < 142.0 {
            self.fx.sparks(p.x, p.y, 2, 80.0, Color::new(0.8, 0.8, 1.0, 1.0));
            return true;
        }
        false
    }

    // ---------------------------------------------------------- kills

    /// Destroys enemy k. `by` = the player who gets the points.
    fn kill_enemy(&mut self, k: usize, by: Option<usize>, level: &Level) {
        let mut blasts: Vec<(f32, f32, Option<usize>)> = Vec::new();
        self.kill_one(k, by, &mut blasts);
        // balloon blasts chain: anything touching the explosion dies
        while let Some((x, y, owner)) = blasts.pop() {
            let r = self.cfg.balloon_blast;
            self.fx.explode(x, y, 2.4);
            self.fx.flash(x, y, r * 1.6, 0.5, Color::new(1.0, 0.85, 0.5, 1.0));
            for j in 0..self.enemies.len() {
                let e = &self.enemies[j];
                if !e.dead && vec2(e.x - x, e.y - y).length() < r + e.kind.half().0 * 0.5 {
                    self.kill_one(j, owner, &mut blasts);
                }
            }
            for s in &mut self.shots {
                if vec2(s.x - x, s.y - y).length() < r {
                    s.dead = true;
                }
            }
            for i in 0..2 {
                let p = &self.players[i];
                if p.alive && p.invuln <= 0.0 {
                    let (c, h) = p.hitbox(&self.cfg);
                    let q = vec2(x.clamp(c.x - h.x, c.x + h.x), y.clamp(c.y - h.y, c.y + h.y));
                    if q.distance(vec2(x, y)) < r {
                        self.hurt_player(i, self.cfg.ram_balloon, level);
                    }
                }
            }
        }
    }

    fn kill_one(&mut self, k: usize, by: Option<usize>, blasts: &mut Vec<(f32, f32, Option<usize>)>) {
        let e = &mut self.enemies[k];
        if e.dead {
            return;
        }
        e.dead = true;
        let (kind, x, y) = (e.kind, e.x, e.y);
        let big = matches!(kind, Kind::Fuel | Kind::Tank | Kind::Truck | Kind::Tower | Kind::Radar | Kind::Flak);
        if kind == Kind::Balloon {
            blasts.push((x, y, by));
            self.sfx_queue.push(("explode-big", 0.9));
        } else if kind == Kind::Rock {
            self.fx.sparks(x, y + 6.0, 10, 120.0, Color::new(0.8, 0.7, 0.6, 1.0));
            return;
        } else {
            self.fx.explode(x, y, if big { 1.5 } else { 1.0 });
            self.sfx_queue.push((if big { "explode-big" } else { "explode-small" }, 0.6));
        }
        if let Some(p) = by {
            let pl = &mut self.players[p];
            pl.score += kind.score();
            if kind == Kind::Fuel {
                pl.fuel = (pl.fuel + self.cfg.fuel_depot).min(1.0);
                self.sfx_queue.push(("fuel-up", 0.7));
            }
            if kind == Kind::Radar {
                self.radar_off.push((x, self.time + self.cfg.radar_time));
                self.message = Some(("RADAR DOWN  FLAK OFFLINE".into(), 2.0));
            }
            // extra lives by score: drop a 1UP for this player
            if pl.score >= pl.next_life {
                pl.next_life += self.cfg.life_every;
                let (cx, cy) = (self.cam_x + SCREEN_W * 0.8, 200.0);
                self.pickups.push(Pickup { owner: p, kind: PickKind::Life, x: cx, y: cy, t: 0.0, dead: false });
            }
            // random pickup drops, only what this player can use
            if kind != Kind::Whirler && gen_range(0.0, 1.0) < self.cfg.drop_chance {
                let pl = &self.players[p];
                let r = gen_range(0.0, 1.0);
                let pk = if pl.shield < 0.34 && r < 0.4 {
                    PickKind::Shield
                } else if !pl.laser && r > 0.8 {
                    PickKind::Laser
                } else {
                    PickKind::Fuel
                };
                let py = if kind.ground() { y - 30.0 } else { y };
                self.pickups.push(Pickup { owner: p, kind: pk, x, y: py, t: 0.0, dead: false });
            }
        }
    }

    /// A hit on a player: the shield soaks `cost` if it is up, otherwise the ship is lost.
    fn hurt_player(&mut self, i: usize, cost: f32, _level: &Level) {
        let p = &mut self.players[i];
        if !p.alive || p.invuln > 0.0 {
            return;
        }
        if p.shield > 0.0 {
            p.shield = (p.shield - cost).max(0.0);
            p.shield_flash = 0.15;
            self.sfx_queue.push((if p.shield <= 0.0 { "shield-down" } else { "shield-hit" }, 0.7));
            return;
        }
        if self.god {
            return;
        }
        self.kill_player(i);
    }

    fn kill_player(&mut self, i: usize) {
        let p = &mut self.players[i];
        if !p.alive {
            return;
        }
        p.alive = false;
        p.dead_t = 0.0;
        p.lives -= 1;
        p.laser = false;
        p.shield = 0.0;
        p.hoist = None;
        let (x, y) = (p.x, p.y);
        self.fx.explode(x, y, 2.5);
        self.sfx_queue.push(("player-die", 0.9));
        for s in &mut self.sites {
            if !s.done {
                s.lift = 0.0;
            }
        }
    }

    fn deaths(&mut self, level: &Level, dt: f32) {
        for i in 0..2 {
            let p = &mut self.players[i];
            if p.joined && !p.alive && !p.out {
                p.dead_t += dt;
                if p.lives <= 0 && p.dead_t > 1.0 {
                    p.out = true;
                }
            }
        }
        let joined: Vec<usize> = (0..2).filter(|&i| self.players[i].joined && !self.players[i].out).collect();
        if joined.is_empty() {
            self.phase = Phase::Over(0.0);
            return;
        }
        let all_down = joined.iter().all(|&i| !self.players[i].alive);
        let wait = joined.iter().all(|&i| self.players[i].alive || self.players[i].dead_t > 2.0);
        if all_down && wait {
            // everyone is down: back to the checkpoint
            self.restart_at_checkpoint(level);
        } else if !all_down {
            // co-op: a downed player comes back in place
            for &i in &joined {
                if !self.players[i].alive && self.players[i].dead_t > 2.0 {
                    self.respawn(i, level, 120.0);
                }
            }
        }
    }

    // ---------------------------------------------------------- projectiles

    fn update_bullets(&mut self, level: &Level, dt: f32) {
        let cam = self.cam_x;
        for bi in 0..self.bullets.len() {
            let (ox, oy) = (self.bullets[bi].x, self.bullets[bi].y);
            let (vx, vy) = (self.bullets[bi].vx, self.bullets[bi].vy);
            let owner = self.bullets[bi].owner;
            // sweep in small steps so fast bullets don't skip thin enemies
            let steps = 4;
            let mut hit = false;
            for s in 1..=steps {
                let q = vec2(ox + vx * dt * s as f32 / steps as f32, oy + vy * dt * s as f32 / steps as f32);
                if level.solid(q.x, q.y) {
                    self.fx.sparks(q.x, q.y, 3, 90.0, Color::new(0.6, 0.9, 1.0, 1.0));
                    hit = true;
                    break;
                }
                if let Some(k) = self.enemies.iter().position(|e| !e.dead && e.contains(q, 2.0)) {
                    let need = if self.enemies[k].armoured() { self.cfg.armour_hits } else { 1 };
                    self.enemies[k].hits += 1;
                    if self.enemies[k].hits >= need {
                        self.kill_enemy(k, Some(owner), level);
                    } else {
                        // a dent: sparks and a quick flash so you can see it counted
                        self.fx.sparks(q.x, q.y, 6, 140.0, Color::new(1.0, 0.9, 0.6, 1.0));
                        self.fx.flash(q.x, q.y, 18.0, 0.12, Color::new(1.0, 0.8, 0.5, 0.8));
                        self.sfx_queue.push(("shield-hit", 0.35));
                    }
                    hit = true;
                    break;
                }
                if self.damage_boss(q, 1, owner) {
                    hit = true;
                    break;
                }
                // shooting enemy shots down: rockets and shells pop
                if let Some(si) = self.shots.iter().position(|s| !s.dead && matches!(s.kind, ShotKind::Rocket | ShotKind::Shell) && vec2(s.x, s.y).distance(q) < 7.0) {
                    self.shots[si].dead = true;
                    self.fx.explode(q.x, q.y, 0.5);
                    hit = true;
                    break;
                }
            }
            let b = &mut self.bullets[bi];
            b.x += vx * dt;
            b.y += vy * dt;
            if hit || b.x > cam + SCREEN_W + 20.0 || b.x < cam - 20.0 || b.y < 0.0 || b.y > SCREEN_H {
                b.dead = true;
            }
        }
    }

    fn update_bombs(&mut self, level: &Level, dt: f32) {
        let cfg = self.cfg.clone();
        for bi in 0..self.bombs.len() {
            let b = &mut self.bombs[bi];
            b.t += dt;
            let fade = (b.t / 0.15).min(1.0);
            let w = cfg.bomb_wobble_px * (2.0 * PI * cfg.bomb_wobble_hz * b.t + b.phase).sin() * fade;
            b.x = b.x0 + b.vx * b.t + 0.6 * w;
            b.y = b.y0 + 0.5 * b.g * b.t * b.t + 0.25 * w;
            let k = (b.t / b.fall_t).clamp(0.0, 1.0);
            let smooth = k * k * (3.0 - 2.0 * k);
            let target = if b.back { 180.0 - cfg.bomb_impact_pitch } else { cfg.bomb_impact_pitch };
            b.pitch = target * smooth + cfg.bomb_wobble_pitch * (2.0 * PI * cfg.bomb_wobble_hz * b.t + b.phase * 1.7).sin() * fade;
            let (x, y, owner) = (b.x, b.y, b.owner);
            let nose = vec2(x, y + 4.0);
            let mut boom = level.solid(nose.x, nose.y) || y > SCREEN_H;
            if !boom {
                boom = self.enemies.iter().any(|e| !e.dead && e.kind != Kind::Rock && e.contains(nose, 2.0));
            }
            if !boom {
                boom = self.damage_boss(nose, 3, owner);
            }
            if boom {
                self.bombs[bi].dead = true;
                self.fx.anim(AnimKind::GroundBlast, x, y - 10.0, 0.0, 0.05, 1.0);
                self.fx.flash(x, y, 40.0, 0.3, Color::new(1.0, 0.7, 0.3, 0.9));
                self.fx.sparks(x, y, 8, 200.0, Color::new(1.0, 0.8, 0.4, 1.0));
                self.sfx_queue.push(("bomb-hit", 0.7));
                // blast radius: kills ground and air targets touching it
                for k in 0..self.enemies.len() {
                    let e = &self.enemies[k];
                    if !e.dead && e.kind != Kind::Rock {
                        let (hw, hh) = e.kind.half();
                        let q = vec2(x.clamp(e.x - hw, e.x + hw), y.clamp(e.y - hh, e.y + hh));
                        if q.distance(vec2(x, y)) < 22.0 {
                            self.kill_enemy(k, Some(owner), level);
                        }
                    }
                }
            }
        }
    }

    fn update_shots(&mut self, level: &Level, dt: f32) {
        let cam = self.cam_x;
        let cfg = self.cfg.clone();
        let mut hurts: Vec<(usize, f32)> = Vec::new();
        for s in &mut self.shots {
            s.t += dt;
            match s.kind {
                ShotKind::Rocket => {
                    let v = vec2(s.vx, s.vy);
                    let n = v.normalize_or_zero();
                    s.vx += n.x * s.accel * dt;
                    s.vy += n.y * s.accel * dt;
                    if gen_range(0.0, 1.0) < 0.6 {
                        self.fx.parts.push(crate::fx::Particle { x: s.x - n.x * 8.0, y: s.y - n.y * 8.0, vx: gen_range(-20.0, 20.0), vy: gen_range(-20.0, 20.0), life: 0.4, max: 0.4, size: 3.0, color: Color::new(0.5, 0.45, 0.55, 0.5), glow: false, gravity: -20.0 });
                    }
                }
                ShotKind::Flak { burst_y } => {
                    if s.y <= burst_y || s.t > 1.5 {
                        s.kind = ShotKind::FlakBurst;
                        s.t = 0.0;
                        s.vx = 0.0;
                        s.vy = 0.0;
                        self.fx.anim(AnimKind::Flak, s.x, s.y, 0.0, cfg.flak_time / 6.0 * 1.6, 1.4);
                        self.fx.flash(s.x, s.y, cfg.flak_radius * 1.4, 0.3, Color::new(1.0, 0.6, 0.3, 0.8));
                        self.sfx_queue.push(("flak-burst", 0.5));
                    }
                }
                ShotKind::FlakBurst => {
                    if s.t > cfg.flak_time {
                        s.dead = true;
                    }
                }
                _ => {}
            }
            s.x += s.vx * dt;
            s.y += s.vy * dt;
            if s.kind != ShotKind::FlakBurst && (level.solid(s.x, s.y) || s.x < cam - 30.0 || s.x > cam + SCREEN_W + 60.0 || s.y < -30.0 || s.y > SCREEN_H + 10.0) {
                if level.solid(s.x, s.y) && s.x > cam && s.x < cam + SCREEN_W {
                    self.fx.sparks(s.x, s.y, 3, 80.0, Color::new(1.0, 0.6, 0.4, 1.0));
                }
                s.dead = true;
                continue;
            }
            // hits on players
            for p in &self.players {
                if !p.alive || p.invuln > 0.0 || s.dead {
                    continue;
                }
                let (c, h) = p.hitbox(&cfg);
                let r = if s.kind == ShotKind::FlakBurst { cfg.flak_radius } else { 3.0 };
                let q = vec2(s.x.clamp(c.x - h.x, c.x + h.x), s.y.clamp(c.y - h.y, c.y + h.y));
                if q.distance(vec2(s.x, s.y)) <= r {
                    if s.kind == ShotKind::FlakBurst {
                        if !s.hit_mask[p.id] {
                            s.hit_mask[p.id] = true;
                            hurts.push((p.id, cfg.shield_shot));
                        }
                    } else {
                        s.dead = true;
                        hurts.push((p.id, cfg.shield_shot));
                    }
                }
            }
        }
        // enemy laser beams
        for &(a, b, _) in &self.lasers {
            for p in &self.players {
                if p.alive && p.invuln <= 0.0 {
                    let (c, h) = p.hitbox(&cfg);
                    if seg_box(a, b, c, h) {
                        hurts.push((p.id, cfg.shield_shot * 0.05)); // continuous drain while in the beam
                        if p.shield <= 0.0 {
                            hurts.push((p.id, 1.0));
                        }
                    }
                }
            }
        }
        for (i, c) in hurts {
            self.hurt_player(i, c, level);
        }
    }

    fn update_pickups(&mut self, dt: f32) {
        let life = self.cfg.pickup_life;
        let cfg = self.cfg.clone();
        for pk in &mut self.pickups {
            pk.t += dt;
            if pk.t > life {
                pk.dead = true;
                continue;
            }
            let p = &mut self.players[pk.owner];
            if p.alive {
                let (c, h) = p.hitbox(&cfg);
                if (pk.x - c.x).abs() < h.x + 10.0 && (pk.y - c.y).abs() < h.y + 10.0 {
                    pk.dead = true;
                    p.score += 100;
                    let snd = match pk.kind {
                        PickKind::Shield => {
                            p.shield = 1.0;
                            "shield-on"
                        }
                        PickKind::Fuel => {
                            p.fuel = (p.fuel + cfg.fuel_can).min(1.0);
                            "fuel-up"
                        }
                        PickKind::Life => {
                            p.lives += 1;
                            "extra-life"
                        }
                        PickKind::Laser => {
                            p.laser = true;
                            "plasma-up"
                        }
                    };
                    self.sfx_queue.push((snd, 0.7));
                }
            }
        }
    }

    /// Terrain, enemy bodies and the boss against each player.
    fn collide_players(&mut self, level: &Level) {
        let cfg = self.cfg.clone();
        for i in 0..2 {
            let p = &self.players[i];
            if !p.alive {
                continue;
            }
            let (c, h) = p.hitbox(&cfg);
            // terrain: sample points around the hitbox
            let mut crash = false;
            for (fx, fy) in [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (-1.0, 0.0), (0.5, 1.0), (-0.5, 1.0), (0.5, -1.0), (-0.5, -1.0)] {
                if level.solid(c.x + fx * h.x, c.y + fy * h.y) {
                    crash = true;
                    break;
                }
            }
            if let Some(b) = &self.boss {
                if !b.dead && c.x + h.x > b.x && (c.y - b.cy).abs() < 142.0 {
                    crash = true;
                }
            }
            if crash {
                if p.falling || !self.god {
                    if p.invuln <= 0.0 || p.falling {
                        self.kill_player(i);
                    } else {
                        // just respawned inside something: nudge to the safe middle
                        let y = self.safe_y(level, c.x);
                        self.players[i].y = y;
                    }
                }
                continue;
            }
            if p.falling {
                continue;
            }
            // enemy bodies (rams)
            for k in 0..self.enemies.len() {
                let e = &self.enemies[k];
                if e.dead {
                    continue;
                }
                let (hw, hh) = e.kind.half();
                let shrink = 0.8;
                if (e.x - c.x).abs() < h.x + hw * shrink && (e.y - c.y).abs() < h.y + hh * shrink {
                    let p = &self.players[i];
                    if p.invuln > 0.0 {
                        continue;
                    }
                    let cost = e.kind.ram_cost(&cfg);
                    let had_shield = p.shield > 0.0;
                    if e.kind != Kind::Rock {
                        self.kill_enemy(k, Some(i), level);
                    } else {
                        self.enemies[k].dead = true;
                    }
                    if had_shield {
                        self.hurt_player(i, cost, level);
                    } else if !self.god {
                        self.kill_player(i);
                        break;
                    }
                }
            }
        }
    }

    // ============================================================== drawing

    pub fn draw_world(&self, level: &Level, sp: &Sprites, ph: &Placeholders, glow: &Texture2D, add: &Material) {
        let t = self.time;
        // hoist sites
        for s in &self.sites {
            if s.done {
                continue;
            }
            let rope_y = s.y - (s.y - self.hoist_belly_y(s)) * s.lift;
            let ex = &sp.ex;
            let sheet = match s.kind {
                HoistKind::Pilot => &ex.pilot[s.var.min(1)],
                HoistKind::Cache => &ex.cache,
                HoistKind::Fuel => &ex.fuel[2],
            };
            let f = if s.kind == HoistKind::Pilot { ((t * 3.0) as usize) % 2 } else { 0 };
            let h = sheet.size(1.0).y;
            sheet.draw(0, f, s.x, rope_y - h / 2.0 + 1.0, 1.0, 0.0, false, WHITE);
            // crane hint when a ship is in range
            if s.lift == 0.0 {
                for p in &self.players {
                    if p.alive && p.hoist.is_none() {
                        let belly = p.local(BELLY, self.cfg.scale);
                        if (belly.x - s.x).abs() < 30.0 && belly.y < s.y && s.y - belly.y < self.cfg.hoist_height && ((t * 4.0) as i32) % 2 == 0 {
                            let c = if p.id == 0 { Color::new(0.35, 0.92, 1.0, 1.0) } else { Color::new(1.0, 0.42, 0.92, 1.0) };
                            crate::pixel::draw_text_px_centered("CRANE", s.x, s.y - h - 14.0, 1.0, c, Some(Color::new(0.05, 0.03, 0.1, 1.0)));
                            draw_triangle(vec2(s.x - 3.0, s.y - h - 7.0), vec2(s.x + 3.0, s.y - h - 7.0), vec2(s.x, s.y - h - 3.0), c);
                        }
                    }
                }
            }
        }
        // enemies
        for e in &self.enemies {
            if !e.dead {
                self.draw_enemy(e, level, sp, ph);
            }
        }
        if let Some(b) = &self.boss {
            if !b.dead {
                let flash = if b.hit_flash > 0.0 { Color::new(1.6, 1.6, 1.6, 1.0) } else { WHITE };
                ph.boss.draw(0, b.x + ph.boss.w / 2.0, b.cy, 0.0, false, WHITE);
                let core = if b.core_open() { 1 + ((t * 8.0) as usize % 2) } else { 0 };
                ph.boss_core.draw(core, b.core_pos().x, b.core_pos().y, 0.0, false, flash);
                for i in 0..2 {
                    if b.pods[i] > 0 {
                        let pp = b.pod_pos(i);
                        ph.boss_pod.draw(if b.pod_fire[i] > 0.0 { 1 } else { 0 }, pp.x, pp.y, 0.0, false, flash);
                    }
                }
            }
        }
        // pickups
        for pk in &self.pickups {
            let blink = pk.t > self.cfg.pickup_life - 2.0 && ((pk.t * 10.0) as i32) % 2 == 0;
            if !blink {
                let y = pk.y + (pk.t * 3.0).sin() * 3.0;
                sp.pickups[pk.owner].draw(pk.kind as usize, ((t * 8.0) as usize) % 4, pk.x, y, 1.0, 0.0, false, WHITE);
            }
        }
        // bombs
        for b in &self.bombs {
            let f = ((b.pitch / 10.0).round().clamp(0.0, 12.0)) as usize;
            sp.torpedo[b.owner].draw(0, f, b.x, b.y, 0.7, 0.0, false, WHITE);
        }
        // enemy shots
        for s in &self.shots {
            let rot = s.vy.atan2(s.vx);
            match s.kind {
                ShotKind::Bullet => sp.e_bullet.draw(0, ((t * 20.0) as usize) % 2, s.x, s.y, 1.0, rot + PI, false, WHITE),
                ShotKind::Shell | ShotKind::Flak { .. } => sp.shell.draw(0, ((t * 20.0) as usize) % 2, s.x, s.y, 1.0, rot + PI, false, WHITE),
                ShotKind::Rocket => sp.rocket.draw(0, ((t * 20.0) as usize) % 4, s.x, s.y, 1.0, rot + PI, false, WHITE),
                ShotKind::FlakBurst => {}
            }
        }
        // players (P2 drawn above P1; fades when the ships overlap)
        for i in 0..2 {
            self.draw_player(i, level, sp);
        }
        // player bullets
        for b in &self.bullets {
            sp.bullet[b.owner].draw(0, ((t * 30.0) as usize) % 2, b.x, b.y, 1.0, b.vy.atan2(b.vx), false, WHITE);
        }
        // explosions (normal blend) and smoke
        for a in &self.fx.anims {
            if a.t < 0.0 {
                continue;
            }
            let f = (a.t / a.frame_time) as usize;
            match a.kind {
                AnimKind::Explosion => sp.explosion.draw(0, f.min(7), a.x, a.y, a.scale, a.rot, false, WHITE),
                AnimKind::GroundBlast => sp.ground_blast.draw(0, f.min(7), a.x, a.y, a.scale, 0.0, false, WHITE),
                AnimKind::Flak => sp.flak.draw(0, f.min(5), a.x, a.y, a.scale, 0.0, false, WHITE),
                _ => {}
            }
        }
        self.fx.draw_solid();

        // ---------- additive glow pass ----------
        gl_use_material(add);
        let g = |x: f32, y: f32, r: f32, c: Color| {
            draw_texture_ex(glow, x - r, y - r, c, DrawTextureParams { dest_size: Some(vec2(r * 2.0, r * 2.0)), ..Default::default() });
        };
        let pc = [Color::new(0.3, 0.9, 1.0, 1.0), Color::new(1.0, 0.35, 0.9, 1.0)];
        for b in &self.bullets {
            let mut c = pc[b.owner];
            c.a = 0.55;
            g(b.x, b.y, 12.0, c);
        }
        for s in &self.shots {
            match s.kind {
                ShotKind::FlakBurst => {}
                ShotKind::Rocket => g(s.x, s.y, 14.0, Color::new(1.0, 0.5, 0.2, 0.6)),
                _ => g(s.x, s.y, 9.0, Color::new(1.0, 0.4, 0.5, 0.6)),
            }
        }
        for pk in &self.pickups {
            let mut c = pc[pk.owner];
            c.a = 0.35 + 0.15 * (t * 6.0).sin();
            g(pk.x, pk.y, 22.0, c);
        }
        for a in &self.fx.anims {
            if a.t >= 0.0 {
                if let AnimKind::Muzzle(_) = a.kind {
                } else {
                    let k = 1.0 - (a.t / (a.frame_time * 8.0)).min(1.0);
                    g(a.x, a.y, 30.0 * a.scale, Color::new(1.0, 0.6, 0.25, 0.5 * k));
                }
            }
        }
        // muzzle flashes
        for p in &self.players {
            if p.alive && p.muzzle_t > 0.0 {
                let tip = p.local(GUN_TIP, self.cfg.scale);
                sp.muzzle.draw(p.id, ((t * 40.0) as usize) % 3, tip.x + 3.0, tip.y, 1.0, p.tilt.to_radians(), false, WHITE);
                g(tip.x, tip.y, 14.0, pc[p.id]);
            }
            // player laser beam
            if let Some(end) = p.laser_end {
                let tip = p.local(GUN_TIP, self.cfg.scale);
                let len = end.x - tip.x;
                let seg: f32 = 40.0;
                let mut x = tip.x;
                let f = ((t * 30.0) as usize) % 4;
                while x < tip.x + len {
                    let w = seg.min(tip.x + len - x);
                    sp.laser[p.id].draw_stretch(0, f, x, tip.y - 5.0, w, 10.0, 0.0, vec2(x, tip.y), WHITE);
                    x += seg;
                }
                sp.laser_head[p.id].draw(0, f % 3, end.x, end.y, 1.2, 0.0, false, WHITE);
                g(end.x, end.y, 18.0, pc[p.id]);
            }
            // thruster glow
            if p.alive {
                let jet = p.local(vec2(-72.0, -6.0), self.cfg.scale);
                g(jet.x, jet.y, 14.0 + (t * 30.0).sin() * 2.0, Color::new(pc[p.id].r, pc[p.id].g, pc[p.id].b, 0.45));
            }
        }
        // enemy lasers and telegraphs
        for &(a, b, alpha) in &self.lasers {
            let d = b - a;
            let len = d.length();
            let ang = d.y.atan2(d.x);
            let f = ((t * 30.0) as usize) % 4;
            let mut l = 0.0;
            while l < len {
                let w = 40.0f32.min(len - l);
                let p0 = a + d / len * l;
                sp.e_laser.draw_stretch(0, f, p0.x, p0.y - 5.0, w, 10.0, ang, p0, Color::new(1.0, 1.0, 1.0, 0.5 + alpha * 0.5));
                l += 40.0;
            }
            g(b.x, b.y, 16.0, Color::new(1.0, 0.3, 0.5, 0.8));
        }
        for &p in &self.telegraphs {
            let k = 0.5 + 0.5 * (t * 30.0).sin();
            g(p.x, p.y, 10.0 + 8.0 * k, Color::new(1.0, 0.3, 0.4, 0.8));
        }
        // shields glow a little
        for p in &self.players {
            if p.alive && p.shield > 0.0 {
                let mut c = pc[p.id];
                c.a = 0.18;
                g(p.x + 8.0, p.y - 2.0, 80.0, c);
            }
        }
        // boss core
        if let Some(b) = &self.boss {
            if !b.dead && b.core_open() {
                let c = b.core_pos();
                g(c.x, c.y, 34.0 + 4.0 * (t * 12.0).sin(), Color::new(1.0, 0.5, 0.2, 0.7));
            }
        }
        self.fx.draw_glow(glow);
        gl_use_default_material();

        // hoist ropes
        for p in &self.players {
            if let Some((s, _)) = p.hoist {
                let belly = p.local(BELLY, self.cfg.scale);
                let site = &self.sites[s];
                let y = site.y - (site.y - belly.y) * site.lift - 16.0;
                draw_line(belly.x, belly.y, site.x, y, 1.0, Color::new(0.85, 0.85, 0.95, 1.0));
            }
        }
    }

    fn hoist_belly_y(&self, s: &Site) -> f32 {
        // where the lifted pilot/crate ends up (under whichever ship is hoisting it)
        for p in &self.players {
            if let Some((k, _)) = p.hoist {
                if std::ptr::eq(&self.sites[k], s) {
                    return p.local(BELLY, self.cfg.scale).y + 16.0;
                }
            }
        }
        s.y
    }

    fn draw_player(&self, i: usize, _level: &Level, sp: &Sprites) {
        let p = &self.players[i];
        if !p.joined || !p.alive {
            return;
        }
        if p.invuln > 0.0 && ((p.invuln * 12.0) as i32) % 2 == 0 {
            return;
        }
        let cfg = &self.cfg;
        let rot = p.tilt.to_radians();
        // overlap fade: P2 drawn on top fades when the ships overlap a lot
        let mut alpha = 1.0;
        if i == 1 && self.players[0].alive {
            let (a, b) = (&self.players[0], p);
            let (w, h) = (130.0 * cfg.scale, 50.0 * cfg.scale);
            let ox = (w - (a.x - b.x).abs()).max(0.0);
            let oy = (h - (a.y - b.y).abs()).max(0.0);
            if ox * oy / (w * h) > cfg.overlap_fade {
                alpha = cfg.top_alpha;
            }
        }
        let col = Color::new(1.0, 1.0, 1.0, alpha);
        // mounted torpedo under the stub wing (drawn first so the wing hides the reload)
        let ready_at = cfg.reload_delay;
        if p.reload_t >= ready_at && !p.falling {
            let k = ((p.reload_t - ready_at) / cfg.reload_time).min(1.0);
            let ease = 1.0 - (1.0 - k).powi(3);
            let bounce = if k >= 1.0 { ((p.reload_t - ready_at - cfg.reload_time) / 0.12).min(1.0) } else { 0.0 };
            let off = -cfg.reload_slide * (1.0 - ease) + (bounce * PI).sin() * 1.0;
            let m = p.local(BOMB_MOUNT + vec2(0.0, off), cfg.scale);
            sp.torpedo[i].draw(0, 0, m.x, m.y, 0.7, rot, false, col);
        }
        // the ship: row 0 = flying, 1 = rescue (crew door), 3 = cargo ramp
        let row = match p.hoist {
            Some((s, _)) => {
                if self.sites[s].kind == HoistKind::Pilot { 1 } else { 3 }
            }
            None => 0,
        };
        let frame = ((p.anim * 24.0) as usize) % 8;
        let bob = if p.move_x.abs() < 0.1 { (p.anim * 3.0).sin() * 1.0 } else { 0.0 };
        sp.kx2[i].draw(row, frame, p.x, p.y + bob, cfg.scale, rot, false, col);
        // lives digit on the hull
        let d = p.lives.clamp(0, 9) as f32;
        let dp = p.local(LIVES_DIGIT, cfg.scale);
        let (dw, dh) = (3.0 * cfg.scale, 5.0 * cfg.scale);
        draw_texture_ex(
            &sp.hull_digits,
            dp.x - dw / 2.0,
            dp.y - dh / 2.0 + bob,
            col,
            DrawTextureParams { dest_size: Some(vec2(dw, dh)), source: Some(Rect::new(d * 6.0, 0.0, 6.0, 10.0)), rotation: rot, ..Default::default() },
        );
        // shield force field
        if p.shield > 0.0 {
            let f = if p.shield_flash > 0.0 { 8 } else { ((p.anim * 12.0) as usize) % 8 };
            let a = 0.55 + 0.45 * p.shield;
            sp.shield[i].draw(0, f, p.x, p.y + bob, cfg.scale, rot, false, Color::new(1.0, 1.0, 1.0, a * alpha));
        }
        if self.god && std::env::var("TK_SHOW_HITBOX").is_ok() {
            let (c, h) = p.hitbox(cfg);
            draw_rectangle_lines(c.x - h.x, c.y - h.y, h.x * 2.0, h.y * 2.0, 1.0, RED);
        }
    }

    fn draw_enemy(&self, e: &Enemy, level: &Level, sp: &Sprites, ph: &Placeholders) {
        let t = self.time;
        let w = WHITE;
        let ex = &sp.ex;
        let _ = ph;
        let anim4 = ((e.t * 10.0) as usize) % 4;
        match e.kind {
            Kind::Fuel => ground_draw(&ex.fuel[e.var.min(1)], 0, e, false),
            Kind::Flak => ground_draw(&ex.flak, if e.fire_t > 0.0 { 1 } else { 0 }, e, false),
            Kind::Radar => ground_draw(&ex.radar[e.var.min(1)], ((t * 4.0) as usize) % 4, e, false),
            Kind::Turret => {
                let v = e.var.min(2);
                let body = &ex.turret[v];
                let bs = body.size(1.0);
                // hang from the roof: top of the sprite at the top of the hitbox
                let (top_x, top_y) = (e.x - bs.x / 2.0, e.y - e.kind.half().1);
                let mount = vec2(top_x, top_y) + ex.turret_mount[v] * 0.5;
                let gun = &ex.turret_gun[v];
                let gs = gun.size(1.0);
                let piv = ex.turret_pivot[v] * 0.5;
                gun.draw_stretch(0, 0, mount.x - piv.x, mount.y - piv.y, gs.x, gs.y, e.aim, mount, w);
                let glow = e.state == 1 && ((t * 16.0) as i32) % 2 == 0;
                body.draw(0, if glow { 1 } else { 0 }, e.x, top_y + bs.y / 2.0, 1.0, 0.0, false, w);
            }
            Kind::Mine => ex.mine[e.var.min(1)].draw(0, ((t * 3.0) as usize) % 2, e.x, e.y, 1.0, 0.0, false, w),
            Kind::Rock => {
                let r = &ex.rock[e.var.min(2)];
                let top = e.y - e.kind.half().1;
                r.draw(0, 0, e.x, top + r.size(1.0).y / 2.0, 1.0, if e.state == 1 { (e.t * 3.0).sin() * 0.3 } else { 0.0 }, false, w);
            }
            Kind::Rocket => {
                if e.state == 0 {
                    let gy = level.ground(e.x);
                    draw_rectangle(e.x - 8.0, gy - 4.0, 16.0, 4.0, Color::new(0.35, 0.35, 0.45, 1.0));
                    draw_rectangle(e.x - 8.0, gy - 4.0, 16.0, 1.0, Color::new(0.7, 0.7, 0.8, 1.0));
                }
                sp.rocket.draw(0, anim4, e.x, e.y, 1.0, -PI / 2.0, false, w);
                if e.state > 0 {
                    // exhaust
                    draw_rectangle(e.x - 1.5, e.y + 11.0, 3.0, 4.0 + (t * 40.0).sin().abs() * 4.0, Color::new(1.0, 0.8, 0.3, 1.0));
                }
            }
            Kind::Tank => {
                let f = ((e.dist / 3.0) as usize) % 4;
                sp.tank.draw(e.elev.min(2), f, e.x, e.y - 3.0, 1.0, e.ang, e.flip, w);
            }
            Kind::Truck => {
                let f = ((e.dist / 3.0) as usize) % 4;
                let row = if e.cd > 0.5 { 1 } else { 0 };
                sp.truck.draw(row, f, e.x, e.y - 3.0, 1.0, e.ang, e.flip, w);
            }
            Kind::Tower => {
                let f = ((e.aim / 30.0).round().clamp(0.0, 6.0)) as usize;
                sp.tower.draw(if e.fire_t > 0.0 { 1 } else { 0 }, f, e.x, e.y - 4.0, 1.0, 0.0, false, w);
            }
            Kind::Pad => {
                sp.pad.draw(0, 0, e.x, e.y, 1.0, 0.0, false, w);
                if e.n < self.cfg.drone_launches && e.cd <= 0.0 {
                    sp.drone.draw(0, anim4, e.x, e.y - 14.0, 1.0, 0.0, false, w);
                }
            }
            Kind::Drone => sp.drone.draw(0, ((e.t * 20.0) as usize) % 4, e.x, e.y, 1.0, 0.0, e.flip, w),
            Kind::Fighter => {
                let rot = e.ang - PI;
                sp.fighter.draw(if e.state == 1 { 1 } else { 0 }, anim4, e.x, e.y, 1.0, rot, false, w);
            }
            Kind::Whirler => {
                let f = ((e.t * 14.0) as usize + e.member) % 6;
                sp.whirler.draw(if e.flip { 1 } else { 0 }, f, e.x, e.y, 1.0, 0.0, false, w);
            }
            Kind::Balloon => sp.balloon.draw(0, anim4, e.x, e.y, 1.0, 0.0, false, w),
            Kind::Chopper => {
                let frame = ((e.t * 24.0) as usize) % 4;
                ex.chopper.draw(0, frame, e.x, e.y, 1.0, 0.08 * (e.t * 2.0).sin(), false, w);
            }
        }
    }

    pub fn draw_message(&self) {
        if let Some((m, t)) = &self.message {
            if *t > 0.3 || ((t * 20.0) as i32) % 2 == 0 {
                draw_text_px_centered(m, SCREEN_W / 2.0, 150.0, 3.0, Color::new(1.0, 0.95, 0.7, 1.0), Some(Color::new(0.1, 0.05, 0.2, 1.0)));
            }
        }
        match self.phase {
            Phase::Clear(t) => {
                draw_text_px_centered("BASE CORE DESTROYED", SCREEN_W / 2.0, 130.0, 4.0, Color::new(1.0, 0.85, 0.3, 1.0), Some(Color::new(0.2, 0.05, 0.1, 1.0)));
                if t > 1.0 {
                    draw_text_px_centered("LEVEL 1 CLEAR", SCREEN_W / 2.0, 180.0, 3.0, WHITE, Some(Color::new(0.1, 0.05, 0.2, 1.0)));
                }
                if t > 2.0 {
                    for p in &self.players {
                        if p.joined {
                            let s = format!("{}UP  SCORE {:07}", p.id + 1, p.score);
                            let c = if p.id == 0 { Color::new(0.4, 0.95, 1.0, 1.0) } else { Color::new(1.0, 0.45, 0.95, 1.0) };
                            draw_text_px_centered(&s, SCREEN_W / 2.0, 230.0 + p.id as f32 * 26.0, 2.0, c, None);
                        }
                    }
                }
            }
            Phase::Over(_) => {
                draw_text_px_centered("GAME OVER", SCREEN_W / 2.0, 220.0, 6.0, Color::new(1.0, 0.3, 0.4, 1.0), Some(Color::new(0.1, 0.02, 0.1, 1.0)));
            }
            _ => {}
        }
    }
}

/// True if no terrain blocks the straight line from a to b.
fn line_of_sight(level: &Level, a: Vec2, b: Vec2) -> bool {
    let d = b - a;
    let n = (d.length() / 8.0) as i32;
    for i in 1..n {
        let p = a + d * (i as f32 / n as f32);
        if level.solid(p.x, p.y) {
            return false;
        }
    }
    true
}

/// Segment (a, b) against an axis-aligned box (centre c, half size h).
fn seg_box(a: Vec2, b: Vec2, c: Vec2, h: Vec2) -> bool {
    let n = 24;
    (0..=n).any(|i| {
        let p = a + (b - a) * (i as f32 / n as f32);
        (p.x - c.x).abs() <= h.x && (p.y - c.y).abs() <= h.y
    })
}

/// Draws a ground sprite with its bottom on the bottom of the enemy's hitbox.
fn ground_draw(sheet: &crate::assets::Sheet, frame: usize, e: &Enemy, flip: bool) {
    let h = sheet.size(1.0).y;
    let bottom = e.y + e.kind.half().1;
    sheet.draw(0, frame, e.x, bottom - h / 2.0 + 1.0, 1.0, 0.0, flip, WHITE);
}

/// Barrel length of each ceiling turret (art px), so lasers start at the muzzle.
fn turret_gun_len(var: usize) -> f32 {
    [19.0, 24.0, 22.0][var.min(2)]
}

/// Which fuel depot art each level thing gets: Some(0) A (shoot), Some(1) B (airstrip),
/// Some(2) C (crane). None for things that aren't fuel depots.
fn fuel_variants(level: &Level) -> Vec<Option<usize>> {
    let mut n = 0;
    level
        .things
        .iter()
        .map(|t| {
            if t.ch != 'F' {
                return None;
            }
            if level.col_at(t.x()).gtype == '=' {
                return Some(1);
            }
            n += 1;
            Some(if n % 3 == 0 { 2 } else { 0 })
        })
        .collect()
}
