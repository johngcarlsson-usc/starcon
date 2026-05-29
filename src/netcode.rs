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
    Snapshot {
        tick: u32,
        entities: Vec<EntityState>,
    },
    /// Host → Guest. Lobby state echo so the guest can render the
    /// per-slot READY / class status during PostMatch.
    Lobby { slots: Vec<LobbySlot> },
    /// Guest → Host. Lobby vote update. Sent only when the vote
    /// changes — the reliable channel guarantees delivery so we
    /// don't need to keep re-sending.
    LobbyVote {
        class: u8,
        ready: bool,
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
}

/// Which family of game object an `EntityState` is describing.
/// Drives guest-side spawn shape (sprite / collider / components)
/// when a new `NetId` appears in a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityKind {
    Ship {
        /// Index into `ship::ALL_CLASSES`.
        class_idx: u8,
        slot: u8,
    },
    Projectile,
    Asteroid,
    Planet,
    SubEntity,
}

/// Per-slot lobby state echoed by the host so the guest's
/// `update_status_banner` sees the same READY / class for every
/// slot the host computed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbySlot {
    pub slot: u8,
    pub class: u8,
    pub ready: bool,
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

/// Host snapshot cadence. 20 Hz matches the canonical SC2 frame
/// rate the rest of our gameplay constants are calibrated against.
/// Bandwidth budget per snapshot is tiny — a 4-ship match comes out
/// to ~200 bytes encoded, so 20 Hz × 200 B = 4 kB/s, fine even on a
/// slow connection.
const SNAPSHOT_INTERVAL_S: f32 = 0.05;

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
            // Local-input → NetInputs writer. Runs in FixedUpdate
            // BEFORE `gather_slot_inputs` (the consumer side) so
            // each tick's local key state lands in `NetInputs` in
            // time for the gameplay pipeline to read it the same
            // tick. Runs only in online mode — local hotseat takes
            // a different path through `gather_slot_inputs`.
            .add_systems(
                FixedUpdate,
                push_local_input_to_netinputs
                    .before(crate::input::SlotInputProducerSet)
                    .run_if(resource_exists::<NetSocket>),
            )
            // Heartbeat + receive loop. Both gate on the
            // `NetSocket` resource existing, which only happens
            // after the matchbox handshake — so they're no-ops
            // in solo / local hotseat.
            .add_systems(
                Update,
                (
                    send_heartbeat,
                    send_ship_snapshot.run_if(role_is_authoritative),
                    drain_messages,
                )
                    .chain()
                    .run_if(resource_exists::<NetSocket>),
            );
    }
}

/// Run-condition: this peer owns the simulation and should send
/// snapshots. True in Host and Solo (Solo is a no-op since there's
/// no NetSocket, but the gate stays consistent).
pub fn role_is_authoritative(role: Res<NetRole>) -> bool {
    role.is_authoritative()
}

/// True iff we're the guest in netplay — used to gate snapshot
/// application.
pub fn role_is_guest(role: Res<NetRole>) -> bool {
    role.is_guest()
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

    // Guest forwards its own input to the host on every tick.
    // Cheap (a handful of bytes) and gives the host a fresh value
    // even when the guest is idle — host's `gather_slot_inputs`
    // reads `NetInputs.current[guest_slot]` directly.
    if !role.is_guest() {
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
) {
    sock.heartbeat_s += time.delta_secs();
    if sock.heartbeat_s < HEARTBEAT_INTERVAL_S {
        return;
    }
    sock.heartbeat_s = 0.0;
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
/// live ship and ship it as a `NetMessage::Snapshot`. Guest's
/// `drain_messages` will apply this onto its local entities.
///
/// Ships are keyed by `player_slot` in the snapshot (we put the slot
/// index in the low byte of `EntityKind::Ship`). Both peers spawn
/// ships in the same slot order via `spawn_match`, so the guest just
/// has to match `Ship.player_slot` to find its local mirror.
fn send_ship_snapshot(
    time: Res<Time<Real>>,
    mut sock: ResMut<NetSocket>,
    ships: Query<
        (
            &crate::ship::Ship,
            &avian2d::prelude::Position,
            &avian2d::prelude::Rotation,
            &avian2d::prelude::LinearVelocity,
            &avian2d::prelude::AngularVelocity,
            &crate::ship::Crew,
            &crate::ship::Battery,
        ),
    >,
    snapshot_tick: Local<u32>,
) {
    sock.heartbeat_s += time.delta_secs();
    // Reuse `heartbeat_s` as the snapshot accumulator — gated by the
    // shorter `SNAPSHOT_INTERVAL_S` here. The standalone `send_heartbeat`
    // also reads it but its threshold (1 s) catches up too.
    if sock.heartbeat_s < SNAPSHOT_INTERVAL_S {
        return;
    }
    sock.heartbeat_s = 0.0;

    let entities: Vec<EntityState> = ships
        .iter()
        .map(|(ship, pos, rot, lin, ang, crew, batt)| EntityState {
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
        })
        .collect();

    let tick = *snapshot_tick;
    let msg = NetMessage::Snapshot {
        tick,
        entities,
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
            warn!("netcode: snapshot send to {peer:?} failed: {e:?}");
        }
    }
}

fn drain_messages(
    mut sock: ResMut<NetSocket>,
    role: Res<NetRole>,
    mut net_inputs: ResMut<crate::input::NetInputs>,
    mut ships: Query<
        (
            &crate::ship::Ship,
            &mut avian2d::prelude::Position,
            &mut avian2d::prelude::Rotation,
            &mut avian2d::prelude::LinearVelocity,
            &mut avian2d::prelude::AngularVelocity,
            &mut crate::ship::Crew,
            &mut crate::ship::Battery,
        ),
    >,
    mut last_snapshot_tick: Local<u32>,
) {
    // Snapshot the slot lookup before taking the channel mut-borrow so
    // we don't fight the borrow checker mid-loop.
    let slot_to_peer = sock.slot_to_peer.clone();
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
                // Only the host applies inbound inputs. Guests get
                // their own slot's input from `push_local_input_to_netinputs`
                // and discover everyone else's via snapshots.
                if !role.is_authoritative() {
                    continue;
                }
                let Some(slot) = slot_to_peer.iter().position(|&p| p == peer) else {
                    continue;
                };
                if slot < net_inputs.current.len() {
                    // `previous` rotation for this slot is handled
                    // each tick by `push_local_input_to_netinputs`.
                    // Overwriting `current` here lands the freshest
                    // guest input ahead of the next `gather_slot_inputs`.
                    net_inputs.current[slot] = input;
                }
            }
            NetMessage::Snapshot { tick, entities } => {
                // Guest applies snapshots onto its local ships;
                // host ignores them (it IS the authority).
                if !role.is_guest() {
                    continue;
                }
                // Discard out-of-order snapshots. matchbox
                // unreliable doesn't guarantee delivery order, but
                // we want the latest authority state — older ticks
                // would overwrite with stale poses.
                if tick != 0 && tick < *last_snapshot_tick {
                    continue;
                }
                *last_snapshot_tick = tick;
                for state in entities {
                    let EntityKind::Ship { slot, .. } = state.kind else {
                        // Only ships in v1.
                        continue;
                    };
                    // Find our local mirror for this slot.
                    for (ship, mut pos, mut rot, mut lin, mut ang, mut crew, mut batt) in
                        &mut ships
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
                        break;
                    }
                }
            }
            NetMessage::Lobby { slots } => {
                debug!("netcode: rx Lobby slots={} from {peer:?}", slots.len());
            }
            NetMessage::LobbyVote { class, ready } => {
                debug!("netcode: rx LobbyVote class={class} ready={ready} from {peer:?}");
            }
        }
    }
}
