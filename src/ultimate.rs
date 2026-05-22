//! "Ultimate" cinematic — press Space and the camera dollies in on
//! P1, a semi-transparent captain portrait fades over the screen,
//! and the Arilou's auto-aim laser becomes a fat sweeping blade as
//! the ship spins furiously, with a fading motion-blur trail.
//!
//! Pure flavour — *nothing* canonical. Documented as such; gated
//! behind a single key press so the regular match flow is untouched.
//! Easy to revert: delete this module and remove its `Plugin` from
//! main.rs.
//!
//! Currently hard-coded to Arilou + P1. Easy to generalise: read
//! the current P1 class and dispatch a different "hyper combo"
//! cosmetic per ship.

use avian2d::prelude::*;
use bevy::prelude::*;

use crate::ship::{Beam, Ship, ShipClass};
use crate::starfield::ZoomState;

/// Sequencer state for the cinematic. `Idle` outside of an
/// ultimate; the other variants advance on a real-time timer.
#[derive(Resource, Debug)]
pub struct UltimateState {
    pub phase: UltimatePhase,
    pub phase_timer_s: f32,
    /// Player slot the cinematic is following.
    pub player_slot: usize,
    pub player_entity: Option<Entity>,
    /// Camera target Transform at the start of `ZoomingIn` — we
    /// tween back to this in `ZoomingOut`.
    pub orig_cam_pos: Vec3,
    pub orig_target_scale: f32,
    /// Spawned portrait entity, despawned on exit.
    pub portrait_entity: Option<Entity>,
}

impl Default for UltimateState {
    fn default() -> Self {
        Self {
            phase: UltimatePhase::Idle,
            phase_timer_s: 0.0,
            player_slot: 0,
            player_entity: None,
            orig_cam_pos: Vec3::ZERO,
            orig_target_scale: 1.0,
            portrait_entity: None,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum UltimatePhase {
    #[default]
    Idle,
    /// Camera dollying in, portrait fading in.
    ZoomingIn,
    /// Spin + thick laser + trails.
    Unleashing,
    /// Camera dollying out, portrait fading out.
    ZoomingOut,
}

/// Marker on a ship that's currently mid-ultimate. While present:
///   - `apply_player_input` skips its angular-velocity update so we
///     can lock the spin rate from this module.
///   - `tick_beams` multiplies the beam's visible width by
///     `beam_width_mult` (collider stays the same — this is purely
///     a visual fattening).
#[derive(Component, Debug)]
pub struct HyperActive {
    pub forced_ang_vel: f32,
    pub beam_width_mult: f32,
}

/// Marker on the on-screen portrait sprite.
#[derive(Component)]
pub struct UltimatePortrait;

/// A fading copy of a beam's quad. `tick_beam_trails` decrements
/// `remaining_s` and adjusts the Sprite alpha; despawns at 0.
#[derive(Component, Debug)]
pub struct BeamTrail {
    pub remaining_s: f32,
    pub total_s: f32,
}

pub struct UltimatePlugin;

impl Plugin for UltimatePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UltimateState>().add_systems(
            Update,
            (
                hyper_trigger,
                tick_ultimate_phases,
                camera_follow_during_ultimate,
                spawn_beam_trails,
                tick_beam_trails,
            ),
        );
    }
}

const PHASE_ZOOM_IN_S: f32 = 0.35;
const PHASE_UNLEASH_S: f32 = 2.5;
const PHASE_ZOOM_OUT_S: f32 = 0.4;
const HYPER_ZOOM_SCALE: f32 = 0.45; // strongly zoomed in
const HYPER_SPIN_RAD_PER_S: f32 = 40.0; // ~6.4 rev/s
const HYPER_BEAM_WIDTH_MULT: f32 = 6.0; // thick "blade" laser
const PORTRAIT_KEY: KeyCode = KeyCode::Space;

fn hyper_trigger(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<UltimateState>,
    cameras: Query<&Transform, With<Camera2d>>,
    zoom_state: Res<ZoomState>,
    ships: Query<(Entity, &Ship, &ShipClass)>,
    mut commands: Commands,
    assets: Res<AssetServer>,
) {
    if state.phase != UltimatePhase::Idle {
        return;
    }
    if !keys.just_pressed(PORTRAIT_KEY) {
        return;
    }

    let Some((entity, ship, class)) =
        ships.iter().find(|(_, s, _)| s.player_slot == 0)
    else {
        return;
    };
    let Ok(cam_xf) = cameras.single() else {
        return;
    };

    state.player_slot = ship.player_slot;
    state.player_entity = Some(entity);
    state.orig_cam_pos = cam_xf.translation;
    state.orig_target_scale = zoom_state.target_scale;
    state.phase = UltimatePhase::ZoomingIn;
    state.phase_timer_s = 0.0;

    // Spawn the portrait — world-space Sprite anchored to the camera
    // (camera_follow system below moves it each frame). Starts at
    // alpha 0; ZoomingIn ramps it to 1.
    let portrait_path = format!("ships/{}/sprites/ship_p00.png", class.code());
    let portrait = commands
        .spawn((
            UltimatePortrait,
            Sprite {
                image: assets.load(portrait_path),
                color: Color::srgba(1.0, 1.0, 1.0, 0.0),
                // Big — captain portrait is ~64×100 px; scale 4× so
                // it dominates the foreground without filling it.
                custom_size: Some(Vec2::new(256.0, 400.0)),
                ..default()
            },
            Transform::from_translation(Vec3::new(0.0, 0.0, 50.0)),
        ))
        .id();
    state.portrait_entity = Some(portrait);

    info!("ULTIMATE: P{} {}", ship.player_slot + 1, class.code());
}

/// Drive the camera + portrait + ship's angular-velocity through the
/// phase machine. Uses `Time<Real>` so the cinematic plays at wall-
/// clock speed even if the rest of the game is time-scaled.
fn tick_ultimate_phases(
    time: Res<Time>,
    mut state: ResMut<UltimateState>,
    mut zoom_state: ResMut<ZoomState>,
    mut portraits: Query<&mut Sprite, With<UltimatePortrait>>,
    mut ships: Query<&mut AngularVelocity, With<Ship>>,
    mut commands: Commands,
) {
    if state.phase == UltimatePhase::Idle {
        return;
    }
    let dt = time.delta_secs();
    state.phase_timer_s += dt;

    let Some(p1_entity) = state.player_entity else {
        if let Some(p) = state.portrait_entity.take() {
            commands.entity(p).despawn();
        }
        state.phase = UltimatePhase::Idle;
        return;
    };

    let (phase_total, target_scale, portrait_alpha) = match state.phase {
        UltimatePhase::ZoomingIn => {
            let progress = (state.phase_timer_s / PHASE_ZOOM_IN_S).clamp(0.0, 1.0);
            let s = state.orig_target_scale * (1.0 - progress) + HYPER_ZOOM_SCALE * progress;
            (PHASE_ZOOM_IN_S, s, progress)
        }
        UltimatePhase::Unleashing => (PHASE_UNLEASH_S, HYPER_ZOOM_SCALE, 1.0),
        UltimatePhase::ZoomingOut => {
            let progress = (state.phase_timer_s / PHASE_ZOOM_OUT_S).clamp(0.0, 1.0);
            let s = HYPER_ZOOM_SCALE * (1.0 - progress) + state.orig_target_scale * progress;
            (PHASE_ZOOM_OUT_S, s, 1.0 - progress)
        }
        UltimatePhase::Idle => unreachable!(),
    };

    zoom_state.target_scale = target_scale;

    if let Some(p_entity) = state.portrait_entity {
        if let Ok(mut sprite) = portraits.get_mut(p_entity) {
            let c = sprite.color.to_linear();
            sprite.color = Color::srgba(c.red, c.green, c.blue, portrait_alpha * 0.7);
        }
    }

    // Force the ship to spin. `HyperActive` tells apply_player_input
    // to leave AngularVelocity alone, and tells tick_beams to fatten
    // the beam quad.
    if let Ok(mut av) = ships.get_mut(p1_entity) {
        let (spin, width_mult) = match state.phase {
            UltimatePhase::ZoomingIn => {
                let p = (state.phase_timer_s / PHASE_ZOOM_IN_S).clamp(0.0, 1.0);
                (HYPER_SPIN_RAD_PER_S * p, HYPER_BEAM_WIDTH_MULT * p.max(0.2))
            }
            UltimatePhase::Unleashing => (HYPER_SPIN_RAD_PER_S, HYPER_BEAM_WIDTH_MULT),
            UltimatePhase::ZoomingOut => {
                let p = (state.phase_timer_s / PHASE_ZOOM_OUT_S).clamp(0.0, 1.0);
                (
                    HYPER_SPIN_RAD_PER_S * (1.0 - p),
                    HYPER_BEAM_WIDTH_MULT * (1.0 - p).max(0.2),
                )
            }
            UltimatePhase::Idle => (0.0, 1.0),
        };
        av.0 = spin;
        commands.entity(p1_entity).insert(HyperActive {
            forced_ang_vel: spin,
            beam_width_mult: width_mult,
        });
    }

    if state.phase_timer_s >= phase_total {
        state.phase_timer_s = 0.0;
        state.phase = match state.phase {
            UltimatePhase::ZoomingIn => UltimatePhase::Unleashing,
            UltimatePhase::Unleashing => UltimatePhase::ZoomingOut,
            UltimatePhase::ZoomingOut => {
                zoom_state.target_scale = state.orig_target_scale;
                if let Ok(mut av) = ships.get_mut(p1_entity) {
                    av.0 = 0.0;
                }
                commands.entity(p1_entity).remove::<HyperActive>();
                if let Some(p) = state.portrait_entity.take() {
                    commands.entity(p).despawn();
                }
                state.player_entity = None;
                UltimatePhase::Idle
            }
            UltimatePhase::Idle => UltimatePhase::Idle,
        };
    }
}

/// During the Unleashing phase, each frame snapshot the active
/// beam's quad and spawn a fading copy as a `BeamTrail`. Creates
/// the "blade leaving a swirling afterimage" effect.
fn spawn_beam_trails(
    state: Res<UltimateState>,
    mut commands: Commands,
    beams: Query<(&Beam, &Transform, &Sprite)>,
) {
    if state.phase != UltimatePhase::Unleashing {
        return;
    }
    let Some(p1) = state.player_entity else {
        return;
    };
    for (beam, beam_xf, beam_sprite) in &beams {
        if beam.owner != p1 {
            continue;
        }
        // One trail per beam per frame.
        let mut trail_color = beam_sprite.color;
        let lin = trail_color.to_linear();
        trail_color = Color::srgba(lin.red, lin.green, lin.blue, 0.55);
        commands.spawn((
            BeamTrail {
                remaining_s: 0.35,
                total_s: 0.35,
            },
            Sprite {
                image: beam_sprite.image.clone(),
                color: trail_color,
                custom_size: beam_sprite.custom_size,
                ..default()
            },
            *beam_xf,
        ));
    }
}

fn tick_beam_trails(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut BeamTrail, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    for (e, mut trail, mut sprite) in &mut q {
        trail.remaining_s -= dt;
        if trail.remaining_s <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let frac = (trail.remaining_s / trail.total_s).clamp(0.0, 1.0);
        let lin = sprite.color.to_linear();
        sprite.color = Color::srgba(lin.red, lin.green, lin.blue, frac * 0.55);
    }
}

/// Camera follow + portrait positioning runs separately so we can
/// query the player ship's `Position` without conflicting with
/// the phase system's `AngularVelocity` write. Runs every frame.
pub fn camera_follow_during_ultimate(
    state: Res<UltimateState>,
    ships: Query<&Position, With<Ship>>,
    mut cameras: Query<&mut Transform, (With<Camera2d>, Without<UltimatePortrait>)>,
    mut portraits: Query<&mut Transform, (With<UltimatePortrait>, Without<Camera2d>)>,
) {
    if state.phase == UltimatePhase::Idle {
        return;
    }
    let Some(p1) = state.player_entity else {
        return;
    };
    let Ok(ship_pos) = ships.get(p1) else {
        return;
    };

    // Tween camera toward ship pos. Phase-dependent blend.
    let blend = match state.phase {
        UltimatePhase::ZoomingIn => (state.phase_timer_s / PHASE_ZOOM_IN_S).clamp(0.0, 1.0),
        UltimatePhase::Unleashing => 1.0,
        UltimatePhase::ZoomingOut => {
            1.0 - (state.phase_timer_s / PHASE_ZOOM_OUT_S).clamp(0.0, 1.0)
        }
        UltimatePhase::Idle => return,
    };
    if let Ok(mut cam_xf) = cameras.single_mut() {
        let target = ship_pos.0.extend(cam_xf.translation.z);
        let start = state.orig_cam_pos;
        cam_xf.translation = start.lerp(target, blend);

        // Portrait follows the camera (lower-right area of the
        // screen) — we use world coords with a screen-space-ish
        // offset that scales with camera scale so it looks the
        // same regardless of zoom. The zoom_state is the source of
        // truth for current scale.
        if let Ok(mut portrait_xf) = portraits.single_mut() {
            let offset = Vec3::new(180.0, -100.0, 50.0);
            portrait_xf.translation = cam_xf.translation + offset;
        }
    }
}
