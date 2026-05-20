# Roadmap

Milestone-driven. Each milestone produces something runnable before moving on.

## M0 — Scaffold (✅ current)

- Cargo project compiles
- Six ships' `.ini`/`.txt` salvaged into `assets/ships/`
- `.dat` sprite/sound archives parked in `assets/legacy-dat/` for later extraction
- Module skeleton: `ship`, `physics`, `input`, `netplay`

## M1 — One ship moves on screen

- Bevy window + 2D camera
- Placeholder sprite (colored triangle) for one ship class
- Local keyboard control drives a `PlayerInput` → `Velocity` → `Transform`
- Toroidal arena wraparound

## M2 — Extract sprites from Allegro `.dat`

The original `.dat` files are Allegro 4 datafiles. They contain ship rotation
frames (typically 64 angles), thruster/explosion animations, and short PCM
sounds. We need to convert these once, offline, into PNG sheets + WAV/OGG so
the runtime stays simple.

Approach: write a small `xtask` (`cargo xtask extract-dat`) that parses the
Allegro datafile header and dumps each object. Format spec is in the legacy
source under `src/libraries/allegro/`.

## M3 — Local two-player melee

- Both keyboards (P1 arrows+ZX, P2 WASD+GH) drive two ships in the same arena
- Projectiles (`Bullet`) with TTL and per-ship damage
- Crew/Battery readouts
- One weapon + one special per ship (Earthling Cruiser is the easiest to start)

## M4 — Rollback netplay

- Move every gameplay system into `bevy_ggrs::GgrsSchedule`
- Audit for nondeterminism: no `Time` reads, no `rand` without a rollback-safe RNG, no `HashMap` iteration over game state
- Add `Rollback` components to ships, projectiles, particle handles
- Matchbox signaling against a public room URL for two-peer melee
- Desync detection via per-frame state hash

## M5 — Fleet selection + win conditions

- Pre-match fleet builder reading `fleets.ini` rules (point budget, ship caps)
- Round-based melee: ship dies → next ship in fleet enters → fleet eliminated = match over

## M6 — Port the remaining ships

176 ships in the original. Each is a `.cpp` of bespoke logic — we'll need to
reimplement weapon/special behaviour per ship in Rust. Group by complexity:

- **Easy** (Earthling, Spathi, Yehat, Mycon, Pkunk): simple projectile + one special
- **Medium** (Chmmr, Ur-Quan, Kohr-Ah, Utwig): area effects, sub-objects
- **Hard** (Slylandro Probe, Orz, Androsynth): mode changes, dimensional shift,
  ship sub-spawning

## M7 — Polish

- Audio (mix legacy WAVs)
- Title screen, fleet builder UI, match settings
- Web (WASM) build pipeline
- Replay recording / desync diagnostics

---

## Non-goals (for now)

- Adventure/exploration mode (TW had ambitions; we keep it melee-first)
- Mod loading (data-driven loading from `.ini` is the foundation, but no plugin API yet)
- Spectator mode
- Matchmaking lobbies (use plain Matchbox rooms; users share a URL)
