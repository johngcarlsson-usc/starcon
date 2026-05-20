# Roadmap

Milestone-driven. Each milestone produces something runnable before moving on.

## M0 — Scaffold ✅

- Cargo project compiles
- Six ships' `.ini`/`.txt` salvaged into `assets/ships/`
- `.dat` sprite/sound archives parked in `assets/legacy-dat/` for later extraction
- Module skeleton: `ship`, `physics`, `input`, `netplay`

## M2 — Sprite extraction ✅

(Got bumped earlier than originally planned because it was easy.) Allegro 4
`dat` CLI shell-out, BMP → PNG with the engine's magenta key converted to
RGBA alpha, per-ship `manifest.json`. Run `cargo run --bin extract_dat --features tools`
to (re)generate.

## M1 — Local 1v1 melee ✅

Two ships, full gameplay loop:

- Avian 2D physics with real angular momentum and per-class damping
  (big ships boat-feel, small ships agile)
- Per-class dispatch in three independent surfaces: physics (`physics_spec`),
  primary weapon (`primary_weapon`), special ability (`trigger_specials`)
- Projectiles, weapon cooldown, crew counts, ram damage, win detection
- Match restart on R, running score, class picker (1-6 / F1-F6)
- Time-scale debug controls + extensibility seam doc (`src/timeflow.rs`)

## M3 — Class behaviour fill-in

The full canonical SC2 Ur-Quan Masters 25-ship roster is loaded and
selectable; physics + weapons + a per-class special arm all dispatch
correctly. Several specials are still placeholders (flagged with
`info!(... placeholder)` in `trigger_specials`) because they need
engine primitives we haven't built yet. Grouped by what's missing:

**AoE damage zones / area effects** (needed for): Shofixti Glory Device,
Kohr-Ah sawblades, Chenjesu DOGI mines, Slylandro self-destruct.

**Sub-entity AI / fighters** (needed for): Ur-Quan Kzer-Za fighters,
Chmmr satellites, Orz space marines.

**Targeting-without-damage** (needed for): Ilwrath cloak (untargetable
but visible), VUX limpet attachment, Mycon homing plasmoid.

**Mode-toggle abilities** (need a state component swap + re-derive of
ShipPhysicsDerived at runtime): Mmrnmhrm X↔Y form, Androsynth Blazer
mode, Melnorme charging fire.

**Cross-ship queries / mind effects** (need cross-entity reads in the
special arms): Syreen siren song, Melnorme confusion ray.

Each of these primitives is one focused commit. Adding them
incrementally lets the placeholder specials light up one at a time
without changing any class-dispatch shape.

## M4 — Rollback netplay

This is the big one. Three pieces, in order:

1. **Determinism audit + lockdown.** Move every gameplay system out of
   `Update`/`FixedUpdate` into `bevy_ggrs::GgrsSchedule`. Audit for non-
   determinism — no `Time` reads, no `HashMap` iteration, no random
   without a rollback-safe RNG. Disable Avian's SIMD paths (already off
   by feature flag) and pin its substep schedule to the GGRS clock.
2. **State snapshot ring buffer.** GGRS requires this; it's also the
   foundation for the time-rewind ability documented in `src/timeflow.rs`.
   Snapshot: ship transforms, velocities, crew, cooldowns, projectile
   list, match phase.
3. **Matchbox wiring.** Public signaling server (`wss://match.helsing.studio`)
   for dev; document self-host in M7. Two-peer melee, desync detection
   via per-frame state hash, optional resync via snapshot exchange.

## M5 — Fleet selection + rounds

- Pre-match fleet builder reading `fleets.ini` (500-point budget, ship caps)
- Multi-ship per side: when your active hull dies, next ship in fleet enters
- Match ends when a side has no ships left

## M6 — User-contributed TW-Light ships (deferred)

The legacy TW-Light repository has ~170 ships, of which 25 are canonical
SC2 (now all loaded — see M3). The remaining ~145 are user-contributed
hulls of varying quality. Not in scope for the initial port; revisit
once netplay and the M3 mechanic primitives are settled.

## M7 — Polish

- Audio (mix legacy WAVs through `bevy_audio` or `kira`)
- Title screen, fleet builder UI, match settings
- Web (WASM) build pipeline + self-host matchbox docs
- Replay recording / desync diagnostics
- Authentic projectile sprites (currently solid colour rectangles)

## Speculative — exotic abilities

These motivated the Avian + GGRS architectural choices. None of them require
a rewrite from where we are now; they're all single-class arms or single new
systems on top of the existing seams:

- **Time bubble** (per-region slow zone) — see `docs/EXTENSIBILITY.md`
- **Time rewind** (replay from snapshot ring buffer) — pairs cleanly with M4
- **Wormhole** (Avian teleport-on-overlap, two-entity pairing)
- **Tether / chain weapons** (Avian distance joints, already supported)
- **Subjective time** (per-ship clock scaling)

---

## Non-goals (for now)

- Adventure/exploration mode (TW had ambitions; we keep it melee-first)
- Mod loading (data-driven loading from `.ini` is the foundation, but no
  plugin API yet)
- Spectator mode
- Matchmaking lobbies (use plain Matchbox rooms; users share a URL)
