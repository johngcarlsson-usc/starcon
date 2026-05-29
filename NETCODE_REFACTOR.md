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

## Until that's done

Networked play is broken. Current `bevy_ggrs`-based path stays in the
tree as the rough scaffolding for matchbox + lobby code paths the new
model will reuse — but `hyper_trigger` is already gated to "no
ultimates in netplay" and we may need additional gates on other
unstable systems as the host/guest model lands incrementally.
