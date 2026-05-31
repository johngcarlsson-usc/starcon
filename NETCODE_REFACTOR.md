# TODO: Netcode refactor — drop GGRS rollback, move to authoritative host

## Why

Rollback netcode (`bevy_ggrs`) requires perfect determinism across peers,
which has proven impossible to achieve cleanly in this codebase. Across
multiple rounds of fixes — registering ~25 components for snapshotting,
seeding all RNG, fixing input replay to read from `NetInputs` instead of
the live keyboard during rollback re-simulation — the game still
desynced in moderate-latency tests. The classic terminal-stage pattern
appeared every time: smooth play, brief rollback hitches, then runaway
divergence.

Remaining suspects we haven't bottomed out:

- System execution order inside `GgrsSchedule`. Dozens of gameplay
  systems with no explicit `.before/.after` constraints — Bevy is free
  to order them differently per peer.
- Query iteration order. Bevy queries iterate in archetype order, which
  depends on the order archetypes were created on each peer.
- Avian internal state (broad-phase caches, collider trees) isn't
  snapshotted by `bevy_ggrs` and may drift on rollback.
- A long tail of subtle differences (e.g. `HashMap` random hash seed,
  any unguarded `std::collections::HashMap` iteration) that each take
  hours to find.

Rather than continue whack-a-mole, switch to a model where determinism
isn't required at all.

## The plan: authoritative host + state snapshots

```
HOST (designated peer)              GUEST
  │                                   │
  ▼                                   ▼
Run full simulation              Read local keyboard
Apply own input                  Send PlayerInput to host
  │            ◄─ PlayerInput ────────┤
  ▼                                   │
Snapshot world                        │
  ├────── World snapshot ────────────►│
  │                                   ▼
  │                              Apply snapshot to local world
  │                                   │
  ▼                                   ▼
Render                            Render
```

- **Host election** at handshake time: lower `PeerId` wins. Both peers
  agree deterministically without negotiation.
- **Host owns the simulation.** Runs all gameplay systems. Determinism
  no longer required.
- **Guest sends inputs** (the same `PlayerInput` struct already used).
  No simulation locally.
- **Host sends snapshots** at ~20 Hz over an unreliable matchbox channel.
  Snapshot is the rollback-tracked component set: `Position`, `Rotation`,
  `LinearVelocity`, `AngularVelocity`, `Crew`, `Battery`, plus births /
  deaths keyed by a stable `NetId: u32` component.
- **Guest applies** snapshots straight onto local entities. Optional
  client-side prediction (move own ship locally, correct when snapshot
  arrives) lands as a polish pass later.

## What gets thrown out / kept

- **Drop**: `bevy_ggrs` dependency, `GgrsSchedule` registrations, all
  `rollback_component_*` registrations, `Session<Config>` lookups,
  `LocalPlayers`, `PlayerInputs<Config>`, the rollback ordering hacks.
- **Keep**: `matchbox_socket` transport, the `MatchConfig` /
  `SlotConfig` setup, the lobby UI, the existing `PlayerInput` Pod
  struct (re-used as the guest → host packet).

## Migration steps

1. **Foundation (this commit and the next few)**
   - Add `NetRole { Solo, Host, Guest }` resource. Election: sorted
     `PeerId` order; lower index = Host. Solo = no session.
   - Add `NetId(u32)` rollback-stable component. Host assigns at spawn
     (monotonic counter, scoped per match). Snapshots key by `NetId`.
   - Add a second matchbox channel (reliable, for ship picks /
     lobby votes) alongside the unreliable one (gameplay
     inputs + snapshots).
   - Define `NetMessage` enum: `Input { tick, input }`,
     `Snapshot { tick, entities: Vec<EntityState> }`,
     `Spawn { net_id, kind, init_state }`, `Despawn { net_id }`,
     `LobbyVote { class, ready }`.
   - Wire bincode encode/decode (already a dep).

2. **Switch gameplay scheduling**
   - Replace `GgrsSchedule` with `FixedUpdate.in_set(GameplaySet)`.
   - Add `.run_if(NetRole::is_host)` to every gameplay system. Guest
     bails out, leaving Avian physics off.
   - Avian's `PhysicsPlugins` runs on the host only too — the guest's
     world is purely visual at this point.

3. **Snapshot + apply**
   - Host system: every other `FixedUpdate` tick (20 Hz), gather the
     snapshot, encode, send over the unreliable matchbox channel.
   - Guest system: drain the matchbox channel, decode the latest
     snapshot, apply `Position` / `Rotation` / velocities / crew /
     battery onto local entities keyed by `NetId`. Spawn missing
     `NetId`s, despawn stragglers.

4. **Input forwarding**
   - Guest system: every frame, send `NetMessage::Input { tick, input }`
     over the unreliable channel.
   - Host system: drain inputs into `SlotInputs.held[guest_slot]` so
     existing gameplay systems read the guest's input the same way
     they already read the local handle's.

5. **Lobby votes**
   - Run the existing lobby plugin on the host only.
   - Guest sends `LobbyVote { class, ready }` over the reliable
     channel; host applies into `MatchConfig`.
   - Match restart still flows through `AppState::Resetting →
     InMatch` on the host; guest follows by snapshotted state.

6. **Re-enable ultimates in netplay**
   - With host-only simulation, the cinematic-pause-freezes-physics
     interaction goes away. The cinematic just runs on the host;
     guest sees the resulting world snapshots normally.
   - Drop the `session.is_some()` gate on `hyper_trigger`.

7. **Tear down GGRS**
   - Once host/guest is shipping, remove `bevy_ggrs` from `Cargo.toml`,
     drop `GgrsSchedule` registrations, delete the rollback component
     registrations, simplify `gather_slot_inputs` (no more
     `NetInputs` indirection — guest's input arrives via the message
     channel, host's input is local kbd).

## Open design questions

- **Snapshot diffing**: send a full snapshot every tick, or just deltas?
  Full snapshot is simpler and easily fits the bandwidth budget
  (~1 kB × 20 Hz = 20 kB/s). Defer deltas to polish.
- **Out-of-order snapshots**: matchbox unreliable can deliver
  out-of-order. Include `tick: u32` in every snapshot; ignore any
  that's older than the last applied. Standard pattern.
- **Reconnect / mid-match join**: out of scope for v1.
- **AI in netplay**: AI ticks on the host alongside human inputs.
  Guest never simulates AI.

## Status (host authority)

Last updated as visual-mirror work landed. Tracks what the guest
actually SEES of a host-authoritative match.

**Synced and rendering on the guest:**
- Ships — pose, velocity, crew, battery (`EntityState`).
- Ship shields — `ShieldActive` presence + `damage_factor`
  (rolled into `EntityState`). `draw_shield_rings` keys off the
  component on both peers, so this is what makes the canonical SC2
  shield bubble appear during an opponent's ability.
- Asteroids — pose, velocity, spawn descriptor (radius + sprite
  frame). Static `RigidBody` on the guest.
- Projectiles — `ProjectileMirror`, pose + sprite descriptor.
- Beams — `BeamMirror`, render quad pose + size + colour
  (Chmmr / Arilou / VUX laser).
- Damage zones — `DamageZoneMirror`, pose + size + sprite path
  (Glory blast, Thraddash fireball, Kohr-Ah ring, mines).
- Attached damage zones — `AttachedZoneMirror` (Umgah cone,
  Zoq-Fot-Pik tongue).
- Tractor beams — `TractorMirror` (Mycon gravity grip etc.).
- Sub-entities — `SubEntityMirror` (DOGI, Orz marines, Syreen
  crew pods, Kzer-Za fighters, MIRV missiles).
- Chmmr satellites — `SatelliteMirror`, orbit pose mirrored each
  tick.
- Asteroid explosions — fire-and-forget `ExplosionSpawn` events;
  guest spawns a local `AsteroidExplosion` that ticks on its own
  clock via the (now ungated) `tick_asteroid_explosions` system.
- Zap flashes — `ZapSpawn` events, same fire-and-forget shape
  as explosions.

**Already worked before this pass (no mirror needed):**
- Overlay sprites (Orz turret) — `spawn_ship` runs on both peers,
  `update_overlay_sprites` reads `Ship` pose locally.
- Gravity-field rings, starfield, HUD — all gizmo / sprite drivers
  that read local state.

**Intentionally NOT synced in this pass:**
- Ultimate cinematic visuals (BeamTrail, BlastTrail, PkunkAura,
  LightspeedGlow, AsteroidGhost, ArilouStinger, MmrxfLaserSegment).
  These are spawned by host-only `Update` systems and would need
  their own per-class mirror types — deferred because the
  cinematic camera control + portrait UI are themselves host-only
  (the guest doesn't enter the cinematic flow), so they'd need a
  parallel "guest sees a faithful cinematic from snapshot data"
  refactor that's well beyond this pass's scope. Net effect for
  the guest: a player using their ultimate looks weird (e.g. no
  saber-trail wisps), but combat result still lands via the
  ship + projectile + damage-zone mirrors that ARE synced.
- Smoothing / dead-reckoning on the guest's own ship between
  snapshots. Tracked as a separate planned task.
- Rollback / prediction. Same.

**Bandwidth note.** Snapshot bytes scale roughly as O(active
visuals × sprite-path-string length). For a typical 1v1 match
with ~5 projectiles, 1-2 beams, 3 satellites: ~1.2 kB per
snapshot × 60 Hz ≈ 70 kB/s. Slightly above the 50 kB/s guideline;
the obvious cost is the sprite-path string in every
projectile / sub-entity / zone row. A v2 polish pass could intern
those into a `u16` registry id at handshake time; deferring that
until we have a real bandwidth complaint.

## Future: revisit bevy_ggrs

The host/guest refactor was prompted by repeated GGRS desync. After
landing host authority, the root causes of those desyncs turned out
to be peer-local determinism violations in the simulation itself,
not GGRS-specific issues:

- `replenish_asteroids` ran during pre-match states (Loading /
  MainMenu / LobbyOnline) and spawned NetId-less asteroids at
  positions derived from the local camera, so each peer entered the
  match with a different starting set of rocks.
- Even in-match, `replenish_asteroids` filtered candidate positions
  by the LOCAL camera — same seeded RNG, but the camera is on each
  peer's own ship, so identical RNG draws produced different
  positions. GGRS rollback can't recover from that.
- Various systems consumed RNG / read `Time<Real>` / read camera
  state in ways that diverged between peers.

GGRS would have stumbled on every one of these because they're not
network errors — they're the two peers' code producing different
outputs from the same inputs. With those bugs now fixed,
"reinstate `bevy_ggrs` alongside host authority" is a much more
tractable project than it was when we gave up on it.

Concrete TODO if/when revisiting:
- Add a determinism-audit test: replay one match from a fixed seed
  twice and assert identical final entity poses.
- Pick the input model carefully — current code routes guest input
  via `NetMessage::Input`; GGRS would want that to ride its own
  channel and apply at the predicted tick.
- Verify Avian's solver iteration is bit-stable across runs (the
  `parallel` feature is off, which helps).
- The current host-authoritative path is fine for 1v1; a GGRS
  reinstatement mainly buys lower input latency for the guest's
  own ship without needing rollback prediction on top of snapshots.

## Until that's done

Networked play is broken. Current `bevy_ggrs`-based path stays in the
tree as the rough scaffolding for matchbox + lobby code paths the new
model will reuse — but `hyper_trigger` is already gated to "no
ultimates in netplay" and we may need additional gates on other
unstable systems as the host/guest model lands incrementally.
