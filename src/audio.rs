//! Our own small sound mixer.
//!
//! The game mixes all sound effects and the music (a live .xm player, see xm.rs) in a background thread,
//! then sends the result to the system:
//! - Linux: PulseAudio (this is what PipeWire desktops provide through pipewire-pulse),
//!   or ALSA directly if no Pulse/PipeWire server is running. Both libraries are
//!   opened at run time (`libloading`), so the game still starts, silently, without them.
//! - Windows and macOS: the `cpal` crate (WASAPI / CoreAudio).
//!
//! Rust note: the mixer state lives in an `Arc<Mutex<..>>`. `Arc` is a shared,
//! reference-counted pointer that can cross threads; `Mutex` makes sure only one
//! thread touches the state at a time. The game thread adds voices, the audio
//! thread mixes them.

#[cfg(target_os = "linux")]
use libloading::{Library, Symbol};
#[cfg(target_os = "linux")]
use std::ffi::{c_char, c_int, c_void, CString};
use crate::xm;
use std::sync::{Arc, Mutex};

pub const RATE: u32 = 44100;
const CHUNK: usize = 1024; // frames per write (~23 ms)

/// A decoded sound: interleaved stereo f32 at 44.1 kHz.
pub struct Clip {
    pub data: Vec<f32>,
}

struct Voice {
    clip: usize,
    pos: usize,
    vol: f32,
    looped: bool,
    id: u64,
}

/// The music: one tracker song playing, and maybe the previous one fading out.
struct Music {
    songs: Vec<Arc<xm::Module>>,
    cur: Option<(usize, xm::Player)>,
    old: Option<(xm::Player, f32)>, // fading out, with its volume
    vol: f32,
    playing: bool,
}

struct State {
    clips: Vec<Clip>,
    voices: Vec<Voice>,
    music: Music,
    master: f32,
    next_id: u64,
}

pub struct Audio {
    state: Arc<Mutex<State>>,
    pub backend: &'static str,
}

impl Audio {
    /// Starts the audio thread. Never fails: with no sound system the game is silent.
    pub fn start() -> Audio {
        let state = Arc::new(Mutex::new(State {
            clips: Vec::new(),
            voices: Vec::new(),
            music: Music { songs: Vec::new(), cur: None, old: None, vol: 0.5, playing: true },
            master: 1.0,
            next_id: 1,
        }));
        let backend = match Output::open() {
            Some(out) => {
                let name = out.name();
                let st = state.clone();
                std::thread::spawn(move || mix_thread(st, out));
                name
            }
            None => "none",
        };
        Audio { state, backend }
    }

    /// Loads a 16-bit PCM WAV (mono or stereo, 44.1 kHz). Returns its clip number.
    pub fn load_wav(&self, bytes: &[u8]) -> Option<usize> {
        let clip = parse_wav(bytes)?;
        let mut s = self.state.lock().ok()?;
        s.clips.push(clip);
        Some(s.clips.len() - 1)
    }

    pub fn play(&self, clip: usize, vol: f32, looped: bool) -> u64 {
        let mut s = self.state.lock().unwrap();
        let id = s.next_id;
        s.next_id += 1;
        if s.voices.len() < 48 {
            s.voices.push(Voice { clip, pos: 0, vol, looped, id });
        }
        id
    }

    pub fn stop(&self, id: u64) {
        self.state.lock().unwrap().voices.retain(|v| v.id != id);
    }

    pub fn set_master(&self, v: f32) {
        self.state.lock().unwrap().master = v;
    }

    /// Loads a .xm song. Returns its song number for `play_song`.
    pub fn load_song(&self, bytes: &[u8]) -> Option<usize> {
        let module = xm::Module::parse(bytes)?;
        let mut s = self.state.lock().ok()?;
        s.music.songs.push(Arc::new(module));
        Some(s.music.songs.len() - 1)
    }

    /// Starts a song from `order` (pattern list position). If another song is playing
    /// it fades out underneath. Playing the song that's already on does nothing.
    pub fn play_song(&self, song: usize, order: usize) {
        let mut s = self.state.lock().unwrap();
        let m = &mut s.music;
        if m.cur.as_ref().map(|c| c.0) == Some(song) {
            return;
        }
        let Some(module) = m.songs.get(song).cloned() else { return };
        let mut p = xm::Player::new(module, RATE);
        p.goto(order);
        if let Some((_, old)) = m.cur.take() {
            m.old = Some((old, 1.0));
        }
        m.cur = Some((song, p));
    }

    /// Runs `f` on the song that's playing, to steer it (channel fades, jumps, tempo).
    /// Rust note: `FnOnce(&mut xm::Player)` means "any function or closure that takes
    /// the player"; the caller writes e.g. `audio.with_song(|p| p.queue_order(4))`.
    pub fn with_song(&self, f: impl FnOnce(&mut xm::Player)) {
        if let Some((_, p)) = self.state.lock().unwrap().music.cur.as_mut() {
            f(p);
        }
    }

    pub fn set_music_volume(&self, v: f32) {
        self.state.lock().unwrap().music.vol = v;
    }

    pub fn set_music(&self, playing: bool) {
        self.state.lock().unwrap().music.playing = playing;
    }
}

fn parse_wav(b: &[u8]) -> Option<Clip> {
    if b.len() < 44 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut i = 12;
    let mut channels = 1;
    let mut rate = RATE;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32_at(i + 4) as usize;
        let body = i + 8;
        if id == b"fmt " {
            channels = u16_at(body + 2) as usize;
            rate = u32_at(body + 4);
        } else if id == b"data" {
            let end = (body + len).min(b.len());
            let samples: Vec<f32> = b[body..end].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
            // to stereo at 44.1 kHz (simple nearest resample if needed)
            let frames = samples.len() / channels.max(1);
            let out_frames = (frames as u64 * RATE as u64 / rate.max(1) as u64) as usize;
            let mut data = Vec::with_capacity(out_frames * 2);
            for f in 0..out_frames {
                let src = (f as u64 * rate as u64 / RATE as u64) as usize;
                let l = samples[src * channels];
                let r = if channels > 1 { samples[src * channels + 1] } else { l };
                data.push(l);
                data.push(r);
            }
            return Some(Clip { data });
        }
        i = body + len + (len & 1);
    }
    None
}

fn mix_thread(state: Arc<Mutex<State>>, mut out: Output) {
    let mut mix = vec![0f32; CHUNK * 2];
    let mut pcm = vec![0i16; CHUNK * 2];
    let mut song_buf = vec![0f32; CHUNK * 2];
    let mut old_buf = vec![0f32; CHUNK * 2];
    loop {
        mix.iter_mut().for_each(|x| *x = 0.0);
        {
            let mut guard = state.lock().unwrap();
            let s = &mut *guard; // reborrow so we can use two fields at once
            for v in s.voices.iter_mut() {
                let clip = &s.clips[v.clip].data;
                for k in 0..CHUNK {
                    if v.pos + 1 >= clip.len() {
                        if v.looped {
                            v.pos = 0;
                        } else {
                            break;
                        }
                    }
                    mix[k * 2] += clip[v.pos] * v.vol;
                    mix[k * 2 + 1] += clip[v.pos + 1] * v.vol;
                    v.pos += 2;
                }
            }
            s.voices.retain(|v| v.looped || v.pos + 1 < s.clips[v.clip].data.len());
            let m = &mut s.music;
            if m.playing {
                song_buf.iter_mut().for_each(|x| *x = 0.0);
                if let Some((_, p)) = m.cur.as_mut() {
                    p.mix(&mut song_buf);
                }
                if let Some((p, v)) = m.old.as_mut() {
                    // the old song fades out over about a second, in a second buffer
                    old_buf.iter_mut().for_each(|x| *x = 0.0);
                    p.mix(&mut old_buf);
                    for (k, x) in old_buf.chunks_exact(2).enumerate() {
                        let g = (*v - k as f32 / (RATE as f32 * 0.9)).max(0.0);
                        song_buf[k * 2] += x[0] * g;
                        song_buf[k * 2 + 1] += x[1] * g;
                    }
                    *v -= CHUNK as f32 / (RATE as f32 * 0.9);
                    if *v <= 0.0 {
                        m.old = None;
                    }
                }
                for (o, x) in mix.iter_mut().zip(song_buf.iter()) {
                    *o += x * m.vol;
                }
            }
            let master = s.master;
            for (o, x) in pcm.iter_mut().zip(mix.iter()) {
                // soft clip so many explosions at once don't crackle
                let y = (x * master).tanh();
                *o = (y * 32000.0) as i16;
            }
        }
        if !out.write(&pcm) {
            return;
        }
    }
}

// ---------------------------------------------------------------- outputs

#[cfg(not(target_os = "linux"))]
mod native;
#[cfg(not(target_os = "linux"))]
use native::Output;

#[cfg(target_os = "linux")]
use linux::Output;

#[cfg(target_os = "linux")]
mod linux {
use super::*;

#[repr(C)]
struct PaSampleSpec {
    format: c_int, // PA_SAMPLE_S16LE = 3
    rate: u32,
    channels: u8,
}

#[repr(C)]
struct PaBufferAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}

type PaNew = unsafe extern "C" fn(*const c_char, *const c_char, c_int, *const c_char, *const c_char, *const PaSampleSpec, *const c_void, *const PaBufferAttr, *mut c_int) -> *mut c_void;
type PaWrite = unsafe extern "C" fn(*mut c_void, *const c_void, usize, *mut c_int) -> c_int;

type SndOpen = unsafe extern "C" fn(*mut *mut c_void, *const c_char, c_int, c_int) -> c_int;
type SndSetParams = unsafe extern "C" fn(*mut c_void, c_int, c_int, u32, u32, c_int, u32) -> c_int;
type SndWritei = unsafe extern "C" fn(*mut c_void, *const c_void, u64) -> i64;
type SndRecover = unsafe extern "C" fn(*mut c_void, c_int, c_int) -> c_int;

pub enum Output {
    Pulse { _lib: Library, handle: *mut c_void, write: PaWrite },
    Alsa { _lib: Library, pcm: *mut c_void, writei: SndWritei, recover: SndRecover },
}

// The raw handles are only ever used by the audio thread.
unsafe impl Send for Output {}

impl Output {
    pub fn open() -> Option<Output> {
        if std::env::var("TK_AUDIO").as_deref() != Ok("alsa") {
            if let Some(o) = Self::open_pulse() {
                return Some(o);
            }
        }
        Self::open_alsa()
    }

    pub fn name(&self) -> &'static str {
        match self {
            Output::Pulse { .. } => "pulseaudio",
            Output::Alsa { .. } => "alsa",
        }
    }

    fn open_pulse() -> Option<Output> {
        // `unsafe`: calling C functions found at run time; we must get the signatures right.
        unsafe {
            let lib = Library::new("libpulse-simple.so.0").ok()?;
            let new: Symbol<PaNew> = lib.get(b"pa_simple_new\0").ok()?;
            let write: Symbol<PaWrite> = lib.get(b"pa_simple_write\0").ok()?;
            let (new, write) = (*new, *write);
            let spec = PaSampleSpec { format: 3, rate: RATE, channels: 2 };
            // keep latency low: about 60 ms of target buffer
            let attr = PaBufferAttr { maxlength: u32::MAX, tlength: (RATE as u32 * 4) * 60 / 1000, prebuf: u32::MAX, minreq: u32::MAX, fragsize: u32::MAX };
            let app = CString::new("Twin Karumba").unwrap();
            let stream = CString::new("game").unwrap();
            let mut err: c_int = 0;
            let h = new(std::ptr::null(), app.as_ptr(), 1 /* PA_STREAM_PLAYBACK */, std::ptr::null(), stream.as_ptr(), &spec, std::ptr::null(), &attr, &mut err);
            if h.is_null() {
                return None;
            }
            Some(Output::Pulse { _lib: lib, handle: h, write })
        }
    }

    fn open_alsa() -> Option<Output> {
        unsafe {
            let lib = Library::new("libasound.so.2").ok()?;
            let open: SndOpen = *lib.get(b"snd_pcm_open\0").ok()?;
            let set: SndSetParams = *lib.get(b"snd_pcm_set_params\0").ok()?;
            let writei: SndWritei = *lib.get(b"snd_pcm_writei\0").ok()?;
            let recover: SndRecover = *lib.get(b"snd_pcm_recover\0").ok()?;
            let mut pcm: *mut c_void = std::ptr::null_mut();
            let dev = CString::new("default").unwrap();
            if open(&mut pcm, dev.as_ptr(), 0 /* playback */, 0) < 0 || pcm.is_null() {
                return None;
            }
            // S16_LE = 2, RW_INTERLEAVED = 3, 2 channels, resample allowed, 60 ms latency
            if set(pcm, 2, 3, 2, RATE, 1, 60_000) < 0 {
                return None;
            }
            Some(Output::Alsa { _lib: lib, pcm, writei, recover })
        }
    }

    /// Blocks until the chunk is queued. Returns false if the device went away.
    pub fn write(&mut self, pcm: &[i16]) -> bool {
        unsafe {
            match self {
                Output::Pulse { handle, write, .. } => {
                    let mut err = 0;
                    write(*handle, pcm.as_ptr() as *const c_void, pcm.len() * 2, &mut err) >= 0
                }
                Output::Alsa { pcm: dev, writei, recover, .. } => {
                    let frames = (pcm.len() / 2) as u64;
                    let r = writei(*dev, pcm.as_ptr() as *const c_void, frames);
                    if r < 0 {
                        recover(*dev, r as c_int, 1) >= 0
                    } else {
                        true
                    }
                }
            }
        }
    }
}
}
