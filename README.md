# Starcon

A modern, from-scratch rewrite of [Star Control: TimeWarp / TW-Light](https://sourceforge.net/projects/timewarp/) —
a fan-made Star Control 2 "Super Melee" combat game. The original is C++/Allegro 4
and difficult to build on modern systems. This rewrite uses **Rust + Bevy + Avian + GGRS**
for proper rollback netcode out of the gate, with angular momentum and exotic-mechanic
extensibility baked in from the start.

## Status

Playable local 1v1 with two of six classes implemented for combat. Architecture
seams in place for: rollback netcode, time-bending mechanics, ship-class plugin
behaviour, joint-based physics (chains, satellites). See `docs/ROADMAP.md` for
milestone progress and `docs/EXTENSIBILITY.md` for the recipe to add a new ship.

## Stack

| Concern    | Choice                              | Why                                                                       |
| ---------- | ----------------------------------- | ------------------------------------------------------------------------- |
| Engine     | Bevy 0.18                           | ECS, hot iteration, native + WASM, mature stable                          |
| Physics    | Avian 2D (XPBD)                     | Modern Bevy-native, real angular momentum, joints (chains/tethers), CCD   |
| Netcode    | GGRS via `bevy_ggrs` + `bevy_matchbox` | Rollback (the gold standard for twitch combat); WebRTC P2P for free       |
| Tick rate  | 60 Hz fixed                         | Deterministic stepping required for rollback                              |
| Targets    | Linux / macOS / Windows / Web (WASM) | One netcode path across all platforms                                     |

## Running

```sh
cargo run
```

First build pulls Bevy + Avian + GGRS, expect ~12 minutes. Incremental rebuilds
are ~5 seconds.

## Controls

| Action | Player 1 | Player 2 |
| ------ | -------- | -------- |
| Rotate left  | `←`       | `A` |
| Rotate right | `→`       | `D` |
| Thrust       | `↑`       | `W` |
| Fire         | `Z`       | `G` |
| Special      | `X`       | `H` |

### Match flow

| Key | Action |
| --- | ------ |
| `R` | Rematch (only between rounds, after a winner is shown) |
| `1`–`6` | Pick P1's class for the next rematch |
| `F1`–`F6` | Pick P2's class for the next rematch |
| `[` / `]` | Slow / speed up the entire physics clock (debug) |
| `\` | Reset time scale to 1.0 |

Class index: `1`/`F1` Earthling, `2`/`F2` Spathi, `3`/`F3` Yehat,
`4`/`F4` Chmmr, `5`/`F5` Ur-Quan, `6`/`F6` Mycon.

## What's implemented today

- 2D physics via Avian — real mass, angular momentum, impulse-at-contact spin
- 6 ship classes with distinct per-class:
  - **Stats** loaded from `assets/ships/<code>.ini` (legacy TW format)
  - **Physics tuning** (collider, mass, damping — big ships boat-feel, small ships agile)
  - **Weapon shape** (direction, speed, lifetime, color — Spathi fires *backwards*!)
  - **Special ability** (Earcr dash, Spael phase-jump, others placeholder)
- Projectile + crew + ram damage + win condition + running score + rematch
- HUD: crew readouts (per slot), centre banner (score + winner)
- Time-scale debug controls (slow-mo / fast-forward) — extensibility seam for
  per-region time dilation, per-ship subjective time, and time rewind (see
  `src/timeflow.rs` docs)

## What isn't yet

- Class behaviour: Yehat shield, Chmmr satellites, Ur-Quan fighters, Mycon plasmoid
- Sound (WAVs are extracted and sitting in `assets/ships/<code>/sounds/`)
- Netplay (Avian is configured single-threaded for determinism, GGRS plumbing
  is stubbed)
- Visual hit feedback (particles, explosions)
- Authentic SC2 pixel-art rotation on the projectile sprites (currently solid
  colored squares)
- Fleet selection screen (currently 1v1 only)

## Asset salvage

`assets/ships/*.ini` and `*.txt` are the original TW-Light per-ship stat files
and lore — clean text, GPLv2. `assets/ships/<code>/sprites/*.png` are extracted
from the original Allegro 4 datafiles via `cargo run --bin extract_dat --features tools`
(see `tools/extract_dat.rs`). PNGs preserve the original magenta-keyed
transparency converted to RGBA alpha.

Starting subset (with assets and stats):

- `earcr` — Earthling Cruiser
- `kzedr` — Ur-Quan (Kzer-Za) Dreadnought
- `chmav` — Chmmr Avatar
- `spael` — Spathi Eluder
- `yehte` — Yehat Terminator
- `mycpo` — Mycon Podship

The remaining ~165 TW-Light ships will be ported once these six have all their
class-specific behaviour wired up.

## License

GPLv2-or-later, inherited from TW-Light (whose source and assets we draw from).
