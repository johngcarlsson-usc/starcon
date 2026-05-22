//! "Ultimate" cinematic — press Space and the game pauses, the
//! camera dollies onto P1, a soft-edged captain portrait fades in,
//! a vocal sample fires, and the Arilou spins furiously while a
//! thick lightsaber-style beam sweeps around it with a luminescent
//! smoke trail. Heavy per-frame damage to anything in the blade's path.
//!
//! Architecture
//! ------------
//! While the cinematic is active we pause `Time<Virtual>`, which
//! halts FixedUpdate (so every other ship/projectile system freezes).
//! Everything in this module uses `Time<Real>` so it keeps running.
//! The Arilou's `Rotation` is advanced manually each Update frame
//! because Avian integration is paused along with FixedUpdate.
//!
//! The beam isn't routed through the regular `Beam` / `tick_beams`
//! systems (those are frozen with the rest of FixedUpdate). Instead
//! we maintain a small set of layered beam-sprite entities
//! (`UltimateBeam`) and refresh their pose + colour each frame, plus
//! spawn `BeamTrail` ghost copies for the luminescent smear.
//!
//! Portrait edges are softened by a custom `PortraitMaterial`
//! (assets/shaders/portrait_fade.wgsl) — a radial alpha falloff so
//! the rectangular sprite doesn't show its corners.

use avian2d::prelude::*;
use bevy::asset::uuid_handle;
use bevy::math::Dir2;
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

use crate::ship::{Crew, Ship, ShipClass};
use crate::starfield::ZoomState;

// ----------------------------------------------------------------
// Custom material — radial alpha fade portrait.
// ----------------------------------------------------------------

/// Renders the captain portrait with a soft radial alpha falloff so
/// the sprite's rectangular edges fade to transparent instead of
/// showing a hard square. `params.x` is a global alpha multiplier
/// driven from the cinematic's fade-in/out timer.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct PortraitMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub image: Handle<Image>,
    #[uniform(2)]
    pub params: Vec4,
}

impl Material2d for PortraitMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/portrait_fade.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

// ----------------------------------------------------------------
// Sequencer state + components.
// ----------------------------------------------------------------

#[derive(Resource, Debug, Default)]
pub struct UltimateState {
    pub phase: UltimatePhase,
    pub phase_timer_s: f32,
    pub player_entity: Option<Entity>,
    pub orig_cam_pos: Vec3,
    pub orig_cam_scale: f32,
    pub portrait_entity: Option<Entity>,
    pub portrait_material: Option<Handle<PortraitMaterial>>,
    pub was_paused: bool,
    /// Blade world-angle on the previous Update tick. We interpolate
    /// between this and the current angle when emitting trails, so
    /// the smear is continuous instead of stepped at 60 fps.
    pub last_blade_angle: Option<f32>,
    /// Per-layer `ColorMaterial` handles — [core, mid, halo]. We
    /// update each material's alpha each frame in `tick_ultimate_beams`
    /// so the layered blade fades in/out cleanly. Cleared on exit.
    pub beam_materials: Vec<Handle<bevy::sprite_render::ColorMaterial>>,
    /// Spawned beam-entity ids — despawned on exit so repeated
    /// triggers don't leak entities.
    pub beam_entities: Vec<Entity>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum UltimatePhase {
    #[default]
    Idle,
    /// Time is paused. Camera rams toward the ship, portrait fades
    /// in. No spin, no blade — the held breath before the move.
    DramaticZoomIn,
    /// Time resumes. Camera pulls back to its original framing
    /// *while* the ship spins and the blade slashes — the zoom-out
    /// itself is the punch of the move.
    Unleashing,
}

/// Marker on the spinning ship while the cinematic is active.
/// `apply_player_input` reads this and stops touching the ship's
/// AngularVelocity. `tick_beams` reads this and fattens the regular
/// beam visual (used outside the cinematic — during it physics is
/// paused so this is mostly cosmetic).
#[derive(Component, Debug)]
pub struct HyperActive {
    pub forced_ang_vel: f32,
    pub beam_width_mult: f32,
}

/// One of the layered beam sprites that make up the lightsaber:
/// 0 = inner white core, 1 = mid cyan glow, 2 = outer halo.
#[derive(Component, Debug)]
pub struct UltimateBeam {
    pub layer: u8,
}

/// Fading copy of an `UltimateBeam` triangle — spawned each Update
/// during `Unleashing` so the blade trails a glowing smoke smear.
/// Stores its own `ColorMaterial` handle so the alpha can be
/// animated per-trail. Width grows over the lifetime (smoke billows
/// outward) while alpha falls. `drift_vel` pushes the ghost off the
/// blade so the smear scatters chaotically rather than sitting in
/// place — the "fiery" wisp effect.
#[derive(Component, Debug)]
pub struct BeamTrail {
    pub remaining_s: f32,
    pub total_s: f32,
    pub peak_alpha: f32,
    pub base_color: Color,
    pub start_width: f32,
    pub start_length: f32,
    pub material: Handle<bevy::sprite_render::ColorMaterial>,
    /// World-units / second the trail position drifts at. Mostly
    /// tangential to the blade so wisps "fling off" sideways.
    pub drift_vel: Vec2,
    /// Width grows by `1 + width_growth * age` over the lifetime.
    /// Randomised per-trail for chaotic flickering.
    pub width_growth: f32,
}

#[derive(Component)]
pub struct UltimatePortraitTag;

// ----------------------------------------------------------------
// Plugin.
// ----------------------------------------------------------------

pub struct UltimatePlugin;

impl Plugin for UltimatePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Material2dPlugin::<PortraitMaterial>::default())
            .init_resource::<UltimateState>()
            .add_systems(
                Update,
                (
                    hyper_trigger,
                    tick_ultimate_phases,
                    drive_camera_during_ultimate,
                    drive_ship_rotation_during_ultimate,
                    tick_ultimate_beams,
                    tick_beam_trails,
                )
                    .chain(),
            );
    }
}

// ----------------------------------------------------------------
// Tuning.
// ----------------------------------------------------------------

/// Fast ram-in while the world is paused — the dramatic pause
/// before the punch. Long enough to read the portrait and feel
/// the held breath.
const PHASE_ZOOM_IN_S: f32 = 0.45;
/// Time resumes for this phase. Camera pulls back from full close-
/// up to the original framing across this duration *while* the
/// blade slashes — the zoom-out itself sells the move.
const PHASE_UNLEASH_S: f32 = 1.6;
/// How far the camera rams in during the pause. Smaller = more
/// dramatic close-up (was 0.45; 0.18 fills the screen with the
/// ship).
const HYPER_CAM_SCALE: f32 = 0.20;
const HYPER_SPIN_RAD_PER_S: f32 = 14.0;
const HYPER_BEAM_LEN: f32 = 380.0;
/// Per-second crew damage applied to anything the blade is currently
/// touching. Scaled by `dt` each tick. ~600 dmg/sec — even high-
/// crew ships die in a couple frames of contact, and since the
/// Unleashing phase is only ~1.6 s the total damage is bounded.
const HYPER_DAMAGE_PER_SEC: f32 = 600.0;
const PORTRAIT_KEY: KeyCode = KeyCode::Space;
const PORTRAIT_PATH: &str = "ultimate/portrait_arisk.png";
const VOICE_PATH: &str = "ultimate/arisk_voi.wav";

/// Shared 1×1 quad mesh used by the portrait. Width/height come from
/// `Transform.scale`.
const QUAD_MESH_HANDLE: Handle<Mesh> = uuid_handle!("ec3a8f1e-7e8b-4f1b-9b3c-43c2e7d39001");
/// Shared isosceles-triangle mesh used by the blade + trails. Apex at
/// local (0, 0), base at local (±0.5, 1). Width comes from
/// `Transform.scale.x`, length from `Transform.scale.y` — so the
/// blade renders as a cone whose tip is wider than its origin.
const BLADE_MESH_HANDLE: Handle<Mesh> = uuid_handle!("ec3a8f1e-7e8b-4f1b-9b3c-43c2e7d39002");

// ----------------------------------------------------------------
// Trigger.
// ----------------------------------------------------------------

fn hyper_trigger(
    keys: Res<ButtonInput<KeyCode>>,
    touch_virt: Res<crate::input::VirtualInput>,
    mut state: ResMut<UltimateState>,
    cameras: Query<(&Transform, &Projection), With<Camera2d>>,
    ships: Query<(Entity, &Ship, &ShipClass)>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PortraitMaterial>>,
    mut color_mats: ResMut<Assets<bevy::sprite_render::ColorMaterial>>,
    assets: Res<AssetServer>,
    mut virt: ResMut<Time<Virtual>>,
) {
    if state.phase != UltimatePhase::Idle {
        return;
    }
    if !keys.just_pressed(PORTRAIT_KEY) && !touch_virt.ultimate_just_pressed {
        return;
    }
    let Some((entity, ship, class)) =
        ships.iter().find(|(_, s, _)| s.player_slot == 0)
    else {
        return;
    };
    let Ok((cam_xf, projection)) = cameras.single() else {
        return;
    };

    state.player_entity = Some(entity);
    state.orig_cam_pos = cam_xf.translation;
    state.orig_cam_scale = match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    };
    state.phase = UltimatePhase::DramaticZoomIn;
    state.phase_timer_s = 0.0;

    if meshes.get(&QUAD_MESH_HANDLE).is_none() {
        let _ = meshes.insert(&QUAD_MESH_HANDLE, Rectangle::new(1.0, 1.0).into());
    }
    if meshes.get(&BLADE_MESH_HANDLE).is_none() {
        // Apex at local (0, 0) → at the ship. Base at (±0.5, 1) →
        // expands outward to the tip. `Transform.scale = (width,
        // length, 1)` shapes it into the desired cone.
        let tri = bevy::math::primitives::Triangle2d::new(
            Vec2::new(0.0, 0.0),
            Vec2::new(-0.5, 1.0),
            Vec2::new(0.5, 1.0),
        );
        let _ = meshes.insert(&BLADE_MESH_HANDLE, tri.into());
    }

    // Portrait: Mesh2d + custom material with radial alpha fade.
    let material = materials.add(PortraitMaterial {
        image: assets.load(PORTRAIT_PATH),
        params: Vec4::new(0.0, 0.0, 0.0, 0.0),
    });
    let portrait = commands
        .spawn((
            UltimatePortraitTag,
            Mesh2d(QUAD_MESH_HANDLE.clone()),
            MeshMaterial2d(material.clone()),
            Transform::from_scale(Vec3::new(620.0, 930.0, 1.0))
                .with_translation(Vec3::new(0.0, 0.0, 60.0)),
        ))
        .id();
    state.portrait_entity = Some(portrait);
    state.portrait_material = Some(material);

    // Three layered beam triangles — core / mid / halo. Each layer
    // gets its own ColorMaterial so we can fade alpha independently
    // each frame. Hidden until the phase progresses to Unleashing.
    state.beam_materials.clear();
    state.beam_entities.clear();
    for layer in 0u8..3 {
        let (_w, color, z) = beam_layer_pose(layer, 0.0);
        let mat_handle =
            color_mats.add(bevy::sprite_render::ColorMaterial::from_color(color));
        let id = commands
            .spawn((
                UltimateBeam { layer },
                Mesh2d(BLADE_MESH_HANDLE.clone()),
                MeshMaterial2d(mat_handle.clone()),
                Transform::from_translation(Vec3::new(0.0, 0.0, z)),
                Visibility::Hidden,
            ))
            .id();
        state.beam_materials.push(mat_handle);
        state.beam_entities.push(id);
    }

    // Pause everything else.
    virt.pause();
    state.was_paused = true;

    // Vocal sample. `PlaybackSettings::DESPAWN` removes the AudioPlayer
    // entity when the clip finishes so we don't accumulate.
    commands.spawn((
        AudioPlayer::<AudioSource>(assets.load(VOICE_PATH)),
        PlaybackSettings::DESPAWN,
    ));

    info!("ULTIMATE: P{} {}", ship.player_slot + 1, class.code());
}

// ----------------------------------------------------------------
// Per-phase tweens.
// ----------------------------------------------------------------

fn tick_ultimate_phases(
    time: Res<Time<Real>>,
    mut state: ResMut<UltimateState>,
    mut commands: Commands,
    mut materials: ResMut<Assets<PortraitMaterial>>,
    mut zoom_state: ResMut<ZoomState>,
    mut virt: ResMut<Time<Virtual>>,
    mut beams: Query<&mut Visibility, With<UltimateBeam>>,
    mut ships: Query<&mut AngularVelocity, With<Ship>>,
) {
    if state.phase == UltimatePhase::Idle {
        return;
    }
    let dt = time.delta_secs();
    state.phase_timer_s += dt;

    let Some(p1) = state.player_entity else {
        exit_cinematic(&mut state, &mut commands, &mut virt, &mut zoom_state);
        return;
    };

    // Per-phase derived values:
    //   `phase_total`        — duration of the current phase
    //   `portrait_alpha`     — 0..1, fade target this tick
    //   `beam_visible`       — whether the blade should be rendering
    //   `spin`               — angular velocity to lock the ship at
    //   `should_be_paused`   — Time<Virtual> pause state
    let (phase_total, portrait_alpha, beam_visible, spin, should_be_paused) =
        match state.phase {
            UltimatePhase::DramaticZoomIn => {
                let p = (state.phase_timer_s / PHASE_ZOOM_IN_S).clamp(0.0, 1.0);
                // Ease-out so the portrait snaps in fast then settles.
                let eased = 1.0 - (1.0 - p).powi(3);
                (PHASE_ZOOM_IN_S, eased, false, 0.0, true)
            }
            UltimatePhase::Unleashing => {
                let p = (state.phase_timer_s / PHASE_UNLEASH_S).clamp(0.0, 1.0);
                // Portrait fades out over the first 60% of the zoom-
                // out so the player's eye returns to the action.
                let portrait_p = (p / 0.6).clamp(0.0, 1.0);
                let portrait = 1.0 - portrait_p;
                (PHASE_UNLEASH_S, portrait, true, HYPER_SPIN_RAD_PER_S, false)
            }
            UltimatePhase::Idle => unreachable!(),
        };

    // Unpause Time<Virtual> the first tick of Unleashing.
    if !should_be_paused && state.was_paused {
        virt.unpause();
        state.was_paused = false;
    }

    if let Some(mat_handle) = state.portrait_material.clone() {
        if let Some(mat) = materials.get_mut(&mat_handle) {
            mat.params.x = portrait_alpha * 0.92;
        }
    }

    let target_vis = if beam_visible {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut v in &mut beams {
        if *v != target_vis {
            *v = target_vis;
        }
    }

    if let Ok(mut av) = ships.get_mut(p1) {
        av.0 = spin;
    }
    commands.entity(p1).insert(HyperActive {
        forced_ang_vel: spin,
        beam_width_mult: if beam_visible { 1.0 } else { 0.0 },
    });

    if state.phase_timer_s >= phase_total {
        state.phase_timer_s = 0.0;
        state.phase = match state.phase {
            UltimatePhase::DramaticZoomIn => UltimatePhase::Unleashing,
            UltimatePhase::Unleashing => {
                exit_cinematic(&mut state, &mut commands, &mut virt, &mut zoom_state);
                return;
            }
            UltimatePhase::Idle => UltimatePhase::Idle,
        };
    }
}

fn exit_cinematic(
    state: &mut UltimateState,
    commands: &mut Commands,
    virt: &mut Time<Virtual>,
    zoom_state: &mut ZoomState,
) {
    if state.was_paused {
        virt.unpause();
        state.was_paused = false;
    }
    if let Some(p1) = state.player_entity.take() {
        commands.entity(p1).remove::<HyperActive>();
    }
    if let Some(p) = state.portrait_entity.take() {
        commands.entity(p).despawn();
    }
    for e in state.beam_entities.drain(..) {
        commands.entity(e).despawn();
    }
    state.beam_materials.clear();
    state.portrait_material = None;
    zoom_state.target_scale = state.orig_cam_scale;
    state.phase = UltimatePhase::Idle;
    state.phase_timer_s = 0.0;
}

// ----------------------------------------------------------------
// Camera follow + portrait positioning.
// ----------------------------------------------------------------

fn drive_camera_during_ultimate(
    state: Res<UltimateState>,
    ships: Query<&Position, With<Ship>>,
    mut cameras: Query<
        (&mut Transform, &mut Projection),
        (
            With<Camera2d>,
            Without<UltimatePortraitTag>,
            Without<UltimateBeam>,
            Without<BeamTrail>,
        ),
    >,
    mut portraits: Query<
        &mut Transform,
        (
            With<UltimatePortraitTag>,
            Without<Camera2d>,
            Without<UltimateBeam>,
            Without<BeamTrail>,
        ),
    >,
) {
    if state.phase == UltimatePhase::Idle {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok(ship_pos) = ships.get(p1) else { return };

    let (blend, scale) = match state.phase {
        UltimatePhase::DramaticZoomIn => {
            let p = (state.phase_timer_s / PHASE_ZOOM_IN_S).clamp(0.0, 1.0);
            // Hard ease-out so the camera *snaps* toward the ship —
            // the punch of the dramatic zoom-in.
            let eased = 1.0 - (1.0 - p).powi(4);
            (
                eased,
                state.orig_cam_scale * (1.0 - eased) + HYPER_CAM_SCALE * eased,
            )
        }
        UltimatePhase::Unleashing => {
            let p = (state.phase_timer_s / PHASE_UNLEASH_S).clamp(0.0, 1.0);
            // Ease-in on the zoom-out so the punch holds for a beat
            // then accelerates outward.
            let eased = p * p;
            (
                1.0 - eased,
                HYPER_CAM_SCALE * (1.0 - eased) + state.orig_cam_scale * eased,
            )
        }
        UltimatePhase::Idle => return,
    };

    if let Ok((mut cam_xf, mut projection)) = cameras.single_mut() {
        let target = ship_pos.0.extend(cam_xf.translation.z);
        cam_xf.translation = state.orig_cam_pos.lerp(target, blend);
        if let Projection::Orthographic(ref mut ortho) = *projection {
            ortho.scale = scale;
        }

        if let Ok(mut portrait_xf) = portraits.single_mut() {
            // Anchor portrait to lower-right of camera in world
            // coords; offsets scale with `scale` so the portrait
            // stays in the same place on screen regardless of zoom.
            let off_x = 150.0 * scale;
            let off_y = -60.0 * scale;
            let z = portrait_xf.translation.z;
            portrait_xf.translation = cam_xf.translation + Vec3::new(off_x, off_y, 0.0);
            portrait_xf.translation.z = z;
            portrait_xf.scale = Vec3::new(620.0 * scale, 930.0 * scale, 1.0);
        }
    }
}

// ----------------------------------------------------------------
// Manual rotation advance (physics is paused so AngularVelocity
// doesn't integrate — we write Rotation directly).
// ----------------------------------------------------------------

fn drive_ship_rotation_during_ultimate(
    time: Res<Time<Real>>,
    state: Res<UltimateState>,
    mut rotations: Query<&mut Rotation, With<HyperActive>>,
) {
    // No manual rotation in DramaticZoomIn (ship is frozen with the
    // world). During Unleashing, Time<Virtual> is unpaused so Avian
    // integrates AngularVelocity normally — we don't need to write
    // Rotation by hand any more. This system stays as a safety in
    // case we ever pause again mid-Unleashing.
    if state.phase != UltimatePhase::Unleashing {
        return;
    }
    let dt = time.delta_secs();
    let _ = (dt, &mut rotations);
}

// ----------------------------------------------------------------
// Beam: pose each tick, deal damage, spawn smear trails.
// ----------------------------------------------------------------

fn tick_ultimate_beams(
    time: Res<Time<Real>>,
    mut state: ResMut<UltimateState>,
    spatial: SpatialQuery,
    ships: Query<(&Position, &Rotation), With<Ship>>,
    ship_lookup: Query<&Ship>,
    mut crews: Query<&mut Crew>,
    mut beams: Query<(&UltimateBeam, &mut Transform, &Visibility), Without<BeamTrail>>,
    mut color_mats: ResMut<Assets<bevy::sprite_render::ColorMaterial>>,
    mut commands: Commands,
) {
    if state.phase == UltimatePhase::Idle {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok((pos, rot)) = ships.get(p1) else { return };
    let owner_pos = pos.0;

    // Ship-local "forward" is +Y; rotate by current Rotation to get
    // world direction. Same convention as ship.rs's beam math.
    let cos = rot.cos;
    let sin = rot.sin;
    let world_dir = Vec2::new(-sin, cos);

    // Blade only exists during Unleashing — DramaticZoomIn is the
    // "held breath" before the punch. Ramps up fast at the start of
    // Unleashing and holds until a brief fade in the last 15%.
    let phase_alpha = match state.phase {
        UltimatePhase::Unleashing => {
            let p = (state.phase_timer_s / PHASE_UNLEASH_S).clamp(0.0, 1.0);
            // Snap on over the first 0.08 of the phase; hold; fade
            // out over the last 0.15.
            if p < 0.08 {
                (p / 0.08).clamp(0.0, 1.0)
            } else if p > 0.85 {
                ((1.0 - p) / 0.15).clamp(0.0, 1.0)
            } else {
                1.0
            }
        }
        _ => 0.0,
    };
    if phase_alpha <= 0.001 {
        return;
    }

    // Raycast for damage. Visually the blade always extends
    // HYPER_BEAM_LEN — like a lightsaber poking through whatever
    // it touches.
    let filter = SpatialQueryFilter::default().with_excluded_entities([p1]);
    let dir = Dir2::new(world_dir).unwrap_or(Dir2::X);
    let hit = spatial.cast_ray(owner_pos, dir, HYPER_BEAM_LEN, true, &filter);

    if let Some(ref h) = hit {
        if let (Ok(target_ship), Ok(owner_ship)) =
            (ship_lookup.get(h.entity), ship_lookup.get(p1))
        {
            if target_ship.player_slot != owner_ship.player_slot {
                if let Ok(mut crew) = crews.get_mut(h.entity) {
                    let dmg = (HYPER_DAMAGE_PER_SEC * time.delta_secs()).round() as i32;
                    if dmg > 0 {
                        crew.current = (crew.current - dmg).max(0);
                    }
                }
            }
        }
    }

    let blade_len = HYPER_BEAM_LEN;
    let angle = world_dir.y.atan2(world_dir.x) - std::f32::consts::FRAC_PI_2;

    // Triangle mesh has apex at local (0, 0) and base at
    // (±0.5, 1). Placing it at the ship's position with
    // Transform.scale = (width, length, 1) makes the apex sit at the
    // ship and the base extend to the tip — a cone whose origin is a
    // point and whose tip is `width` units wide.
    for (beam, mut xf, _vis) in &mut beams {
        let (w, color, z) = beam_layer_pose(beam.layer, phase_alpha);
        xf.translation = owner_pos.extend(z);
        xf.rotation = Quat::from_rotation_z(angle);
        xf.scale = Vec3::new(w, blade_len, 1.0);
        if let Some(mat_handle) = state.beam_materials.get(beam.layer as usize) {
            if let Some(mat) = color_mats.get_mut(mat_handle) {
                mat.color = color;
            }
        }
    }

    // Trail emission — interpolate between last frame's blade angle
    // and this frame's so the smear is continuous at any frame rate.
    // Spawn TRAIL_SAMPLES_PER_FRAME * 3 layers of ghost copies (one
    // set per layer), each at a sub-frame interpolated angle. Result:
    // at 60 fps + 42 rad/s the blade moves ~12° per frame; we emit
    // 6 copies across that arc, so the trail looks smoothly swept.
    if state.phase != UltimatePhase::Unleashing {
        // Always record the latest angle even on phases that don't
        // emit, so the next-Unleashing frame doesn't see a stale gap.
        state.last_blade_angle = Some(angle);
        return;
    }

    const TRAIL_SAMPLES_PER_FRAME: usize = 8;
    const TRAIL_LIFETIME_S: f32 = 1.0;
    const TRAIL_PEAK_ALPHA: f32 = 0.95;
    /// Max tangential drift speed (world units/sec) of a trail ghost.
    /// Higher = wisps fling off sideways more aggressively.
    const TRAIL_DRIFT_MAX: f32 = 65.0;

    let last_angle = state.last_blade_angle.unwrap_or(angle);
    // Take the shorter signed sweep between angles so we interpolate
    // along the actual blade motion rather than the long way around.
    let mut sweep = angle - last_angle;
    while sweep > std::f32::consts::PI {
        sweep -= std::f32::consts::TAU;
    }
    while sweep < -std::f32::consts::PI {
        sweep += std::f32::consts::TAU;
    }
    state.last_blade_angle = Some(angle);

    for i in 0..TRAIL_SAMPLES_PER_FRAME {
        // t goes 0..1 (exclusive) — 0 = last frame's pose, 1 would be
        // this frame's pose. We never emit at t=1 because that's the
        // live blade.
        let t = (i as f32 + 0.5) / TRAIL_SAMPLES_PER_FRAME as f32;
        // Add a tiny per-sample angle jitter so even the interpolated
        // ghosts don't sit on the perfect arc — feels less mechanical.
        let angle_jitter = (fastrand::f32() - 0.5) * 0.10;
        let sample_angle = last_angle + sweep * t + angle_jitter;

        // Direction along this ghost's blade, for the tangential
        // drift below.
        let dir_polar = sample_angle + std::f32::consts::FRAC_PI_2;
        let sample_dir = Vec2::new(dir_polar.cos(), dir_polar.sin());
        // Tangent = perpendicular to the blade; flick wisps sideways
        // off the swing.
        let tangent = Vec2::new(-sample_dir.y, sample_dir.x);
        // Mostly tangential, with a small radial outward component
        // — flames lick off and outward.
        let drift_tang = tangent
            * (fastrand::f32() * 2.0 - 1.0)
            * TRAIL_DRIFT_MAX;
        let drift_rad = sample_dir * fastrand::f32() * TRAIL_DRIFT_MAX * 0.4;
        let drift_vel = drift_tang + drift_rad;

        // Spawn all three layers as trails. Earlier interpolated
        // samples (smaller `t`) start with slightly lower alpha so
        // the freshest part of the smear is brightest.
        let freshness = t;
        for layer in 0u8..3 {
            let (w, color, z) = beam_layer_pose(layer, phase_alpha);
            let (life_mult, alpha_mult) = match layer {
                0 => (0.55, 0.75),
                1 => (1.00, 1.00),
                _ => (1.30, 0.85),
            };
            // Per-ghost lifetime jitter (0.65..1.35×) so trails
            // don't all snuff out together — flickering effect.
            let life_jitter = 0.65 + fastrand::f32() * 0.70;
            let lifetime = TRAIL_LIFETIME_S * life_mult * life_jitter;
            // Width / length jitter so the smear has texture rather
            // than reading as uniform slabs.
            let width_jitter = 0.6 + fastrand::f32() * 0.9; // 0.6..1.5
            let length_jitter = 0.8 + fastrand::f32() * 0.4; // 0.8..1.2
            let peak =
                TRAIL_PEAK_ALPHA * phase_alpha * alpha_mult * (0.55 + 0.45 * freshness);
            let lin = color.to_linear();
            // Small per-ghost RGB jitter — hue chaos for the fiery
            // flicker (each wisp a slightly different shade).
            let r_jit = (fastrand::f32() - 0.5) * 0.18;
            let g_jit = (fastrand::f32() - 0.5) * 0.12;
            let b_jit = (fastrand::f32() - 0.5) * 0.10;
            let trail_color = Color::srgba(
                (lin.red + r_jit).clamp(0.0, 1.5),
                (lin.green + g_jit).clamp(0.0, 1.5),
                (lin.blue + b_jit).clamp(0.0, 1.5),
                peak,
            );
            let mat_handle =
                color_mats.add(bevy::sprite_render::ColorMaterial::from_color(trail_color));
            // Per-ghost width growth — some wisps puff out big,
            // others stay tight. Range 1.5..4.0× so the mix of
            // tight cores and big billows gives texture.
            let width_growth = 1.5 + fastrand::f32() * 2.5;
            commands.spawn((
                BeamTrail {
                    remaining_s: lifetime,
                    total_s: lifetime,
                    peak_alpha: peak,
                    base_color: trail_color,
                    start_width: w * width_jitter,
                    start_length: blade_len * length_jitter,
                    material: mat_handle.clone(),
                    drift_vel,
                    width_growth,
                },
                Mesh2d(BLADE_MESH_HANDLE.clone()),
                MeshMaterial2d(mat_handle),
                Transform {
                    translation: owner_pos.extend(z - 0.05 - layer as f32 * 0.01),
                    rotation: Quat::from_rotation_z(sample_angle),
                    scale: Vec3::new(w * width_jitter, blade_len * length_jitter, 1.0),
                },
            ));
        }
    }
}

/// Width / colour / z per beam layer (0 core, 1 mid, 2 halo).
/// `phase_alpha` scales overall brightness; widths are constant so
/// the layering doesn't shimmer.
fn beam_layer_pose(layer: u8, phase_alpha: f32) -> (f32, Color, f32) {
    let pa = phase_alpha.clamp(0.0, 1.0);
    match layer {
        0 => (12.0, Color::srgba(1.0, 1.0, 1.0, pa), 0.35),
        1 => (32.0, Color::srgba(0.6, 0.95, 1.0, 0.85 * pa), 0.32),
        _ => (68.0, Color::srgba(0.25, 0.55, 1.0, 0.45 * pa), 0.30),
    }
}

fn tick_beam_trails(
    time: Res<Time<Real>>,
    mut commands: Commands,
    mut color_mats: ResMut<Assets<bevy::sprite_render::ColorMaterial>>,
    mut q: Query<(Entity, &mut BeamTrail, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut trail, mut xf) in &mut q {
        trail.remaining_s -= dt;
        if trail.remaining_s <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let frac = (trail.remaining_s / trail.total_s).clamp(0.0, 1.0);
        let age = 1.0 - frac;

        // Alpha: gentle ease-out so the trail stays bright through
        // most of its life. Then *flickers* in the last 25% of the
        // lifetime — a low-amplitude sinusoid in `age` adds the
        // "ember sputtering out" feel.
        let mut alpha = trail.peak_alpha * frac.powf(0.6);
        if age > 0.75 {
            let flicker = (age * 60.0).sin() * 0.18;
            alpha = (alpha + alpha * flicker).max(0.0);
        }

        // Width billows outward as the smoke disperses (apex stays
        // at the ship — only the tip widens). Per-trail growth rate
        // so the puffs don't all expand uniformly. Length contracts
        // so the smear becomes a "puff" rather than a line.
        let width = trail.start_width * (1.0 + trail.width_growth * age);
        let length = trail.start_length * (1.0 - 0.30 * age);
        xf.scale = Vec3::new(width, length, 1.0);

        // Drift the ghost off the blade — wisps fling outward as
        // they age, gradually slowing (linear decel).
        let drift_factor = 1.0 - age * 0.6;
        let drift = trail.drift_vel * dt * drift_factor.max(0.0);
        xf.translation.x += drift.x;
        xf.translation.y += drift.y;

        if let Some(mat) = color_mats.get_mut(&trail.material) {
            let lin = trail.base_color.to_linear();
            mat.color = Color::srgba(lin.red, lin.green, lin.blue, alpha);
        }
    }
}
