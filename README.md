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
| `1`–`9`, `0` | Pick P1's class for the next rematch (bank 1) |
| `F1`–`F10` | Pick P2's class for the next rematch (bank 1) |
| `Shift` + above | Same key but pick from bank 2 (classes 10–19) |
| `Ctrl` + above | Pick from bank 3 (classes 20–24) |
| `[` / `]` | Slow / speed up the entire physics clock (debug) |
| `\` | Reset time scale to 1.0 |
| `M` | Cycle angular-control override (off → force Classic → force Inertial → off) |

Class index (same number = same ship for both players):

| # | Class | Note |
| - | ----- | ---- |
| 1 / F1  | Earthling Cruiser   | Forward gun, dash special |
| 2 / F2  | Spathi Eluder       | Fires *backwards*, warp-jump special |
| 3 / F3  | Yehat Terminator    | Twin cannons, energy shield (25% incoming damage) |
| 4 / F4  | Chmmr Avatar        | Forward laser, brake (placeholder special) |
| 5 / F5  | Ur-Quan Dreadnought | Heavy bolt, brake (placeholder) |
| 6 / F6  | Mycon Podship       | Slow projectile, brake (placeholder) |
| 7 / F7  | Shofixti Scout      | Fast gun, Glory-charge dash |
| 8 / F8  | Arilou Skiff        | Halo shot, perpendicular hyperspace teleport |
| 9 / F9  | Pkunk Fury          | Fast forward fire, phase-shift invuln (1 s) |
| 0 / F10 | Ilwrath Avenger     | Flamethrower, cloak invuln (2.5 s) |
| ⇧+1 / ⇧+F1 | Thraddash Torch    | Forward bolt, afterburner dash |
| ⇧+2 / ⇧+F2 | VUX Intruder       | Slow limpet, hit-and-stop brake |
| ⇧+3 / ⇧+F3 | Supox Blade        | Plasma grenade, direction-aware strafe |
| ⇧+4 / ⇧+F4 | Kohr-Ah Marauder   | Cleansing flame, sawblade anchor (placeholder) |
| ⇧+5 / ⇧+F5 | Syreen Penetrator  | Razor shot, siren song (placeholder) |
| ⇧+6 / ⇧+F6 | Androsynth Guardian| Bubble shot, Blazer-comet long dash |
| ⇧+7 / ⇧+F7 | Chenjesu Broodhome | Crystal shard, DOGI deploy (placeholder) |
| ⇧+8 / ⇧+F8 | Druuge Mauler      | Heavy cannon **with recoil** (firing pushes you back), ship-jump |
| ⇧+9 / ⇧+F9 | Utwig Jugger       | Twin prong, 2 s ricochet shield |
| ⇧+0 / ⇧+F10| Zoq-Fot-Pik Stinger| Tongue lash, taunt-dash |
| ⌃+1 / ⌃+F1 | Mmrnmhrm X-Form    | Beam shot, transform placeholder |
| ⌃+2 / ⌃+F2 | Orz Nemesis        | Flex-arm shot, marines (placeholder) |
| ⌃+3 / ⌃+F3 | Slylandro Probe    | Lightning shot, perpendicular jump |
| ⌃+4 / ⌃+F4 | Umgah Drone        | Cone shot, **anti-grav reverse impulse** |
| ⌃+5 / ⌃+F5 | Melnorme Trader    | Plasma shot, confusion (placeholder) |

## What's implemented today

- 2D physics via Avian — real mass, angular momentum, impulse-at-contact spin
- Full canonical SC2 (Ur-Quan Masters) **25-ship roster** with distinct per-class:
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

The full canonical SC2 Ur-Quan Masters lineup is in:

  Alliance:  earcr, yehte, shosc, mmrxf, syrpe, chebr, arisk
  Hierarchy: kzedr, mycpo, spael, vuxin, ilwav, andgu, umgdr
  SC2-only:  chmav, kohma, druma, orzne, supbl, thrto, utwju,
             zfpst, pkufu, meltr, slypr

The remaining ~145 TW-Light user-contributed ships from the legacy
repository are not in scope for the initial port.

## License

GPLv2-or-later, inherited from TW-Light (whose source and assets we draw from).
