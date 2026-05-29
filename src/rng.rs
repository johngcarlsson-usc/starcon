//! Deterministic per-match RNG. Gameplay-affecting random
//! choices (projectile spread, asteroid spawn positions, crystal-
//! shard angles, etc.) must produce the same outputs on every
//! peer for GGRS rollback to converge. This module owns the
//! seeded RNG resource and the policy.
//!
//! ## Policy
//!
//! Two RNG channels coexist in the codebase:
//!
//!   1. `GameRng` (this module) — seeded `fastrand::Rng`,
//!      reset at the start of every match. **Use for anything
//!      that affects game state**: damage rolls, projectile
//!      spread, spawn positions, AI decisions, ability dice.
//!
//!   2. Global `fastrand::f32()` / `fastrand::bool()` etc. —
//!      non-deterministic across runs (and across peers).
//!      **Use only for purely visual effects** that don't feed
//!      back into game state: starfield star positions, sprite
//!      colour jitter, particle alpha noise, trail rendering.
//!
//! Both peers running the same binary version with the same
//! seed will produce the same `GameRng` output stream **as long
//! as call order and call count are identical**. Beware:
//!
//!   - Iterating an unordered `Query` and consuming RNG calls
//!     per entity is order-sensitive. Sort by `Entity` first
//!     (`q.iter().sort_by_key(|(e, _)| e.index())`) or pre-bake
//!     the iteration order.
//!   - Branching on `Time<Real>` to decide whether to consume
//!     RNG is non-deterministic. Use `Time<Physics>` /
//!     `Time<Fixed>` for gameplay logic instead.
//!   - Conditional RNG draws (e.g. `if some_flag { rng.f32() }`)
//!     must be driven only by deterministic state.
//!
//! Until the full GGRS schedule lands, the seed defaults to 0 —
//! matches replay identically across the same binary but not
//! across peers. Once the matchmaking handshake negotiates a
//! shared seed, populate it via `MatchSeed.0` before
//! transitioning into InMatch.

use bevy::prelude::*;

/// Seed used to initialise `GameRng` at the start of every
/// match. Mutate before transitioning into `AppState::InMatch`
/// (e.g. from a netplay handshake) to control the per-match
/// random sequence.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct MatchSeed(pub u64);

/// Per-match seeded RNG. Mutated by gameplay systems to draw
/// numbers; reset on every `OnEnter(InMatch)` from `MatchSeed`.
///
/// Wraps `fastrand::Rng` for the same generator the rest of the
/// codebase already used in its non-deterministic form. Same
/// distribution, same speed — just owned by a Bevy resource so
/// peers see the same draws in the same order. Clone enables
/// bevy_ggrs to snapshot/restore the stream position on rollback.
#[derive(Resource, Clone)]
pub struct GameRng(pub fastrand::Rng);

impl Default for GameRng {
    fn default() -> Self {
        Self(fastrand::Rng::with_seed(0))
    }
}

impl GameRng {
    pub fn f32(&mut self) -> f32 {
        self.0.f32()
    }
    pub fn i32(&mut self, range: std::ops::RangeInclusive<i32>) -> i32 {
        self.0.i32(range)
    }
    pub fn usize_range(&mut self, range: std::ops::Range<usize>) -> usize {
        self.0.usize(range)
    }
    pub fn u32_exclusive(&mut self, range: std::ops::Range<u32>) -> u32 {
        self.0.u32(range)
    }
    pub fn bool(&mut self) -> bool {
        self.0.bool()
    }
    /// Convenience: signed in [-1.0, 1.0).
    pub fn signed_unit(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }
}

pub struct RngPlugin;

impl Plugin for RngPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MatchSeed>()
            .init_resource::<GameRng>()
            // Reset the RNG at the start of every match so the
            // sequence is reproducible from the seed. The seed
            // itself can be mutated by the netplay handshake
            // (or by tests) before entering InMatch.
            //
            // **Must** run before `ship::spawn_match` — that's the
            // first consumer of the per-match RNG (planet quadrant
            // + jitter, asteroid placements). Without an explicit
            // `.before(spawn_match)`, Bevy is free to schedule
            // `reseed_for_match` either side of `spawn_match`, and
            // when it runs after, every peer's planet draws from
            // stale RNG state — desync.
            .add_systems(
                OnEnter(crate::AppState::InMatch),
                reseed_for_match.before(crate::ship::spawn_match),
            );
    }
}

fn reseed_for_match(seed: Res<MatchSeed>, mut rng: ResMut<GameRng>) {
    rng.0 = fastrand::Rng::with_seed(seed.0);
}
