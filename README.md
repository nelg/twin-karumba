# Twin Karumba: Rust prototype (level 1)

By Glen Ogilvie. **Beta: not yet free to copy or change.** See [LICENSE](LICENSE) and [AUTHORS](AUTHORS).

This is the first of the three bake-off prototypes (Rust + macroquad, then Go + Ebitengine, then C++ + raylib). It plays the whole of level 1 from `levels/level1.txt`, from the coastline through the cave to the Base Core boss, in 1 or 2 players.

## Run it

Ready-made builds for Windows, macOS and Linux are on the [releases page](https://github.com/nelg/twin-karumba/releases) and the [website](https://nelg.github.io/twin-karumba/). Each is a single file with everything built in:

- **Windows:** unzip `twin-karumba-windows-x86_64.zip` and run `twin-karumba.exe`. It isn't signed yet, so Windows may say "Windows protected your PC": choose *More info*, then *Run anyway*.
- **macOS** (Apple Silicon and Intel): unzip `twin-karumba-macos-universal.zip` and open *Twin Karumba*. The first time, right-click it and choose *Open*, because it isn't signed yet.
- **Linux** (x86-64): unpack `twin-karumba-linux-x86_64.tar.gz` and run `./twin-karumba`.

The builds come from `.github/workflows/build.yml` on every push. Pushing a `v*` tag also attaches them to a GitHub release.

Build it yourself (any OS):

```sh
# one-off: install Rust from https://rustup.rs
# Ubuntu/Debian also needs:
sudo apt install build-essential pkg-config libasound2-dev libx11-dev libxi-dev libgl1-mesa-dev
cargo run --release
```

The first build downloads and compiles the dependencies, which takes about a minute. After that a rebuild takes a few seconds.

**Single-file build** (for packaging): `cargo build --release --features embed` packs everything in `assets/` into the executable (about 5.4 MB instead of 1.9 MB), so `target/release/twin-karumba-rs` runs on its own with no `assets/` folder. The high score is then saved next to the executable. Without the flag the game reads the `assets/` folder as usual, which is handier while editing art, levels or tuning.json. `build.rs` does the packing; no extra libraries are needed.

## Controls

| | Move | Fire | Bomb | Crane |
| --- | --- | --- | --- | --- |
| 1 player | Arrows or WASD | Z, Space, F or Left Ctrl | X, C, G or Left Shift | E, H, / or Right Alt |
| 2 players: P1 | WASD | Space or F | Left Shift or G | E |
| 2 players: P2 | Arrows | Right Ctrl, Enter or K | Right Shift or L | / or Right Alt |
| Mouse (M toggles) | Heli chases the cursor | Left button | Right button | Middle button |

Hold fire for the cannon and hold bomb to keep dropping torpedoes as they reload. P2 can drop in any time by pressing their fire key.

Debug keys: **1** to **9** (top row or keypad) jump to that section. **F3** swaps the music to level 2's song (Fifth Rotor) and back.

The title logo is the chrome style; set `title.logo` to `sunset` in tuning.json for the sunset racer one. The game opens full screen in CRT mode. Other keys: **V** or **F2** toggles CRT mode, **N** toggles sound, **P** pauses, **F11** toggles full screen, **Esc** returns to the title (or quits from the title).

## What's in

- **Level:** reads the shared ASCII file and smooths it like `level_render.py`. Meadows roll, rock slopes, while cliffs and metal keep hard edges. The `<` and `>` markers slow the scroll in the cave, and the section digits act as checkpoints.
- **Land:** baked on the fly, 512 px at a time, from the 64 px palette textures. It has a lit rim, grass tufts, cave drips, snow caps on high peaks, metal decks, and a dithered 40 px palette blend at section changes. The three parallax layers crossfade between palettes.
- **KX-2:** direct flight with a 7° visual tilt. It's limited to 3/4 of the screen forward, and holding back slows the scroll to 60%. The lives digit is drawn on the hull. The mounted torpedo reloads with the slide and bounce, and the shield force field shows while the shield has charge.
- **Weapons:**
  - Cannon: up to 5 bullets on screen, fired along the ship's tilt.
  - Torpedo bombs: arc, jitter, wobble and pitch as in tuning.json, up to 3 falling.
  - Laser pickup: kills any enemy after 0.1 s of contact (`player.laser.dwell_to_kill`), armoured ones too, and balloons at once.
- **Enemies:** all 16 types from the level, plus the boss.
  - Fuel depot, rocket, tank, flak, ceiling turret and whirler swarms.
  - Scout chopper, rocket truck, gun tower, drone pad with its drones, and fighter.
  - H2 balloon, whose blast chains. Mine, radar dish (shuts down flak) and falling rocks.
  - Base Core: two gun pods, and a core that opens between volleys.
- **Rules:**
  - Fuel drains over 45 s, with an alarm under 20%. With an empty tank the heli falls.
  - Bullets destroy everything. Armoured ground targets (tank, rocket truck, turrets, gun tower, flak, radar, drone pad, rockets on their pad, rocks) take 2 bullets (`level_flow.armour_bullet_hits`). A bomb kills anything at once.
  - One hit kills unless the shield is up. Shots cost a third of the shield, and rams cost what tuning.json says.
  - In 1 player, a lost life restarts the section. In co-op, a downed player comes back in place while the other is still flying.
  - Extra life at 10,000 points, then every 30,000.
  - Pickups are per player in their colour: S, F, 1UP and L.
  - Hover low over a stranded pilot (@), a supply cache (+) or a crane fuel tank (fuel C) and press the crane key to run a short hoist. "CRANE" blinks over the item when you're close enough. The scroll stops, and the crew door or cargo door frame plays. Each rescued pilot is +1 life, and a cache fills fuel and shield.
- **Art variants:** fuel A is shot for fuel, fuel B sits on airstrip metal, and fuel C is lifted with the crane. Radar B appears where enemies are dense. Ceiling turrets (3 kinds), mines (2), rocks (3) and pilots (2) are picked at random. Turret barrels swing to track you.
- **Sound output:** our own mixer. On Linux it sends sound to PulseAudio, so it works with PipeWire's Pulse layer, and falls back to ALSA if there's no Pulse server (`TK_AUDIO=alsa` forces ALSA). On Windows and macOS it plays through `cpal` (WASAPI / CoreAudio). The game prints which one it uses at start.
- **Look and sound:** additive glow, layered explosions with sparks, smoke and debris, and screen shake. HUD bar, title screen and the 27 sound effects.
- **Music:** the .xm songs play live through our own small XM player (`src/xm.rs`, no extra library), so the music follows the game (`src/music.rs`):
  - Each section starts its own part of Karumba Beach. Flying into the next section changes at the end of the current pattern, so it stays in time.
  - Player 1's lead is the chip in sections 1 and 3, the accordion in 2, the twang guitar in 4, and chip plus twang in 5.
  - Player 2's lead is a flute (or a harp: `music.p2_lead` in tuning.json) while P2 is flying. In co-op each lead only plays while its player is alive.
  - In the cave, kick and snare drop out, the hats go quiet and the tempo eases.
  - The title screen plays Karumba Beach with the twang guitar lead. The boss has no song of its own yet.
  - Pausing turns the music right down.
  - Level 2 (not built yet) will play Fifth Rotor (`assets/music/fifth-rotor.xm`, set by `music.level2_song`), Beethoven's 5th as a techno chip tune that loops. Press **F3** in level 1 to hear it.

## Not yet / placeholders

- **Placeholder art:** only the Base Core boss is still painted in code (`src/pixel.rs`), while new boss ideas are being picked.
- **Not in this level:** clouds, the radar pickup, airport landing and enemy airfields aren't in `level1.txt` yet, so they aren't coded.
- **Hoist:** a short version of the crane scene. There's no swinging arm animation yet.
- **Music:** no boss song yet, no low-fuel cue, and no victory or game-over sting.
- **No gamepad support yet.**

## Tuning

Every number comes from `assets/tuning.json`, so you can edit it without recompiling. The prototype added its own guesses under `player.flight` and a new `level_flow` block:

- `player.flight.speed_x`, `speed_y` and `mouse_speed`: flight speed (260, 240 and 300 art px/s)
- Fuel tank time and refill amounts
- `hitbox_half`
- Pickup drop chance, extra-life scores, hoist time, crane reach (`hoist_max_height`, now 152 art px) and boss hit points

## Code tour (src/)

| File | What it does |
| --- | --- |
| `build.rs` (project root) | with `--features embed`, lists assets/ so they're packed into the executable |
| `main.rs` | window, title screen, input mapping, render target, CRT shader |
| `level.rs` | parses the ASCII level and builds the ground and roof height maps |
| `terrain.rs` | bakes land chunks and draws the parallax sky |
| `game.rs` | players, enemies, weapons, pickups, boss, checkpoints (the big one) |
| `fx.rs` | explosions, particles, glow flashes |
| `hud.rs` | the HUD bar |
| `xm.rs` | the .xm tracker player |
| `music.rs` | picks songs and channel mixes from the game state |
| `pixel.rs` | placeholder sprites painted in code, glow texture, the 5x5 HUD font |
| `assets.rs` | file loading, sprite sheets, sound effects |
| `tuning.rs` | tuning.json to a typed `Cfg` struct |

Comments marked "Rust note" explain the Rust-specific parts.

## Test switches (environment variables)

| Variable | Effect |
| --- | --- |
| `TK_PLAY=1` or `TK_PLAY=2` | skip the title |
| `TK_START_SECTION=4` | start at section 4 |
| `TK_GOD=1` | can't die |
| `TK_LASER=1` | P1 starts with the laser |
| `TK_AUTOPILOT=1` | the game flies P1 |
| `TK_MUTE=1` | no sound |
| `TK_AUDIO=alsa` | skip PulseAudio and use ALSA |
| `TK_CRT=0` | start without CRT mode (it's on by default) |
| `TK_WINDOWED=1` | start in a window instead of full screen |
| `TK_SHOTS=5:a.png,9:b.png` | save 1080p screenshots at those times |
| `TK_QUIT=10` | quit after 10 seconds |
| `TK_RENDER_XM=song.xm:out.raw:30` | render 30 s of a song to raw 16-bit stereo and quit |
| `TK_RENDER_MUTE=6,7,8,9` | with `TK_RENDER_XM`: silence those channels (6-9 are Karumba Beach's extra in-game leads) |
