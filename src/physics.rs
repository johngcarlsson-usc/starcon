//! Deterministic 2D physics for melee combat.
//!
//! Combat runs in a `FixedUpdate`-style tick at a fixed rate so it can be
//! driven by GGRS rollback. All motion uses `f32` math kept simple enough
//! that it produces the same result on every machine in the session.
//! The original TimeWarp engine ran at 36 Hz; we target 60 Hz here.

use bevy::prelude::*;

pub const TICK_HZ: u32 = 60;
pub const TICK_DT: f32 = 1.0 / TICK_HZ as f32;

/// World wraps around at ±half this value on each axis (toroidal arena),
/// matching the SC2 Super Melee behaviour.
pub const ARENA_HALF_EXTENT: f32 = 1000.0;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Velocity(pub Vec2);

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Heading(pub f32);

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct AngularVelocity(pub f32);

#[derive(Component, Debug, Clone, Copy)]
pub struct PhysicsBody {
    pub mass: f32,
    pub max_speed: f32,
}

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, _app: &mut App) {}
}

/// Integrate motion for one tick. Will be plugged into the GGRS schedule
/// once the netplay layer is wired up — kept as a free function so the
/// rollback driver can call it directly without going through `Update`.
pub fn integrate(mut q: Query<(&mut Transform, &mut Velocity, &Heading, &PhysicsBody)>) {
    for (mut tf, mut vel, _heading, body) in &mut q {
        let speed = vel.0.length();
        if speed > body.max_speed {
            vel.0 = vel.0 * (body.max_speed / speed);
        }
        let pos = tf.translation.truncate() + vel.0 * TICK_DT;
        let wrapped = wrap_arena(pos);
        tf.translation.x = wrapped.x;
        tf.translation.y = wrapped.y;
    }
}

fn wrap_arena(p: Vec2) -> Vec2 {
    let extent = ARENA_HALF_EXTENT * 2.0;
    Vec2::new(
        ((p.x + ARENA_HALF_EXTENT).rem_euclid(extent)) - ARENA_HALF_EXTENT,
        ((p.y + ARENA_HALF_EXTENT).rem_euclid(extent)) - ARENA_HALF_EXTENT,
    )
}
