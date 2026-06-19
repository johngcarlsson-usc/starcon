//! Physics integration. Owned by Avian 2D (XPBD).
//!
//! Avian's `PhysicsSchedule` is bolted onto `FixedUpdate`. Every
//! gameplay system also lives on `FixedUpdate` (post the GGRS
//! removal — see `NETCODE_REFACTOR.md`), so physics integrates one
//! tick per gameplay tick, both online and offline.

use avian2d::prelude::*;
use bevy::prelude::*;

/// Arena wraps at ±this value on each axis. Matches the SC2 Super Melee feel
/// — small enough that combat stays close, large enough that you can run.
/// Must comfortably exceed the spawn distance (±900 in `spawn_match`) so
/// that ships don't immediately wrap on the very first physics step and
/// end up on the wrong side of the arena facing outward.
///
/// Sized at twice the original 1500 so the torus wraps over a longer
/// distance — duels (and the lumbering capital-ship boss) get room to
/// manoeuvre without the world flipping under you too quickly. The
/// camera auto-fits and the starfield/minimap key off this, so nothing
/// else needs hand-tuning when it changes.
pub const ARENA_HALF_EXTENT: f32 = 3000.0;

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
        app.add_plugins(PhysicsPlugins::new(FixedUpdate))
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
