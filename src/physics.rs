//! Physics integration. Owned by Avian 2D (XPBD).
//!
//! The Avian `PhysicsSchedule` is bolted onto `GgrsSchedule`
//! instead of `FixedUpdate`. This makes physics state part of
//! the rollback window: when a peer's input prediction misses,
//! bevy_ggrs rewinds + replays from the last confirmed frame,
//! and physics integrates the corrected trajectory.
//!
//! For LOCAL play (no `Session<Config>` resource), bevy_ggrs
//! doesn't run `GgrsSchedule` itself, so we add an
//! `offline_tick` system in `FixedUpdate` that just calls
//! `world.run_schedule(GgrsSchedule)` directly. Same code
//! path; same tick rate. Online and offline produce the same
//! gameplay simulation — online just has GGRS sitting on top
//! correcting mispredictions.

use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_ggrs::GgrsSchedule;

/// Arena wraps at ±this value on each axis. Matches the SC2 Super Melee feel
/// — small enough that combat stays close, large enough that you can run.
/// Must comfortably exceed the spawn distance (±900 in `spawn_match`) so
/// that ships don't immediately wrap on the very first physics step and
/// end up on the wrong side of the arena facing outward.
pub const ARENA_HALF_EXTENT: f32 = 1500.0;

/// Full wrap period on each axis — the arena is a torus of this size.
pub const ARENA_SIZE: f32 = ARENA_HALF_EXTENT * 2.0;

/// Minimum-image convention: wrap a displacement to the shortest
/// equivalent vector on the torus (into `[-half, half]` per axis).
/// This is the only correct notion of "the vector from A to B" when
/// space wraps — two ships on opposite edges are actually adjacent.
pub fn min_image(delta: Vec2) -> Vec2 {
    Vec2::new(
        delta.x - ARENA_SIZE * (delta.x / ARENA_SIZE).round(),
        delta.y - ARENA_SIZE * (delta.y / ARENA_SIZE).round(),
    )
}

/// The periodic image of `pos` nearest to `focus`. Rendering draws
/// every wrappable entity in the copy of the torus closest to the
/// camera, so a ship crossing an edge slides in from the opposite
/// side instead of teleporting — and the camera (which shares the
/// same `focus`) never jerks to chase the jump.
pub fn nearest_image(pos: Vec2, focus: Vec2) -> Vec2 {
    focus - min_image(focus - pos)
}

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PhysicsPlugins::new(GgrsSchedule))
            .insert_resource(Gravity(Vec2::ZERO))
            // Avian's default `transform_to_position: true` reads
            // `Transform` back into `Position+Rotation` each step. We
            // explicitly set `transform.rotation = Quat::IDENTITY` in
            // `swap_rotation_frame` (so the pre-rotated sprite image
            // isn't double-rotated by Avian's Transform sync) — without
            // disabling the back-sync, that zero gets pulled into the
            // ship's `Rotation` every frame, resetting the player's
            // turning input. Disabling the read-back keeps Avian
            // authoritative on Rotation; Transform is render-only.
            .insert_resource(avian2d::physics_transform::PhysicsTransformConfig {
                transform_to_position: false,
                ..default()
            })
            .add_systems(
                PhysicsSchedule,
                wrap_arena.in_set(PhysicsStepSystems::Last),
            )
            // Offline driver: when no `Session<Config>` resource
            // exists (local play), bevy_ggrs won't run
            // `GgrsSchedule` for us. Drive it ourselves from
            // FixedUpdate so the same gameplay code runs at the
            // same tick rate either way.
            .add_systems(FixedUpdate, offline_tick);
    }
}

/// Run `GgrsSchedule` once per FixedUpdate when no GGRS session
/// is active. bevy_ggrs handles this when a Session exists;
/// without one, the schedule would never run and physics +
/// gameplay would freeze.
fn offline_tick(world: &mut World) {
    use bevy_ggrs::Session;
    if world.contains_resource::<Session<crate::netplay::Config>>() {
        return;
    }
    world.run_schedule(GgrsSchedule);
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
