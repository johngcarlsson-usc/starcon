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

**Cinematic visuals now mirrored to the guest:**
- Persistent (per-tick `CinematicVisualState` row keyed by NetId,
  rendered as `CinematicVisualMirror` on the guest):
  - `UltimateBeam` — the three layered Arilou lightsaber blade
    triangles. Mirrored as flat sprite quads on the guest (the
    host's `SoftBladeMaterial` shader is dropped — the guest's
    rendered blade is solid-coloured, not soft-edged, which reads
    fine for the ~1.6 s sweep).
  - `LightspeedGlow` — the bluish Earthling jump halo. Mirrored
    as a flat disc (loses the radial-fade shader; still clearly
    "ship is glowing about to do something dramatic").
  - `PkunkAura` — soft halo per Pkunk clone. Critical so the
    opponent can see the clones light up; halos appear next to
    the clones (which themselves are full `Ship`s, already
    synced).
  - `MmrxfOverlaySprite` — the Mmrnmhrm "unleashed" sprite
    overlaid on the firing ship during transform / unleash.
    Without this the ship just goes invisible to the guest.
  - `YehatFighter` — each of the three orbiting Yehat fighter
    sprites. Their muzzle projectiles ride the existing
    `Projectile` snapshot path.
  - `MyconOrbit` — each of the eight Mycon plasma orbs spiralling
    in during MyconGathering. (The Hurricane phase converts them
    to homing `Projectile`s which are already synced.)
- Fire-and-forget (one-shot `CinematicSpawn` event, guest spawns
  a `GuestCinematicFade`-tagged sprite that ticks down on its own
  clock):
  - `BeamTrail` — Arilou blade smoke wisps. ~1 s lifetime each.
  - `BlastTrail` — Earthling blast streak fragments. ~0.5 s each.
  - `AsteroidGhost` — Slylandro launched-asteroid trail ghosts.
    Inherits the asteroid's sprite texture so the trail visually
    matches.
  - `MmrxfLaserSegment` — Mmrnmhrm tangled-laser bolt segments.
    ~0.1 s each.

Wire-format shape: one snapshot row per persistent visual
(`CinematicVisualState`), one event per fire-and-forget visual
(`CinematicSpawn`). Both ride alongside the existing combat
snapshot stream. NetIds for persistent visuals come from a new
`auto_assign_net_id_pub` on-add hook attached to the relevant
marker components (`UltimateBeam`, `LightspeedGlow`, `PkunkAura`,
`MmrxfOverlaySprite`, `YehatFighter`, `MyconOrbit`). A separate
`scan_cinematic_visuals` host system gathers the rows into
`CinematicVisualBuffer` so `send_ship_snapshot` stays under
Bevy's ~16-system-param cap.

**Intentionally NOT synced in this pass:**
- Ultimate cinematic camera close-up, black bars, captain
  portrait — these are local to the firing player. The opponent
  keeps control of their own view and sees the resulting world
  state with the persistent visuals + spawn trails above. Same
  model SC2 / TimeWarp used.
- `ChmmrFlash` — full-screen white flash on each Chmmr volley
  contact. UI Node overlay (covers the whole viewport regardless
  of camera scale); only meaningful to the firing player. Skipped
  for the same reason as the portrait.
- Audio entities (`ArilouStinger`, `UltimateVoicePlayer`) — no
  visible presence. The opponent hears their own combat SFX.
- `AlaryGrowing` permanent scale change — the ship's scale lives
  in `Transform.scale`, which the snapshot stream doesn't carry
  (the guest reconstructs poses from `Position` + `Rotation`,
  not `Transform`). Out of scope for this pass — would need a
  separate per-ship "visual scale" snapshot field.
- `SlylandroLaunched` per-asteroid tint pulse — the asteroid's
  POSE is already mirrored (`Asteroid` snapshot path), so the
  rocks fly correctly on the guest's screen, but they stay grey
  instead of glowing cyan. Fixable with a one-bit "launched"
  marker on the asteroid wire row; deferred.
- `MmrxfSplitMissile` parent + children — both are spawned as
  full `Projectile`s and so ride the existing projectile mirror
  path. The children's split-out is host-only, but the
  resulting child projectiles ARE snapshotted.
- `KohrAhBlade` — spawned as a `Projectile`; already mirrored.
- `Beam` (cinematic widened version): `tick_beams` runs on the
  host and the resulting beam pose is already in `BeamMirror`.
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
