//! Stub AI player. Marker component + a single tick system that
//! drives any ship tagged `AiControlled`:
//!   - Pick the nearest enemy ship (different `player_slot`).
//!   - Steer toward it. The ship's `Classic` angular control
//!     overwrites `AngularVelocity` each frame, so we write the
//!     turn rate directly.
//!   - Thrust forward at full power once roughly on bearing
//!     (within ~75°). Going full throttle while pointed sideways
//!     just wastes momentum.
//!   - Primary fire is force-pressed every frame by
//!     `dispatch_primary` checking for the `AiControlled` marker
//!     (similar plumbing to Pkunk clones).
//!
//! This is intentionally crude — "fly at + shoot" — as a
//! placeholder. Real per-class AI (Druuge sniping at range,
//! Mmrnmhrm form-toggling, Pkunk taunt-and-juke) belongs in a
//! future per-ship strategy table; for now this gives us
//! warm-bodies opponents for solo play and netcode testing.

use avian2d::prelude::*;
use bevy::prelude::*;

use crate::ship::{Ship, ShipPhysicsDerived};

/// Marker: this ship is driven by the stub AI instead of by
/// player input. Inserted by `spawn_match` when a slot's
/// `PlayerKind` is `Ai`. Removed only by despawn (ship death /
/// rematch teardown).
#[derive(Component, Debug)]
pub struct AiControlled;

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        // Same schedule as `apply_player_input` so AI ships
        // participate in the same physics step as humans. The
        // `.after(apply_player_input)` chaining is implicit
        // because the apply system bails on AI-tagged ships, so
        // there's no write conflict in either order.
        app.add_systems(
            FixedUpdate,
            tick_ai_pilots.run_if(in_state(crate::AppState::InMatch)),
        );
    }
}

/// Drive every AI ship toward its nearest enemy. Run in
/// FixedUpdate so the steering happens at the same rate as
/// physics integration.
fn tick_ai_pilots(
    targets: Query<(Entity, &Ship, &Position)>,
    mut pilots: Query<
        (
            Entity,
            &Ship,
            &Position,
            &Rotation,
            &ShipPhysicsDerived,
            &mut ConstantLocalForce,
            &mut AngularVelocity,
        ),
        With<AiControlled>,
    >,
) {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};

    for (ai_entity, ai_ship, ai_pos, ai_rot, derived, mut thrust, mut ang_vel) in &mut pilots {
        // Find nearest non-friendly ship.
        let mut best: Option<(Vec2, f32)> = None;
        for (e, s, p) in &targets {
            if e == ai_entity || s.player_slot == ai_ship.player_slot {
                continue;
            }
            let d2 = (p.0 - ai_pos.0).length_squared();
            if best.map_or(true, |(_, b)| d2 < b) {
                best = Some((p.0, d2));
            }
        }
        let Some((target_pos, _)) = best else {
            // No enemies on the field — just drift.
            thrust.0 = Vec2::ZERO;
            ang_vel.0 = 0.0;
            continue;
        };

        let cur_heading = ai_rot.sin.atan2(ai_rot.cos);
        let to_target = target_pos - ai_pos.0;
        // Sprite forward is +Y; subtract π/2 so the heading
        // matches the bearing math.
        let bearing = to_target.y.atan2(to_target.x) - FRAC_PI_2;
        let mut steer_err = bearing - cur_heading;
        while steer_err > PI {
            steer_err -= TAU;
        }
        while steer_err < -PI {
            steer_err += TAU;
        }

        // Direct angular velocity write — same `Classic`
        // override the player would do.
        ang_vel.0 = if steer_err.abs() < 1e-3 {
            0.0
        } else {
            steer_err.signum() * derived.target_omega
        };
        // Thrust once roughly on-bearing (within ~75°). Going
        // full throttle while still pointing sideways wastes
        // momentum and pulls us off-track.
        thrust.0 = if steer_err.abs() < FRAC_PI_2 * 0.85 {
            Vec2::new(0.0, derived.thrust_force)
        } else {
            Vec2::ZERO
        };
    }
}
