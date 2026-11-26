//! The music director: watches the game and steers the .xm songs.
//!
//! Karumba Beach (game version) has several lead voices on separate channels that
//! all play the same tune in sync, so we can switch between them without losing our
//! place in the song:
//!   0 chip lead | 1 arpeggio | 2 bass | 3 kick+snare | 4 hats | 5 guitar echo
//!   6 twang guitar | 7 accordion | 8 flute | 9 harp
//!
//! The cues:
//! - each section starts its own part of the song (at the next pattern, so it stays in time)
//! - player 1's lead: chip in sections 1 and 3, accordion in 2, twang guitar in 4,
//!   chip + twang in 5
//! - player 2's lead: flute (or harp, `music.p2_lead` in tuning.json) while P2 is flying
//! - in co-op each lead only plays while its player is alive, so if P1 goes down,
//!   P2 hears just their own lead
//! - cave (scroll slowed by `<`): kick, snare and most of the hats drop out, and
//!   the tempo eases a little
//! - paused: music turned right down; game over: quieter
//! - boss: no song of its own yet (Mountain King was dropped), so section 5 carries on
//! - level 2 (not built yet) gets Fifth Rotor (`music.level2_song` in tuning.json), a
//!   plain looping song with no section cues. F3 previews it during level 1.

use crate::audio::Audio;
use crate::game::{Game, Phase};
use crate::level::{Level, SCREEN_W, TILE};
use crate::xm::Player;

const CHIP: usize = 0;
const KICK_SNARE: usize = 3;
const HATS: usize = 4;
const ECHO: usize = 5;
const TWANG: usize = 6;
const ACCORDION: usize = 7;
const FLUTE: usize = 8;
const HARP: usize = 9;

/// Where each section starts in Karumba Beach (its order list):
/// intro, verse, verse up an octave, middle section, key change.
const SECTION_ORDER: [usize; 5] = [0, 4, 8, 14, 16];
const VOLUME: f32 = 0.16;

/// The channel mix we want right now. Compared each frame; only changes are sent.
#[derive(Clone, Copy, PartialEq)]
struct Mix {
    chip: f32,
    twang: f32,
    accordion: f32,
    p2: f32,
    cave: bool,
}

pub struct Director {
    beach: Option<usize>,
    level2: Option<usize>,
    /// which level's music to play: 1 = Karumba Beach with its cues, 2+ = level 2's song
    pub level: usize,
    p2_ch: usize,
    section: Option<usize>,
    mix: Option<Mix>,
    playing: Option<usize>, // which song we last started
}

impl Director {
    pub fn load(audio: &Audio, p2_lead: &str, level2_song: &str) -> Director {
        let song = |name: &str| {
            let bytes = crate::assets::try_read(&format!("music/{name}.xm"))?;
            audio.load_song(&bytes)
        };
        Director {
            beach: song("karumba-beach-game"),
            level2: song(level2_song),
            level: 1,
            p2_ch: if p2_lead == "harp" { HARP } else { FLUTE },
            section: None,
            mix: None,
            playing: None,
        }
    }

    /// Starts a song unless it's already on. A new song forgets the old channel mix.
    fn start(&mut self, audio: &Audio, song: usize) {
        if self.playing != Some(song) {
            audio.play_song(song, 0);
            self.playing = Some(song);
            self.section = None;
            self.mix = None;
        }
    }

    /// Title screen music.
    /// The title screen plays Karumba Beach from the top, with the twang guitar lead.
    pub fn title(&mut self, audio: &Audio) {
        if let Some(s) = self.beach {
            if self.playing != Some(s) || self.section.is_some() {
                audio.play_song(s, 0);
                self.playing = Some(s);
                audio.with_song(|p| {
                    for ch in [CHIP, ACCORDION, FLUTE, HARP] {
                        p.snap_channel_gain(ch, 0.0);
                    }
                    p.snap_channel_gain(TWANG, 1.0);
                    p.snap_channel_gain(ECHO, 1.0);
                });
            }
            self.section = None; // the game picks its own section part when it starts
            self.mix = None;
        }
        audio.set_music_volume(VOLUME);
    }

    /// Call every frame while a game is on.
    pub fn update(&mut self, audio: &Audio, game: &Game, level: &Level, paused: bool) {
        let vol = match game.phase {
            _ if paused => VOLUME * 0.25,
            Phase::Over(_) => VOLUME * 0.5,
            _ => VOLUME,
        };
        audio.set_music_volume(vol);

        // level 2 and on: just loop that level's song (Fifth Rotor loops back to its theme)
        if self.level >= 2 {
            if let Some(s) = self.level2 {
                self.start(audio, s);
                return;
            }
        }

        let Some(beach) = self.beach else { return };
        self.start(audio, beach);
        let fresh = self.mix.is_none();

        // which section is the middle of the screen in?
        let mid_col = ((game.cam_x + SCREEN_W / 2.0) / TILE) as usize;
        let sec = game.sections.iter().rposition(|s| s.0 <= mid_col).unwrap_or(0);
        if self.section != Some(sec) {
            let order = SECTION_ORDER[sec.min(SECTION_ORDER.len() - 1)];
            match self.section {
                // flew into the next section: change at the end of this pattern
                Some(old) if sec == old + 1 => audio.with_song(|p| p.queue_order(order)),
                // started, restarted or jumped (debug keys): change now
                _ => audio.with_song(|p| p.goto(order)),
            }
            self.section = Some(sec);
        }

        // whose leads play: in co-op only living players' leads
        let [p1, p2] = &game.players;
        let coop = p2.joined && !p2.out;
        let p1_on = if coop { p1.alive && !p1.out } else { true };
        let p2_on = coop && p2.alive;
        let one = if p1_on { 1.0 } else { 0.0 };
        let (chip, twang, accordion) = match sec {
            0 | 2 => (one, 0.0, 0.0),
            1 => (0.0, 0.0, one),
            3 => (0.0, one, 0.0),
            _ => (one * 0.8, one * 0.8, 0.0),
        };
        let cave = level.scroll_factor(game.cam_x + SCREEN_W / 2.0, game.cfg.cave_factor) < 0.99;
        let mix = Mix { chip, twang, accordion, p2: if p2_on { 0.9 } else { 0.0 }, cave };

        if self.mix != Some(mix) {
            let p2_ch = self.p2_ch;
            audio.with_song(|p| {
                // a small inner function: snap the mix on a fresh start, fade it otherwise
                fn set(p: &mut Player, fresh: bool, ch: usize, g: f32) {
                    if fresh { p.snap_channel_gain(ch, g) } else { p.set_channel_gain(ch, g) }
                }
                set(p, fresh, CHIP, mix.chip);
                set(p, fresh, TWANG, mix.twang);
                set(p, fresh, ECHO, mix.twang);
                set(p, fresh, ACCORDION, mix.accordion);
                set(p, fresh, FLUTE, if p2_ch == FLUTE { mix.p2 } else { 0.0 });
                set(p, fresh, HARP, if p2_ch == HARP { mix.p2 } else { 0.0 });
                set(p, fresh, KICK_SNARE, if mix.cave { 0.0 } else { 1.0 });
                set(p, fresh, HATS, if mix.cave { 0.35 } else { 1.0 });
                p.tempo_scale = if mix.cave { 0.92 } else { 1.0 };
            });
            self.mix = Some(mix);
        }
    }
}
