//! Netplay post-match lobby. Lives between rounds:
//!
//!   `InMatch (MatchPhase::PostMatch)` — each peer can change their
//!     own slot's ship class (existing hotkeys) and toggle Ready (R).
//!   Both peers Ready → `AppState::Resetting → InMatch` with each
//!     slot's class taken from that peer's `PlayerInput.class` vote.
//!
//! The voting flows through the existing GGRS input channel — every
//! `PlayerInput` carries `class: u8` (index into `ALL_CLASSES`) and
//! `FLAG_READY` on its `flags` byte. The local peer drives those
//! values from `MatchConfig.slots[local_slot]` and `LobbyVote.ready`
//! every `gather_slot_inputs`; the remote peer reads them back out of
//! `SlotInputs` to learn what each opponent picked.
//!
//! For local hotseat / solo-vs-AI we keep the legacy "press R for an
//! instant rematch" — no Ready toggle, no class re-pick UI between
//! rounds (the existing in-match `class_picker_input` covers that).

use bevy::prelude::*;

use crate::hud::MatchPhase;
use crate::input::{self, FLAG_READY, SlotInputs};
use crate::ship::{
    class_from_index, class_to_index, ALL_CLASSES, MatchConfig, PlayerKind,
};
use crate::AppState;

pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LobbyVote>()
            .add_systems(OnEnter(AppState::InMatch), reset_lobby_at_match_start)
            .add_systems(
                Update,
                (
                    tick_local_class_cycle,
                    tick_local_ready_toggle,
                    detect_all_ready,
                )
                    .chain()
                    .run_if(in_state(AppState::InMatch))
                    .run_if(resource_exists::<crate::netcode::NetSocket>),
            );
        // `project_local_vote_into_inputs` is gone. `push_local_input_to_netinputs`
        // (netcode plugin, FixedUpdate) packs MatchConfig.class + the
        // Ready flag into `NetInputs.current[local_slot]` every tick,
        // which `gather_slot_inputs` then copies into `SlotInputs.held`.
        // Running both wrote the same value through two different
        // schedules; the in-Update projection raced the FixedUpdate
        // gather and flickered the HUD's class label between Earthling
        // Cruiser (the zero-byte default that gather copied from the
        // pre-fix wire format) and the actually-picked class. With the
        // class now riding on the wire, the projection is redundant.
    }
}

/// Local-peer lobby state. Not synced — only the LOCAL peer writes
/// it; remote peers' equivalent state is read out of `SlotInputs`
/// each frame.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct LobbyVote {
    /// `true` while the local peer is holding their Ready vote.
    /// Toggled by KeyR during `MatchPhase::PostMatch`. Cleared at
    /// the start of every new match (`OnEnter(InMatch)`).
    pub ready: bool,
}

fn reset_lobby_at_match_start(mut lobby: ResMut<LobbyVote>) {
    *lobby = LobbyVote::default();
}

/// During PostMatch in netplay, let the local peer cycle their slot's
/// class (Tab / Shift+Tab). Writes straight into `MatchConfig.slots`
/// — the remote peer learns about the new class through the next
/// `gather_slot_inputs` tick, which copies it into the local peer's
/// `PlayerInput.class`.
fn tick_local_class_cycle(
    keys: Res<ButtonInput<KeyCode>>,
    phase: Res<MatchPhase>,
    local_players: Option<Res<crate::netcode::LocalHandle>>,
    mut config: ResMut<MatchConfig>,
) {
    if *phase != MatchPhase::PostMatch {
        return;
    }
    let Some(local_slot) = local_players.as_ref().map(|lh| lh.0) else {
        return;
    };
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let cycle = if keys.just_pressed(KeyCode::Tab) {
        Some(if shift { -1 } else { 1 })
    } else {
        None
    };
    if let Some(dir) = cycle {
        if let Some(cur) = config.first(local_slot) {
            let cur_idx = class_to_index(cur) as i32;
            let n = ALL_CLASSES.len() as i32;
            let new_idx = (cur_idx + dir).rem_euclid(n);
            config.set_single(local_slot, class_from_index(new_idx as u8));
            info!(
                "lobby: P{} cycled to {:?}",
                local_slot + 1,
                ALL_CLASSES[new_idx as usize]
            );
        }
    }
}

/// During PostMatch in netplay, KeyR toggles the local peer's Ready
/// vote. We toggle on `just_pressed` so a single tap flips state
/// instead of mashing in and out of Ready while the key's held.
fn tick_local_ready_toggle(
    keys: Res<ButtonInput<KeyCode>>,
    virt: Res<crate::input::VirtualInput>,
    phase: Res<MatchPhase>,
    mut lobby: ResMut<LobbyVote>,
) {
    if *phase != MatchPhase::PostMatch {
        return;
    }
    // `R` toggles; the on-screen REMATCH button (touch devices have no
    // keyboard) *sets* ready — a one-way "I want a rematch" so a single
    // tap can't accidentally un-ready.
    if keys.just_pressed(KeyCode::KeyR) {
        lobby.ready = !lobby.ready;
        info!("lobby: local peer ready = {}", lobby.ready);
    } else if virt.rematch_just_pressed && !lobby.ready {
        lobby.ready = true;
        info!("lobby: local peer ready = true (touch)");
    }
}

/// During PostMatch, if EVERY human slot is currently holding the
/// Ready vote, sync each slot's class from its `PlayerInput.class`
/// vote into `MatchConfig` and trigger the rematch. AI slots are
/// auto-ready (they don't get a vote — they're excluded from
/// `voter_slots`). Remote peers are voters too: from this peer's
/// POV the other side's `kind` is `PlayerKind::Remote`, but their
/// `FLAG_READY` bit rides over the wire just like a local human's.
fn detect_all_ready(
    phase: Res<MatchPhase>,
    slot_inputs: Res<SlotInputs>,
    mut config: ResMut<MatchConfig>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if *phase != MatchPhase::PostMatch {
        return;
    }
    let voter_slots: Vec<usize> = config
        .slots
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            matches!(s.kind, PlayerKind::Human | PlayerKind::Remote).then_some(i)
        })
        .collect();
    if voter_slots.is_empty() {
        return;
    }
    if !voter_slots
        .iter()
        .all(|&i| input::SlotInputs::flag(&slot_inputs, i, FLAG_READY))
    {
        return;
    }
    // Apply every peer's class vote — including our own (which is
    // already in sync, but reading it back from `SlotInputs` rather
    // than `MatchConfig` keeps the code symmetric and means a peer
    // that joins mid-PostMatch with a different selection wins).
    for (i, slot_cfg) in config.slots.iter_mut().enumerate() {
        if matches!(slot_cfg.kind, PlayerKind::Human | PlayerKind::Remote) {
            let vote = slot_inputs.held[i].class;
            let class = class_from_index(vote);
            if slot_cfg.first() != class {
                info!(
                    "lobby: P{} locked in {:?} (was {:?})",
                    i + 1,
                    class,
                    slot_cfg.first()
                );
                slot_cfg.fleet = vec![class];
            }
        }
    }
    info!("lobby: both peers ready — rematch");
    next_state.set(AppState::Resetting);
}
