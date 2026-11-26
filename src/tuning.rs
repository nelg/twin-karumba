//! Reads `assets/tuning.json` (the shared tuning file from the design brief).
//!
//! Rust note: `serde_json::Value` is a dynamic JSON tree, a bit like a Python dict.
//! We read it once at start-up into a plain struct (`Cfg`) with typed fields, so
//! the game loop never does string look-ups. Every value has a default, so a
//! missing key never crashes the game.

use serde_json::Value;

pub struct Tuning {
    v: Value,
}

impl Tuning {
    pub fn parse(text: &str) -> Tuning {
        // `unwrap_or` gives a fallback instead of crashing on bad JSON.
        let v = serde_json::from_str(text).unwrap_or(Value::Null);
        Tuning { v }
    }

    /// Walks a dotted path like "player.bomb.gravity".
    fn get(&self, path: &str) -> Option<&Value> {
        let mut cur = &self.v;
        for key in path.split('.') {
            cur = cur.get(key)?; // `?` returns None early if the key is missing
        }
        Some(cur)
    }

    pub fn f(&self, path: &str, default: f32) -> f32 {
        self.get(path).and_then(|v| v.as_f64()).map(|x| x as f32).unwrap_or(default)
    }

    pub fn arr(&self, path: &str, default: &[f32]) -> Vec<f32> {
        match self.get(path).and_then(|v| v.as_array()) {
            Some(a) => a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect(),
            None => default.to_vec(),
        }
    }

    pub fn s(&self, path: &str, default: &str) -> String {
        self.get(path).and_then(|v| v.as_str()).unwrap_or(default).to_string()
    }
}

/// All the numbers the game uses, read once from tuning.json.
/// Units: art pixels (the screen is 960x540 art px) and seconds.
#[derive(Clone)]
pub struct Cfg {
    // player
    pub scale: f32,
    pub speed_x: f32,
    pub speed_y: f32,
    pub mouse_speed: f32,
    pub bullet_speed: f32,
    pub fire_interval: f32,
    pub max_bullets: usize,
    pub forward_limit: f32,
    pub scroll_slow_back: f32,
    pub scroll_recover_s: f32,
    pub tilt_deg: f32,
    pub fuel_tank_s: f32,
    pub fuel_depot: f32,
    pub fuel_can: f32,
    pub fuel_low: f32,
    pub lives: i32,
    pub invuln_s: f32,
    pub hitbox_half: (f32, f32),
    // bombs
    pub bomb_max: usize,
    pub bomb_gravity: f32,
    pub bomb_angle_stopped: f32,
    pub bomb_angle_forward: f32,
    pub bomb_angle_backward: f32,
    pub bomb_jitter_angle: f32,
    pub bomb_jitter_gravity: f32,
    pub bomb_wobble_px: f32,
    pub bomb_wobble_hz: f32,
    pub bomb_wobble_pitch: f32,
    pub bomb_impact_pitch: f32,
    pub reload_delay: f32,
    pub reload_time: f32,
    pub reload_slide: f32,
    // laser
    pub laser_dwell: f32,
    // enemy shots
    pub e_bullet_speed: f32,
    pub e_shell_speed: f32,
    pub e_rocket_speed: f32,
    pub e_rocket_accel: f32,
    // enemies
    pub wh_enter_speed: f32,
    pub wh_unfurl_frac: f32,
    pub wh_omega: f32,
    pub wh_r0: f32,
    pub wh_growth: f32,
    pub wh_phase_step: f32,
    pub wh_drift: f32,
    pub drone_trigger: f32,
    pub drone_climb: f32,
    pub drone_tol: f32,
    pub drone_speed: f32,
    pub drone_launches: i32,
    pub fighter_speed: f32,
    pub fighter_turn: f32,
    pub fighter_burst: i32,
    pub fighter_gap: f32,
    pub fighter_range: (f32, f32),
    pub chop_hold: f32,
    pub chop_bob: f32,
    pub chop_follow: f32,
    pub chop_fire: f32,
    pub chop_stay: f32,
    pub balloon_bob: f32,
    pub balloon_period: f32,
    pub balloon_blast: f32,
    pub tower_turn: f32,
    pub tower_burst: i32,
    pub tower_cooldown: f32,
    pub tower_range: f32,
    pub tank_slope: f32,
    pub tank_speed: f32,
    pub tank_elev: Vec<f32>,
    pub tank_tol: f32,
    pub tank_cooldown: f32,
    pub truck_speed: f32,
    pub truck_elev: f32,
    pub truck_salvo: i32,
    pub truck_gap: f32,
    pub truck_cooldown: f32,
    pub rocket_trigger: f32,
    pub rocket_speed: f32,
    pub rocket_accel: f32,
    pub flak_cooldown: f32,
    pub flak_radius: f32,
    pub flak_time: f32,
    pub turret_arc: (f32, f32),
    pub turret_telegraph: f32,
    pub turret_on: f32,
    pub turret_cooldown: f32,
    pub mine_drift: f32,
    pub rocks_trigger: f32,
    pub rocks_gravity: f32,
    pub radar_range: f32,
    pub radar_time: f32,
    // two players
    pub overlap_fade: f32,
    pub top_alpha: f32,
    pub retarget_s: f32,
    // shield
    pub shield_shot: f32,
    pub ram_balloon: f32,
    pub ram_drone: f32,
    pub ram_fighter: f32,
    pub ram_whirler: f32,
    // level flow (prototype values)
    pub cave_factor: f32,
    pub drop_chance: f32,
    pub pickup_life: f32,
    pub life_first: i32,
    pub life_every: i32,
    pub hoist_time: f32,
    pub hoist_height: f32,
    pub armour_hits: i32,
    pub boss_core_hp: i32,
    pub boss_pod_hp: i32,
}

impl Cfg {
    pub fn from(t: &Tuning) -> Cfg {
        let pair = |p: &str, a: f32, b: f32| {
            let v = t.arr(p, &[a, b]);
            (v.first().copied().unwrap_or(a), v.get(1).copied().unwrap_or(b))
        };
        Cfg {
            scale: t.f("player.scale", 0.8),
            speed_x: t.f("player.flight.speed_x", 260.0),
            speed_y: t.f("player.flight.speed_y", 240.0),
            mouse_speed: t.f("player.flight.mouse_speed", 300.0),
            bullet_speed: t.f("player.bullet_speed", 1600.0),
            fire_interval: t.f("player.fire_interval", 0.1),
            max_bullets: t.f("player.max_bullets_on_screen", 5.0) as usize,
            forward_limit: t.f("player.flight.forward_limit_screen_frac", 0.75),
            scroll_slow_back: t.f("player.flight.scroll_slow_when_back", 0.6),
            scroll_recover_s: t.f("player.flight.scroll_recover_s", 0.3),
            tilt_deg: t.f("player.flight.tilt_deg", 7.0),
            fuel_tank_s: t.f("player.flight.fuel_full_tank_s", 45.0),
            fuel_depot: t.f("player.flight.fuel_depot_refill", 0.25),
            fuel_can: t.f("player.flight.fuel_can_refill", 0.25),
            fuel_low: t.f("player.flight.fuel_low", 0.2),
            lives: t.f("player.flight.lives", 3.0) as i32,
            invuln_s: t.f("player.flight.respawn_invulnerable_s", 2.0),
            hitbox_half: pair("player.flight.hitbox_half", 44.0, 13.0),
            bomb_max: t.f("player.bomb.max_falling", 3.0) as usize,
            bomb_gravity: t.f("player.bomb.gravity", 1351.6),
            bomb_angle_stopped: t.f("player.bomb.angle_stopped", 20.0),
            bomb_angle_forward: t.f("player.bomb.angle_forward", 40.0),
            bomb_angle_backward: t.f("player.bomb.angle_backward", -20.0),
            bomb_jitter_angle: t.f("player.bomb.jitter_angle", 4.0),
            bomb_jitter_gravity: t.f("player.bomb.jitter_gravity", 0.06),
            bomb_wobble_px: t.f("player.bomb.wobble_px", 3.0),
            bomb_wobble_hz: t.f("player.bomb.wobble_hz", 6.0),
            bomb_wobble_pitch: t.f("player.bomb.wobble_pitch_deg", 7.0),
            bomb_impact_pitch: t.f("player.bomb.impact_pitch", 60.0),
            reload_delay: t.f("player.bomb.mount_reload.delay_s", 0.1),
            reload_time: t.f("player.bomb.mount_reload.time_s", 0.3),
            reload_slide: t.f("player.bomb.mount_reload.slide_art_px", 12.5),
            laser_dwell: t.f("player.laser.dwell_to_kill", 0.1),
            e_bullet_speed: t.f("enemy_shots.bullet_speed", 1100.0),
            e_shell_speed: t.f("enemy_shots.shell_speed", 900.0),
            e_rocket_speed: t.f("enemy_shots.rocket_speed_start", 450.0),
            e_rocket_accel: t.f("enemy_shots.rocket_accel", 900.0),
            wh_enter_speed: t.f("whirler_swarm.enter_speed", 220.0),
            wh_unfurl_frac: 0.65,
            wh_omega: t.f("whirler_swarm.orbit_omega", 3.0),
            wh_r0: t.f("whirler_swarm.orbit_r0", 12.0),
            wh_growth: t.f("whirler_swarm.orbit_growth", 55.0),
            wh_phase_step: t.f("whirler_swarm.phase_step", 0.6),
            wh_drift: t.f("whirler_swarm.centre_drift_to_player_y", 40.0),
            drone_trigger: t.f("drone.trigger_ahead", 500.0),
            drone_climb: t.f("drone.climb_speed", 200.0),
            drone_tol: t.f("drone.match_y_tolerance", 10.0),
            drone_speed: t.f("drone.attack_speed", 320.0),
            drone_launches: t.f("drone.launches_per_pad", 2.0) as i32,
            fighter_speed: t.f("fighter.speed", 380.0),
            fighter_turn: t.f("fighter.turn_rate", 70.0),
            fighter_burst: t.f("fighter.burst", 3.0) as i32,
            fighter_gap: t.f("fighter.burst_gap", 0.1),
            fighter_range: pair("fighter.fire_range", 350.0, 700.0),
            chop_hold: t.f("scout_chopper.hold_ahead", 450.0),
            chop_bob: t.f("scout_chopper.bob", 20.0),
            chop_follow: t.f("scout_chopper.y_follow", 60.0),
            chop_fire: t.f("scout_chopper.fire_interval", 1.2),
            chop_stay: t.f("scout_chopper.stay", 6.0),
            balloon_bob: t.f("balloon.bob", 12.0),
            balloon_period: t.f("balloon.bob_period", 3.0),
            balloon_blast: t.f("balloon.blast_radius", 60.0),
            tower_turn: t.f("gun_tower.turn_rate", 90.0),
            tower_burst: t.f("gun_tower.burst", 2.0) as i32,
            tower_cooldown: t.f("gun_tower.cooldown", 1.5),
            tower_range: t.f("gun_tower.range", 600.0),
            tank_slope: t.f("tank.max_slope", 25.0),
            tank_speed: t.f("tank.drive_speed", 40.0),
            tank_elev: t.arr("tank.barrel_elevations", &[15.0, 35.0, 55.0]),
            tank_tol: t.f("tank.aim_tolerance", 6.0),
            tank_cooldown: t.f("tank.cooldown", 2.0),
            truck_speed: t.f("rocket_truck.drive_speed", 50.0),
            truck_elev: t.f("rocket_truck.launcher_elevation", 30.0),
            truck_salvo: t.f("rocket_truck.salvo", 3.0) as i32,
            truck_gap: t.f("rocket_truck.salvo_gap", 0.15),
            truck_cooldown: t.f("rocket_truck.cooldown", 4.0),
            rocket_trigger: t.f("ground_rocket.trigger_ahead", 200.0),
            rocket_speed: t.f("ground_rocket.speed_start", 300.0),
            rocket_accel: t.f("ground_rocket.accel", 700.0),
            flak_cooldown: t.f("flak_gun.cooldown", 1.8),
            flak_radius: t.f("flak_gun.burst_radius", 40.0),
            flak_time: t.f("flak_gun.burst_time", 0.3),
            turret_arc: pair("ceiling_turret.arc", 200.0, 340.0),
            turret_telegraph: t.f("ceiling_turret.telegraph", 0.5),
            turret_on: t.f("ceiling_turret.laser_on", 0.3),
            turret_cooldown: t.f("ceiling_turret.cooldown", 2.0),
            mine_drift: t.f("mine.drift_to_player", 30.0),
            rocks_trigger: t.f("falling_rocks.trigger_ahead", 40.0),
            rocks_gravity: t.f("falling_rocks.gravity", 900.0),
            radar_range: t.f("radar.disables_flak_within", 800.0),
            radar_time: t.f("radar.for_seconds", 10.0),
            overlap_fade: t.f("two_players.overlap_fade_threshold", 0.5),
            top_alpha: t.f("two_players.top_ship_alpha", 0.55),
            retarget_s: 2.0,
            shield_shot: t.f("shield.enemy_shot_cost", 0.3333),
            ram_balloon: t.f("shield.ram_cost.balloon", 1.0),
            ram_drone: t.f("shield.ram_cost.drone", 0.25),
            ram_fighter: t.f("shield.ram_cost.fighter", 0.5),
            ram_whirler: t.f("shield.ram_cost.whirler_segment", 0.15),
            cave_factor: t.f("level_flow.cave_scroll_factor", 0.7),
            drop_chance: t.f("level_flow.pickup_drop_chance", 0.18),
            pickup_life: t.f("level_flow.pickup_life_s", 8.0),
            life_first: t.f("level_flow.extra_life_first", 10000.0) as i32,
            life_every: t.f("level_flow.extra_life_every", 30000.0) as i32,
            hoist_time: t.f("level_flow.hoist_time_s", 1.6),
            hoist_height: t.f("level_flow.hoist_max_height", 152.0),
            armour_hits: t.f("level_flow.armour_bullet_hits", 2.0) as i32,
            boss_core_hp: t.f("level_flow.boss_core_hp", 16.0) as i32,
            boss_pod_hp: t.f("level_flow.boss_pod_hp", 6.0) as i32,
        }
    }
}
