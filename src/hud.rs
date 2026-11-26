//! The HUD bar: 23 art px across the top, over the level's marker lane.
//! P1 on the left, P2 on the right, high score and section lights in the middle
//! (layout from hud/src/hud_art.py).

use crate::assets::Sprites;
use crate::game::{Game, HUD_H};
use crate::level::SCREEN_W;
use crate::pixel::{draw_text_px, draw_text_px_centered, text_width};
use macroquad::prelude::*;

const OL: Color = Color::new(14.0 / 255.0, 12.0 / 255.0, 26.0 / 255.0, 1.0);

pub fn player_color(i: usize) -> Color {
    if i == 0 { Color::new(0.35, 0.92, 1.0, 1.0) } else { Color::new(1.0, 0.42, 0.92, 1.0) }
}

fn bar(x: f32, y: f32, w: f32, h: f32, frac: f32, cols: [Color; 3]) {
    draw_rectangle(x - 1.0, y - 1.0, w + 2.0, h + 2.0, OL);
    draw_rectangle(x, y, w, h, Color::new(34.0 / 255.0, 30.0 / 255.0, 56.0 / 255.0, 1.0));
    let n = (w * frac.clamp(0.0, 1.0)).round();
    if n > 0.0 {
        draw_rectangle(x, y, n, h, cols[1]);
        draw_rectangle(x, y, n, 1.0, cols[0]);
        draw_rectangle(x, y + h - 1.0, n, 1.0, cols[2]);
    }
    // tick marks every 8 px
    let mut k = x + 8.0;
    while k < x + w {
        draw_rectangle(k, y + h - 1.0, 1.0, 1.0, OL);
        k += 8.0;
    }
}

pub fn draw(g: &Game, sp: &Sprites, t: f32) {
    draw_rectangle(0.0, 0.0, SCREEN_W, HUD_H, Color::new(20.0 / 255.0, 16.0 / 255.0, 38.0 / 255.0, 0.94));
    draw_rectangle(0.0, HUD_H - 2.0, SCREEN_W, 1.0, Color::new(90.0 / 255.0, 80.0 / 255.0, 140.0 / 255.0, 1.0));
    draw_rectangle(0.0, HUD_H - 1.0, SCREEN_W, 1.0, OL);
    for i in 0..2 {
        let p = &g.players[i];
        let col = player_color(i);
        let x0 = if i == 0 { 6.0 } else { SCREEN_W - 330.0 };
        if !p.joined {
            if ((t * 2.0) as i32) % 2 == 0 {
                draw_text_px("2UP  PRESS FIRE TO JOIN", x0 + 40.0, 8.0, 1.0, col, None);
            }
            continue;
        }
        draw_text_px(&format!("{}UP", i + 1), x0, 3.0, 1.0, col, None);
        draw_text_px(&format!("{:07}", p.score), x0, 10.0, 2.0, WHITE, Some(OL));
        // lives as tiny KX-2 icons
        let lx = x0 + 66.0;
        for k in 0..p.lives.clamp(0, 6) {
            sp.kx2[i].draw(0, 0, lx + 10.0 + k as f32 * 16.0, 13.0, 0.09, 0.0, false, WHITE);
        }
        if p.out {
            draw_text_px("GAME OVER", lx, 9.0, 1.0, Color::new(1.0, 0.4, 0.4, 1.0), None);
        }
        // gauges
        let gx = x0 + 170.0;
        let low = p.fuel < g.cfg.fuel_low;
        let fuel_col = if low {
            if ((t * 4.0) as i32) % 2 == 0 { Color::new(1.0, 0.25, 0.25, 1.0) } else { Color::new(0.5, 0.1, 0.1, 1.0) }
        } else {
            Color::new(1.0, 0.82, 0.29, 1.0)
        };
        draw_text_px("FUEL", gx, 4.0, 1.0, if low { fuel_col } else { Color::new(1.0, 0.9, 0.6, 1.0) }, None);
        bar(gx + 22.0, 4.0, 80.0, 5.0, p.fuel, [Color::new(1.0, 1.0, 0.8, 1.0), fuel_col, Color::new(0.6, 0.4, 0.1, 1.0)]);
        draw_text_px("SHLD", gx, 13.0, 1.0, col, None);
        let sc = Color::new(col.r * 0.8, col.g * 0.8, col.b * 0.8, 1.0);
        bar(gx + 22.0, 13.0, 80.0, 5.0, p.shield, [WHITE, sc, Color::new(col.r * 0.4, col.g * 0.4, col.b * 0.5, 1.0)]);
        if p.laser {
            draw_text_px("LASER", gx + 108.0, 8.0, 1.0, col, None);
        }
    }
    // centre: high score and section lights
    let cx = SCREEN_W / 2.0;
    let hi = format!("HI {:07}", g.hiscore);
    draw_text_px(&hi, cx - text_width(&hi, 1.0) / 2.0, 3.0, 1.0, Color::new(1.0, 0.9, 0.6, 1.0), None);
    let (cur, n) = g.section_info();
    let sx = cx - (n as f32 * 10.0) / 2.0;
    for k in 0..n {
        let x = sx + k as f32 * 10.0;
        draw_rectangle(x, 12.0, 8.0, 6.0, OL);
        let c = if k < cur {
            Color::new(1.0, 0.82, 0.29, 1.0)
        } else if k == cur {
            if ((t * 4.0) as i32) % 2 == 0 { WHITE } else { Color::new(0.24, 0.21, 0.35, 1.0) }
        } else {
            Color::new(0.24, 0.21, 0.35, 1.0)
        };
        draw_rectangle(x + 1.0, 13.0, 6.0, 4.0, c);
    }
    let _ = draw_text_px_centered;
}
