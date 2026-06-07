//! Data-driven AI pilot.
//!
//! Each ship's `.ini` `[AI3_Default]` block names the weapon + special
//! tactics, parsed into `crate::ship::AiTactics` at load time. This
//! system turns those tactics — plus per-tick context (target,
//! incoming projectiles, battery, crew) — into virtual button-presses
//! on `SlotInputs`, so the existing input pipeline drives the AI
//! identically to a human.
//!
//! Beyond the raw tactic dispatch, this version layers in:
//!   - **Difficulty tiers** (Easy / Medium / Hard) via the
//!     `AiDifficulty` resource. Each tier scales reaction time, aim
//!     jitter, special eagerness, retreat threshold, and prediction.
//!   - **Per-class engagement range** — every class has an optimal
//!     orbit radius and the AI maintains it (close to target if far,
//!     hold position if at range). Spathi runs the other way and
//!     fires its BUTT missile backward.
//!   - **Lead-the-target** — on Medium/Hard the firing direction is
//!     biased to where the target WILL be at projectile-impact time,
//!     so a moving target gets shot in front of, not at.
//!   - **Retreat** — at ≤25% crew the AI breaks off, faces away,
//!     thrusts to escape, and only re-engages once crew is back above
//!     ~50% (which it usually isn't — most ships can't heal — so this
//!     is effectively "die running" for the badly-wounded).
//!
//! Determinism: runs in `GgrsSchedule` after the input producer and
//! before any consumer; uses only rollback-tracked state. The
//! `SpecialFreq` 1/N gate uses a per-ship counter (no RNG), so this
//! still replays identically across peers.

use avian2d::prelude::{AngularVelocity, LinearVelocity, Physics, Position, Rotation};
use bevy::prelude::*;

use crate::input::{self, PlayerInput, SlotInputs};
use crate::ship::{
    AiSpecialTactic, AiWeaponTactic, AngularControl, AngularControlOverride, Battery, Crew,
    OrzTurret, Projectile, Ship, ShipClass, SC2_RANGE_SCALE,
};

/// Marker: this ship is driven by the AI instead of by player input.
#[derive(Component, Debug, Clone)]
pub struct AiControlled;

/// Global difficulty preset. `Resource` so the settings menu can swap
/// it live. Defaults to Medium — the same opponents you'd expect on
/// the original SC2 default difficulty.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiDifficulty {
    Easy,
    Medium,
    Hard,
}

impl Default for AiDifficulty {
    fn default() -> Self {
        AiDifficulty::Medium
    }
}

/// Per-tier knob bundle. Constructed from `AiDifficulty::tuning()`
/// each tick; cheap (it's just six scalars + a bool).
#[derive(Debug, Clone, Copy)]
struct AiTuning {
    /// How tight (radians) we tolerate the firing solution before
    /// committing to FIRE. Easy adds extra jitter so even Precedence
    /// tactic effectively becomes Narrow.
    aim_jitter_rad: f32,
    /// Multiplier on the special-tactic trigger conditions; >1 means
    /// the AI uses its special more eagerly (Hard), <1 means it
    /// hoards (Easy). Combined with the per-class `SpecialFreq`.
    special_eagerness: f32,
    /// Crew fraction at which retreat kicks in. Easy: 50% (timid),
    /// Medium: 25%, Hard: 20% (commits longer).
    retreat_threshold: f32,
    /// Should the AI lead its shots based on target velocity? Easy:
    /// no (shoot where the target IS); Medium/Hard: yes.
    predict_lead: bool,
}

impl AiDifficulty {
    fn tuning(self) -> AiTuning {
        match self {
            AiDifficulty::Easy => AiTuning {
                aim_jitter_rad: 0.25, // ~14° wobble
                special_eagerness: 0.4,
                retreat_threshold: 0.50,
                predict_lead: false,
            },
            AiDifficulty::Medium => AiTuning {
                aim_jitter_rad: 0.06, // ~3.5°
                special_eagerness: 1.0,
                retreat_threshold: 0.25,
                predict_lead: true,
            },
            AiDifficulty::Hard => AiTuning {
                aim_jitter_rad: 0.0,
                special_eagerness: 1.5,
                retreat_threshold: 0.20,
                predict_lead: true,
            },
        }
    }
}

/// Per-ship AI runtime state. Edge-detect, gates, retreat memory.
#[derive(Component, Debug, Default, Clone)]
pub struct AiBrain {
    pub last_buttons: u8,
    pub special_tick_counter: u32,
    /// Currently in "retreat" mode — flipped on at low crew, off again
    /// when crew has recovered through the upper hysteresis threshold.
    /// Most ships can't regenerate crew so once it flips on it stays
    /// on for the rest of the engagement.
    pub retreating: bool,
    /// Pseudo-random per-ship phase used by aim jitter. Increments
    /// each tick and feeds into a deterministic hash so the jitter
    /// looks varied without an RNG dependency.
    pub jitter_phase: u32,
    /// Time (seconds) accumulated since this ship's last
    /// "turn-and-burst" moment (Spathi). Drives the periodic flip.
    pub burst_timer_s: f32,
    /// True if the Spathi is currently in the brief turn-around
    /// portion of run-and-burst (facing target to fire primary).
    pub bursting: bool,
    /// In **Inertial** angular mode `ang_vel` only resets on key-press
    /// / key-release transitions, so an AI that isn't actively turning
    /// would tumble forever after a collision (no damping). To brake
    /// in-mode, the AI pulses a turn key: tick N press → tick N+1
    /// release. The release snaps ω to 0. `brake_pulse_on` is the
    /// "we pressed last tick" flag so the cycle alternates.
    pub brake_pulse_on: bool,
}

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource_if_absent::<AiDifficulty>()
            .add_systems(
                FixedUpdate,
                tick_ai_pilots
                    .after(crate::input::SlotInputProducerSet)
                    .before(crate::ship::apply_player_input)
                    .run_if(in_state(crate::AppState::InMatch))
                    // AI only drives ships on the authoritative peer;
                    // the guest receives AI-controlled ships as snapshot
                    // state like any other.
                    .run_if(crate::netcode::role_is_authoritative),
            );
    }
}

/// Init the resource only if no startup code has already set it.
trait InitIfAbsent {
    fn init_resource_if_absent<R: Resource + Default>(&mut self) -> &mut Self;
}
impl InitIfAbsent for App {
    fn init_resource_if_absent<R: Resource + Default>(&mut self) -> &mut Self {
        if !self.world().contains_resource::<R>() {
            self.init_resource::<R>();
        }
        self
    }
}

const PRECEDENCE_CONE: f32 = 0.12; // ~7°
const NARROW_CONE: f32 = 0.40; // ~23°
const HOMING_CONE: f32 = 1.05; // ~60°
const DEFAULT_CONE: f32 = 0.70; // ~40°

const STANDOFF_DISTANCE: f32 = 80.0;
const THRUST_BEARING_TOLERANCE: f32 = std::f32::consts::FRAC_PI_2 * 0.85;
const LOW_CREW_FRACTION_BASE: f32 = 1.0 / 3.0;
const INCOMING_RADIUS_WU: f32 = 280.0;
const INCOMING_DOT: f32 = 0.4;
const ORZ_TURRET_TOLERANCE: f32 = 0.08;

/// Hysteresis upper bound for retreat: once retreating, only re-engage
/// once crew is back above this fraction of max. Most ships can't
/// regen, so this is mostly aspirational ("die running").
const RETREAT_RECOVER_FRACTION: f32 = 0.50;

/// Generic "the projectile we fire goes this fast" used for lead
/// computation. Real ships range 50-150 SC2 = 480-1440 wu/s; the
/// average is ~1100. Hardcoding this for the lead estimate is fine —
/// lead is approximate, not analytic.
const ASSUMED_PROJ_SPEED: f32 = 1100.0;

/// Optimal orbit radius per class, world units. Picked from canon
/// engagement habits: kite classes (Spathi, Druuge, Slylandro) want
/// long range; brawlers (Zfp, Andgu, Shofixti) want point-blank;
/// most ships sit at ~70% of their weapon range for safety.
/// Range-driven mode selection for ships with `ToggleMode` as their
/// special (Mmrnmhrm T↔Y, Androsynth normal↔Blazer, etc.). Returns the
/// mode index the AI WANTS to be in this tick; `tick_ai_pilots`
/// triggers SPECIAL whenever the actual mode differs. `None` = ship
/// has no mode-toggle behaviour the AI cares about.
fn next_state_desired_mode(class: ShipClass, distance: f32, batt: i32) -> Option<usize> {
    match class {
        // Mmrnmhrm: Y-form (mode 1) is fast — use it to close. T-form
        // (mode 0) has the rapid lasers + homing missiles — fight at
        // close range. Threshold ≈ 400 wu (just under generic optimal).
        ShipClass::Mmrxf => Some(if distance > 400.0 { 1 } else { 0 }),
        // Androsynth: Blazer (mode 1) is a high-speed RAM; it has no
        // weapons + drains battery while active. Switch into it only
        // when point-blank AND we've got enough charge to commit to a
        // 1-2 second ramming run. Back to normal (mode 0) when either
        // condition fails.
        ShipClass::Andgu => {
            let want_blazer = distance < 200.0 && batt >= 30;
            Some(if want_blazer { 1 } else { 0 })
        }
        _ => None,
    }
}

fn optimal_range(class: ShipClass) -> f32 {
    match class {
        // Long-range / kite classes.
        ShipClass::Spael => 320.0, // close-ish; BUTT does the rest while harassing
        ShipClass::Druma => 700.0, // recoil cannon best at standoff
        ShipClass::Chmav => 600.0, // tractor's only useful in close, but laser long
        ShipClass::Meltr => 600.0, // charged plasma reaches far
        ShipClass::Slypr => 250.0, // lightning is short-range, but drift handles distance
        ShipClass::Chebr => 550.0, // crystal launcher
        ShipClass::Kohma => 600.0, // saw blades fly forever
        // Brawler / ram classes.
        ShipClass::Shosc => 120.0, // glory device wants point-blank
        ShipClass::Zfpst => 120.0, // tongue is range 2 SC2 = 80 wu
        ShipClass::Umgdr => 100.0, // anti-grav cone is right ahead
        ShipClass::Andgu => 200.0, // Blazer ram works close
        // Midrange default — about 60% of typical weapon range.
        _ => 380.0,
    }
}

fn tick_ai_pilots(
    difficulty: Res<AiDifficulty>,
    angular_override: Res<AngularControlOverride>,
    mut slot_inputs: ResMut<SlotInputs>,
    targets: Query<
        (Entity, &Ship, &Position, &LinearVelocity),
        Without<crate::ultimate::HyperActive>,
    >,
    mut pilots: Query<
        (
            Entity,
            &Ship,
            &ShipClass,
            &Position,
            &Rotation,
            &AngularVelocity,
            &Battery,
            &Crew,
            &mut AiBrain,
            Option<&OrzTurret>,
            Option<&crate::ship::ShipModes>,
        ),
        With<AiControlled>,
    >,
    projectiles: Query<(&Projectile, &Position, &LinearVelocity)>,
    time: Res<Time<Physics>>,
) {
    use std::f32::consts::FRAC_PI_2;
    let tuning = difficulty.tuning();
    let dt = time.delta_secs();
    let inertial_active = matches!(angular_override.0, Some(AngularControl::Inertial));
    for (entity, ship, class, pos, rot, ang_vel, batt, crew, mut brain, turret, modes) in
        &mut pilots
    {
        let slot = ship.player_slot.min(3);
        brain.burst_timer_s += dt;
        brain.jitter_phase = brain.jitter_phase.wrapping_add(1);

        // 1) Pick a target — nearest live non-friendly ship (min-image).
        let Some((target_pos_world, target_vel_world)) =
            nearest_enemy(entity, ship.player_slot, pos.0, &targets)
        else {
            write_input(&mut slot_inputs, slot, &mut brain, 0);
            continue;
        };
        let to_target = crate::physics::min_image(target_pos_world - pos.0);
        let distance = to_target.length().max(1.0);
        let bearing = to_target.y.atan2(to_target.x) - FRAC_PI_2;
        let current_heading = rot.sin.atan2(rot.cos);

        // 2) Retreat hysteresis — drop into retreat at the difficulty's
        //    threshold, only leave it when crew is fully restored
        //    (effectively "never" for ships without regen).
        let crew_frac = (crew.current as f32) / (ship.stats.crew_max.max(1) as f32);
        if brain.retreating {
            if crew_frac >= RETREAT_RECOVER_FRACTION {
                brain.retreating = false;
            }
        } else if crew_frac <= tuning.retreat_threshold {
            brain.retreating = true;
        }

        // 3) Orz takes a fully dedicated control loop (turret tracking,
        //    fire when aligned). Skip the generic path entirely. Note:
        //    the Orz body doesn't ship-rotate while special is held, so
        //    retreat doesn't apply — it's stationary by design.
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

        // 4) Aim leading — predict where the target will be at the
        //    moment our shot arrives, so a moving target gets shot in
        //    front of, not at. Easy disables this so it's beatable by
        //    strafing.
        let lead_pos = if tuning.predict_lead {
            let tof = distance / ASSUMED_PROJ_SPEED;
            target_pos_world + target_vel_world * tof
        } else {
            target_pos_world
        };
        let to_lead = crate::physics::min_image(lead_pos - pos.0);
        let lead_bearing = to_lead.y.atan2(to_lead.x) - FRAC_PI_2;
        // Add aim jitter — deterministic per-ship-and-tick wobble so
        // Easy AI misses on purpose without an RNG dependency.
        let jitter = jitter_angle(brain.jitter_phase, slot) * tuning.aim_jitter_rad;
        let aim_bearing = lead_bearing + jitter;

        // 5) Spathi run-and-burst (canon `shpspael.cpp`):
        //    default = face AWAY (back to target) and run, so BUTT
        //    missile (special) flies backward into the opponent;
        //    every ~3 seconds spin around briefly and fire primary.
        let spathi_run = *class == ShipClass::Spael;
        let spathi_burst_window = 0.7; // seconds spent facing target (more aggressive)
        let spathi_run_window = 1.5; // seconds spent running between bursts
        if spathi_run {
            let cycle = spathi_run_window + spathi_burst_window;
            let phase = brain.burst_timer_s % cycle;
            brain.bursting = phase >= spathi_run_window;
        }

        // 6) Desired heading. Retreat overrides everything except Spathi
        //    (which already faces away by default and just keeps doing so).
        let desired_heading = if brain.retreating || spathi_run && !brain.bursting {
            // Run away — bearing + π.
            wrap_pi(aim_bearing + std::f32::consts::PI)
        } else {
            aim_bearing
        };
        let steer_err = wrap_pi(desired_heading - current_heading);

        // 7) Orbit steering. If at the optimal range, hold; if too far,
        //    close in; if too close, stop thrusting (the natural drift
        //    + opponent movement will open the gap). Retreating ships
        //    just thrust away regardless of distance.
        let opt = optimal_range(*class);
        let want_thrust = if brain.retreating {
            // Always thrust away when retreating.
            steer_err.abs() < THRUST_BEARING_TOLERANCE
        } else if spathi_run && !brain.bursting {
            // Running phase: keep the throttle open so the BUTT
            // missile fires from a moving platform (harder to dodge),
            // but don't flee to the far side of the arena — once we're a
            // bit past optimal, stop running so the next burst re-engages.
            steer_err.abs() < THRUST_BEARING_TOLERANCE && distance < opt * 1.5
        } else if distance > opt {
            // Too far — close in.
            steer_err.abs() < THRUST_BEARING_TOLERANCE && distance > STANDOFF_DISTANCE
        } else {
            // At or inside optimal: don't burn fuel chasing.
            false
        };
        let mut want_left = steer_err > 0.02;
        let mut want_right = steer_err < -0.02;

        // Inertial-mode brake pulse: if the ship is spinning from a
        // collision and the AI isn't actively steering, alternate
        // press → release on a turn key so Inertial's release-snap
        // (ang_vel → 0) actually kills the spin. The press direction
        // matches the spin so the brief +target_omega tick doesn't
        // reverse heading (just decelerates). Skipped in Classic
        // since apply_player_input zeroes ω each tick anyway.
        const BRAKE_THRESHOLD: f32 = 0.6; // rad/s
        if inertial_active && !want_left && !want_right && ang_vel.0.abs() > BRAKE_THRESHOLD {
            if brain.brake_pulse_on {
                // Release tick — no input, snaps ω to 0.
                brain.brake_pulse_on = false;
            } else {
                if ang_vel.0 > 0.0 {
                    want_left = true;
                } else {
                    want_right = true;
                }
                brain.brake_pulse_on = true;
            }
        } else {
            brain.brake_pulse_on = false;
        }


        // 8) Weapon decision. Skip firing while retreating (focus on
        //    escape); Spathi's BUTT missile fires via SPECIAL even
        //    while running, so primary skip during run-phase is fine.
        let weapon_range = ai_weapon_range(ship);
        let in_range = distance <= weapon_range;
        let firing_err = steer_err;
        let want_fire = if brain.retreating {
            false
        } else if spathi_run && !brain.bursting {
            false
        } else {
            // For backward-firing tactics, the firer aims AWAY from the
            // target — the "firing error" is the angle between the
            // ship's rear and the target.
            let rear_err = wrap_pi(firing_err + std::f32::consts::PI);
            match ship.stats.ai.weapon {
                AiWeaponTactic::Precedence => firing_err.abs() < PRECEDENCE_CONE && in_range,
                AiWeaponTactic::Narrow => firing_err.abs() < NARROW_CONE && in_range,
                // `Sides` ships (Pkunk triple-cone) fire whenever the
                // target is anywhere in a generous forward arc — the
                // three barrels handle the lateral spread.
                AiWeaponTactic::Homing | AiWeaponTactic::Launched | AiWeaponTactic::Sides => {
                    firing_err.abs() < HOMING_CONE && in_range
                }
                AiWeaponTactic::Field => true,
                AiWeaponTactic::Mine => firing_err.abs() < HOMING_CONE && in_range,
                AiWeaponTactic::Back => rear_err.abs() < NARROW_CONE && in_range,
                AiWeaponTactic::ReserveBattery => {
                    // Hoard the battery until BattRecharge floor; then
                    // fire like the generic default.
                    batt.current >= ship.stats.ai.batt_floor
                        && firing_err.abs() < DEFAULT_CONE
                        && in_range
                }
                AiWeaponTactic::Default => firing_err.abs() < DEFAULT_CONE && in_range,
            }
        };

        // 9) Special decision. Defense triggers regardless of retreat
        //    (shields/teleports are how you SURVIVE retreating).
        //    Spathi: fires SPECIAL (BUTT missile) continuously while
        //    running, so add a slot-1 "Back-fire" override.
        let special_range = ship
            .stats
            .ai
            .special_range
            .unwrap_or(weapon_range)
            .max(60.0);
        let crew_low = crew_frac <= LOW_CREW_FRACTION_BASE;
        let incoming = incoming_projectile(entity, ship.player_slot, pos.0, &projectiles);
        let drain_ok = batt.current >= ship.stats.special_drain;

        let mut want_special = false;
        if spathi_run && drain_ok && !brain.bursting {
            // While running, hammer the BUTT missile.
            want_special = true;
        }
        // Rear-arc angle of the target relative to ship facing — used
        // by Back tactic and any other "fire from behind" logic.
        let rear_err = wrap_pi(bearing - current_heading + std::f32::consts::PI);
        // Mode-toggle ships (`NextState` tactic): pick the desired form
        // by engagement context, then trigger SPECIAL once whenever the
        // ship's current `ShipModes.current` doesn't match desired.
        // `ToggleMode` in `dispatch_special` is edge-only, so holding the
        // bit through the apply tick still toggles exactly once.
        let desired_mode = next_state_desired_mode(*class, distance, batt.current);
        let mode_mismatch = match (modes, desired_mode) {
            (Some(m), Some(d)) => m.current != d,
            _ => false,
        };
        // `Reserve_Battery` listed in the special chain just GATES the
        // rest of the chain — until batt is above the floor, no special
        // fires. (Different from `Battery` which TRIGGERS at the floor.)
        let reserve_block = ship
            .stats
            .ai
            .specials
            .iter()
            .any(|t| matches!(t, AiSpecialTactic::ReserveBattery))
            && batt.current < ship.stats.ai.batt_floor;
        for &t in &ship.stats.ai.specials {
            if reserve_block {
                continue;
            }
            let trigger = match t {
                AiSpecialTactic::Defense | AiSpecialTactic::Cloak => {
                    drain_ok && (incoming || crew_low)
                }
                AiSpecialTactic::Proximity | AiSpecialTactic::Mine => {
                    drain_ok && distance <= special_range
                }
                AiSpecialTactic::NoProximity => drain_ok && distance > special_range,
                AiSpecialTactic::Battery => batt.current >= ship.stats.ai.batt_floor,
                AiSpecialTactic::Back => {
                    drain_ok && distance <= special_range && rear_err.abs() < NARROW_CONE
                }
                AiSpecialTactic::NextState => mode_mismatch,
                AiSpecialTactic::PlusFire
                | AiSpecialTactic::None
                | AiSpecialTactic::ReserveBattery
                | AiSpecialTactic::Default => false,
            };
            if trigger {
                want_special = true;
                break;
            }
        }
        let freq = ship.stats.ai.special_freq.max(1);
        if want_special {
            brain.special_tick_counter = brain.special_tick_counter.wrapping_add(1);
            // Eagerness scales the frequency gate — Hard fires every
            // 1/(N·1.5) ticks, Easy every 1/(N·0.4). Always 1 minimum.
            let scaled = ((freq as f32) / tuning.special_eagerness).max(1.0) as u32;
            if scaled > 1 && brain.special_tick_counter % scaled != 0 {
                want_special = false;
            }
        }

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

/// Deterministic ±1 pseudo-jitter from (counter, slot). Cheap hash; no
/// RNG dependency so the AI replays identically on rollback.
fn jitter_angle(counter: u32, slot: usize) -> f32 {
    let h = counter
        .wrapping_mul(2654435761)
        .wrapping_add((slot as u32).wrapping_mul(40503));
    // Map to [-1, 1] via sin of a small angle.
    ((h & 0xffff) as f32 / 32768.0 - 1.0)
}

fn nearest_enemy(
    me: Entity,
    me_slot: usize,
    me_pos: Vec2,
    ships: &Query<
        (Entity, &Ship, &Position, &LinearVelocity),
        Without<crate::ultimate::HyperActive>,
    >,
) -> Option<(Vec2, Vec2)> {
    let mut best: Option<(Vec2, Vec2, f32)> = None;
    for (e, s, p, v) in ships {
        if e == me || s.player_slot == me_slot {
            continue;
        }
        let d2 = crate::physics::min_image(p.0 - me_pos).length_squared();
        if best.map_or(true, |(_, _, b)| d2 < b) {
            best = Some((p.0, v.0, d2));
        }
    }
    best.map(|(p, v, _)| (p, v))
}

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
    let desired_offset = wrap_pi(target_bearing - ship_heading);
    let err = wrap_pi(desired_offset - turret.offset_rad);
    let aligned = err.abs() < ORZ_TURRET_TOLERANCE;
    let in_range = distance <= weapon_range;

    let mut bits = 0u8;
    if !aligned {
        bits |= input::INPUT_SPECIAL;
        if err > 0.0 {
            bits |= input::INPUT_LEFT;
        } else {
            bits |= input::INPUT_RIGHT;
        }
    } else if in_range {
        bits |= input::INPUT_FIRE;
    }
    if distance > STANDOFF_DISTANCE {
        bits |= input::INPUT_THRUST;
    }
    bits
}

fn ai_weapon_range(ship: &Ship) -> f32 {
    if let Some(r) = ship.stats.ai.weapon_range {
        return r;
    }
    if ship.stats.weapon_range > 0.0 {
        return ship.stats.weapon_range * SC2_RANGE_SCALE;
    }
    800.0
}

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

fn write_input(slot_inputs: &mut SlotInputs, slot: usize, brain: &mut AiBrain, bits: u8) {
    let pressed = bits & !brain.last_buttons;
    let released = !bits & brain.last_buttons;
    // `apply_player_input` reads the analog `turn` field, not the LEFT /
    // RIGHT bits directly (the keyboard reader in `input::read_local_input`
    // mirrors them onto `turn = ±100` for the same reason — touch-stick
    // analog needed a single source of steering truth). The AI was
    // sending bits but turn: 0, so apply_player_input saw "no turn
    // intent" and the ships flew straight no matter what the AI did.
    let turn: i8 = if bits & input::INPUT_LEFT != 0 {
        100
    } else if bits & input::INPUT_RIGHT != 0 {
        -100
    } else {
        0
    };
    // AI never votes for a rematch — leave `flags = 0`.
    slot_inputs.held[slot] = PlayerInput {
        buttons: bits,
        turn,
        aim_x: 0,
        aim_y: 0,
        flags: 0,
        // AI doesn't participate in the lobby — keep class at 0.
        // The lobby system only looks at human slots' votes.
        class: 0,
    };
    slot_inputs.just_pressed[slot] = PlayerInput {
        buttons: pressed,
        turn: 0,
        aim_x: 0,
        aim_y: 0,
        flags: 0,
        class: 0,
    };
    slot_inputs.just_released[slot] = PlayerInput {
        buttons: released,
        turn: 0,
        aim_x: 0,
        aim_y: 0,
        flags: 0,
        class: 0,
    };
    brain.last_buttons = bits;
}
