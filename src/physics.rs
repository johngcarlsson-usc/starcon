//! Physics integration. Owned by Avian 2D (XPBD); we just configure it for
//! space combat: no gravity and a toroidal arena wrap that runs after each
//! physics step. Tick rate comes from Bevy's `FixedUpdate` (60 Hz default)
//! which Avian drives its `PhysicsSchedule` off of — we don't pin it
//! ourselves. `Time<Physics>` is owned by Avian's plugin; gameplay timers
//! read it (not Bevy's wall-clock `Time`) so global slow-mo / fast-forward
//! also scales weapon cooldowns, projectile lifetimes, etc.

use avian2d::prelude::*;
use bevy::prelude::*;

/// Arena wraps at ±this value on each axis. Matches the SC2 Super Melee feel
/// — small enough that combat stays close, large enough that you can run.
pub const ARENA_HALF_EXTENT: f32 = 800.0;

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PhysicsPlugins::default())
            .insert_resource(Gravity(Vec2::ZERO))
            .add_systems(
                PhysicsSchedule,
                wrap_arena.in_set(PhysicsStepSystems::Last),
            );
    }
}

/// Wrap any rigid body that crosses the arena edge to the opposite side.
/// Runs after the physics solver so we don't fight Avian over positions.
fn wrap_arena(mut q: Query<&mut Position>) {
    let extent = ARENA_HALF_EXTENT * 2.0;
    for mut pos in &mut q {
        let p = pos.0;
        let wrapped = Vec2::new(
            ((p.x + ARENA_HALF_EXTENT).rem_euclid(extent)) - ARENA_HALF_EXTENT,
            ((p.y + ARENA_HALF_EXTENT).rem_euclid(extent)) - ARENA_HALF_EXTENT,
        );
        if wrapped != p {
            pos.0 = wrapped;
        }
    }
}
