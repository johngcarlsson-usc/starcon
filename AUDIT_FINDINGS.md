# Netcode Audit — Findings

Branch: `worktree-agent-ad64f418ac2e73dd1` (from `claude/review-timewarp-legacy-ABWTL`)

Audit pass over the host-authoritative refactor + visual-sync work.
Scope: `src/netcode.rs`, `src/netplay.rs`, `src/ship.rs` (gates), `src/ability.rs`,
`src/ai.rs`, `src/ultimate.rs`, `src/lobby.rs`, `src/input.rs`, `src/hud.rs`.

Severity:
- `BUG` — must fix; visible to players / breaks something users will hit immediately
- `RISK` — latent; will surface under specific conditions
- `POLISH` — works as-is but ugly / brittle

---

## [BUG] [FIXED in 0f8fdbc] Guest can never see host's lobby vote → rematch is host-only

> **Resolved by `0f8fdbc` (symmetric input forwarding).**
> `push_local_input_to_netinputs` now forwards the local peer's
> `PlayerInput` in BOTH directions (gated only by `!Solo &&
> !peers.is_empty()`), and `drain_messages` applies inbound input on
> BOTH peers — writing the sender's input into
> `NetInputs.current[their_slot]`. The host's class pick + `FLAG_READY`
> bit (packed onto `PlayerInput`) therefore reach the guest, so the
> guest's `gather_slot_inputs` → `SlotInputs.held[host_slot]` carries
> the real values and its own `detect_all_ready` fires the rematch
> transition. Safe alongside `b219555`: `apply_player_input` is
> local-slot-only on the guest, so the host's input bytes never
> double-drive the host's ship (its pose stays snapshot-driven).
> Original analysis retained below for context.

`src/lobby.rs:135-182` (`detect_all_ready`), `src/netcode.rs:544-602`
(`push_local_input_to_netinputs`), `src/netcode.rs:1096-1113` (Input arm).

**What's wrong.** `push_local_input_to_netinputs` writes the LOCAL peer's
input into `NetInputs.current[local_slot]` and then, only if `role.is_guest()`,
forwards it as a `NetMessage::Input` to the host. The host writes the
guest's input into `NetInputs.current[guest_slot]` from `drain_messages`.
Nothing ever writes the host's input into `NetInputs.current[host_slot]`
on the guest. The host's `NetMessage::Lobby` / `NetMessage::LobbyVote`
wire variants exist but are pure debug logs in `drain_messages`
(`netcode.rs:1683-1688`) — never sent.

Concretely on the guest:
- `NetInputs.current[host_slot]` is permanently `PlayerInput::default()`
  (zeros).
- `gather_slot_inputs` copies that into `SlotInputs.held[host_slot]`,
  so the guest's view of the host has `class = 0` and `FLAG_READY = false`
  forever.
- `detect_all_ready` runs on both peers, but only the host's invocation
  ever observes both slots' READY flag set; the guest can never see
  the host's vote.

**Failure mode.** When both peers press R post-match:
1. Host detects all-ready → `AppState::Resetting` → `teardown_match`
   despawns host's ships, planet, asteroids, etc.
2. Snapshot after teardown carries no ship rows; the guest reads it as
   a heartbeat (`has_ships == false`) and skips all despawn sweeps.
3. Host re-enters `InMatch` → `spawn_match` repopulates the world,
   `NetIdAllocator.reset()` runs inside `spawn_asteroids` so host's
   new asteroids are NetId 1..8.
4. Host snapshots resume. Guest is **still** in `MatchPhase::PostMatch`
   in `AppState::InMatch` — never went through Resetting. So:
   - Guest's stale ship entities from the previous match get their
     poses reconciled onto the host's new spawn positions; visually
     it kinda works, but `MatchPhase` is stuck → status banner shows
     the post-match overlay forever, READY toggle still active.
   - `detect_winner` is gated out (returns early in PostMatch) so it
     can never reset the phase.
   - Guest's stale-NetId asteroid mirrors from the previous match
     match-update to the new asteroid poses (see "Stale-match NetId
     collisions" below) — the guest never sees genuinely-new rocks
     because the IDs collide with old ones it still has.

**Why not fixed in this pass.** Two viable fixes:
1. Have the host send its slot's input over the wire (mirror of
   the guest→host path), or
2. Send `NetMessage::Lobby` from the host each tick during PostMatch
   and use it to drive `MatchPhase` transitions on the guest.

(1) is the smaller change but needs a new direction on the wire format
and host-side send. (2) needs a real `MatchPhase` reconciler on the guest.
Both feel structural enough that I left them for the orchestrator to
route — the other agent is in `netcode.rs` for cinematic visuals and we
don't want to fight over the wire format mid-edit.

---

## [BUG] [FIXED in 0f8fdbc] Guest never enters `Resetting`; stale match entities leak forever

> **Resolved by `0f8fdbc`** as a consequence of the lobby-vote fix
> above. With the host's `FLAG_READY` bit now reaching the guest, the
> guest's own `detect_all_ready` fires and drives its local
> `AppState::Resetting` transition — so `teardown_match` runs on the
> guest too, clearing the previous match's ship / asteroid / mirror
> entities before the next round spawns. No new wire-format signal was
> needed; the existing input-echo path carries enough state.

`src/main.rs:122` (`teardown_match` on Resetting), `src/lobby.rs:181`
(only host sets `AppState::Resetting`).

**What's wrong.** `teardown_match` only runs on `OnEnter(Resetting)`.
The only places that transition the local state machine into Resetting
are:
- `main.rs:request_rematch` — bails when `session.is_some()`, so doesn't
  fire in netplay.
- `lobby.rs:detect_all_ready` — gated by all-slots-ready, which the
  guest can never observe (see previous finding).
- `class_picker_input` — gated by `session.is_some() → return` in netplay.

So the guest's `teardown_match` literally never runs in a netplay session.
Every match's ship / asteroid / mirror entities accumulate across rematches
forever. Combined with stale-NetId reconcile, the visible result on the
guest's window after a few rematches is a growing population of mirror
entities with stuck poses.

**Proposed fix (out-of-scope).** Either tie the guest's `AppState`
transition to a wire-format signal from the host (cleanest: a
`NetMessage::AppState` enum), or piggyback on the snapshot — e.g. a
`match_id: u32` field that the guest watches for changes and uses to
trigger its own Resetting transition. Same wire-format edit reluctance
as above.

---

## [BUG] [FIXED in 0f8fdbc] Stale-match NetId collisions on guest

> **Resolved by `0f8fdbc`.** This finding's own analysis noted the
> root cause was the previous bug: "without a guest-side
> `teardown_match` trigger, fixing the allocator alone doesn't help."
> Now that the guest goes through `Resetting` → `teardown_match` on a
> rematch, its previous-match mirror entities (and their NetIds) are
> despawned before the host's new NetId-1.. entities arrive, so the
> reconcile loops no longer match new host data onto stale mirrors.

`src/netcode.rs:1241-1260` (asteroid spawn fallback),
`src/ship.rs:7680` (`alloc.reset()` in `spawn_asteroids`).

**What's wrong.** Even if the host-side rematch is the only thing that
works (preceding bugs), the guest's `NetIdAllocator` and live mirror
entities persist into the next match. Host's new asteroid NetIds restart
at 1..8 (because `spawn_asteroids` resets the allocator). Guest's
**previous-match** asteroid mirrors still carry NetIds 1..8 (or any
projectile / beam / sub-entity from the old match that happened to land
in that range). The reconcile loops match by NetId, so they update the
poses of the OLD entities to the NEW host's data — wrong sprite, wrong
radius, wrong physics body type (Static vs Dynamic) — and the
`has_ships` sweep can't despawn them because the host's first
post-teardown snapshot is heartbeat-shaped.

**Visible failure.** After a rematch, the guest's "new" asteroid field
still looks like the previous match's rocks, just teleporting to the
new positions; new projectiles render with old sprite assets if the
NetId range overlaps.

**Why not fixed in this pass.** Root cause is the previous bug — without
a guest-side `teardown_match` trigger, fixing the allocator alone
doesn't help.

---

## [BUG] [FIXED in b219555] Guest's `apply_player_input` overrides opponent ships' angular velocity to 0 every tick

`src/ship.rs:3289-3542` (`apply_player_input`),
`src/ship.rs:1282-1296` (registration — no role gate, runs on all peers).

**What's wrong.** `apply_player_input` queries `&mut q: Query<(&Ship, ...)>`
(ALL ships, not just the local slot) and writes `ang_vel.0`,
`thrust.0`, and in the Arilou case `lin_vel.0`, based on
`slot_inputs.held[ship.player_slot.min(3)]`. On the guest,
`SlotInputs.held[host_slot]` is permanently zeros (see lobby-vote bug),
so every FixedUpdate tick the guest sets the opponent ship's angular
velocity to `0 * derived.target_omega = 0` (Classic mode) and its thrust
to zero. The snapshot reconcile in Update then writes the host's
authoritative velocity back, but Avian's solver in the next FixedUpdate
integrates with the clobbered angvel.

The comment on `apply_player_input`'s registration is explicit that this
should be local-only prediction:

> Movement + per-ship state. Runs on EVERY peer — including the
> guest — so the guest keeps locally predicting its own ship's motion
> (snapshots then correct it).

The intent is right but the system iterates every ship in the query, so
it's "predicting" opponent ships with zero input. For Arilou's
inertialess case (line 3513-3527), this is fatal: `lin_vel.0 = Vec2::ZERO`
gets written every tick because the opponent's `INPUT_THRUST` is
unset, then snapshot updates it, then it's zeroed again.

**Failure mode.** Opponent ships visually micro-stutter — Avian gets one
physics step of bad angular vel between snapshots — and an opponent
Arilou freezes between snapshots because lin_vel is zeroed twice per
snapshot interval. At 60 Hz snapshot rate this is just below
perceptual, but at any lower rate it's a strobe.

**Proposed fix.** Filter `apply_player_input` (and `cap_velocity`,
`tick_orz_turret`, `tick_slylandro_drift`, `tick_ship_modes`,
`tick_weapon_cooldown`, `tick_special_cooldown`, `tick_shield`,
`tick_battery_recharge`, `tick_point_defense`, `orient_projectiles`,
`process_mode_toggle_requests`) to only operate on the local slot when
`role.is_guest()`. The cleanest shape is a query-time filter:
`if role.is_guest() && ship.player_slot != local.0 { continue; }`.

In this pass: applied the smallest possible local-slot guard inside
`apply_player_input` itself — see commit below — and left the other
"per-ship state" systems untouched because most are no-ops on the guest
already (e.g. weapon cooldowns aren't read by anything on the guest;
recharge timers don't drive any visible state because Battery is
snapshot-reconciled). Worth a follow-up audit pass.

---

## [RISK] `destroy_zero_crew_ships` and `detect_winner` run on guest without authority gate

`src/hud.rs:79-84` (registration).

> **Update — partial fix landed.** Live 2-peer testing exposed the
> hole this item hand-waved: `destroy_zero_crew_ships` despawns the
> host's ship the *same* tick crew hits 0, so the crew=0 value never
> reaches a snapshot and the ship's row simply vanishes — the guest's
> `Changed<Crew>` never fires and it keeps a ghost ship alive ("died on
> one screen, alive on the other"). `drain_messages` now carries a
> ship-slot-absent despawn sweep (mirror of the asteroid sweep): on a
> real snapshot, any local ship whose `player_slot` is missing gets
> despawned, so the guest's `detect_winner` can end the match too.

**What's wrong.** Both systems run on every peer in `FixedUpdate` with
no `role_is_authoritative` gate.
- `destroy_zero_crew_ships` watches `Changed<Crew>` and despawns ships
  where `crew.current <= 0`. Crew is snapshot-reconciled, so when the
  host's snapshot says crew=0, the guest's `Changed<Crew>` fires the
  same tick and the local despawn keeps the guest in sync.
- `detect_winner` mutates `MatchPhase` and `MatchOutcome.wins[]` based
  on the local Crew query. Both peers reach the same conclusion
  independently because they're working from identical snapshot data.

**Why I reconsidered and didn't gate.** Initially flagged as needing
gates. On closer look:
- Gating `destroy_zero_crew_ships` host-only would leave a zombie
  ship on the guest's screen because there's currently no
  "ship row vanished from snapshot → despawn the mirror ship" sweep
  in `drain_messages` (the asteroid + projectile + visual-mirror
  sweeps exist; ships are by-slot and rely on the despawn happening
  locally). Until that reconcile path is added, the local despawn IS
  the only correct behaviour for the guest.
- Gating `detect_winner` host-only would leave the guest stuck in
  `MatchPhase::Live`, which suppresses the post-match status banner
  AND the Ready toggle (`tick_local_ready_toggle` bails on non-
  PostMatch). The guest's local computation produces the same answer
  as the host's because the input data (Crew per slot) is identical,
  so running it twice independently is benign — both peers reach the
  same `MatchPhase` + same `wins[]` increments.

The "double counting" wins concern doesn't apply: each peer maintains
its OWN `MatchOutcome` and reads only its own copy for display, so
host's `wins[0] = 1` and guest's `wins[0] = 1` show the same `(1)`
score — symmetric, not doubled.

The proper fix is to add ship-row despawn reconcile to `drain_messages`
and then gate both systems host-authoritative + echo the outcome over
the wire. That's the same wire-format edit the rematch / lobby-vote
bugs need; left for the structural pass.

---

## [RISK] [FIXED in 0f8fdbc] `update_status_banner` shows wrong opponent class on guest

> **Resolved by `0f8fdbc`.** The host's `class` byte now rides its
> echoed `PlayerInput` to the guest every tick, so
> `slot_inputs.held[host_slot].class` carries the host's real pick and
> the banner renders the correct opponent class.

`src/hud.rs:482-490`, depends on the lobby-vote-bug above.

**What's wrong.** In netplay PostMatch, the banner renders one line per
slot showing each peer's class vote, read from
`slot_inputs.held[i].class`. On the guest, `held[host_slot].class` is
permanently 0 (never populated — see lobby-vote bug), so the host's
slot always shows "EARTHLING CRUISER" (the index-0 class) regardless of
what the host actually picked.

**Status.** Symptom of the lobby-vote bug — fixing that fixes this.
Logged separately so we don't lose it after the root cause is fixed.

---

## [RISK] [RESOLVED — stubs deleted] `LobbyVote` / `Lobby` wire-format variants are receive-only stubs

> **Resolved by deleting the dead stubs.** Now that the lobby vote
> definitively rides on `NetMessage::Input` (the host's class +
> `FLAG_READY` bits are packed onto its echoed `PlayerInput`), the
> `NetMessage::Lobby` / `NetMessage::LobbyVote` variants and the
> `LobbySlot` struct were never emitted and only logged on receipt.
> They've been removed along with their debug-log receive arms, so the
> enum no longer advertises a path that doesn't exist.

**What was wrong.** The enum variants existed but nothing in the host's
`send_ship_snapshot` (or anywhere else) emitted them. The receive arms
just logged at debug level — dead code that masked the bug because a
reader saw them in the enum and assumed they worked.

---

## [RISK] `tick_asteroid_explosions` and `tick_zap_flashes` despawn on guest's local clock

`src/ship.rs:1390-1393` (ungated registration),
`src/ship.rs:8125-8151` + `7190-` (the tickers).

**What's wrong.** These tick systems run on both peers (intentional —
they animate guest-spawned mirrors of host explosions). They despawn
the entity on `remaining_s <= 0.0`. On the guest, the entity was spawned
locally from a `VisualEventQueue::explosions` row (carrying `total_s`
implicitly via the spawn helper's hardcoded `0.4`s). Local `Time` may
differ from host's by a frame or two, but the explosion's own clock
starts at spawn and is independent on each side — no real race.

Marked as a risk because the contract is fragile: if any future
visual-event spawn passes a non-default duration, the guest needs the
same duration (currently encoded in `ExplosionSpawn.radius` only —
total duration is hardcoded in `spawn_asteroid_explosion`). Zaps already
carry `total_s` in the wire format, so they're fine.

**No fix in this pass.** Working as intended; document is the patch.

---

## [RISK] Mirror entity sprite asset updates only at spawn time

`src/netcode.rs:1290-1320` (projectile mirror update loop) — note
projectile updates don't re-set `sprite.image`, only pose. Same for the
first-spawn branch in tractor mirror (line 1486-1500) updates
`sprite.image` but only conditionally. Beam / damage_zone mirrors
don't update sprite.image after spawn either.

**What's wrong.** If a host-side projectile or beam changes its sprite
mid-flight (e.g. animation frame swap, charge-stage change), the
guest's mirror keeps showing the first sprite it saw. Today this only
matters for ships whose abilities mutate the projectile sprite over
its lifetime; none of the current weapons do that, so it's latent.

**No fix in this pass.** Would need per-mirror "update sprite if path
changed" branches, which is a structural edit to the wire format /
reconciler. Other agent's lane.

---

## [POLISH] `send_heartbeat` and `send_ship_snapshot` both increment `sock.heartbeat_s`

`src/netcode.rs:609` and `src/netcode.rs:730`.

`send_heartbeat` adds delta to `heartbeat_s` then checks against
`HEARTBEAT_INTERVAL_S` (1.0s). `send_ship_snapshot` ALSO adds delta to
`heartbeat_s` then checks against `SNAPSHOT_INTERVAL_S` (1/60s). Both
run every Update (chained via `.chain()`), so on the host the same
`delta_secs` gets added twice per frame. Effective heartbeat / snapshot
rate is ~2× what it claims.

**No fix.** Documented in the comments at line 731-733; the snapshot
loop is the load-bearing one and overshoots happily.

---

## [POLISH] [RESOLVED] `bevy_ggrs`-era comments + `auto_add_rollback` stub remain

> **Resolved.** The no-op `auto_add_rollback` hook and its four
> `#[component(on_add = auto_add_rollback)]` attributes (Ship,
> OrzMarineBoarded, Asteroid, Planet) were deleted, along with the
> stale GGRS-era module doc in `src/netplay.rs` (which still described
> a rollback session that no longer exists) and the orphaned
> `read_local_inputs` doc comment. The dead GGRS constants `FPS` and
> `INPUT_DELAY` and the unused `role_is_guest` run-condition went too.

The hook was a no-op and `bevy_ggrs` is gone; the stub's own comment
said "Remove once those attrs are cleaned up." Done.

---

## [POLISH] Visual-only Update systems for ultimates rely on guest's `UltimateState` staying Idle

`src/ultimate.rs:843-868` — `tick_beam_trails`, `tick_blast_trails`,
`tick_pkunk_clone_visual`, `drive_camera_during_ultimate`, etc., are
registered on Update with no role gate. They read `UltimateState`.

On the guest, `UltimateState` is mutated only by `tick_ultimate_phases`
and `hyper_trigger`, both gated `role_is_authoritative` → never tick on
the guest. So `UltimateState.phase` stays at the default. None of these
visual ticker systems spawn or destroy gameplay state, so the gate
isn't strictly necessary. But it means anything that DOES rely on
`UltimateState` being updated to render correctly (e.g. portrait, glow
material updates) is silently dead on the guest.

The documented intent is "guest doesn't see the cinematic" → matches.
But this should be a deliberate gate at registration time, not an
implicit consequence of state never updating.

**No fix in this pass.** Cosmetic.

---

# Fixes applied in this pass

## Commit b219555 — `ship: gate apply_player_input to local slot on guest`

Added `role.is_guest()` + `LocalHandle` checks to `apply_player_input` so
non-local-slot ships are skipped on the guest. Before this, the guest
was clobbering opponent ships' `AngularVelocity` (and Arilou
`LinearVelocity`) to 0 every FixedUpdate based on the all-zero default
input the opponent slot has in `SlotInputs.held` on the guest. After
the fix, opponent ships' poses + velocities are exclusively driven by
the snapshot reconciler.

Also added a clarifying comment block in `src/hud.rs` next to the
`(destroy_zero_crew_ships, detect_winner)` registration explaining why
those systems intentionally don't carry an authority gate (the rationale
in `[RISK]` `destroy_zero_crew_ships / detect_winner` above).

`cargo check --target wasm32-unknown-unknown`: clean.

## Commit 0f8fdbc — `netcode: symmetric input forwarding`

Forwarded each peer's `PlayerInput` in BOTH directions and applied
inbound input on BOTH peers. This was the single fix the "Quick
takeaways" below called for (option 1) and it resolved the three
top BUGs (#1 lobby vote, #2 guest-never-Resetting, #3 stale-match
NetId collisions) plus the two dependent RISK items (opponent class
display, lobby-vote stubs) as one coherent change — no new wire-format
variant or `Snapshot` field was needed, because the host's class +
`FLAG_READY` bits already fit on the echoed `PlayerInput`.

## Cleanup pass — dead-code removal + doc reconcile

Following the input-forwarding fix:
- Deleted the now-dead `NetMessage::Lobby` / `NetMessage::LobbyVote`
  variants, the `LobbySlot` struct, and their debug-log receive arms
  (lobby votes ride `NetMessage::Input`).
- Removed the no-op `auto_add_rollback` hook + its four
  `#[component(on_add = …)]` attributes.
- Rewrote the stale GGRS-era `src/netplay.rs` module doc to describe
  the host-authoritative handoff that's actually shipping; removed the
  orphaned `read_local_inputs` doc comment.
- Removed the dead GGRS constants `FPS`, `INPUT_DELAY` and the unused
  `role_is_guest` run-condition.
- Reconciled this document: findings #1–#3 and the two dependent RISK
  items are marked FIXED/RESOLVED above.

`cargo check --target wasm32-unknown-unknown`: clean (24 warnings, all
pre-existing domain-level dead-code, none from the netcode path).

# What remains (still open / deliberately untouched)

- **RISK** — Mirror entity sprite asset only updates at spawn time.
  Latent; no current weapon swaps a projectile/beam sprite mid-flight.
- **RISK** — `destroy_zero_crew_ships` / `detect_winner` run on the
  guest without an authority gate. Re-examined and left as-is: both
  peers reach the same answer from identical snapshot data, and gating
  host-only would need a ship-row despawn reconcile in `drain_messages`
  that doesn't exist yet.
- **POLISH** — `send_heartbeat` + `send_ship_snapshot` both increment
  `heartbeat_s`; effective rate ~2× nominal. Harmless overshoot.
- **POLISH** — Visual-only ultimate Update systems lack explicit role
  gates (correct by virtue of `UltimateState` never updating on the
  guest, but implicit).
- Pre-existing domain dead-code warnings (ship-stat config fields,
  placeholder ability variants the ROADMAP tracks as in-progress,
  unused ultimate-cinematic mesh consts). Left untouched — these are
  parsed config / intentional placeholders, not refactor debris.

# Quick takeaways for the orchestrator

The top-priority **rematch / lobby vote sync** bug is now fixed
(`0f8fdbc`). The cleanest remaining proof-of-correctness is a real
2-peer run: host + guest, play a round, both press R, confirm the
guest tears down and re-enters cleanly with the host's actual class
pick shown. Everything still open is RISK/POLISH (see list above) —
no remaining BUG-severity items.
