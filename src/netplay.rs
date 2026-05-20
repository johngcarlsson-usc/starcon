//! GGRS + matchbox glue. Kept intentionally minimal in the scaffold — the
//! goal of this file is to declare the contract (input type, frame rate,
//! room URL) so callers know what to import once the rollback schedule
//! is wired up against the physics tick.
//!
//! Next milestone: register `PlayerInput` as the GGRS input type, add
//! `Rollback` components to ships/projectiles, and drive `physics::integrate`
//! from `bevy_ggrs::GgrsSchedule`.

use bevy::prelude::*;

pub const NUM_PLAYERS: usize = 2;
pub const FPS: usize = 60;
pub const INPUT_DELAY: usize = 2;

/// Matchbox signaling server URL used to find peers. The public test server
/// is fine for development; swap for a self-hosted one before any real launch.
pub const DEFAULT_ROOM_URL: &str = "wss://match.helsing.studio/starcon?next=2";

pub struct NetplayPlugin;

impl Plugin for NetplayPlugin {
    fn build(&self, _app: &mut App) {}
}
