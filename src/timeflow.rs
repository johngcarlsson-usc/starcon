//! Time-flow control: the seam where future "exotic" mechanics plug in.
//!
//! ## What this module owns
//!
//! A single resource — [`TimeScale`] — that multiplies the rate at which
//! Avian's physics clock advances. Setting it to `0.5` puts the whole game
//! in slow motion; `2.0` makes it fast; `0.0` pauses; `1.0` is normal.
//!
//! Debug controls (held while running the game): `[` slows time, `]`
//! speeds it up, `\` snaps back to 1.0.
//!
//! ## Extensibility seams left open for future work
//!
//! The interesting gameplay starts when ships affect time. The shape of
//! the API is intentionally simple so it can grow without rewrites:
//!
//! ### 1. Per-region time dilation (the "slow zone" / wormhole case)
//!
//! Add a component like:
//! ```ignore
//! #[derive(Component)]
//! pub struct TimeDilationField {
//!     pub center: Vec2,
//!     pub radius: f32,
//!     pub inner_scale: f32, // e.g. 0.25 = quarter speed
//! }
//! ```
//!
//! A system runs each tick, finds every body inside any field, and
//! scales that body's `ConstantLocalForce`/`ConstantTorque` and damping
//! by the field's `inner_scale`. Avian still ticks globally; bodies
//! inside the field just *feel* slower because the forces on them are
//! attenuated. Cheaper than running multiple physics worlds and
//! "good enough" for visuals; can be upgraded to a full per-island
//! substep schedule if the cheat becomes visible.
//!
//! ### 2. Time reversal (the "rewind button" ship)
//!
//! Don't try to run the physics solver with negative `dt` — it
//! diverges. Instead, exploit the ring buffer that rollback netcode
//! (M4) is going to maintain anyway: every frame, snapshot the
//! world state (Position, Rotation, Linear/AngularVelocity, Crew,
//! WeaponCooldown, projectiles) into a `VecDeque<Snapshot>` of the
//! last N seconds. When a ship triggers rewind, suspend the normal
//! physics tick and pop snapshots out of the buffer, applying them
//! to the world — visually a clean reverse. When the effect ends or
//! the buffer empties, hand control back to the live solver.
//!
//! This pairs cleanly with GGRS because GGRS *already* requires the
//! game state to be snapshot-able. The rewind ability becomes a
//! UI on top of plumbing we have to build anyway.
//!
//! ### 3. Per-ship subjective time
//!
//! Same shape as the per-region case but keyed off the ship's own
//! entity — a `SubjectiveTime(f32)` component. Useful for "this ship
//! moves at 2× while the rest of the world is normal" abilities.

use avian2d::prelude::*;
use bevy::prelude::*;

/// Global multiplier on physics-clock speed.
///
/// `1.0` is real-time, `0.0` is paused, values above 1 fast-forward.
/// Negative values are rejected — see the module docs for why time
/// reversal needs a snapshot-replay implementation, not negative dt.
#[derive(Resource, Debug, Clone, Copy)]
pub struct TimeScale(pub f32);

impl Default for TimeScale {
    fn default() -> Self {
        Self(1.0)
    }
}

pub struct TimeflowPlugin;

impl Plugin for TimeflowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimeScale>()
            .add_systems(Update, (debug_controls, apply_time_scale).chain());
    }
}

fn debug_controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut scale: ResMut<TimeScale>,
    session: Option<Res<crate::netcode::NetSocket>>,
) {
    // Bail in netplay: this reads local `KeyCode` and writes into
    // `TimeScale`, which `apply_time_scale` then bakes into
    // `Time<Physics>.relative_speed()`. If one peer slowed time the
    // other side wouldn't see it through GGRS, so physics would tick
    // at different rates per peer — instant desync.
    if session.is_some() {
        return;
    }
    let changed = if keys.just_pressed(KeyCode::BracketLeft) {
        scale.0 = (scale.0 - 0.25).max(0.0);
        true
    } else if keys.just_pressed(KeyCode::BracketRight) {
        scale.0 = (scale.0 + 0.25).min(4.0);
        true
    } else if keys.just_pressed(KeyCode::Backslash) {
        scale.0 = 1.0;
        true
    } else {
        false
    };
    if changed {
        info!("time scale → {:.2}", scale.0);
    }
}

fn apply_time_scale(scale: Res<TimeScale>, mut physics_time: ResMut<Time<Physics>>) {
    let s = scale.0.max(0.0);
    if (physics_time.relative_speed() - s).abs() > f32::EPSILON {
        physics_time.set_relative_speed(s);
    }
}
