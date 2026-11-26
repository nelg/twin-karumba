//! Level loader: reads the shared ASCII level file (levels/level1.txt).
//!
//! Format (see the design brief): `key: value` header lines, a `---` line, then
//! 24 rows. Row 0 is the marker lane (section digits, palette letters, `<` `>`),
//! rows 1..23 are the playfield. Each character is one tile, 22.5 art px square.
//!
//! This follows `levels/src/level_render.py` (the reference parser) so all three
//! prototypes read the file the same way.

pub const TILE: f32 = 22.5;
pub const SCREEN_W: f32 = 960.0;
pub const SCREEN_H: f32 = 540.0;

/// Terrain characters that are solid.
pub fn is_solid(c: char) -> bool {
    matches!(c, '#' | '=' | '~' | '▲' | '█')
}

/// Palette names, in the order of the marker letters a..f.
pub const PALETTES: [&str; 6] = ["green", "pink", "blue", "orange", "purple", "teal"];

#[derive(Clone, Debug)]
pub struct Thing {
    pub col: usize,
    pub row: usize, // playfield row, 0 = just under the HUD lane
    pub ch: char,
}

impl Thing {
    /// Centre x of the tile, in world art px.
    pub fn x(&self) -> f32 {
        self.col as f32 * TILE + TILE / 2.0
    }
    /// Centre y of the tile, in art px.
    pub fn y(&self) -> f32 {
        (self.row as f32 + 1.0) * TILE + TILE / 2.0
    }
}

/// Per-column info from the grid.
#[derive(Clone, Copy)]
pub struct Column {
    pub ground: usize, // solid tiles counted up from the bottom
    pub gtype: char,   // type of the top ground tile
    pub ceil: usize,   // solid tiles counted down from the top of the playfield
    pub ctype: char,
}

pub struct Level {
    pub name: String,
    pub scroll: f32, // tiles per second
    pub cols: Vec<Column>,
    pub marks: Vec<char>,
    pub things: Vec<Thing>,
    pub pal: Vec<usize>, // palette index per column
    /// Surface heights per art px (index = world x). `ground_y[x]` is the y of the
    /// top of the land; `ceil_y[x]` is the y of the bottom of the cave roof (0 = none).
    pub ground_y: Vec<f32>,
    pub ceil_y: Vec<f32>,
    pub width: f32, // art px
}

impl Level {
    pub fn parse(text: &str) -> Level {
        // Windows line endings (CRLF, e.g. from a git checkout on Windows) become plain \n,
        // or the header and the rows would be misread.
        let text = &text.replace("\r\n", "\n");
        // Split header and grid. `split_once` returns Option<(&str, &str)>.
        let (head, grid) = text.split_once("---\n").unwrap_or(("", text));
        let mut name = String::from("Level");
        let mut scroll = 3.0;
        let mut rows = 24;
        for line in head.lines() {
            if let Some((k, v)) = line.split_once(':') {
                match k.trim() {
                    "name" => name = v.trim().to_string(),
                    "scroll" => scroll = v.trim().parse().unwrap_or(3.0),
                    "tile_rows" => rows = v.trim().parse().unwrap_or(24),
                    _ => {}
                }
            }
        }
        // Columns count characters (code points), not bytes, so collect into Vec<char>.
        let mut lines: Vec<Vec<char>> = grid.trim_end_matches('\n').split('\n').map(|l| l.chars().collect()).collect();
        lines.resize(rows, Vec::new());
        let width = lines.iter().map(|l| l.len()).max().unwrap_or(0);
        for l in lines.iter_mut() {
            l.resize(width, ' ');
        }
        let marks = lines[0].clone();
        let play = &lines[1..];
        let pf = play.len();

        let mut cols = Vec::with_capacity(width);
        for x in 0..width {
            let mut g = 0;
            while g < pf && is_solid(play[pf - 1 - g][x]) {
                g += 1;
            }
            let mut c = 0;
            while c < pf - g && is_solid(play[c][x]) {
                c += 1;
            }
            cols.push(Column {
                ground: g,
                gtype: if g > 0 { play[pf - g][x] } else { ' ' },
                ceil: c,
                ctype: if c > 0 { play[c - 1][x] } else { ' ' },
            });
        }
        let mut things = Vec::new();
        for (y, row) in play.iter().enumerate() {
            for (x, &ch) in row.iter().enumerate() {
                if !is_solid(ch) && ch != ' ' && ch != '.' {
                    things.push(Thing { col: x, row: y, ch });
                }
            }
        }
        things.sort_by(|a, b| a.col.cmp(&b.col));
        let mut pal = Vec::with_capacity(width);
        let mut cur = 0;
        for &m in &marks {
            if let Some(i) = "abcdef".find(m) {
                cur = i;
            }
            pal.push(cur);
        }
        let mut lvl = Level {
            name,
            scroll,
            cols,
            marks,
            things,
            pal,
            ground_y: Vec::new(),
            ceil_y: Vec::new(),
            width: width as f32 * TILE,
        };
        lvl.build_surfaces(pf);
        lvl
    }

    /// Smooths the blocky grid into surfaces, like the reference parser:
    /// meadows (~) roll, rock slopes, cliffs (█) and metal (=) keep hard edges.
    fn build_surfaces(&mut self, pf: usize) {
        let h = (pf as f32 + 1.0) * TILE;
        let n = self.cols.len();
        let w = self.width as usize;
        let top = |c: &Column| h - c.ground as f32 * TILE;
        let bot = |c: &Column| if c.ceil > 0 { TILE + c.ceil as f32 * TILE } else { 0.0 };
        let interp = |x: f32, ceiling: bool| -> f32 {
            let i = ((x / TILE) as usize).min(n - 1);
            let f = (x % TILE) / TILE;
            let j = if f >= 0.5 { (i + 1).min(n - 1) } else { i.saturating_sub(1) };
            let (ci, cj) = (&self.cols[i], &self.cols[j]);
            let (a, mut b) = if ceiling { (bot(ci), bot(cj)) } else { (top(ci), top(cj)) };
            if ceiling && a == 0.0 {
                return 0.0;
            }
            if ceiling && b == 0.0 {
                b = a;
            }
            let (ti, tj) = if ceiling { (ci.ctype, cj.ctype) } else { (ci.gtype, cj.gtype) };
            let hard = |t: char| t == '█' || t == '=';
            if hard(ti) || hard(tj) {
                return a;
            }
            let t = (f - 0.5).abs();
            let s = if ti == '~' || tj == '~' { t * t * (3.0 - 2.0 * t) } else { t };
            a + (b - a) * s
        };
        let mut gs: Vec<f32> = (0..w).map(|x| interp(x as f32, false)).collect();
        let cs: Vec<f32> = (0..w).map(|x| interp(x as f32, true)).collect();
        // two light smoothing passes on soft ground
        for _ in 0..2 {
            let prev = gs.clone();
            for x in 0..w {
                let t = self.cols[((x as f32 / TILE) as usize).min(n - 1)].gtype;
                if t != '█' && t != '=' {
                    gs[x] = (prev[x.saturating_sub(3)] + prev[x] + prev[(x + 3).min(w - 1)]) / 3.0;
                }
            }
        }
        self.ground_y = gs;
        self.ceil_y = cs;
    }

    pub fn col_at(&self, x: f32) -> &Column {
        let i = ((x / TILE).max(0.0) as usize).min(self.cols.len() - 1);
        &self.cols[i]
    }

    /// Ground surface y at world x (art px). Off the ends of the level counts as the edge value.
    pub fn ground(&self, x: f32) -> f32 {
        let i = (x.max(0.0) as usize).min(self.ground_y.len() - 1);
        self.ground_y[i]
    }

    /// Cave roof bottom y at world x, or 0.0 when there is no roof.
    pub fn ceil(&self, x: f32) -> f32 {
        let i = (x.max(0.0) as usize).min(self.ceil_y.len() - 1);
        self.ceil_y[i]
    }

    /// True if the point is inside solid land. The HUD lane (y < TILE) counts as open sky.
    pub fn solid(&self, x: f32, y: f32) -> bool {
        y >= self.ground(x) || (y <= self.ceil(x) && self.ceil(x) > 0.0)
    }

    /// Slope angle of the ground at x in degrees (0 = flat). Positive = rising to the right.
    pub fn slope_deg(&self, x: f32) -> f32 {
        let a = self.ground(x - 4.0);
        let b = self.ground(x + 4.0);
        ((a - b) / 8.0).atan().to_degrees()
    }

    /// Section start columns (digits in the marker lane) as (col, digit).
    pub fn sections(&self) -> Vec<(usize, u32)> {
        self.marks.iter().enumerate().filter_map(|(i, c)| c.to_digit(10).map(|d| (i, d))).collect()
    }

    /// Scroll speed factor at world x: `<` slows (cave), `>` restores.
    pub fn scroll_factor(&self, x: f32, cave_factor: f32) -> f32 {
        let col = ((x / TILE) as usize).min(self.marks.len() - 1);
        for i in (0..=col).rev() {
            match self.marks[i] {
                '<' => return cave_factor,
                '>' => return 1.0,
                _ => {}
            }
        }
        1.0
    }

    pub fn palette_at_col(&self, col: usize) -> usize {
        self.pal[col.min(self.pal.len() - 1)]
    }
}
