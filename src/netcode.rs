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
}

/// Cadence of the connectivity heartbeat sent on the NetMessage
/// channel. Slow (1 Hz) — this is only here to confirm the channel
/// is wired up; once snapshots are flowing they'll dominate.
const HEARTBEAT_INTERVAL_S: f32 = 1.0;

pub struct NetcodePlugin;

impl Plugin for NetcodePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetRole>()
            .init_resource::<NetIdAllocator>()
            // Heartbeat + receive loop. Both gate on the
            // `NetSocket` resource existing, which only happens
            // after the matchbox handshake — so they're no-ops
            // in solo / local hotseat.
            .add_systems(
                Update,
                (send_heartbeat, drain_messages).chain().run_if(
                    resource_exists::<NetSocket>,
                ),
            );
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
        channel.send(bytes.clone().into(), peer);
    }
    let _ = role; // role isn't acted on yet; the dispatcher uses it next session
}

fn drain_messages(mut sock: ResMut<NetSocket>) {
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
        // Dispatch by message type. The host/guest gameplay split
        // isn't wired yet — for the foundation pass we only log
        // receipt so the user can confirm the channel is live.
        match msg {
            NetMessage::Input { tick, .. } => {
                debug!("netcode: rx Input tick={tick} from {peer:?}");
            }
            NetMessage::Snapshot { tick, entities } => {
                debug!(
                    "netcode: rx Snapshot tick={tick} entities={} from {peer:?}",
                    entities.len()
                );
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
