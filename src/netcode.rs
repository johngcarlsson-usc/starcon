//! Authoritative-host netcode foundation. See `NETCODE_REFACTOR.md`
//! at the project root for the full migration plan. This module owns
//! the pieces that the eventual host / guest split will share:
//!
//!   - `NetRole` resource — Solo / Host / Guest, decided once at
//!     `LobbyOnline` handshake time and never re-elected mid-match.
//!   - `NetId` component — stable per-entity identifier used as the
//!     join key in snapshots. Host assigns at spawn; guest mirrors.
//!   - `NetMessage` enum — wire-format envelope for everything the
//!     two peers exchange (input forwarding, state snapshots, lobby
//!     votes, spawn / despawn).
//!
//! Nothing in this module wires gameplay yet — `bevy_ggrs` still runs
//! the live simulation. The split-out lets us land the wire format,
//! host election, and ID scheme in isolation, so the gameplay-tick
//! cutover stays small.

use bevy::prelude::*;
use bevy_matchbox::matchbox_socket::{PeerId, WebRtcChannel};
use serde::{Deserialize, Serialize};

/// Who's running the simulation for this match.
///
/// Election happens once during the matchbox handshake
/// (`netplay.rs:start_p2p_session`): the connected peer with the
/// lexicographically lower `PeerId` becomes the `Host`, the other
/// becomes the `Guest`. Both peers compute the same answer
/// independently — no negotiation packet needed.
///
/// `Solo` is the default for hotseat / vs-AI play with no matchbox
/// session.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum NetRole {
    #[default]
    Solo,
    Host,
    Guest,
}

impl NetRole {
    /// True iff this peer owns the gameplay simulation. Host in
    /// netplay; the local process in single-player. Guests render
    /// snapshotted state from the host and never run gameplay
    /// systems locally.
    pub fn is_authoritative(self) -> bool {
        matches!(self, NetRole::Solo | NetRole::Host)
    }
    pub fn is_guest(self) -> bool {
        matches!(self, NetRole::Guest)
    }
}

/// Pick the `NetRole` for THIS peer given the sorted PeerId list
/// (which is the same on both peers because both received the same
/// set of `MessageReceived(_, Connected)` events). The peer whose own
/// PeerId is at index 0 of the sorted list is the Host.
pub fn elect_role(local: PeerId, sorted_peers: &[PeerId]) -> NetRole {
    match sorted_peers.first() {
        Some(first) if *first == local => NetRole::Host,
        Some(_) => NetRole::Guest,
        None => NetRole::Solo,
    }
}

/// Stable cross-peer identifier for a netplay entity. The host
/// allocates a fresh value at spawn time (monotonic counter scoped
/// per match); guests apply the same id to whichever local entity
/// they spawned to mirror it. Snapshots use this — NOT Bevy `Entity`
/// values, which can't match across processes.
///
/// 0 is reserved as "not yet assigned"; first real id is 1.
#[derive(
    Component, Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct NetId(pub u32);

/// Per-match monotonic counter for `NetId` allocation. Lives on the
/// host; guests don't touch it.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct NetIdAllocator {
    next: u32,
}

impl NetIdAllocator {
    pub fn allocate(&mut self) -> NetId {
        self.next = self.next.wrapping_add(1);
        // Skip 0 — the "unassigned" sentinel.
        if self.next == 0 {
            self.next = 1;
        }
        NetId(self.next)
    }
    pub fn reset(&mut self) {
        self.next = 0;
    }
}

/// Wire-format envelope for everything the peers exchange. Encoded
/// with `bincode` (already a project dep). One enum so the receive
/// loop is a single `bincode::deserialize::<NetMessage>(bytes)`
/// instead of a per-channel split.
///
/// `tick` fields are the host's `FixedUpdate` tick counter; the
/// guest uses them to discard out-of-order snapshots and to
/// reconstruct timing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetMessage {
    /// Guest → Host. Sent every frame the guest has a new input.
    Input {
        tick: u32,
        input: crate::input::PlayerInput,
    },
    /// Host → Guest. Periodic full-world snapshot. Carries every
    /// netplay entity's `NetId` + gameplay state (pose, velocity,
    /// crew, batt). Guest applies straight onto local entities,
    /// spawning missing IDs and despawning stragglers.
    ///
    /// `projectiles` rides alongside in its own list (not `entities`)
    /// because a projectile needs a sprite descriptor — path + size +
    /// tint — that ships and asteroids don't, and keeping it separate
    /// avoids bloating every ship/asteroid row with empty visual
    /// fields. Under host authority the guest runs no combat sim, so
    /// these are the ONLY projectiles it sees — render-only mirrors of
    /// the host's authoritative shots.
    ///
    /// The visual lists (`beams`, `damage_zones`, `attached_zones`,
    /// `tractors`, `sub_entities`, `satellites`) ride along for the
    /// same reason: each combat ability has its own host-side
    /// component flavour the guest can't see, so we project them all
    /// down to "pose + sprite" rows and let the guest spawn one
    /// mirror entity per NetId. Heartbeats omit them (empty vecs);
    /// real snapshots drive both pose-update and lifecycle (a NetId
    /// missing from a real snapshot is a despawn signal).
    ///
    /// `explosions` and `zaps` are fire-and-forget spawn events
    /// instead of per-snapshot rows because they're millisecond-
    /// lifetime sprite-only animations — re-streaming every active
    /// one each tick would burn bandwidth on visuals that already
    /// have a perfectly fine local tick system on the guest. Host
    /// pushes one event into the queue at spawn time, the guest
    /// spawns a local copy that ticks down on its own clock.
    Snapshot {
        tick: u32,
        entities: Vec<EntityState>,
        projectiles: Vec<ProjState>,
        beams: Vec<BeamState>,
        damage_zones: Vec<DamageZoneState>,
        attached_zones: Vec<AttachedZoneState>,
        tractors: Vec<TractorState>,
        sub_entities: Vec<SubEntityState>,
        satellites: Vec<SatelliteState>,
        explosions: Vec<ExplosionSpawn>,
        zaps: Vec<ZapSpawn>,
        /// Per-tick state for persistent cinematic visuals
        /// (UltimateBeam, LightspeedGlow, PkunkAura, ...).
        /// Empty on heartbeats; full rows on real snapshots.
        /// A NetId missing from a real snapshot is the despawn signal.
        cinematic_visuals: Vec<CinematicVisualState>,
        /// Fire-and-forget cinematic spawns (trail wisps, laser
        /// segments, asteroid ghosts). Accumulated since the last
        /// snapshot via `Added<>` queries on the host.
        cinematic_spawns: Vec<CinematicSpawn>,
    },
}

/// One row of a `NetMessage::Snapshot`. Minimal for now — covers
/// what we already snapshot via `bevy_ggrs`. Future additions
/// (cooldowns, mode index, shield) land here as the host/guest
/// model expands beyond the position-and-crew baseline.
///
/// `net_id` is set to 0 for ships in the v1 snapshot — ships are
/// keyed by `player_slot` instead because both peers spawn slots in
/// the same order via `spawn_match`. `NetId` becomes meaningful
/// once projectiles + sub-entities start syncing (they have no
/// natural cross-peer key).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityState {
    pub net_id: NetId,
    pub kind: EntityKind,
    pub pos_x: f32,
    pub pos_y: f32,
    pub rot_cos: f32,
    pub rot_sin: f32,
    pub vel_x: f32,
    pub vel_y: f32,
    pub ang_vel: f32,
    pub crew: i32,
    pub batt: i32,
    /// Active shield damage-factor on this ship, or `None` if the
    /// host's `ShieldActive` component isn't present this tick.
    /// `draw_shield_rings` runs on both peers and keys off the
    /// component's presence, so reconciling this on the guest is
    /// what makes the canonical SC2 shield bubble actually render
    /// during the opponent's ability — without it, the guest sees
    /// damage being absorbed with no visible cause.
    pub shield_factor: Option<f32>,
}

/// Which family of game object an `EntityState` is describing.
/// Drives guest-side spawn shape (sprite / collider / components)
/// when a new `NetId` appears in a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EntityKind {
    Ship {
        /// Index into `ship::ALL_CLASSES`.
        class_idx: u8,
        slot: u8,
    },
    Projectile,
    Asteroid {
        /// Collider + visual radius. The guest needs it to build a
        /// matching mirror the first time this NetId appears.
        radius: f32,
        /// 1-based `ASTERO01..64` sprite frame.
        frame_idx: u8,
    },
    Planet,
    SubEntity,
}

/// One projectile in a `NetMessage::Snapshot`. Carries pose + a
/// self-describing sprite (asset path, on-screen size, tint) so the
/// guest can spawn a render-only mirror without a shared weapon
/// registry — the host just reads these straight off the live
/// projectile's `Sprite` component. `vel_*` lets the guest dead-
/// reckon the mirror between snapshots so fast shots don't strobe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjState {
    pub net_id: NetId,
    pub pos_x: f32,
    pub pos_y: f32,
    pub rot_cos: f32,
    pub rot_sin: f32,
    pub vel_x: f32,
    pub vel_y: f32,
    /// `Sprite.image` asset path (e.g. `ships/chebr/sprites/..png`).
    pub sprite_path: String,
    /// `Sprite.custom_size` (square side); 0 → leave sprite native.
    pub size: f32,
    /// `Sprite.color` linear RGBA.
    pub color: [f32; 4],
}

/// Marker on a guest-side render-only projectile mirror. Carries a
/// `Transform` + `Sprite` only; the host's snapshot stream is the
/// sole driver of its pose and lifetime (no local physics/collision).
#[derive(Component)]
pub struct ProjectileMirror;

/// Last-known snapshot velocity (world units/sec) for a moving mirror
/// (projectiles, sub-entities). Between snapshots — which arrive at
/// `SNAPSHOT_INTERVAL_S`, ~33 ms apart — `extrapolate_mirrors` advances
/// the mirror's `Transform` by this velocity so fast-moving projectiles
/// glide smoothly instead of stepping once per snapshot. Each snapshot
/// resets both pose and velocity, so prediction error never accumulates
/// past one interval. Ships don't need this: the guest spawns them as
/// real Avian bodies that integrate their snapshot velocity already.
#[derive(Component, Default)]
pub struct MirrorVel(pub Vec2);

// ----------------------------------------------------------------
// Visual-mirror wire formats.
//
// Each persistent visual entity the host can spawn during combat
// gets one of these state rows + a guest-side `*Mirror` marker
// component. The mirror entities carry ONLY `Transform` + `Sprite`
// (no physics, no collider, no gameplay markers): the snapshot
// stream is the sole driver of their pose, size, colour, and
// lifetime. A NetId that drops out of a real (non-heartbeat)
// snapshot is the despawn signal.
//
// Wire rep choice: every visual ships its sprite descriptor
// (asset path / size / colour / endpoint geometry) verbatim each
// snapshot — same approach as `ProjState`. That's a few bytes per
// row vs. a shared "weapon flavour enum" but lets the guest spawn
// faithful mirrors without any cross-peer registry of sprite ids.
// Costs O(active visuals × snapshot rate) bandwidth; for a 1v1
// match with ~5 combat visuals this is sub-kB/s and easily fits
// in the existing ~50 kB/s budget.
// ----------------------------------------------------------------

/// One beam (Chmmr / Arilou / VUX laser) in a snapshot. The host
/// recomputes the beam's full owner→hit pose every `tick_beams`
/// pass, so we just snapshot the final `Transform`-equivalent
/// (midpoint + rotation) plus the sprite's width / length — the
/// guest doesn't need the firer's pose, just the rendered quad.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeamState {
    pub net_id: NetId,
    pub pos_x: f32,
    pub pos_y: f32,
    pub rot_cos: f32,
    pub rot_sin: f32,
    /// `Sprite.custom_size.x` — beam visible width.
    pub width: f32,
    /// `Sprite.custom_size.y` — beam length (owner → hit point).
    pub length: f32,
    pub color: [f32; 4],
}

/// A render-only mirror of a host-side `Beam` entity.
#[derive(Component)]
pub struct BeamMirror;

/// A free-standing damage zone (Glory Device burst, Kohr-Ah blade
/// ring, mine field, Thraddash afterburn fireball). Round sprite,
/// pose-driven by the snapshot. The guest doesn't need radius /
/// damage / lifetime — those are host-side gameplay fields. We
/// ship sprite path because the Thraddash fireball uses a textured
/// disc instead of a flat colour.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DamageZoneState {
    pub net_id: NetId,
    pub pos_x: f32,
    pub pos_y: f32,
    /// Square side length of the sprite (`radius * 2`).
    pub size: f32,
    pub color: [f32; 4],
    /// Optional sprite asset path; empty string means flat-colour.
    pub sprite_path: String,
}

#[derive(Component)]
pub struct DamageZoneMirror;

/// An attached damage zone (Umgah cone, Zoq-Fot-Pik tongue) — like
/// a free-standing zone but the host recomputes its world pose
/// each tick from the owner's pose. We snapshot the final world
/// pose; the guest doesn't need the owner / offset relationship.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachedZoneState {
    pub net_id: NetId,
    pub pos_x: f32,
    pub pos_y: f32,
    pub size: f32,
    pub color: [f32; 4],
}

#[derive(Component)]
pub struct AttachedZoneMirror;

/// A tractor / repulsor beam visual. `tick_tractors` paints the
/// effect as a pulsing disc around the gripped target (or hides
/// the sprite if there's no target), so we just snapshot whatever
/// the host's render quad turned out to be.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TractorState {
    pub net_id: NetId,
    pub pos_x: f32,
    pub pos_y: f32,
    pub size: f32,
    pub color: [f32; 4],
    /// Optional sprite asset path (the disc uses `ui/joystick_base.png`).
    pub sprite_path: String,
}

#[derive(Component)]
pub struct TractorMirror;

/// A `SubEntity` — DOGI, Orz marine, Syreen crew pod, Kzer-Za
/// fighter. These are MOVING sprites with their own pose, so the
/// snapshot row matches `ProjState`'s shape (pose + velocity +
/// sprite descriptor). Velocity rides along so the guest can
/// dead-reckon between snapshots, same as projectiles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubEntityState {
    pub net_id: NetId,
    pub pos_x: f32,
    pub pos_y: f32,
    pub rot_cos: f32,
    pub rot_sin: f32,
    pub vel_x: f32,
    pub vel_y: f32,
    pub sprite_path: String,
    pub size: f32,
    pub color: [f32; 4],
}

#[derive(Component)]
pub struct SubEntityMirror;

/// One of the three satellites orbiting a Chmmr Avatar. Pose-only;
/// the sprite asset is fixed (`shot_b01.png`) and the size is a
/// constant, so the guest can hardcode them — we still ship the
/// size to keep the mirror code symmetric with the others (and to
/// future-proof against a multi-class "owns satellites" mechanic).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SatelliteState {
    pub net_id: NetId,
    pub pos_x: f32,
    pub pos_y: f32,
    pub size: f32,
    pub color: [f32; 4],
}

#[derive(Component)]
pub struct SatelliteMirror;

/// Fire-and-forget asteroid explosion (KABOOM animation) spawn
/// event. The guest spawns a regular `AsteroidExplosion` entity
/// from this — the local `tick_asteroid_explosions` system then
/// drives the frame animation and despawn on its own clock.
/// Re-streaming the explosion's state every snapshot would burn
/// bandwidth for no benefit: the animation is fully deterministic
/// from the spawn pose + radius.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplosionSpawn {
    pub pos_x: f32,
    pub pos_y: f32,
    pub radius: f32,
}

/// Fire-and-forget zap-flash spawn event. Same logic as
/// `ExplosionSpawn`: short-lived sprite line, local tick on the
/// guest fades it out and despawns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZapSpawn {
    /// Midpoint of the flash line in world space.
    pub pos_x: f32,
    pub pos_y: f32,
    /// Rotation Z in radians (line orientation).
    pub angle: f32,
    /// Length of the flash.
    pub length: f32,
    /// Width of the flash sprite.
    pub width: f32,
    /// Total lifetime in seconds (mirror uses this to drive its fade).
    pub total_s: f32,
    pub color: [f32; 4],
}

// ----------------------------------------------------------------
// Cinematic visual mirrors.
//
// The "ultimate" cinematics (`src/ultimate.rs`) spawn a menagerie of
// host-only visual entities — saber-trail wisps, charging halos, the
// Yehat fleet, etc. The host's snapshot stream is the guest's only
// window onto the match, so anything not mirrored here is invisible
// to the opponent. The split below follows the same pattern as the
// combat mirrors above:
//
//   - Persistent visuals (UltimateBeam, LightspeedGlow, PkunkAura,
//     MmrxfOverlaySprite, YehatFighter, MyconOrbit) get a per-tick
//     `CinematicVisualState` row keyed by NetId. The reconciler
//     updates the pose of known IDs, spawns a mirror the first time
//     a new ID appears, and despawns mirrors the host dropped — same
//     contract as `ProjectileMirror` et al.
//
//   - Fire-and-forget spawn animations (BeamTrail, BlastTrail,
//     AsteroidGhost, MmrxfLaserSegment) ride along as one-shot
//     `CinematicSpawn` events. The guest spawns a local copy with
//     the appropriate `*Trail` / `*Ghost` / `*Segment` component;
//     the SAME local tick system the host runs then animates and
//     despawns it. No per-frame reconciliation — sub-second
//     animations don't benefit from being re-streamed every tick,
//     and dropping a packet just costs the guest one trail wisp.
//
// What's *intentionally* not mirrored:
//   - The cinematic portrait, black bars, and camera close-up are
//     local to the firing player — see the long-form recommendation
//     in the task brief. The opponent keeps control of their view.
//   - The full-screen Chmmr volley flash (`ChmmrFlash`) is the
//     same — it's a UI overlay that exists for the firing player's
//     "WOW" moment, not the opponent's. (If a future pass decides
//     the opponent SHOULD see it, the flash trivially fits the
//     `CinematicSpawn` mould.)
//   - Audio stingers (`ArilouStinger`, `UltimateVoicePlayer`):
//     these are AudioPlayer entities with no visual presence, and
//     the per-class voice-line WAVs aren't network-distributable
//     metadata. The opponent hears their own combat SFX instead.
//
// Wire format is the same shape as `SubEntityState`: pose + sprite
// descriptor + size + colour. Each cinematic visual flavour gets a
// `kind` discriminator so the guest knows which marker to attach
// (the kind controls *which* host-side tick system the mirror
// becomes a target of, even though most mirrors carry no behaviour
// other than what their `Sprite` / `Mesh2d` does on its own).
// ----------------------------------------------------------------

/// Which family of cinematic visual a `CinematicVisualState` /
/// `CinematicSpawn` describes. The guest uses this to attach the
/// matching marker component so its local tick systems
/// (`tick_beam_trails`, `tick_blast_trails`, `tick_asteroid_ghosts`,
/// `tick_mmrxf_laser_segments`) drive the animation.
///
/// For persistent visuals the kind also tells the reconciler which
/// `*Mirror` marker query to look in for an existing match (the
/// queries are split per-kind because Bevy `Or<>` filter tuples
/// have a 15-entry cap and we're already close to the limit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CinematicKind {
    // -- Persistent (per-tick mirrors) --
    /// One of the three layered Arilou blade triangles. Mesh-based
    /// on the host; mirrored as a flat Sprite quad on the guest
    /// (good enough for a brief sweep — the wisps trailing behind
    /// it carry the visual weight).
    UltimateBeam,
    /// Earthling lightspeed glow halo around the firer.
    LightspeedGlow,
    /// Pkunk clone aura disc — one per clone.
    PkunkAura,
    /// Mmrnmhrm "unleashed" overlay sprite glued to the firer.
    MmrxfOverlay,
    /// Yehat fighter sprite orbiting the Terminator.
    YehatFighter,
    /// Mycon plasma orb spiraling around the Podship.
    MyconOrbit,
    // -- One-shot spawn events --
    /// One ghost copy of the Arilou blade triangle — the wisp smear.
    BeamTrail,
    /// One streak fragment behind the Earthling blasting ship.
    BlastTrail,
    /// One fading silhouette behind a Slylandro launched asteroid.
    AsteroidGhost,
    /// One short segment of the Mmrnmhrm tangled-laser bolt.
    MmrxfLaserSegment,
}

/// Pose + appearance for one cinematic visual that lives across
/// multiple snapshots. The host samples this from the live entity
/// every snapshot tick; the guest's reconciler updates the matching
/// `*Mirror` entity (spawn / update / despawn).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CinematicVisualState {
    pub net_id: NetId,
    pub kind: CinematicKind,
    pub pos_x: f32,
    pub pos_y: f32,
    pub rot_cos: f32,
    pub rot_sin: f32,
    /// Square side length for non-stretched visuals; for stretched
    /// rectangles (UltimateBeam, the YehatFighter sprite has its
    /// own aspect) the host packs width here and length in `size_y`.
    pub size_x: f32,
    pub size_y: f32,
    pub color: [f32; 4],
    /// Optional sprite asset path. Empty string → flat-colour
    /// `Sprite::from_color`. The UltimateBeam mirror ignores this
    /// (its colour-only shader doesn't need an image).
    pub sprite_path: String,
}

/// Marker on a guest-side cinematic-visual mirror. The kind it was
/// spawned for lives on it so the despawn sweep can match against
/// the snapshot's `kind`-tagged rows without an extra component per
/// flavour.
#[derive(Component, Debug, Clone, Copy)]
pub struct CinematicVisualMirror {
    pub kind: CinematicKind,
}

/// Fire-and-forget cinematic spawn event — short-lived animation
/// the guest creates a local copy of using its own clock. Carries
/// every field needed to reconstruct the host's spawn faithfully
/// (kind picks which `Component` to attach, the rest matches the
/// per-kind host-side spawn helper's arguments).
///
/// `lifetime_s` is the visual's total lifetime; the local tick
/// system divides remaining time by total to drive the fade.
/// `extra_*` carry per-flavour trail metadata (drift, width
/// growth) so BeamTrail / BlastTrail render identically to host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CinematicSpawn {
    pub kind: CinematicKind,
    pub pos_x: f32,
    pub pos_y: f32,
    /// Rotation Z in radians.
    pub angle: f32,
    /// Visual width / length. For BlastTrail and BeamTrail these
    /// drive `Transform.scale` against the shared blade mesh; for
    /// AsteroidGhost the host packs the sprite's `custom_size` here;
    /// for MmrxfLaserSegment it's the sprite's `custom_size`.
    pub width: f32,
    pub length: f32,
    pub color: [f32; 4],
    pub lifetime_s: f32,
    /// AsteroidGhost only: the asteroid's sprite path so the ghost
    /// inherits its texture. Empty for everything else.
    pub sprite_path: String,
    /// BeamTrail only: tangential drift velocity (x, y) so the wisp
    /// flings off the blade. Zero for everything else.
    pub drift_x: f32,
    pub drift_y: f32,
    /// BeamTrail only: width-growth coefficient (the wisp puffs out
    /// over time). Zero for everything else.
    pub width_growth: f32,
}

/// Host-side queue of fire-and-forget visual events accumulated
/// since the last snapshot send. Combat helpers (asteroid
/// explosion, satellite zap, fighter laser) push into this; the
/// snapshot sender drains it into the outgoing `Snapshot` message
/// and clears. Empty on the guest / in solo.
#[derive(Resource, Default, Debug)]
pub struct VisualEventQueue {
    pub explosions: Vec<ExplosionSpawn>,
    pub zaps: Vec<ZapSpawn>,
    pub cinematics: Vec<CinematicSpawn>,
}

/// Host-side staging buffer for persistent cinematic-visual rows.
/// A separate "scan" system (`scan_cinematic_visuals`) writes into
/// this every frame so the snapshot sender (`send_ship_snapshot`)
/// can pull it back without itself owning the six per-flavour
/// queries — Bevy systems are capped at ~16 params and the
/// snapshot sender is already near the limit with combat-mirror
/// queries. Cleared after each successful drain.
#[derive(Resource, Default, Debug)]
pub struct CinematicVisualBuffer {
    pub rows: Vec<CinematicVisualState>,
}

/// Which slot the local peer owns. Replaces `bevy_ggrs::LocalPlayers`
/// post-refactor. `0` is the default — it's also the slot the lone
/// peer in a solo / hotseat game is on, so the default is harmless
/// when this resource hasn't been written by the netplay handshake.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct LocalHandle(pub usize);

/// Owns the second matchbox channel (the one NOT given to GGRS) for
/// authoritative-host traffic: input forwarding, state snapshots,
/// lobby votes. Inserted by `netplay::start_p2p_session` after
/// taking ownership of `channel(1)` off the `MatchboxSocket`.
///
/// `channel` is wrapped in `Option` so the receive system can take
/// it temporarily across a mutation boundary without re-checking
/// the resource handle. `heartbeat_s` ticks the periodic ping the
/// connectivity-test system uses to verify the channel actually
/// reaches the other peer.
#[derive(Resource)]
pub struct NetSocket {
    pub channel: Option<WebRtcChannel>,
    pub heartbeat_s: f32,
    /// Remote peers we should send to. `WebRtcChannel` itself
    /// doesn't expose a connected-peer list (that's on
    /// `MatchboxSocket`), so we cache it here at handshake time.
    pub peers: Vec<PeerId>,
    /// Full sorted peer list, indexed by match slot. Slot N's peer
    /// is `slot_to_peer[N]`. Used on the host to map an incoming
    /// `NetMessage::Input` from `peer` back to the slot whose
    /// `NetInputs.current` entry the host should write into.
    pub slot_to_peer: Vec<PeerId>,
}

/// Host snapshot cadence. Driven against `Time<Real>` in the
/// Update schedule, so the actual rate is at most one snapshot per
/// frame. 16ms targets ~60Hz on the host's render loop; the guest
/// will integrate forward with the snapshotted velocity between
/// snapshots, so a faster cadence keeps velocity-derived drift
/// (e.g. an asteroid being accelerated by planet gravity on the
/// host with the guest's extrapolation using the stale, pre-tick
/// velocity) bounded to one frame's worth.
///
/// Bandwidth note: the original 60 Hz looked cheap for a 10-entity
/// snapshot (~500 B → ~30 kB/s), but during heavy combat a snapshot
/// balloons to ~3 kB (a sprite-path string in every projectile / sub-
/// entity / zone row), so 60 Hz ≈ 180 kB/s. On the *unreliable*
/// matchbox channel (channel 0) that's enough to outrun a relayed /
/// internet path: the SCTP send buffer backs up, latency climbs, and
/// after a while projectile frames get dropped (they "vanish" on the
/// guest while still dealing damage host-side) and even the tiny input
/// packets — which carry the rematch READY bit — stall behind the
/// backlog. 30 Hz halves the steady-state load while staying smooth
/// enough without client-side interpolation. Interning sprite paths
/// to a `u16` id (see NETCODE_REFACTOR.md) is the complementary
/// size-side fix if 30 Hz isn't enough.
const SNAPSHOT_INTERVAL_S: f32 = 1.0 / 30.0;

/// Cadence of the connectivity heartbeat sent before / between
/// snapshots. Once the host snapshot loop is running the heartbeat
/// is redundant; it stays around as a "did anything come through?"
/// debug aid.
const HEARTBEAT_INTERVAL_S: f32 = 1.0;

pub struct NetcodePlugin;

impl Plugin for NetcodePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetRole>()
            .init_resource::<NetIdAllocator>()
            .init_resource::<LocalHandle>()
            .init_resource::<VisualEventQueue>()
            .init_resource::<CinematicVisualBuffer>()
            .add_systems(
                FixedUpdate,
                push_local_input_to_netinputs
                    .before(crate::input::SlotInputProducerSet)
                    .run_if(resource_exists::<NetSocket>),
            )
            .add_systems(
                Update,
                (
                    send_heartbeat,
                    // Sample the live cinematic visuals before
                    // `send_ship_snapshot` drains the buffer, so a
                    // new cinematic spawn lands on the very next
                    // snapshot tick rather than one frame late.
                    scan_cinematic_visuals.run_if(role_is_authoritative),
                    send_ship_snapshot.run_if(role_is_authoritative),
                    drain_messages,
                )
                    .chain()
                    .run_if(resource_exists::<NetSocket>),
            )
            // Guest mirrors are pure `Transform`+`Sprite` (no `Position`),
            // so `starfield::apply_toroidal_render_offset` — which keys off
            // `Position` — skips them and they render at their raw snapshot
            // cell, vanishing off-screen until the player wraps around the
            // torus to that cell. Re-image them around the camera in the
            // same PostUpdate / pre-Propagate slot as the real bodies.
            .add_systems(
                PostUpdate,
                // Smooth fast movers between 30 Hz snapshots, THEN re-image
                // the result around the camera — both before transform
                // propagation, same slot as the real bodies' offset pass.
                (extrapolate_mirrors, wrap_guest_mirrors)
                    .chain()
                    .before(bevy::transform::TransformSystems::Propagate)
                    .run_if(resource_exists::<NetSocket>),
            );
    }
}

/// Dead-reckon moving mirrors (projectiles, sub-entities) by their last
/// snapshot velocity so they glide smoothly between the ~33 ms snapshot
/// steps instead of teleporting once per snapshot. Each snapshot resets
/// pose + velocity in `drain_messages`, so error never accumulates past
/// one interval; `wrap_guest_mirrors` (next in the chain) re-images the
/// advanced position around the camera.
fn extrapolate_mirrors(time: Res<Time>, mut q: Query<(&mut Transform, &MirrorVel)>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (mut xf, vel) in &mut q {
        xf.translation.x += vel.0.x * dt;
        xf.translation.y += vel.0.y * dt;
    }
}

/// PostUpdate (pre-Propagate): re-image guest mirror entities to the
/// torus copy nearest the camera, mirroring what
/// `starfield::apply_toroidal_render_offset` does for `Position`-bearing
/// bodies. `nearest_image` depends only on the position's equivalence
/// class mod the arena size, so imaging an already-imaged transform on a
/// frame with no fresh snapshot is stable.
#[allow(clippy::type_complexity)]
fn wrap_guest_mirrors(
    camera: Query<&Transform, With<Camera2d>>,
    mut mirrors: Query<
        &mut Transform,
        (
            Without<Camera2d>,
            Or<(
                With<ProjectileMirror>,
                With<BeamMirror>,
                With<DamageZoneMirror>,
                With<AttachedZoneMirror>,
                With<TractorMirror>,
                With<SubEntityMirror>,
                With<SatelliteMirror>,
                With<CinematicVisualMirror>,
                With<crate::ultimate::GuestCinematicFade>,
            )>,
        ),
    >,
) {
    let Ok(cam) = camera.single() else { return };
    let focus = cam.translation.truncate();
    for mut xf in &mut mirrors {
        let img = crate::physics::nearest_image(xf.translation.truncate(), focus);
        xf.translation.x = img.x;
        xf.translation.y = img.y;
    }
}

/// Run-condition: this peer owns the simulation and should send
/// snapshots. True in Host and Solo (Solo is a no-op since there's
/// no NetSocket, but the gate stays consistent).
pub fn role_is_authoritative(role: Res<NetRole>) -> bool {
    role.is_authoritative()
}

/// Per-FixedUpdate, online only: rotate `NetInputs.previous = current`,
/// then write THIS peer's local key state into
/// `NetInputs.current[local_slot]`. Slot 0's keymap is the canonical
/// "online" keymap (arrows + slash + period) — every peer uses it
/// regardless of which match slot they own, so an online peer in
/// slot 1 still steers with arrow keys, not WASD.
///
/// On the guest, also fire a `NetMessage::Input` to the host so the
/// host's authoritative sim sees the guest's input on its own slot.
/// Host writes that incoming input into its own `NetInputs.current`
/// from `drain_messages`.
fn push_local_input_to_netinputs(
    keys: Res<bevy::input::ButtonInput<KeyCode>>,
    virt: Res<crate::input::VirtualInput>,
    local: Res<LocalHandle>,
    role: Res<NetRole>,
    config: Res<crate::ship::MatchConfig>,
    lobby: Res<crate::lobby::LobbyVote>,
    phase: Res<crate::hud::MatchPhase>,
    mut net: ResMut<crate::input::NetInputs>,
    mut sock: ResMut<NetSocket>,
) {
    let slot = local.0.min(3);
    let mut local_input = crate::input::read_local_input_with_virtual(&keys, Some(&virt), 0);

    // Pack the local peer's lobby-vote bits into the input so the
    // remote peer sees them without a separate side channel. Both are
    // level-triggered (not edge-triggered), so it's safe to re-send
    // them every tick — receivers just read the latest value.
    //
    //   - `class`: the slot's currently-picked ship class. Sent every
    //     tick (not only during PostMatch) so a peer that joins
    //     mid-match still sees the correct ship.
    //   - `FLAG_READY`: only set during PostMatch. Outside of
    //     PostMatch the bit must read zero or `detect_all_ready` could
    //     trip from a stray value that survived state reset.
    if let Some(slot_cfg) = config.slots.get(slot) {
        local_input.class = crate::ship::class_to_index(slot_cfg.class);
    }
    if *phase == crate::hud::MatchPhase::PostMatch && lobby.ready {
        local_input.flags |= crate::input::FLAG_READY;
    }

    net.previous = net.current;
    net.current[slot] = local_input;

    // Forward this peer's input to the other side EVERY tick — both
    // directions. The host needs the guest's keypresses to drive the
    // authoritative sim (read by `gather_slot_inputs` from
    // `NetInputs.current[guest_slot]`); the guest needs the host's
    // lobby-vote bytes (class + FLAG_READY ride on `PlayerInput`) so
    // its own `detect_all_ready` can fire and trigger the rematch
    // transition. Without the host->guest direction, the guest's view
    // of `NetInputs.current[host_slot]` stays at zeros forever and
    // PostMatch is a one-way door.
    if matches!(*role, NetRole::Solo) || sock.peers.is_empty() {
        return;
    }
    let msg = NetMessage::Input {
        tick: 0,
        input: local_input,
    };
    let Ok(bytes) = bincode::serde::encode_to_vec(&msg, bincode::config::standard()) else {
        return;
    };
    let peers = sock.peers.clone();
    let Some(channel) = sock.channel.as_mut() else {
        return;
    };
    for peer in peers {
        if let Err(e) = channel.try_send(bytes.clone().into(), peer) {
            warn!("netcode: input send to {peer:?} failed: {e:?}");
        }
    }
}

fn send_heartbeat(
    time: Res<Time<Real>>,
    role: Res<NetRole>,
    mut sock: ResMut<NetSocket>,
    mut heartbeat_s: Local<f32>,
) {
    // Own accumulator — must NOT share `sock.heartbeat_s` with
    // `send_ship_snapshot`. Both run every Update (chained); if both
    // add `delta` to the same field, the snapshot accumulator advances
    // at ~2× real time and the host streams snapshots at up to twice
    // the intended `SNAPSHOT_INTERVAL_S` rate — extra load on the
    // unreliable channel that shows up as dropped projectile frames on
    // the guest under combat.
    *heartbeat_s += time.delta_secs();
    if *heartbeat_s < HEARTBEAT_INTERVAL_S {
        return;
    }
    *heartbeat_s = 0.0;
    if sock.peers.is_empty() {
        return;
    }
    // Encode an empty snapshot as the heartbeat for now. Once host
    // snapshots are wired up this gets replaced by the real
    // periodic snapshot send; for the foundation commit we just
    // verify the channel round-trips.
    let msg = NetMessage::Snapshot {
        tick: 0,
        entities: Vec::new(),
        projectiles: Vec::new(),
        beams: Vec::new(),
        damage_zones: Vec::new(),
        attached_zones: Vec::new(),
        tractors: Vec::new(),
        sub_entities: Vec::new(),
        satellites: Vec::new(),
        explosions: Vec::new(),
        zaps: Vec::new(),
        cinematic_visuals: Vec::new(),
        cinematic_spawns: Vec::new(),
    };
    let Ok(bytes) = bincode::serde::encode_to_vec(&msg, bincode::config::standard()) else {
        return;
    };
    let peers = sock.peers.clone();
    let Some(channel) = sock.channel.as_mut() else {
        return;
    };
    for peer in peers {
        // try_send instead of send: `WebRtcChannel::send` panics if
        // the channel's outbound queue is closed (which happens when
        // the WebRTC data channel hasn't fully come up yet, or when
        // the peer drops). A disconnect mid-match is a real
        // network-layer event we want to surface as a log, not a
        // crash that kills the whole window.
        if let Err(e) = channel.try_send(bytes.clone().into(), peer) {
            warn!("netcode: heartbeat send to {peer:?} failed: {e:?}");
        }
    }
    let _ = role; // role isn't acted on yet; the dispatcher uses it next session
}

/// Host-only: every `SNAPSHOT_INTERVAL_S`, gather the state of every
/// live ship + asteroid and ship it as a `NetMessage::Snapshot`.
/// Guest's `drain_messages` applies it onto its local entities.
///
/// Ships are keyed by `player_slot` (both peers spawn ships in the
/// same slot order via `spawn_match`). Asteroids are keyed by `NetId`:
/// the opening field gets 1..N deterministically on both peers (same
/// seeded RNG + allocation order), and the host hands out fresh ids
/// for rocks added by `replenish_asteroids`. Each asteroid row also
/// carries its radius + sprite frame so the guest can spawn a
/// matching mirror the first time a new id appears.
fn send_ship_snapshot(
    time: Res<Time<Real>>,
    mut sock: ResMut<NetSocket>,
    mut events: ResMut<VisualEventQueue>,
    ships: Query<
        (
            &crate::ship::Ship,
            &avian2d::prelude::Position,
            &avian2d::prelude::Rotation,
            &avian2d::prelude::LinearVelocity,
            &avian2d::prelude::AngularVelocity,
            &crate::ship::Crew,
            &crate::ship::Battery,
            Option<&crate::ship::ShieldActive>,
        ),
    >,
    asteroids: Query<(
        &NetId,
        &crate::ship::Asteroid,
        &avian2d::prelude::Position,
        &avian2d::prelude::Rotation,
        &avian2d::prelude::LinearVelocity,
        &avian2d::prelude::AngularVelocity,
    )>,
    projectiles: Query<
        (
            &NetId,
            &avian2d::prelude::Position,
            &avian2d::prelude::Rotation,
            &avian2d::prelude::LinearVelocity,
            &Sprite,
        ),
        With<crate::ship::Projectile>,
    >,
    // Visual classes — pose-and-sprite snapshots so the guest can
    // mirror them. The Avian `Position` is the authoritative pose for
    // anything with a collider; the `Transform` is the render pose
    // for sprite-only beams / tractors that have neither.
    beams: Query<(&NetId, &Transform, &Sprite), With<crate::ship::Beam>>,
    damage_zones: Query<
        (&NetId, &avian2d::prelude::Position, &Sprite),
        With<crate::ship::DamageZone>,
    >,
    attached_zones: Query<
        (&NetId, &avian2d::prelude::Position, &Sprite),
        With<crate::ship::AttachedDamageZone>,
    >,
    tractors: Query<(&NetId, &Transform, &Sprite), With<crate::ship::TractorBeam>>,
    sub_entities: Query<
        (
            &NetId,
            &avian2d::prelude::Position,
            &avian2d::prelude::Rotation,
            &avian2d::prelude::LinearVelocity,
            &Sprite,
        ),
        With<crate::ship::SubEntity>,
    >,
    satellites: Query<
        (&NetId, &avian2d::prelude::Position, &Sprite),
        With<crate::ship::ChmmrSatellite>,
    >,
    // Cinematic persistent visuals are gathered by a separate
    // `scan_cinematic_visuals` system (see below) that writes into
    // this buffer. The split keeps `send_ship_snapshot`'s param
    // count under Bevy's ~16-arg system limit — the six per-flavour
    // queries the cinematic scan owns would push us over.
    mut cinematic_buffer: ResMut<CinematicVisualBuffer>,
    mut snapshot_tick: Local<u32>,
    mut size_log_s: Local<f32>,
) {
    // `sock.heartbeat_s` is the snapshot accumulator, owned solely by
    // this system — `send_heartbeat` keeps its own `Local` so the two
    // don't double-advance it (which used to inflate the send rate to
    // ~2× `SNAPSHOT_INTERVAL_S`).
    sock.heartbeat_s += time.delta_secs();
    if sock.heartbeat_s < SNAPSHOT_INTERVAL_S {
        return;
    }
    sock.heartbeat_s = 0.0;

    let mut entities: Vec<EntityState> = ships
        .iter()
        .map(|(ship, pos, rot, lin, ang, crew, batt, shield)| EntityState {
            net_id: NetId(0),
            kind: EntityKind::Ship {
                // Guest reads the class for this slot from its own
                // `MatchConfig` (set at match start) rather than
                // from the snapshot — `Ship.stats` doesn't expose
                // the canonical `ShipClass` enum directly, just the
                // 5-char code. Class is stable for the duration of
                // a match so the snapshot byte is redundant; leave
                // it for the future "host says use class X for slot
                // Y" path when class can change mid-match.
                class_idx: 0,
                slot: ship.player_slot as u8,
            },
            pos_x: pos.0.x,
            pos_y: pos.0.y,
            rot_cos: rot.cos,
            rot_sin: rot.sin,
            vel_x: lin.0.x,
            vel_y: lin.0.y,
            ang_vel: ang.0,
            crew: crew.current,
            batt: batt.current,
            shield_factor: shield.map(|s| s.damage_factor),
        })
        .collect();

    entities.extend(asteroids.iter().map(|(net_id, ast, pos, rot, lin, ang)| {
        EntityState {
            net_id: *net_id,
            kind: EntityKind::Asteroid {
                radius: ast.radius,
                frame_idx: ast.frame_idx,
            },
            pos_x: pos.0.x,
            pos_y: pos.0.y,
            rot_cos: rot.cos,
            rot_sin: rot.sin,
            vel_x: lin.0.x,
            vel_y: lin.0.y,
            ang_vel: ang.0,
            crew: 0,
            batt: 0,
            shield_factor: None,
        }
    }));

    let projectiles: Vec<ProjState> = projectiles
        .iter()
        .map(|(net_id, pos, rot, lin, sprite)| {
            let sprite_path = sprite
                .image
                .path()
                .map(|p| p.to_string())
                .unwrap_or_default();
            let size = sprite.custom_size.map(|s| s.x).unwrap_or(0.0);
            ProjState {
                net_id: *net_id,
                pos_x: pos.0.x,
                pos_y: pos.0.y,
                rot_cos: rot.cos,
                rot_sin: rot.sin,
                vel_x: lin.0.x,
                vel_y: lin.0.y,
                sprite_path,
                size,
                color: sprite.color.to_linear().to_f32_array(),
            }
        })
        .collect();

    // Visual classes — each query maps directly to its wire state.
    // Beams + tractors use `Transform` as their pose source because
    // they have no Avian `Position` (they're sprite-only entities);
    // damage zones / attached zones / satellites do have Position
    // because their sensor collider is the gameplay hit-volume.
    let beams: Vec<BeamState> = beams
        .iter()
        .map(|(net_id, xf, sprite)| {
            let (w, l) = sprite
                .custom_size
                .map(|s| (s.x, s.y))
                .unwrap_or((0.0, 0.0));
            let (_, _, ang) = xf.rotation.to_euler(bevy::math::EulerRot::XYZ);
            BeamState {
                net_id: *net_id,
                pos_x: xf.translation.x,
                pos_y: xf.translation.y,
                rot_cos: ang.cos(),
                rot_sin: ang.sin(),
                width: w,
                length: l,
                color: sprite.color.to_linear().to_f32_array(),
            }
        })
        .collect();
    let damage_zones: Vec<DamageZoneState> = damage_zones
        .iter()
        .map(|(net_id, pos, sprite)| {
            let size = sprite.custom_size.map(|s| s.x).unwrap_or(0.0);
            let sprite_path = sprite
                .image
                .path()
                .map(|p| p.to_string())
                .unwrap_or_default();
            DamageZoneState {
                net_id: *net_id,
                pos_x: pos.0.x,
                pos_y: pos.0.y,
                size,
                color: sprite.color.to_linear().to_f32_array(),
                sprite_path,
            }
        })
        .collect();
    let attached_zones: Vec<AttachedZoneState> = attached_zones
        .iter()
        .map(|(net_id, pos, sprite)| {
            let size = sprite.custom_size.map(|s| s.x).unwrap_or(0.0);
            AttachedZoneState {
                net_id: *net_id,
                pos_x: pos.0.x,
                pos_y: pos.0.y,
                size,
                color: sprite.color.to_linear().to_f32_array(),
            }
        })
        .collect();
    let tractors: Vec<TractorState> = tractors
        .iter()
        .map(|(net_id, xf, sprite)| {
            let size = sprite.custom_size.map(|s| s.x).unwrap_or(0.0);
            let sprite_path = sprite
                .image
                .path()
                .map(|p| p.to_string())
                .unwrap_or_default();
            TractorState {
                net_id: *net_id,
                pos_x: xf.translation.x,
                pos_y: xf.translation.y,
                size,
                color: sprite.color.to_linear().to_f32_array(),
                sprite_path,
            }
        })
        .collect();
    let sub_entities: Vec<SubEntityState> = sub_entities
        .iter()
        .map(|(net_id, pos, rot, lin, sprite)| {
            let size = sprite.custom_size.map(|s| s.x).unwrap_or(0.0);
            let sprite_path = sprite
                .image
                .path()
                .map(|p| p.to_string())
                .unwrap_or_default();
            SubEntityState {
                net_id: *net_id,
                pos_x: pos.0.x,
                pos_y: pos.0.y,
                rot_cos: rot.cos,
                rot_sin: rot.sin,
                vel_x: lin.0.x,
                vel_y: lin.0.y,
                sprite_path,
                size,
                color: sprite.color.to_linear().to_f32_array(),
            }
        })
        .collect();
    let satellites: Vec<SatelliteState> = satellites
        .iter()
        .map(|(net_id, pos, sprite)| {
            let size = sprite.custom_size.map(|s| s.x).unwrap_or(0.0);
            SatelliteState {
                net_id: *net_id,
                pos_x: pos.0.x,
                pos_y: pos.0.y,
                size,
                color: sprite.color.to_linear().to_f32_array(),
            }
        })
        .collect();

    // Drain the host's fire-and-forget visual event queues. The
    // guest spawns local copies of these and ticks them with its
    // own clock — they're not in any per-snapshot reconcile loop,
    // so missing a snapshot just costs a single visual frame.
    let explosions = std::mem::take(&mut events.explosions);
    let zaps = std::mem::take(&mut events.zaps);
    let cinematic_spawns = std::mem::take(&mut events.cinematics);
    let cinematic_visuals = std::mem::take(&mut cinematic_buffer.rows);

    let tick = *snapshot_tick;
    *snapshot_tick = snapshot_tick.wrapping_add(1);
    // Counts captured before the vecs move into `msg`, for the size
    // diagnostic below.
    let (n_entities, n_proj, n_sub) = (entities.len(), projectiles.len(), sub_entities.len());
    let msg = NetMessage::Snapshot {
        tick,
        entities,
        projectiles,
        beams,
        damage_zones,
        attached_zones,
        tractors,
        sub_entities,
        satellites,
        explosions,
        zaps,
        cinematic_visuals,
        cinematic_spawns,
    };
    let Ok(bytes) = bincode::serde::encode_to_vec(&msg, bincode::config::standard()) else {
        return;
    };
    // Bug-#2 diagnostic: the snapshot rides matchbox channel 0
    // (`ChannelConfig::unreliable()`). Oversized messages get dropped
    // by the WebRTC data channel, and sending 60 Hz of multi-KB
    // payloads can saturate its send buffer — both show up on the
    // guest as projectiles that "stop appearing but still deal damage"
    // during heavy combat. ~16 KB is the conservative cross-browser
    // SCTP single-message ceiling; warn past it so a playtest tells us
    // immediately whether size is the culprit. Periodic info log keeps
    // an eye on the steady-state size without flooding the console.
    const SNAPSHOT_WARN_BYTES: usize = 16_000;
    if bytes.len() > SNAPSHOT_WARN_BYTES {
        warn!(
            "netcode: snapshot {} bytes over {SNAPSHOT_WARN_BYTES} ceiling \
             ({n_entities} entities, {n_proj} projectiles, {n_sub} sub-entities) \
             — guest may drop this frame",
            bytes.len()
        );
    }
    *size_log_s += time.delta_secs();
    if *size_log_s >= 2.0 {
        *size_log_s = 0.0;
        info!(
            "netcode: snapshot {} bytes ({n_entities} entities, {n_proj} projectiles, {n_sub} sub-entities)",
            bytes.len()
        );
    }
    let peers = sock.peers.clone();
    let Some(channel) = sock.channel.as_mut() else {
        return;
    };
    for peer in peers {
        if let Err(e) = channel.try_send(bytes.clone().into(), peer) {
            warn!("netcode: snapshot send to {peer:?} failed: {e:?}");
        }
    }
}

/// Host-only: every Update tick, sample the pose + sprite descriptor
/// for every persistent cinematic visual entity and write the rows
/// into `CinematicVisualBuffer.rows`. `send_ship_snapshot` drains
/// the buffer into the next outbound snapshot.
///
/// Lives in its own system (rather than inline in
/// `send_ship_snapshot`) so the snapshot sender can stay under
/// Bevy's ~16-system-param cap with the six per-flavour queries
/// the cinematic scan owns. Side benefit: the scan runs every
/// Update frame even between snapshot sends, so a freshly-spawned
/// visual lands in the queue right away rather than waiting for
/// the next snapshot tick to query the world.
pub fn scan_cinematic_visuals(
    mut buffer: ResMut<CinematicVisualBuffer>,
    cinematic_beams: Query<
        (&NetId, &Transform, &crate::ultimate::UltimateBeam),
    >,
    cinematic_lsg: Query<
        (&NetId, &Transform),
        With<crate::ultimate::LightspeedGlow>,
    >,
    cinematic_pkunk: Query<
        (&NetId, &Transform),
        With<crate::ultimate::PkunkAura>,
    >,
    cinematic_mmrxf: Query<
        (&NetId, &Transform, &Sprite),
        With<crate::ultimate::MmrxfOverlaySprite>,
    >,
    cinematic_yehat: Query<
        (&NetId, &Transform, &Sprite),
        With<crate::ultimate::YehatFighter>,
    >,
    cinematic_mycon: Query<
        (&NetId, &Transform, &Sprite),
        With<crate::ultimate::MyconOrbit>,
    >,
) {
    buffer.rows.clear();
    let extract_angle = |xf: &Transform| -> (f32, f32) {
        let (_, _, ang) = xf.rotation.to_euler(bevy::math::EulerRot::XYZ);
        (ang.cos(), ang.sin())
    };
    for (net_id, xf, beam) in &cinematic_beams {
        let (rc, rs) = extract_angle(xf);
        // The blade colour is per-layer and lives on the host's
        // `SoftBladeMaterial` rather than the Transform — we
        // approximate by encoding the layer index into the alpha
        // channel and reconstructing on the guest. The 0/1/2 layer
        // ordering matches `beam_layer_pose`.
        let layer = beam.layer as f32 / 8.0;
        buffer.rows.push(CinematicVisualState {
            net_id: *net_id,
            kind: CinematicKind::UltimateBeam,
            pos_x: xf.translation.x,
            pos_y: xf.translation.y,
            rot_cos: rc,
            rot_sin: rs,
            size_x: xf.scale.x,
            size_y: xf.scale.y,
            color: [1.0, 1.0, 1.0, layer],
            sprite_path: String::new(),
        });
    }
    for (net_id, xf) in &cinematic_lsg {
        let (rc, rs) = extract_angle(xf);
        buffer.rows.push(CinematicVisualState {
            net_id: *net_id,
            kind: CinematicKind::LightspeedGlow,
            pos_x: xf.translation.x,
            pos_y: xf.translation.y,
            rot_cos: rc,
            rot_sin: rs,
            size_x: xf.scale.x,
            size_y: xf.scale.y,
            color: [0.55, 0.80, 1.0, 0.85],
            sprite_path: String::new(),
        });
    }
    for (net_id, xf) in &cinematic_pkunk {
        let (rc, rs) = extract_angle(xf);
        buffer.rows.push(CinematicVisualState {
            net_id: *net_id,
            kind: CinematicKind::PkunkAura,
            pos_x: xf.translation.x,
            pos_y: xf.translation.y,
            rot_cos: rc,
            rot_sin: rs,
            size_x: xf.scale.x,
            size_y: xf.scale.y,
            color: [1.0, 0.55, 0.95, 0.65],
            sprite_path: String::new(),
        });
    }
    for (net_id, xf, sprite) in &cinematic_mmrxf {
        let (rc, rs) = extract_angle(xf);
        let size = sprite.custom_size.unwrap_or(Vec2::splat(200.0));
        let sprite_path = sprite
            .image
            .path()
            .map(|p| p.to_string())
            .unwrap_or_default();
        buffer.rows.push(CinematicVisualState {
            net_id: *net_id,
            kind: CinematicKind::MmrxfOverlay,
            pos_x: xf.translation.x,
            pos_y: xf.translation.y,
            rot_cos: rc,
            rot_sin: rs,
            size_x: size.x,
            size_y: size.y,
            color: sprite.color.to_linear().to_f32_array(),
            sprite_path,
        });
    }
    for (net_id, xf, sprite) in &cinematic_yehat {
        let (rc, rs) = extract_angle(xf);
        let size = sprite.custom_size.unwrap_or(Vec2::splat(36.0));
        let sprite_path = sprite
            .image
            .path()
            .map(|p| p.to_string())
            .unwrap_or_default();
        buffer.rows.push(CinematicVisualState {
            net_id: *net_id,
            kind: CinematicKind::YehatFighter,
            pos_x: xf.translation.x,
            pos_y: xf.translation.y,
            rot_cos: rc,
            rot_sin: rs,
            size_x: size.x,
            size_y: size.y,
            color: sprite.color.to_linear().to_f32_array(),
            sprite_path,
        });
    }
    for (net_id, xf, sprite) in &cinematic_mycon {
        let (rc, rs) = extract_angle(xf);
        let size = sprite.custom_size.unwrap_or(Vec2::splat(28.0));
        let sprite_path = sprite
            .image
            .path()
            .map(|p| p.to_string())
            .unwrap_or_default();
        buffer.rows.push(CinematicVisualState {
            net_id: *net_id,
            kind: CinematicKind::MyconOrbit,
            pos_x: xf.translation.x,
            pos_y: xf.translation.y,
            rot_cos: rc,
            rot_sin: rs,
            size_x: size.x,
            size_y: size.y,
            color: sprite.color.to_linear().to_f32_array(),
            sprite_path,
        });
    }
}

/// Cross-call state for `drain_messages`, bundled into one `Local` so
/// the system stays under Bevy's 16-param ceiling.
#[derive(Default)]
struct DrainState {
    /// Highest snapshot tick applied, for out-of-order discard.
    last_snapshot_tick: u32,
    /// Ship slots present in the previous REAL snapshot. The death
    /// sweep is edge-triggered off this (present→absent = a death) so
    /// it never despawns a freshly-spawned rematch ship just because a
    /// stale snapshot from the host's previous match phase listed fewer
    /// ships.
    last_seen_ship_slots: Vec<u8>,
}

fn drain_messages(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut sock: ResMut<NetSocket>,
    role: Res<NetRole>,
    mut net_inputs: ResMut<crate::input::NetInputs>,
    mut ships: Query<
        (
            Entity,
            &crate::ship::Ship,
            &mut avian2d::prelude::Position,
            &mut avian2d::prelude::Rotation,
            &mut avian2d::prelude::LinearVelocity,
            &mut avian2d::prelude::AngularVelocity,
            &mut crate::ship::Crew,
            &mut crate::ship::Battery,
            Option<&mut crate::ship::ShieldActive>,
        ),
        Without<crate::ship::Asteroid>,
    >,
    mut asteroids: Query<
        (
            Entity,
            &NetId,
            &mut avian2d::prelude::Position,
            &mut avian2d::prelude::Rotation,
            &mut avian2d::prelude::LinearVelocity,
            &mut avian2d::prelude::AngularVelocity,
        ),
        With<crate::ship::Asteroid>,
    >,
    // Guest-side render-only mirrors. Pure `Transform` + `Sprite`
    // (no Position/RigidBody/Collider) — the reconciler drives them
    // straight from the snapshot, so they don't conflict with the
    // Position-based ship/asteroid queries above. Each visual class
    // gets its own mirror query so the despawn sweep at the bottom
    // of the snapshot arm can drop NetIds the host dropped.
    mut proj_mirrors: Query<
        (Entity, &NetId, &mut Transform, &mut MirrorVel),
        (With<ProjectileMirror>, Without<BeamMirror>),
    >,
    mut beam_mirrors: Query<
        (Entity, &NetId, &mut Transform, &mut Sprite),
        (
            With<BeamMirror>,
            Without<ProjectileMirror>,
            Without<DamageZoneMirror>,
            Without<AttachedZoneMirror>,
            Without<TractorMirror>,
            Without<SubEntityMirror>,
            Without<SatelliteMirror>,
        ),
    >,
    mut zone_mirrors: Query<
        (Entity, &NetId, &mut Transform, &mut Sprite),
        (
            With<DamageZoneMirror>,
            Without<ProjectileMirror>,
            Without<BeamMirror>,
            Without<AttachedZoneMirror>,
            Without<TractorMirror>,
            Without<SubEntityMirror>,
            Without<SatelliteMirror>,
        ),
    >,
    mut attached_mirrors: Query<
        (Entity, &NetId, &mut Transform, &mut Sprite),
        (
            With<AttachedZoneMirror>,
            Without<ProjectileMirror>,
            Without<BeamMirror>,
            Without<DamageZoneMirror>,
            Without<TractorMirror>,
            Without<SubEntityMirror>,
            Without<SatelliteMirror>,
        ),
    >,
    mut tractor_mirrors: Query<
        (Entity, &NetId, &mut Transform, &mut Sprite),
        (
            With<TractorMirror>,
            Without<ProjectileMirror>,
            Without<BeamMirror>,
            Without<DamageZoneMirror>,
            Without<AttachedZoneMirror>,
            Without<SubEntityMirror>,
            Without<SatelliteMirror>,
        ),
    >,
    mut sub_mirrors: Query<
        (Entity, &NetId, &mut Transform, &mut Sprite, &mut MirrorVel),
        (
            With<SubEntityMirror>,
            Without<ProjectileMirror>,
            Without<BeamMirror>,
            Without<DamageZoneMirror>,
            Without<AttachedZoneMirror>,
            Without<TractorMirror>,
            Without<SatelliteMirror>,
        ),
    >,
    mut sat_mirrors: Query<
        (Entity, &NetId, &mut Transform, &mut Sprite),
        (
            With<SatelliteMirror>,
            Without<ProjectileMirror>,
            Without<BeamMirror>,
            Without<DamageZoneMirror>,
            Without<AttachedZoneMirror>,
            Without<TractorMirror>,
            Without<SubEntityMirror>,
        ),
    >,
    // Cinematic-visual mirrors — one query for the whole bag,
    // disambiguated by the `kind` field on the marker. We can't
    // attach the cinematic-specific markers from `src/ultimate.rs`
    // here (would create a dep cycle), so the reconciler uses
    // `CinematicVisualMirror.kind` to route. Same Without<> dance
    // as the other mirror queries to keep them disjoint.
    mut cine_mirrors: Query<
        (Entity, &NetId, &mut Transform, &mut Sprite, &CinematicVisualMirror),
        (
            Without<ProjectileMirror>,
            Without<BeamMirror>,
            Without<DamageZoneMirror>,
            Without<AttachedZoneMirror>,
            Without<TractorMirror>,
            Without<SubEntityMirror>,
            Without<SatelliteMirror>,
        ),
    >,
    mut state: Local<DrainState>,
) {
    // Snapshot the slot lookup before taking the channel mut-borrow so
    // we don't fight the borrow checker mid-loop.
    let slot_to_peer = sock.slot_to_peer.clone();
    // NetIds the guest has spawned a mirror for during THIS drain pass.
    // `commands.spawn` is deferred, so a freshly-spawned asteroid won't
    // show up in the `asteroids` query until next frame — without this,
    // two snapshots in one frame both listing a new NetId would spawn
    // it twice.
    let mut spawned_this_drain: Vec<NetId> = Vec::new();
    let Some(channel) = sock.channel.as_mut() else {
        return;
    };
    for (peer, bytes) in channel.receive() {
        let Ok((msg, _)) =
            bincode::serde::decode_from_slice::<NetMessage, _>(&bytes, bincode::config::standard())
        else {
            warn!("netcode: decode failed from {peer:?} ({} bytes)", bytes.len());
            continue;
        };
        match msg {
            NetMessage::Input { tick: _, input } => {
                // BOTH peers process inbound inputs now, not just the
                // host. The host needs the guest's input to drive the
                // authoritative sim; the guest needs the HOST's input
                // bytes so its local `NetInputs.current[host_slot]`
                // carries the host's class pick + `FLAG_READY` bit,
                // which `detect_all_ready` reads off `SlotInputs.held`
                // to fire the PostMatch -> Resetting transition on the
                // guest's side too. Without this, the guest can never
                // see the host be ready and stays stuck in PostMatch
                // forever while the host rematches alone.
                //
                // On the guest, the host's `input.buttons` / `turn` /
                // etc. land in `NetInputs.current[host_slot]` but are
                // overwritten by every snapshot's per-ship pose/velocity
                // — they only meaningfully drive the host's
                // class+ready bits the guest's lobby reconciler reads.
                let Some(slot) = slot_to_peer.iter().position(|&p| p == peer) else {
                    continue;
                };
                if slot < net_inputs.current.len() {
                    net_inputs.current[slot] = input;
                }
            }
            NetMessage::Snapshot {
                tick,
                entities,
                projectiles,
                beams,
                damage_zones,
                attached_zones,
                tractors,
                sub_entities,
                satellites,
                explosions,
                zaps,
                cinematic_visuals,
                cinematic_spawns,
            } => {
                // Guest applies snapshots onto its local ships;
                // host ignores them (it IS the authority).
                if !role.is_guest() {
                    continue;
                }
                // Discard out-of-order snapshots. matchbox
                // unreliable doesn't guarantee delivery order, but
                // we want the latest authority state — older ticks
                // would overwrite with stale poses.
                if tick != 0 && tick < state.last_snapshot_tick {
                    continue;
                }
                state.last_snapshot_tick = tick;

                // A real world snapshot always carries the ships. An
                // empty / shipless one is a connectivity heartbeat —
                // it must NOT drive asteroid reconciliation, or a
                // stray ping would despawn the entire field.
                let has_ships = entities
                    .iter()
                    .any(|e| matches!(e.kind, EntityKind::Ship { .. }));

                let mut seen_asteroids: Vec<NetId> = Vec::with_capacity(entities.len());
                for state in &entities {
                    match state.kind {
                        EntityKind::Ship { slot, .. } => {
                            for (
                                ship_entity,
                                ship,
                                mut pos,
                                mut rot,
                                mut lin,
                                mut ang,
                                mut crew,
                                mut batt,
                                shield,
                            ) in &mut ships
                            {
                                if ship.player_slot as u8 != slot {
                                    continue;
                                }
                                pos.0.x = state.pos_x;
                                pos.0.y = state.pos_y;
                                rot.cos = state.rot_cos;
                                rot.sin = state.rot_sin;
                                lin.0.x = state.vel_x;
                                lin.0.y = state.vel_y;
                                ang.0 = state.ang_vel;
                                crew.current = state.crew;
                                batt.current = state.batt;
                                // Mirror the host's shield presence
                                // onto the guest's ship. `draw_shield_rings`
                                // (in Update on both peers) renders an
                                // outline iff `ShieldActive` is present,
                                // so the toggle here is what makes
                                // opponent shields actually appear /
                                // disappear during their abilities. We
                                // copy `damage_factor` even when a local
                                // `ShieldActive` already exists (e.g.
                                // permanent Alabc shield) — same value,
                                // so the insert is idempotent.
                                match (state.shield_factor, shield) {
                                    (Some(factor), Some(mut s)) => {
                                        s.damage_factor = factor;
                                        s.remaining = f32::INFINITY;
                                    }
                                    (Some(factor), None) => {
                                        if let Ok(mut ec) =
                                            commands.get_entity(ship_entity)
                                        {
                                            ec.try_insert(crate::ship::ShieldActive {
                                                remaining: f32::INFINITY,
                                                damage_factor: factor,
                                            });
                                        }
                                    }
                                    (None, Some(_)) => {
                                        if let Ok(mut ec) =
                                            commands.get_entity(ship_entity)
                                        {
                                            ec.try_remove::<crate::ship::ShieldActive>();
                                        }
                                    }
                                    (None, None) => {}
                                }
                                break;
                            }
                        }
                        EntityKind::Asteroid { radius, frame_idx } => {
                            seen_asteroids.push(state.net_id);
                            let mut hit = false;
                            for (_e, net_id, mut pos, mut rot, mut lin, mut ang) in &mut asteroids
                            {
                                if *net_id != state.net_id {
                                    continue;
                                }
                                pos.0.x = state.pos_x;
                                pos.0.y = state.pos_y;
                                rot.cos = state.rot_cos;
                                rot.sin = state.rot_sin;
                                lin.0.x = state.vel_x;
                                lin.0.y = state.vel_y;
                                ang.0 = state.ang_vel;
                                hit = true;
                                break;
                            }
                            // A NetId in the freshest snapshot with no
                            // local match is a rock the host added mid-
                            // match (`replenish_asteroids`). Spawn a
                            // Static mirror at the snapshot pose; future
                            // snapshots keep it in sync like the opening
                            // field. `spawned_this_drain` guards against
                            // a second snapshot in the same frame double-
                            // spawning before the deferred spawn applies.
                            if !hit && !spawned_this_drain.contains(&state.net_id) {
                                spawned_this_drain.push(state.net_id);
                                crate::ship::spawn_one_asteroid(
                                    &mut commands,
                                    &assets,
                                    crate::ship::AsteroidSpawn {
                                        pos: Vec2::new(state.pos_x, state.pos_y),
                                        vel: Vec2::new(state.vel_x, state.vel_y),
                                        radius,
                                        frame_idx,
                                        // Mass is irrelevant for a Static
                                        // body (the guest never integrates
                                        // it), but keep it sane.
                                        mass: 5.0,
                                        ang_vel: state.ang_vel,
                                        net_id: state.net_id.0,
                                        as_static: true,
                                    },
                                );
                            }
                        }
                        EntityKind::Projectile
                        | EntityKind::Planet
                        | EntityKind::SubEntity => {
                            // Not synced yet — see NETCODE_REFACTOR.md.
                        }
                    }
                }

                // Reconcile deletions: any local asteroid the host
                // dropped from a real snapshot was destroyed (planet
                // contact, laser, ram) — follow suit so the guest
                // doesn't accumulate ghosts. Skipped for heartbeats.
                if has_ships {
                    for (e, net_id, _, _, _, _) in &asteroids {
                        if !seen_asteroids.contains(net_id) {
                            if let Ok(mut ec) = commands.get_entity(e) {
                                ec.try_despawn();
                            }
                        }
                    }
                }

                // Reconcile ship deaths. Ships are keyed by `player_slot`,
                // not NetId, so the asteroid sweep above doesn't cover
                // them. When a ship dies on the host (crew hits 0), the
                // host despawns it *the same FixedUpdate tick* via
                // `destroy_zero_crew_ships` — so the crew=0 value never
                // makes it into a snapshot, and the ship's row simply
                // stops appearing. Without handling this the guest keeps
                // a ghost ship at its last-synced crew (the "died on one
                // screen, alive on the other" bug).
                //
                // EDGE-TRIGGERED, not level: only despawn a ship whose
                // slot was present in the PREVIOUS real snapshot and is
                // absent now (a genuine death). A purely "absent now"
                // test would also kill freshly-spawned rematch ships when
                // a stale snapshot from the host's previous match phase
                // (fewer ships) lands in the window after the guest has
                // already respawned — which manifested as the guest stuck
                // a match behind. Skipped for heartbeats (no ships).
                if has_ships {
                    let seen_ship_slots: Vec<u8> = entities
                        .iter()
                        .filter_map(|e| match e.kind {
                            EntityKind::Ship { slot, .. } => Some(slot),
                            _ => None,
                        })
                        .collect();
                    for (e, ship, ..) in &ships {
                        let slot = ship.player_slot as u8;
                        if state.last_seen_ship_slots.contains(&slot)
                            && !seen_ship_slots.contains(&slot)
                        {
                            if let Ok(mut ec) = commands.get_entity(e) {
                                ec.try_despawn();
                            }
                        }
                    }
                    state.last_seen_ship_slots = seen_ship_slots;
                }

                // --- Projectile mirrors ---
                // Same reconcile shape as asteroids: update the pose of
                // any mirror we already have, spawn a render-only mirror
                // for a NetId we don't, and (on real snapshots) despawn
                // mirrors the host dropped — a projectile that hit or
                // expired vanishes on the host's authority.
                for p in &projectiles {
                    let angle = p.rot_sin.atan2(p.rot_cos);
                    let mut hit = false;
                    for (_e, net_id, mut xf, mut vel) in &mut proj_mirrors {
                        if *net_id != p.net_id {
                            continue;
                        }
                        xf.translation.x = p.pos_x;
                        xf.translation.y = p.pos_y;
                        xf.rotation = Quat::from_rotation_z(angle);
                        vel.0 = Vec2::new(p.vel_x, p.vel_y);
                        hit = true;
                        break;
                    }
                    if !hit && !spawned_this_drain.contains(&p.net_id) {
                        spawned_this_drain.push(p.net_id);
                        let custom_size = (p.size > 0.0).then(|| Vec2::splat(p.size));
                        commands.spawn((
                            ProjectileMirror,
                            MirrorVel(Vec2::new(p.vel_x, p.vel_y)),
                            p.net_id,
                            Sprite {
                                image: assets.load(p.sprite_path.clone()),
                                color: Color::linear_rgba(
                                    p.color[0], p.color[1], p.color[2], p.color[3],
                                ),
                                custom_size,
                                ..default()
                            },
                            Transform::from_translation(Vec3::new(p.pos_x, p.pos_y, 0.5))
                                .with_rotation(Quat::from_rotation_z(angle)),
                        ));
                    }
                }
                if has_ships {
                    for (e, net_id, _, _) in &proj_mirrors {
                        if !projectiles.iter().any(|p| p.net_id == *net_id) {
                            if let Ok(mut ec) = commands.get_entity(e) {
                                ec.try_despawn();
                            }
                        }
                    }
                }

                // --- Beam mirrors ---
                // The host has already collapsed the beam to its final
                // owner→hit render quad; we just lift pose + size +
                // colour onto a sprite. No owner relationship is
                // mirrored — the guest doesn't need one, the beam pose
                // is fully captured by the snapshot.
                for b in &beams {
                    let angle = b.rot_sin.atan2(b.rot_cos);
                    let mut hit = false;
                    for (_e, net_id, mut xf, mut sprite) in &mut beam_mirrors {
                        if *net_id != b.net_id {
                            continue;
                        }
                        xf.translation.x = b.pos_x;
                        xf.translation.y = b.pos_y;
                        xf.translation.z = 0.3;
                        xf.rotation = Quat::from_rotation_z(angle);
                        sprite.custom_size = Some(Vec2::new(b.width, b.length));
                        sprite.color = Color::linear_rgba(
                            b.color[0], b.color[1], b.color[2], b.color[3],
                        );
                        hit = true;
                        break;
                    }
                    if !hit && !spawned_this_drain.contains(&b.net_id) {
                        spawned_this_drain.push(b.net_id);
                        commands.spawn((
                            BeamMirror,
                            b.net_id,
                            Sprite::from_color(
                                Color::linear_rgba(
                                    b.color[0], b.color[1], b.color[2], b.color[3],
                                ),
                                Vec2::new(b.width, b.length),
                            ),
                            Transform::from_translation(Vec3::new(b.pos_x, b.pos_y, 0.3))
                                .with_rotation(Quat::from_rotation_z(angle)),
                        ));
                    }
                }
                if has_ships {
                    for (e, net_id, _, _) in &beam_mirrors {
                        if !beams.iter().any(|b| b.net_id == *net_id) {
                            if let Ok(mut ec) = commands.get_entity(e) {
                                ec.try_despawn();
                            }
                        }
                    }
                }

                // --- DamageZone mirrors ---
                // Damage zones are static colourful discs (Glory blast,
                // Thraddash fireball, mine field). Sprite path may be
                // empty (flat-colour `Sprite::from_color`); load the
                // image only when one's provided.
                for z in &damage_zones {
                    let mut hit = false;
                    for (_e, net_id, mut xf, mut sprite) in &mut zone_mirrors {
                        if *net_id != z.net_id {
                            continue;
                        }
                        xf.translation.x = z.pos_x;
                        xf.translation.y = z.pos_y;
                        xf.translation.z = 0.2;
                        sprite.custom_size = Some(Vec2::splat(z.size));
                        sprite.color = Color::linear_rgba(
                            z.color[0], z.color[1], z.color[2], z.color[3],
                        );
                        hit = true;
                        break;
                    }
                    if !hit && !spawned_this_drain.contains(&z.net_id) {
                        spawned_this_drain.push(z.net_id);
                        let color = Color::linear_rgba(
                            z.color[0], z.color[1], z.color[2], z.color[3],
                        );
                        let sprite = if z.sprite_path.is_empty() {
                            Sprite::from_color(color, Vec2::splat(z.size))
                        } else {
                            Sprite {
                                image: assets.load(z.sprite_path.clone()),
                                color,
                                custom_size: Some(Vec2::splat(z.size)),
                                ..default()
                            }
                        };
                        commands.spawn((
                            DamageZoneMirror,
                            z.net_id,
                            sprite,
                            Transform::from_translation(Vec3::new(z.pos_x, z.pos_y, 0.2)),
                        ));
                    }
                }
                if has_ships {
                    for (e, net_id, _, _) in &zone_mirrors {
                        if !damage_zones.iter().any(|z| z.net_id == *net_id) {
                            if let Ok(mut ec) = commands.get_entity(e) {
                                ec.try_despawn();
                            }
                        }
                    }
                }

                // --- AttachedDamageZone mirrors ---
                // Same shape as DamageZone, separate mirror type so the
                // despawn sweep can treat them independently (an Umgah
                // cone has different lifecycle from a Shofixti blast).
                for z in &attached_zones {
                    let mut hit = false;
                    for (_e, net_id, mut xf, mut sprite) in &mut attached_mirrors {
                        if *net_id != z.net_id {
                            continue;
                        }
                        xf.translation.x = z.pos_x;
                        xf.translation.y = z.pos_y;
                        xf.translation.z = 0.2;
                        sprite.custom_size = Some(Vec2::splat(z.size));
                        sprite.color = Color::linear_rgba(
                            z.color[0], z.color[1], z.color[2], z.color[3],
                        );
                        hit = true;
                        break;
                    }
                    if !hit && !spawned_this_drain.contains(&z.net_id) {
                        spawned_this_drain.push(z.net_id);
                        let color = Color::linear_rgba(
                            z.color[0], z.color[1], z.color[2], z.color[3],
                        );
                        commands.spawn((
                            AttachedZoneMirror,
                            z.net_id,
                            Sprite::from_color(color, Vec2::splat(z.size)),
                            Transform::from_translation(Vec3::new(z.pos_x, z.pos_y, 0.2)),
                        ));
                    }
                }
                if has_ships {
                    for (e, net_id, _, _) in &attached_mirrors {
                        if !attached_zones.iter().any(|z| z.net_id == *net_id) {
                            if let Ok(mut ec) = commands.get_entity(e) {
                                ec.try_despawn();
                            }
                        }
                    }
                }

                // --- Tractor mirrors ---
                // `tick_tractors` may hide the sprite (custom_size =
                // ZERO) when no target is in range; we mirror that
                // verbatim by carrying the host's `custom_size` through.
                for t in &tractors {
                    let mut hit = false;
                    for (_e, net_id, mut xf, mut sprite) in &mut tractor_mirrors {
                        if *net_id != t.net_id {
                            continue;
                        }
                        xf.translation.x = t.pos_x;
                        xf.translation.y = t.pos_y;
                        xf.translation.z = 0.3;
                        sprite.custom_size = Some(Vec2::splat(t.size));
                        sprite.color = Color::linear_rgba(
                            t.color[0], t.color[1], t.color[2], t.color[3],
                        );
                        if !t.sprite_path.is_empty() {
                            sprite.image = assets.load(t.sprite_path.clone());
                        }
                        hit = true;
                        break;
                    }
                    if !hit && !spawned_this_drain.contains(&t.net_id) {
                        spawned_this_drain.push(t.net_id);
                        let color = Color::linear_rgba(
                            t.color[0], t.color[1], t.color[2], t.color[3],
                        );
                        let sprite = if t.sprite_path.is_empty() {
                            Sprite {
                                color,
                                custom_size: Some(Vec2::splat(t.size)),
                                ..default()
                            }
                        } else {
                            Sprite {
                                image: assets.load(t.sprite_path.clone()),
                                color,
                                custom_size: Some(Vec2::splat(t.size)),
                                ..default()
                            }
                        };
                        commands.spawn((
                            TractorMirror,
                            t.net_id,
                            sprite,
                            Transform::from_translation(Vec3::new(t.pos_x, t.pos_y, 0.3)),
                        ));
                    }
                }
                if has_ships {
                    for (e, net_id, _, _) in &tractor_mirrors {
                        if !tractors.iter().any(|t| t.net_id == *net_id) {
                            if let Ok(mut ec) = commands.get_entity(e) {
                                ec.try_despawn();
                            }
                        }
                    }
                }

                // --- SubEntity mirrors ---
                // Moving sprites (DOGI, marines, crew pods, fighters).
                // Same shape as projectiles: pose + sprite descriptor,
                // velocity is in the wire format for future dead-
                // reckoning between snapshots (we don't extrapolate
                // yet, but the data's there so the bandwidth cost is
                // sunk).
                for s in &sub_entities {
                    let angle = s.rot_sin.atan2(s.rot_cos);
                    let mut hit = false;
                    for (_e, net_id, mut xf, mut sprite, mut vel) in &mut sub_mirrors {
                        if *net_id != s.net_id {
                            continue;
                        }
                        xf.translation.x = s.pos_x;
                        xf.translation.y = s.pos_y;
                        xf.translation.z = 0.4;
                        xf.rotation = Quat::from_rotation_z(angle);
                        vel.0 = Vec2::new(s.vel_x, s.vel_y);
                        sprite.custom_size = Some(Vec2::splat(s.size));
                        sprite.color = Color::linear_rgba(
                            s.color[0], s.color[1], s.color[2], s.color[3],
                        );
                        if !s.sprite_path.is_empty() {
                            sprite.image = assets.load(s.sprite_path.clone());
                        }
                        hit = true;
                        break;
                    }
                    if !hit && !spawned_this_drain.contains(&s.net_id) {
                        spawned_this_drain.push(s.net_id);
                        let color = Color::linear_rgba(
                            s.color[0], s.color[1], s.color[2], s.color[3],
                        );
                        let sprite = if s.sprite_path.is_empty() {
                            Sprite::from_color(color, Vec2::splat(s.size))
                        } else {
                            Sprite {
                                image: assets.load(s.sprite_path.clone()),
                                color,
                                custom_size: Some(Vec2::splat(s.size)),
                                ..default()
                            }
                        };
                        commands.spawn((
                            SubEntityMirror,
                            MirrorVel(Vec2::new(s.vel_x, s.vel_y)),
                            s.net_id,
                            sprite,
                            Transform::from_translation(Vec3::new(s.pos_x, s.pos_y, 0.4))
                                .with_rotation(Quat::from_rotation_z(angle)),
                        ));
                    }
                }
                if has_ships {
                    for (e, net_id, _, _, _) in &sub_mirrors {
                        if !sub_entities.iter().any(|s| s.net_id == *net_id) {
                            if let Ok(mut ec) = commands.get_entity(e) {
                                ec.try_despawn();
                            }
                        }
                    }
                }

                // --- ChmmrSatellite mirrors ---
                // The satellite art is a fixed `shot_b01.png` frame;
                // we still ship size + colour in case future ships
                // get satellites with different visuals.
                for s in &satellites {
                    let mut hit = false;
                    for (_e, net_id, mut xf, mut sprite) in &mut sat_mirrors {
                        if *net_id != s.net_id {
                            continue;
                        }
                        xf.translation.x = s.pos_x;
                        xf.translation.y = s.pos_y;
                        xf.translation.z = 0.25;
                        sprite.custom_size = Some(Vec2::splat(s.size));
                        sprite.color = Color::linear_rgba(
                            s.color[0], s.color[1], s.color[2], s.color[3],
                        );
                        hit = true;
                        break;
                    }
                    if !hit && !spawned_this_drain.contains(&s.net_id) {
                        spawned_this_drain.push(s.net_id);
                        let color = Color::linear_rgba(
                            s.color[0], s.color[1], s.color[2], s.color[3],
                        );
                        commands.spawn((
                            SatelliteMirror,
                            s.net_id,
                            Sprite {
                                image: assets.load("ships/chmav/sprites/shot_b01.png"),
                                color,
                                custom_size: Some(Vec2::splat(s.size)),
                                ..default()
                            },
                            Transform::from_translation(Vec3::new(s.pos_x, s.pos_y, 0.25)),
                        ));
                    }
                }
                if has_ships {
                    for (e, net_id, _, _) in &sat_mirrors {
                        if !satellites.iter().any(|s| s.net_id == *net_id) {
                            if let Ok(mut ec) = commands.get_entity(e) {
                                ec.try_despawn();
                            }
                        }
                    }
                }

                // --- Cinematic persistent visuals ---
                // Same shape as the other persistent mirrors: pose-
                // update a known NetId, spawn a `CinematicVisualMirror`
                // the first time it appears, despawn missing ones on
                // real snapshots. Spawn shape is per-kind because
                // the visuals have different sprite paths / colour
                // sources / sizes; the `kind` discriminator on the
                // mirror entity is what the despawn sweep matches
                // against.
                for v in &cinematic_visuals {
                    let angle = v.rot_sin.atan2(v.rot_cos);
                    let mut hit = false;
                    for (_e, net_id, mut xf, mut sprite, _marker) in &mut cine_mirrors {
                        if *net_id != v.net_id {
                            continue;
                        }
                        xf.translation.x = v.pos_x;
                        xf.translation.y = v.pos_y;
                        // Z-stack so trails sit under the firer's
                        // ship sprite but above the planet etc.
                        xf.translation.z = match v.kind {
                            CinematicKind::UltimateBeam => 0.35,
                            CinematicKind::LightspeedGlow => 0.10,
                            CinematicKind::PkunkAura => 0.10,
                            CinematicKind::MmrxfOverlay => 0.45,
                            CinematicKind::YehatFighter => 0.45,
                            CinematicKind::MyconOrbit => 0.40,
                            _ => 0.30,
                        };
                        xf.rotation = Quat::from_rotation_z(angle);
                        sprite.custom_size = Some(Vec2::new(v.size_x, v.size_y));
                        sprite.color = Color::linear_rgba(
                            v.color[0], v.color[1], v.color[2], v.color[3],
                        );
                        if !v.sprite_path.is_empty() {
                            sprite.image = assets.load(v.sprite_path.clone());
                        }
                        hit = true;
                        break;
                    }
                    if !hit && !spawned_this_drain.contains(&v.net_id) {
                        spawned_this_drain.push(v.net_id);
                        spawn_cinematic_visual_mirror(&mut commands, &assets, v, angle);
                    }
                }
                if has_ships {
                    for (e, net_id, _, _, _) in &cine_mirrors {
                        if !cinematic_visuals.iter().any(|v| v.net_id == *net_id) {
                            if let Ok(mut ec) = commands.get_entity(e) {
                                ec.try_despawn();
                            }
                        }
                    }
                }

                // --- Cinematic fire-and-forget spawns ---
                // Trails, ghosts, lightning segments. The guest
                // spawns a local copy carrying the appropriate
                // `*Trail` / `*Ghost` / `*Segment` component so its
                // own (unconditional) Update tick handles the fade
                // and despawn. We can't attach the cinematic-side
                // marker types here without a dep cycle, so we
                // call a helper in `src/ultimate.rs` to do it.
                for sp in &cinematic_spawns {
                    crate::ultimate::spawn_guest_cinematic(&mut commands, &assets, sp);
                }

                // --- Fire-and-forget visual events ---
                // Spawn one local copy per event; the corresponding
                // local tick system (`tick_asteroid_explosions`,
                // `tick_zap_flashes`) drives the animation and
                // despawn. These bypass the per-snapshot reconcile
                // because they're sub-half-second visuals where a
                // dropped packet is invisible (the host already
                // moved on by the time the next snapshot lands).
                for ev in &explosions {
                    crate::ship::spawn_asteroid_explosion(
                        &mut commands,
                        &assets,
                        Vec2::new(ev.pos_x, ev.pos_y),
                        ev.radius,
                    );
                }
                for ev in &zaps {
                    let color = Color::linear_rgba(
                        ev.color[0], ev.color[1], ev.color[2], ev.color[3],
                    );
                    commands.spawn((
                        crate::ship::ZapFlash {
                            remaining_s: ev.total_s,
                            total_s: ev.total_s,
                        },
                        Sprite::from_color(color, Vec2::new(ev.width, ev.length)),
                        Transform {
                            translation: Vec3::new(ev.pos_x, ev.pos_y, 0.30),
                            rotation: Quat::from_rotation_z(ev.angle),
                            scale: Vec3::ONE,
                        },
                    ));
                }
            }
        }
    }
}

/// Spawn a guest-side `CinematicVisualMirror` for a freshly-seen
/// persistent cinematic visual. Pulled out of `drain_messages` so
/// the per-kind branching is local — kept here (rather than in
/// `src/ultimate.rs`) because all this needs is the cinematic
/// mirror marker + a `Sprite` + a `Transform`. The host-side mesh
/// pipeline (Mesh2d + SoftBladeMaterial / GlowMaterial) is
/// deliberately swapped for a flat Sprite on the guest:
///
///   - Spinning up Material2d on the guest would mean linking the
///     shader assets into the asset graph from the guest's spawn
///     path AND keeping `Assets<SoftBladeMaterial>` accessible
///     from the netcode plugin (which doesn't depend on `bevy_render`'s
///     mesh types). Sprite-only mirrors are cheaper and read just
///     as well for a brief sweep.
///   - Per-frame snapshot rate keeps the pose tracking the host
///     within a tick, so the loss of the radial-fade shader on
///     the LightspeedGlow / PkunkAura halos amounts to a slightly
///     harder-edged disc — visible from across the screen, which
///     is the actual job.
fn spawn_cinematic_visual_mirror(
    commands: &mut Commands,
    assets: &AssetServer,
    v: &CinematicVisualState,
    angle: f32,
) {
    let z = match v.kind {
        CinematicKind::UltimateBeam => 0.35,
        CinematicKind::LightspeedGlow => 0.10,
        CinematicKind::PkunkAura => 0.10,
        CinematicKind::MmrxfOverlay => 0.45,
        CinematicKind::YehatFighter => 0.45,
        CinematicKind::MyconOrbit => 0.40,
        _ => 0.30,
    };
    let color = Color::linear_rgba(v.color[0], v.color[1], v.color[2], v.color[3]);
    let custom_size = Vec2::new(v.size_x, v.size_y);
    let sprite = if v.sprite_path.is_empty() {
        Sprite::from_color(color, custom_size)
    } else {
        Sprite {
            image: assets.load(v.sprite_path.clone()),
            color,
            custom_size: Some(custom_size),
            ..default()
        }
    };
    commands.spawn((
        CinematicVisualMirror { kind: v.kind },
        v.net_id,
        sprite,
        Transform::from_translation(Vec3::new(v.pos_x, v.pos_y, z))
            .with_rotation(Quat::from_rotation_z(angle)),
    ));
}
