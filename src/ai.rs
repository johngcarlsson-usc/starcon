//! Data-driven AI pilot.
//!
//! Each ship's `.ini` `[AI3_Default]` block names the **weapon tactic**
//! (when to press FIRE) and a chain of **special tactics** (when to
//! press SPECIAL), parsed into `crate::ship::AiTactics` at load time.
//! This system turns those tactics — plus per-tick context (target
//! position, incoming projectiles, battery, etc.) — into virtual
//! button-presses on `SlotInputs`, exactly as if the slot had a
//! keyboard player. The existing input pipeline (`apply_player_input`,
//! `dispatch_primary`, `tick_orz_turret`, …) then handles everything
//! else, so per-ship mechanics (Orz turret aim, Pkunk shields, …) come
//! along for free.
//!
//! The library implemented here covers the eight most-common tactic
//! names in the canon `.ini`s (`Precedence`, `Homing`, `Narrow`,
//! `Field`, `Launched` for the weapon; `Defense`, `Proximity`,
//! `Battery` for the special). Unrecognised tactics fall back to a
//! sensible default so every ship plays at least competently.
//!
//! Determinism: runs in `GgrsSchedule` after the input producer and
//! before any consumer; uses only rollback-tracked state plus a
//! deterministic per-ship counter for the `SpecialFreq` 1/N gate.
//! No RNG, so it replays identically.

use avian2d::prelude::{LinearVelocity, Position, Rotation};
use bevy::prelude::*;

use crate::input::{self, PlayerInput, SlotInputs};
use crate::ship::{
    AiSpecialTactic, AiWeaponTactic, Battery, Crew, OrzTurret, Projectile, Ship, ShipClass,
    SC2_RANGE_SCALE,
};

/// Marker: this ship is driven by the AI instead of by player input.
#[derive(Component, Debug)]
pub struct AiControlled;

/// Per-ship AI runtime state. Edge-detect, gates, cooldowns. Inserted
/// alongside `AiControlled` in `spawn_match`.
#[derive(Component, Debug, Default)]
pub struct AiBrain {
    /// Last frame's button bits — used to compute the just_pressed /
    /// just_released edges we write to `SlotInputs`.
    pub last_buttons: u8,
    /// Frame counter for the `SpecialFreq` 1/N gate (a `Special` with
    /// `SpecialFreq=3` only fires every third eligible frame). Wraps
    /// at u32::MAX; we only look at it modulo `freq`.
    pub special_tick_counter: u32,
}

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        // Run after the input producer (so we see the latest non-AI slots)
        // and before any consumer (apply_player_input, dispatch_primary,
        // tick_orz_turret) so our virtual button-presses drive the same
        // tick.
        app.add_systems(
            bevy_ggrs::GgrsSchedule,
            tick_ai_pilots
                .after(crate::input::SlotInputProducerSet)
                .before(crate::ship::apply_player_input)
                .run_if(in_state(crate::AppState::InMatch)),
        );
    }
}

/// How close (radians) the firing direction must be to the target
/// bearing before each weapon tactic fires.
const PRECEDENCE_CONE: f32 = 0.12; // ~7°
const NARROW_CONE: f32 = 0.40; // ~23°
const HOMING_CONE: f32 = 1.05; // ~60°
const DEFAULT_CONE: f32 = 0.70; // ~40°

/// Stop thrusting if the target is closer than this (don't ram).
const STANDOFF_DISTANCE: f32 = 80.0;
/// Don't bother thrusting unless we're within this much of pointing
/// at the target — going full throttle sideways wastes momentum.
const THRUST_BEARING_TOLERANCE: f32 = std::f32::consts::FRAC_PI_2 * 0.85;
/// "Crew is running low" threshold for `Defense` — at or below 1/3 of
/// max we treat the ship as in danger.
const LOW_CREW_FRACTION: f32 = 1.0 / 3.0;
/// Range within which a hostile projectile counts as "incoming" for
/// the `Defense` tactic.
const INCOMING_RADIUS_WU: f32 = 280.0;
/// Dot-product threshold for "this projectile is heading at me" —
/// 0.4 ≈ within 66° of straight at us.
const INCOMING_DOT: f32 = 0.4;
/// Tolerance for "the Orz turret is aimed at the target" (radians).
const ORZ_TURRET_TOLERANCE: f32 = 0.08;

fn tick_ai_pilots(
    mut slot_inputs: ResMut<SlotInputs>,
    targets: Query<(Entity, &Ship, &Position), Without<crate::ultimate::HyperActive>>,
    mut pilots: Query<
        (
            Entity,
            &Ship,
            &ShipClass,
            &Position,
            &Rotation,
            &Battery,
            &Crew,
            &mut AiBrain,
            Option<&OrzTurret>,
        ),
        With<AiControlled>,
    >,
    projectiles: Query<(&Projectile, &Position, &LinearVelocity)>,
) {
    use std::f32::consts::FRAC_PI_2;
    for (entity, ship, class, pos, rot, batt, crew, mut brain, turret) in &mut pilots {
        let slot = ship.player_slot.min(3);

        // 1) Pick a target — nearest live non-friendly ship in the
        //    minimum-image arena (so a wrap-side opponent is correctly
        //    treated as close).
        let Some((target_pos_world, _target_e)) = nearest_enemy(entity, ship.player_slot, pos.0, &targets)
        else {
            // No targets — write a neutral input and bail.
            write_input(&mut slot_inputs, slot, &mut brain, 0);
            continue;
        };
        let to_target = crate::physics::min_image(target_pos_world - pos.0);
        let distance = to_target.length().max(1.0);
        let bearing = to_target.y.atan2(to_target.x) - FRAC_PI_2;
        let current_heading = rot.sin.atan2(rot.cos);

        // 2) Orz takes a fully dedicated control loop — its turret has
        //    to track the target, not the hull, so the normal "face
        //    target then fire" doesn't apply. The marine launch chord
        //    is intentionally not modelled in v1 (it's a future tuning
        //    pass).
        if *class == ShipClass::Orzne {
            let bits = orz_ai_input(
                turret,
                current_heading,
                bearing,
                distance,
                ai_weapon_range(ship),
            );
            write_input(&mut slot_inputs, slot, &mut brain, bits);
            continue;
        }

        // 3) Steering. Default: face the target; thrust when roughly
        //    on-bearing AND not point-blank. Per-class overrides go
        //    here when a ship can't be flown like that (none today
        //    beyond Orz above; Spathi/Druuge/Arilou play fine with
        //    "face + fire" because their odd mechanics are encoded
        //    elsewhere — Spathi's BUTT missile is the *special*, not
        //    the primary; Druuge's recoil is just physics; Arilou's
        //    halo is `Field` so it fires regardless of aim).
        let steer_err = wrap_pi(bearing - current_heading);
        let want_left = steer_err > 0.02; // ~1°
        let want_right = steer_err < -0.02;
        let want_thrust = steer_err.abs() < THRUST_BEARING_TOLERANCE && distance > STANDOFF_DISTANCE;

        // 4) Weapon decision — does the firing direction line up well
        //    enough for this tactic to commit to firing?
        let firing_err = steer_err; // most ships fire forward; Spathi's
        // backward shot is its SPECIAL volley, not the primary.
        let weapon_range = ai_weapon_range(ship);
        let in_range = distance <= weapon_range;
        let want_fire = match ship.stats.ai.weapon {
            AiWeaponTactic::Precedence => firing_err.abs() < PRECEDENCE_CONE && in_range,
            AiWeaponTactic::Narrow => firing_err.abs() < NARROW_CONE && in_range,
            AiWeaponTactic::Homing => firing_err.abs() < HOMING_CONE && in_range,
            AiWeaponTactic::Launched => firing_err.abs() < HOMING_CONE && in_range,
            // Field-of-effect / auto-aim weapons: just press fire on
            // every cooldown the dispatcher allows.
            AiWeaponTactic::Field => true,
            AiWeaponTactic::Default => firing_err.abs() < DEFAULT_CONE && in_range,
        };

        // 5) Special decision — first eligible tactic in the chain
        //    wins. `Defense` triggers on incoming projectile or low
        //    crew; `Proximity` on target inside special_range;
        //    `Battery` once we've banked enough charge.
        let special_range = ship
            .stats
            .ai
            .special_range
            .unwrap_or(weapon_range)
            .max(60.0);
        let crew_low = (crew.current as f32) <= (ship.stats.crew_max as f32) * LOW_CREW_FRACTION;
        let incoming = incoming_projectile(entity, ship.player_slot, pos.0, &projectiles);
        let drain_ok = batt.current >= ship.stats.special_drain;
        let mut want_special = false;
        for &t in &ship.stats.ai.specials {
            let trigger = match t {
                AiSpecialTactic::Defense => drain_ok && (incoming || crew_low),
                AiSpecialTactic::Proximity => drain_ok && distance <= special_range,
                AiSpecialTactic::NoProximity => drain_ok && distance > special_range,
                AiSpecialTactic::Battery => batt.current >= ship.stats.ai.batt_floor,
                AiSpecialTactic::PlusFire | AiSpecialTactic::None | AiSpecialTactic::Default => {
                    false
                }
            };
            if trigger {
                want_special = true;
                break;
            }
        }
        // `SpecialFreq` gate: only fires every Nth eligible tick so
        // cheap specials don't continuously trigger. Counter is
        // per-ship and deterministic across rollback.
        let freq = ship.stats.ai.special_freq.max(1);
        if want_special {
            brain.special_tick_counter = brain.special_tick_counter.wrapping_add(1);
            if freq > 1 && brain.special_tick_counter % freq != 0 {
                want_special = false;
            }
        }

        // 6) Compose the button mask + write to SlotInputs.
        let mut bits = 0u8;
        if want_left {
            bits |= input::INPUT_LEFT;
        } else if want_right {
            bits |= input::INPUT_RIGHT;
        }
        if want_thrust {
            bits |= input::INPUT_THRUST;
        }
        if want_fire {
            bits |= input::INPUT_FIRE;
        }
        if want_special {
            bits |= input::INPUT_SPECIAL;
        }
        write_input(&mut slot_inputs, slot, &mut brain, bits);
    }
}

/// Find the nearest enemy ship on the toroidal arena.
fn nearest_enemy(
    me: Entity,
    me_slot: usize,
    me_pos: Vec2,
    ships: &Query<(Entity, &Ship, &Position), Without<crate::ultimate::HyperActive>>,
) -> Option<(Vec2, Entity)> {
    let mut best: Option<(Vec2, Entity, f32)> = None;
    for (e, s, p) in ships {
        if e == me || s.player_slot == me_slot {
            continue;
        }
        let d2 = crate::physics::min_image(p.0 - me_pos).length_squared();
        if best.map_or(true, |(_, _, b)| d2 < b) {
            best = Some((p.0, e, d2));
        }
    }
    best.map(|(p, e, _)| (p, e))
}

/// True if a hostile projectile is heading at this ship within
/// `INCOMING_RADIUS_WU`. Used by the `Defense` special tactic.
fn incoming_projectile(
    me: Entity,
    me_slot: usize,
    me_pos: Vec2,
    projectiles: &Query<(&Projectile, &Position, &LinearVelocity)>,
) -> bool {
    for (proj, pos, vel) in projectiles {
        if proj.owner == me {
            continue;
        }
        // Same-slot (clones / sub-entities owned by ourselves) skip.
        // We don't have the projectile's firer's slot here without a
        // second lookup — INCOMING_DOT + RADIUS catches obvious cases.
        let _ = me_slot;
        let to_me = crate::physics::min_image(me_pos - pos.0);
        let d = to_me.length();
        if d > INCOMING_RADIUS_WU {
            continue;
        }
        let speed = vel.0.length();
        if speed < 1.0 {
            continue;
        }
        let aimed = (vel.0 / speed).dot(to_me / d.max(1.0));
        if aimed > INCOMING_DOT {
            return true;
        }
    }
    false
}

/// Orz Nemesis: aim the turret at the target (hold SPECIAL + turn
/// keys to rotate the turret), and fire once the turret is on-bearing.
/// Marine launches (SPECIAL+FIRE chord) are intentionally skipped in
/// v1 — that's a follow-up. Body steering is also skipped: an Orz that
/// rotates its hull would shift its turret aim by the same amount, so
/// keeping the body still while the turret tracks is the simplest
/// behaviour that actually hits things.
fn orz_ai_input(
    turret: Option<&OrzTurret>,
    ship_heading: f32,
    target_bearing: f32,
    distance: f32,
    weapon_range: f32,
) -> u8 {
    let Some(turret) = turret else {
        return 0;
    };
    // Desired turret angle relative to ship facing — `wrap_pi`'d so
    // we don't try to rotate the long way round.
    let desired_offset = wrap_pi(target_bearing - ship_heading);
    let err = wrap_pi(desired_offset - turret.offset_rad);
    let aligned = err.abs() < ORZ_TURRET_TOLERANCE;
    let in_range = distance <= weapon_range;

    let mut bits = 0u8;
    if !aligned {
        // Hold special + press the turn key to rotate the turret. No
        // ship rotation (special-held suppresses it in tick_orz_turret).
        bits |= input::INPUT_SPECIAL;
        if err > 0.0 {
            bits |= input::INPUT_LEFT;
        } else {
            bits |= input::INPUT_RIGHT;
        }
    } else if in_range {
        // Aligned + in range: fire.
        bits |= input::INPUT_FIRE;
    }
    // Thrust toward the target if we're farther than the standoff.
    // Orz can't ship-turn under our current scheme, so it'll drift in
    // its initial direction — acceptable; a quick-fix is to start the
    // ship facing roughly arena-centre, which `spawn_match` already does.
    if distance > STANDOFF_DISTANCE {
        bits |= input::INPUT_THRUST;
    }
    bits
}

/// Cone-of-fire range used by every tactic. Prefers the `.ini`'s
/// `[AI3_Default] Weapon_Range` (scale ×40), falls back to
/// `[Weapon] Range`, then to a generous 800 wu so AI ships at least
/// try to engage.
fn ai_weapon_range(ship: &Ship) -> f32 {
    if let Some(r) = ship.stats.ai.weapon_range {
        return r;
    }
    if ship.stats.weapon_range > 0.0 {
        return ship.stats.weapon_range * SC2_RANGE_SCALE;
    }
    800.0
}

/// Wrap an angle into [-π, π].
fn wrap_pi(mut a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    while a > PI {
        a -= TAU;
    }
    while a < -PI {
        a += TAU;
    }
    a
}

/// Write the AI's `bits` to `SlotInputs.held[slot]` and compute the
/// just_pressed / just_released edges against the brain's last frame
/// so edge-triggered consumers (Orz marine launch, Yehat shield
/// toggle, etc.) see fresh transitions.
fn write_input(slot_inputs: &mut SlotInputs, slot: usize, brain: &mut AiBrain, bits: u8) {
    let pressed = bits & !brain.last_buttons;
    let released = !bits & brain.last_buttons;
    slot_inputs.held[slot] = PlayerInput {
        buttons: bits,
        turn: 0,
        aim_x: 0,
        aim_y: 0,
    };
    slot_inputs.just_pressed[slot] = PlayerInput {
        buttons: pressed,
        turn: 0,
        aim_x: 0,
        aim_y: 0,
    };
    slot_inputs.just_released[slot] = PlayerInput {
        buttons: released,
        turn: 0,
        aim_x: 0,
        aim_y: 0,
    };
    brain.last_buttons = bits;
}
