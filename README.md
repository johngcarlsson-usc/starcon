# Starcon

A modern, from-scratch rewrite of [Star Control: TimeWarp / TW-Light](https://sourceforge.net/projects/timewarp/) —
a fan-made Star Control 2 "Super Melee" combat game. The original is C++/Allegro 4
and difficult to build on modern systems. This rewrite uses **Rust + Bevy + GGRS**
for proper rollback netcode out of the gate.

## Status

Pre-alpha. The scaffold compiles and loads ship stats from the salvaged
`.ini` data; no rendering, physics, or netplay is wired up yet. See
[`docs/ROADMAP.md`](docs/ROADMAP.md) for the milestone plan.

## Stack

| Concern    | Choice                              | Why                                                                       |
| ---------- | ----------------------------------- | ------------------------------------------------------------------------- |
| Engine     | Bevy 0.18                           | ECS, hot iteration, native + WASM, mature stable                          |
| Netcode    | GGRS via `bevy_ggrs` + `bevy_matchbox` | Rollback (the gold standard for twitch combat); WebRTC P2P for free       |
| Tick rate  | 60 Hz fixed                         | Deterministic stepping required for rollback                              |
| Targets    | Linux / macOS / Windows / Web (WASM) | One netcode path across all platforms                                     |

## Running

```sh
cargo run
```

First build pulls Bevy and friends, expect ~5–10 minutes.

## Asset salvage

`assets/ships/*.ini` and `*.txt` are the original TW-Light per-ship stat files
and lore — clean text, GPLv2. `assets/legacy-dat/*.dat` are the original Allegro 4
datafiles containing sprites and sounds; these still need an extractor written
(see `docs/ROADMAP.md`). Starting subset:

- `earcr` — Earthling Cruiser
- `kzedr` — Ur-Quan (Kzer-Za) Dreadnought
- `chmav` — Chmmr Avatar
- `spael` — Spathi Eluder
- `yehte` — Yehat Terminator
- `mycpo` — Mycon Podship

The remaining ~165 ships will be ported once the engine handles these six.

## License

GPLv2-or-later, inherited from TW-Light (whose source and assets we draw from).
