//! A small FastTracker 2 (.xm) module player.
//!
//! The music is note data plus short instrument samples (a few hundred KB), and we
//! play it live, so the game can steer it: mute or fade channels (chip lead vs.
//! guitar lead, drums), jump to another part of the song at a checkpoint, or ease
//! the tempo in the cave.
//!
//! What's supported is what our generated tracks use, plus the common effects most
//! .xm files need:
//! - linear frequency table, 8- and 16-bit samples, forward and ping-pong loops
//! - volume and panning envelopes (sustain, loop), fadeout after key-off
//! - effects 0 arpeggio, 1/2 portamento, 3 tone portamento, 4 vibrato, 5/6 combos,
//!   8 pan, 9 sample offset, A volume slide, B jump, C volume, D pattern break,
//!   E1/E2 fine porta, EA/EB fine volume, EC note cut, ED note delay,
//!   F speed/BPM, G global volume, K key-off
//! - the volume column (set, slides, fine slides, pan)
//!
//! How a tracker works, in short: a song is a list of patterns ("orders"). A pattern
//! is a grid of rows x channels; each cell can start a note with an instrument, set
//! a volume and apply an effect. Every row lasts `speed` ticks, and a tick lasts
//! 2.5 / BPM seconds. Effects like slides and vibrato update once per tick.

use std::sync::Arc;

// ------------------------------------------------------------------ the module (file data)

#[derive(Clone, Copy, Default)]
pub struct Cell {
    pub note: u8, // 0 none, 1..=96 C-0..B-7, 97 key-off
    pub ins: u8,  // 0 none, else 1-based instrument
    pub vol: u8,  // volume column byte
    pub fx: u8,
    pub param: u8,
}

pub struct Pattern {
    pub rows: usize,
    pub cells: Vec<Cell>, // rows * channels
}

pub struct Sample {
    pub data: Vec<f32>, // mono, -1..1
    pub loop_start: usize,
    pub loop_len: usize,
    pub loop_kind: u8, // 0 none, 1 forward, 2 ping-pong
    pub volume: u8,    // 0..64
    pub finetune: i8,  // -128..127 (1/128 semitone)
    pub pan: u8,
    pub relnote: i8,
}

#[derive(Clone, Default)]
pub struct Envelope {
    pub points: Vec<(u16, u16)>, // (tick, value 0..64)
    pub on: bool,
    pub sustain: Option<usize>,
    pub looped: Option<(usize, usize)>,
}

pub struct Instrument {
    pub keymap: [u8; 96],
    pub samples: Vec<Sample>,
    pub vol_env: Envelope,
    pub pan_env: Envelope,
    pub fadeout: u16,
}

pub struct Module {
    pub name: String,
    pub channels: usize,
    pub orders: Vec<u8>,
    pub restart: usize,
    pub speed: u32,
    pub bpm: u32,
    pub patterns: Vec<Pattern>,
    pub instruments: Vec<Instrument>,
}

/// Little helpers to read numbers out of the byte slice without panicking.
/// Rust note: `get(..)` returns an `Option`, so a short or broken file gives
/// `None` (and `?` passes that up) instead of crashing the game.
fn u8_at(b: &[u8], i: usize) -> Option<u8> {
    b.get(i).copied()
}
fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
}
fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

impl Module {
    pub fn parse(b: &[u8]) -> Option<Module> {
        if b.get(0..17)? != b"Extended Module: " {
            return None;
        }
        let name = String::from_utf8_lossy(b.get(17..37)?).trim_end_matches('\0').trim().to_string();
        let hdr = u32_at(b, 60)? as usize;
        let song_len = u16_at(b, 64)? as usize;
        let restart = u16_at(b, 66)? as usize;
        let channels = (u16_at(b, 68)? as usize).clamp(1, 32);
        let npat = u16_at(b, 70)? as usize;
        let nins = u16_at(b, 72)? as usize;
        let speed = u16_at(b, 76)?.max(1) as u32;
        let bpm = u16_at(b, 78)?.max(32) as u32;
        let orders = b.get(80..80 + song_len.min(256))?.to_vec();

        // patterns
        let mut p = 60 + hdr;
        let mut patterns = Vec::with_capacity(npat);
        for _ in 0..npat {
            let plen = u32_at(b, p)? as usize;
            let rows = (u16_at(b, p + 5)? as usize).max(1);
            let size = u16_at(b, p + 7)? as usize;
            let mut data = &b[(p + plen).min(b.len())..(p + plen + size).min(b.len())];
            let mut cells = vec![Cell::default(); rows * channels];
            if size > 0 {
                for c in cells.iter_mut() {
                    // packed format: a first byte with the top bit set says which fields follow
                    let mut next = || -> u8 {
                        let v = data.first().copied().unwrap_or(0);
                        data = data.get(1..).unwrap_or(&[]);
                        v
                    };
                    let first = next();
                    if first & 0x80 != 0 {
                        if first & 1 != 0 { c.note = next(); }
                        if first & 2 != 0 { c.ins = next(); }
                        if first & 4 != 0 { c.vol = next(); }
                        if first & 8 != 0 { c.fx = next(); }
                        if first & 16 != 0 { c.param = next(); }
                    } else {
                        c.note = first;
                        c.ins = next();
                        c.vol = next();
                        c.fx = next();
                        c.param = next();
                    }
                }
            }
            patterns.push(Pattern { rows, cells });
            p += plen + size;
        }

        // instruments, each followed by its sample headers and sample data
        let mut instruments = Vec::with_capacity(nins);
        for _ in 0..nins {
            let isize = u32_at(b, p)? as usize;
            let nsamp = u16_at(b, p + 27)? as usize;
            let mut ins = Instrument {
                keymap: [0; 96],
                samples: Vec::new(),
                vol_env: Envelope::default(),
                pan_env: Envelope::default(),
                fadeout: 0,
            };
            if nsamp == 0 {
                p += isize;
                instruments.push(ins);
                continue;
            }
            let shsize = u32_at(b, p + 29)? as usize;
            ins.keymap.copy_from_slice(b.get(p + 33..p + 129)?);
            let env = |pts_at: usize, n: usize, sus: usize, ls: usize, le: usize, kind: u8| -> Envelope {
                let n = n.min(12);
                let points = (0..n)
                    .filter_map(|k| Some((u16_at(b, pts_at + k * 4)?, u16_at(b, pts_at + k * 4 + 2)?)))
                    .collect();
                Envelope {
                    points,
                    on: kind & 1 != 0 && n > 0,
                    sustain: if kind & 2 != 0 { Some(sus) } else { None },
                    looped: if kind & 4 != 0 { Some((ls, le)) } else { None },
                }
            };
            let g = |o: usize| u8_at(b, p + o).unwrap_or(0) as usize;
            ins.vol_env = env(p + 129, g(225), g(227), g(228), g(229), g(233) as u8);
            ins.pan_env = env(p + 177, g(226), g(230), g(231), g(232), g(234) as u8);
            ins.fadeout = u16_at(b, p + 239).unwrap_or(0);
            p += isize;
            // sample headers
            let mut heads = Vec::new();
            for k in 0..nsamp {
                let h = p + k * shsize;
                heads.push((
                    u32_at(b, h)? as usize,
                    u32_at(b, h + 4)? as usize,
                    u32_at(b, h + 8)? as usize,
                    u8_at(b, h + 12)?,
                    u8_at(b, h + 13)? as i8,
                    u8_at(b, h + 14)?,
                    u8_at(b, h + 15)?,
                    u8_at(b, h + 16)? as i8,
                ));
            }
            p += nsamp * shsize;
            // sample data: stored as differences from the previous value ("delta" coding)
            for (len, ls, ll, vol, fine, kind, pan, rel) in heads {
                let bits16 = kind & 0x10 != 0;
                let raw = b.get(p..(p + len).min(b.len())).unwrap_or(&[]);
                p += len;
                let mut data = Vec::with_capacity(len);
                if bits16 {
                    let mut acc: i16 = 0;
                    for c in raw.chunks_exact(2) {
                        acc = acc.wrapping_add(i16::from_le_bytes([c[0], c[1]]));
                        data.push(acc as f32 / 32768.0);
                    }
                } else {
                    let mut acc: i8 = 0;
                    for &v in raw {
                        acc = acc.wrapping_add(v as i8);
                        data.push(acc as f32 / 128.0);
                    }
                }
                let w = if bits16 { 2 } else { 1 };
                let (mut ls, mut ll) = (ls / w, ll / w);
                if ls > data.len() { ls = 0; ll = 0; }
                ll = ll.min(data.len() - ls);
                let loop_kind = if ll > 1 { kind & 3 } else { 0 };
                ins.samples.push(Sample {
                    data,
                    loop_start: ls,
                    loop_len: ll,
                    loop_kind: if loop_kind == 3 { 1 } else { loop_kind },
                    volume: vol.min(64),
                    finetune: fine,
                    pan,
                    relnote: rel,
                });
            }
            instruments.push(ins);
        }
        Some(Module { name, channels, orders, restart, speed, bpm, patterns, instruments })
    }
}

impl Envelope {
    /// Value (0..64) at envelope tick `t`, interpolated between points.
    fn value(&self, t: u16) -> f32 {
        let pts = &self.points;
        if pts.is_empty() {
            return 64.0;
        }
        if t <= pts[0].0 {
            return pts[0].1 as f32;
        }
        for w in pts.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            if t < x1 {
                let k = if x1 > x0 { (t - x0) as f32 / (x1 - x0) as f32 } else { 0.0 };
                return y0 as f32 + (y1 as f32 - y0 as f32) * k;
            }
        }
        pts[pts.len() - 1].1 as f32
    }
    /// Moves the envelope position one tick on, honouring sustain and loop points.
    fn advance(&self, t: u16, key_on: bool) -> u16 {
        let pts = &self.points;
        if pts.is_empty() {
            return t;
        }
        if key_on {
            if let Some(s) = self.sustain {
                if let Some(&(x, _)) = pts.get(s) {
                    if t >= x {
                        return x;
                    }
                }
            }
        }
        let mut n = t.saturating_add(1);
        if let Some((ls, le)) = self.looped {
            if let (Some(&(xs, _)), Some(&(xe, _))) = (pts.get(ls), pts.get(le)) {
                if n > xe {
                    n = xs;
                }
            }
        }
        n.min(pts[pts.len() - 1].0)
    }
}

// ------------------------------------------------------------------ the player

#[derive(Default, Clone)]
struct Chan {
    ins: usize,           // 1-based, 0 none
    smp: Option<usize>,   // sample index inside the instrument
    note: i32,            // final note (with relnote), 0..119
    period: f32,          // linear period (lower = higher pitch)
    target: f32,          // tone portamento target period
    pos: f64,             // position in the sample, in sample frames
    dir: f64,             // +1 forward, -1 backward (ping-pong)
    active: bool,
    vol: i32,             // 0..64
    pan: i32,             // 0..255
    key_on: bool,
    fade: i32,            // 0..65536 fadeout volume after key-off
    venv_t: u16,
    penv_t: u16,
    // effect memory
    fx: u8,
    param: u8,
    volfx: u8,
    porta_up: u8,
    porta_down: u8,
    porta_speed: u8,
    vib_speed: u8,
    vib_depth: u8,
    vib_pos: u8,
    vol_slide: u8,
    fine_up: u8,
    fine_down: u8,
    offset: u8,
    arp: i32,     // semitones added this tick by arpeggio
    vib: f32,     // period offset from vibrato this tick
    cut_at: i32,  // tick to cut the note on (EC), -1 none
    delay: Option<(u8, Cell)>, // note delay (ED): tick and the cell to trigger then
    // smoothed output gains, to avoid clicks
    gl: f32,
    gr: f32,
}

pub struct Player {
    pub module: Arc<Module>,
    rate: f32,
    chans: Vec<Chan>,
    order: usize,
    row: usize,
    tick: u32,
    speed: u32,
    bpm: u32,
    global_vol: i32,
    tick_left: f32,     // output frames left in this tick
    pending: Option<(usize, usize)>, // (order, row) to go to after this row (B/D effects)
    /// Game controls: per-channel mix gain (fades towards `target_gain`).
    gain: Vec<f32>,
    target_gain: Vec<f32>,
    pub fade_per_s: f32,
    /// Tempo multiplier (1.0 = as written).
    pub tempo_scale: f32,
    /// Jump to this order when the current pattern ends.
    queued_order: Option<usize>,
    pub volume: f32,
}

impl Player {
    pub fn new(module: Arc<Module>, rate: u32) -> Player {
        let n = module.channels;
        let mut p = Player {
            rate: rate as f32,
            chans: vec![Chan { pan: 128, dir: 1.0, cut_at: -1, ..Default::default() }; n],
            order: 0,
            row: 0,
            tick: 0,
            speed: module.speed,
            bpm: module.bpm,
            global_vol: 64,
            tick_left: 0.0,
            pending: None,
            gain: vec![1.0; n],
            target_gain: vec![1.0; n],
            fade_per_s: 1.5,
            tempo_scale: 1.0,
            queued_order: None,
            volume: 1.0,
            module,
        };
        p.goto(0);
        p
    }

    pub fn channels(&self) -> usize {
        self.chans.len()
    }

    /// Sets how loud a channel should be (0..1); it fades there at `fade_per_s`.
    pub fn set_channel_gain(&mut self, ch: usize, g: f32) {
        if let Some(t) = self.target_gain.get_mut(ch) {
            *t = g;
        }
    }
    /// Like `set_channel_gain` but with no fade.
    pub fn snap_channel_gain(&mut self, ch: usize, g: f32) {
        if ch < self.gain.len() {
            self.gain[ch] = g;
            self.target_gain[ch] = g;
        }
    }

    /// Jumps to an order at once (and stops all notes).
    pub fn goto(&mut self, order: usize) {
        self.order = order.min(self.module.orders.len().saturating_sub(1));
        self.row = 0;
        self.tick = 0;
        self.tick_left = 0.0;
        self.pending = None;
        self.queued_order = None;
        for c in self.chans.iter_mut() {
            c.active = false;
        }
    }

    /// Jumps to an order when the pattern playing now ends, so it stays in time.
    pub fn queue_order(&mut self, order: usize) {
        self.queued_order = Some(order);
    }

    pub fn order(&self) -> usize {
        self.order
    }

    fn pattern(&self) -> Option<&Pattern> {
        let pi = *self.module.orders.get(self.order)? as usize;
        self.module.patterns.get(pi)
    }

    /// Renders `out.len() / 2` stereo frames, *adding* into `out`.
    pub fn mix(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        let mut k = 0;
        while k < frames {
            if self.tick_left <= 0.0 {
                self.do_tick();
                let bpm = (self.bpm as f32 * self.tempo_scale).max(16.0);
                self.tick_left += self.rate * 2.5 / bpm;
            }
            let n = (self.tick_left.ceil() as usize).clamp(1, frames - k);
            self.render(&mut out[k * 2..(k + n) * 2]);
            self.tick_left -= n as f32;
            k += n;
        }
    }

    // --------------------------------------------------------- per-tick song logic

    fn do_tick(&mut self) {
        if self.tick == 0 {
            self.do_row();
        } else {
            for ch in 0..self.chans.len() {
                self.tick_effects(ch);
            }
        }
        // envelopes, fadeout, and the channel volume fades the game asked for
        let step = self.fade_per_s * 2.5 / self.bpm.max(1) as f32;
        for ch in 0..self.chans.len() {
            let g = &mut self.gain[ch];
            let t = self.target_gain[ch];
            *g = if *g < t { (*g + step).min(t) } else { (*g - step).max(t) };
            let c = &mut self.chans[ch];
            if let Some(ins) = self.module.instruments.get(c.ins.wrapping_sub(1)) {
                if ins.vol_env.on {
                    c.venv_t = ins.vol_env.advance(c.venv_t, c.key_on);
                }
                if ins.pan_env.on {
                    c.penv_t = ins.pan_env.advance(c.penv_t, c.key_on);
                }
                if !c.key_on {
                    c.fade = (c.fade - ins.fadeout as i32 * 2).max(0);
                    if !ins.vol_env.on {
                        c.fade = 0; // key-off without an envelope cuts the note
                    }
                }
            }
        }
        self.tick += 1;
        if self.tick >= self.speed {
            self.tick = 0;
            self.next_row();
        }
    }

    fn next_row(&mut self) {
        let rows = self.pattern().map(|p| p.rows).unwrap_or(64);
        if let Some((o, r)) = self.pending.take() {
            self.order = o;
            self.row = r;
        } else {
            self.row += 1;
            if self.row >= rows {
                self.row = 0;
                self.order += 1;
            }
        }
        if self.row == 0 {
            if let Some(q) = self.queued_order.take() {
                self.order = q;
            }
        }
        if self.order >= self.module.orders.len() {
            self.order = self.module.restart.min(self.module.orders.len().saturating_sub(1));
        }
        let rows = self.pattern().map(|p| p.rows).unwrap_or(64);
        if self.row >= rows {
            self.row = 0;
        }
    }

    fn do_row(&mut self) {
        let n = self.chans.len();
        for ch in 0..n {
            let cell = match self.pattern() {
                Some(p) => p.cells.get(self.row * n + ch).copied().unwrap_or_default(),
                None => Cell::default(),
            };
            // ED: note delay, trigger the whole cell later in the row
            if cell.fx == 0x0E && cell.param >> 4 == 0xD && cell.param & 15 > 0 {
                self.chans[ch].delay = Some((cell.param & 15, cell));
                self.chans[ch].fx = 0;
                continue;
            }
            self.chans[ch].delay = None;
            self.row_cell(ch, cell);
        }
    }

    fn row_cell(&mut self, ch: usize, cell: Cell) {
        let m = self.module.clone(); // cheap: it's an Arc
        // Rust note: we borrow several fields of `self` at once by destructuring it;
        // `&mut self.chans[ch]` alone would lock all of `self` while `c` is alive.
        let Player { chans, pending, speed, bpm, global_vol, order, .. } = self;
        let c = &mut chans[ch];
        c.fx = cell.fx;
        c.param = cell.param;
        c.volfx = cell.vol;
        c.arp = 0;
        c.vib = 0.0;
        c.cut_at = -1;
        let porta = cell.fx == 3 || cell.fx == 5 || cell.vol >> 4 == 0xF;

        // instrument number alone: reset volume and envelopes
        if cell.ins > 0 {
            c.ins = cell.ins as usize;
            if let Some(ins) = m.instruments.get(c.ins - 1) {
                if let Some(s) = c.smp.and_then(|i| ins.samples.get(i)) {
                    c.vol = s.volume as i32;
                    c.pan = s.pan as i32;
                }
            }
            c.key_on = true;
            c.fade = 65536;
            c.venv_t = 0;
            c.penv_t = 0;
        }
        if cell.note == 97 {
            c.key_on = false;
        } else if cell.note >= 1 && cell.note <= 96 {
            if let Some(ins) = m.instruments.get(c.ins.wrapping_sub(1)) {
                let si = ins.keymap[(cell.note - 1) as usize] as usize;
                if let Some(s) = ins.samples.get(si) {
                    let note = cell.note as i32 - 1 + s.relnote as i32;
                    let period = 7680.0 - note as f32 * 64.0 - s.finetune as f32 / 2.0;
                    if porta && c.active {
                        c.target = period;
                    } else {
                        c.smp = Some(si);
                        c.note = note;
                        c.period = period;
                        c.target = period;
                        c.pos = 0.0;
                        c.dir = 1.0;
                        c.active = true;
                        c.vib_pos = 0;
                        c.gl = 0.0; // fade in over a few samples (no click)
                        c.gr = 0.0;
                        if cell.ins > 0 {
                            c.vol = s.volume as i32;
                            c.pan = s.pan as i32;
                        }
                        c.key_on = true;
                        c.fade = 65536;
                        c.venv_t = 0;
                        c.penv_t = 0;
                    }
                }
            }
        }

        // volume column, row part
        match cell.vol >> 4 {
            1..=5 => c.vol = (cell.vol as i32 - 0x10).min(64),
            8 => c.vol = (c.vol - (cell.vol & 15) as i32).max(0),
            9 => c.vol = (c.vol + (cell.vol & 15) as i32).min(64),
            0xA => c.vib_speed = cell.vol & 15,
            0xB => c.vib_depth = cell.vol & 15,
            0xC => c.pan = ((cell.vol & 15) as i32) * 17,
            0xF => {
                if cell.vol & 15 > 0 {
                    c.porta_speed = (cell.vol & 15) << 4;
                }
            }
            _ => {}
        }

        // effects, row part
        let p = cell.param;
        match cell.fx {
            1 if p > 0 => c.porta_up = p,
            2 if p > 0 => c.porta_down = p,
            3 if p > 0 => c.porta_speed = p,
            4 => {
                if p >> 4 > 0 { c.vib_speed = p >> 4; }
                if p & 15 > 0 { c.vib_depth = p & 15; }
            }
            5 | 6 | 0xA if p > 0 => c.vol_slide = p,
            8 => c.pan = p as i32,
            9 => {
                if p > 0 { c.offset = p; }
                if cell.note >= 1 && cell.note <= 96 {
                    c.pos = c.offset as f64 * 256.0;
                }
            }
            0xB => *pending = Some((p as usize, 0)),
            0xC => c.vol = (p as i32).min(64),
            0xD => {
                let row = (p >> 4) as usize * 10 + (p & 15) as usize;
                let o = pending.map(|x| x.0).unwrap_or(*order + 1);
                *pending = Some((o, row));
            }
            0xE => match p >> 4 {
                1 => {
                    if p & 15 > 0 { c.fine_up = p & 15; }
                    c.period -= c.fine_up as f32 * 4.0;
                }
                2 => {
                    if p & 15 > 0 { c.fine_down = p & 15; }
                    c.period += c.fine_down as f32 * 4.0;
                }
                0xA => c.vol = (c.vol + (p & 15) as i32).min(64),
                0xB => c.vol = (c.vol - (p & 15) as i32).max(0),
                0xC => c.cut_at = (p & 15) as i32,
                _ => {}
            },
            0xF => {
                if p == 0 {
                } else if p < 32 {
                    *speed = p as u32;
                } else {
                    *bpm = p as u32;
                }
            }
            0x10 => *global_vol = (p as i32).min(64), // G
            0x14 => c.key_on = false,                     // K
            _ => {}
        }
        if c.cut_at == 0 {
            c.vol = 0;
        }
    }

    fn tick_effects(&mut self, ch: usize) {
        let tick = self.tick;
        if let Some((t, cell)) = self.chans[ch].delay {
            if tick == t as u32 {
                self.chans[ch].delay = None;
                self.row_cell(ch, cell);
                return;
            }
        }
        let c = &mut self.chans[ch];
        c.arp = 0;
        c.vib = 0.0;
        // volume column, tick part
        let v = c.volfx;
        match v >> 4 {
            6 => c.vol = (c.vol - (v & 15) as i32).max(0),
            7 => c.vol = (c.vol + (v & 15) as i32).min(64),
            0xB => vibrato(c),
            0xF => tone_porta(c),
            _ => {}
        }
        let p = c.param;
        match c.fx {
            0 if p > 0 => {
                c.arp = match tick % 3 {
                    1 => (p >> 4) as i32,
                    2 => (p & 15) as i32,
                    _ => 0,
                }
            }
            1 => c.period = (c.period - c.porta_up as f32 * 4.0).max(1.0),
            2 => c.period += c.porta_down as f32 * 4.0,
            3 => tone_porta(c),
            4 => vibrato(c),
            5 => {
                tone_porta(c);
                vol_slide(c);
            }
            6 => {
                vibrato(c);
                vol_slide(c);
            }
            0xA => vol_slide(c),
            0xE if p >> 4 == 0xC && tick as i32 == c.cut_at => c.vol = 0,
            _ => {}
        }
    }

    // --------------------------------------------------------- sample mixing

    fn render(&mut self, out: &mut [f32]) {
        let m = self.module.clone();
        let gv = self.global_vol as f32 / 64.0 * self.volume;
        for (ch, c) in self.chans.iter_mut().enumerate() {
            if !c.active {
                continue;
            }
            let Some(ins) = m.instruments.get(c.ins.wrapping_sub(1)) else { continue };
            let Some(s) = c.smp.and_then(|i| ins.samples.get(i)) else { continue };
            if s.data.is_empty() {
                c.active = false;
                continue;
            }
            // pitch: linear frequency table (period 4608 = 8363 Hz)
            let period = c.period - c.arp as f32 * 64.0 + c.vib;
            let freq = 8363.0 * 2f32.powf((4608.0 - period) / 768.0);
            let step = (freq / self.rate) as f64;
            // loudness
            let env = if ins.vol_env.on { ins.vol_env.value(c.venv_t) / 64.0 } else { 1.0 };
            let vol = c.vol as f32 / 64.0 * env * c.fade as f32 / 65536.0 * gv * self.gain[ch];
            let mut pan = c.pan as f32;
            if ins.pan_env.on {
                let e = ins.pan_env.value(c.penv_t) - 32.0;
                pan += e * (128.0 - (pan - 128.0).abs()) / 32.0;
            }
            let pan = (pan / 255.0).clamp(0.0, 1.0);
            let tl = vol * (1.0 - pan).sqrt();
            let tr = vol * pan.sqrt();
            let len = s.data.len();
            let (ls, le) = (s.loop_start as f64, (s.loop_start + s.loop_len) as f64);
            for f in out.chunks_exact_mut(2) {
                // smooth gain changes over ~3 ms so volume jumps don't click
                c.gl += (tl - c.gl) * 0.02;
                c.gr += (tr - c.gr) * 0.02;
                let i = c.pos as usize;
                if i >= len {
                    c.active = false;
                    break;
                }
                let a = s.data[i];
                let b = if i + 1 < len { s.data[i + 1] } else if s.loop_kind == 1 { s.data[s.loop_start] } else { a };
                let x = a + (b - a) * (c.pos - i as f64) as f32;
                f[0] += x * c.gl;
                f[1] += x * c.gr;
                c.pos += step * c.dir;
                match s.loop_kind {
                    1 if c.pos >= le => c.pos = ls + (c.pos - le) % (le - ls).max(1.0),
                    2 => {
                        if c.dir > 0.0 && c.pos >= le {
                            c.pos = le - (c.pos - le) - 1.0;
                            c.dir = -1.0;
                        } else if c.dir < 0.0 && c.pos < ls {
                            c.pos = ls + (ls - c.pos);
                            c.dir = 1.0;
                        }
                        c.pos = c.pos.clamp(0.0, (len - 1) as f64);
                    }
                    _ => {}
                }
            }
            if c.fade <= 0 && !c.key_on {
                c.active = false;
            }
        }
    }
}

fn tone_porta(c: &mut Chan) {
    let sp = c.porta_speed as f32 * 4.0;
    if c.period < c.target {
        c.period = (c.period + sp).min(c.target);
    } else {
        c.period = (c.period - sp).max(c.target);
    }
}

fn vibrato(c: &mut Chan) {
    // sine vibrato: 64 steps per cycle, depth in 1/16 semitone units
    let s = (c.vib_pos as f32 / 64.0 * std::f32::consts::TAU).sin();
    c.vib = s * c.vib_depth as f32 * 8.0;
    c.vib_pos = (c.vib_pos + c.vib_speed) & 63;
}

fn vol_slide(c: &mut Chan) {
    let p = c.vol_slide;
    if p >> 4 > 0 {
        c.vol = (c.vol + (p >> 4) as i32).min(64);
    } else {
        c.vol = (c.vol - (p & 15) as i32).max(0);
    }
}
