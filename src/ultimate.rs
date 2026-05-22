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
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum UltimatePhase {
    #[default]
    Idle,
    ZoomingIn,
    Unleashing,
    ZoomingOut,
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

/// Fading copy of an `UltimateBeam` quad — spawned each Update during
/// `Unleashing` so the blade trails a glowing smoke smear.
#[derive(Component, Debug)]
pub struct BeamTrail {
    pub remaining_s: f32,
    pub total_s: f32,
    pub peak_alpha: f32,
    pub base_color: Color,
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

const PHASE_ZOOM_IN_S: f32 = 0.45;
const PHASE_UNLEASH_S: f32 = 2.8;
const PHASE_ZOOM_OUT_S: f32 = 0.5;
const HYPER_CAM_SCALE: f32 = 0.45;
const HYPER_SPIN_RAD_PER_S: f32 = 36.0;
const HYPER_BEAM_LEN: f32 = 380.0;
/// Per-second crew damage applied to anything the blade is currently
/// touching. Scaled by `dt` each tick. ~480 dmg/sec means even high-
/// crew ships die in well under a second of contact.
const HYPER_DAMAGE_PER_SEC: f32 = 480.0;
const PORTRAIT_KEY: KeyCode = KeyCode::Space;
const PORTRAIT_PATH: &str = "ultimate/portrait_arisk.png";
const VOICE_PATH: &str = "ultimate/arisk_voi.wav";

/// Shared 1×1 quad mesh used by the portrait. Width/height come from
/// `Transform.scale`.
const QUAD_MESH_HANDLE: Handle<Mesh> = uuid_handle!("ec3a8f1e-7e8b-4f1b-9b3c-43c2e7d39001");

// ----------------------------------------------------------------
// Trigger.
// ----------------------------------------------------------------

fn hyper_trigger(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<UltimateState>,
    cameras: Query<(&Transform, &Projection), With<Camera2d>>,
    ships: Query<(Entity, &Ship, &ShipClass)>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PortraitMaterial>>,
    assets: Res<AssetServer>,
    mut virt: ResMut<Time<Virtual>>,
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
    let Ok((cam_xf, projection)) = cameras.single() else {
        return;
    };

    state.player_entity = Some(entity);
    state.orig_cam_pos = cam_xf.translation;
    state.orig_cam_scale = match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    };
    state.phase = UltimatePhase::ZoomingIn;
    state.phase_timer_s = 0.0;

    if meshes.get(&QUAD_MESH_HANDLE).is_none() {
        let _ = meshes.insert(&QUAD_MESH_HANDLE, Rectangle::new(1.0, 1.0).into());
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
            Transform::from_scale(Vec3::new(380.0, 570.0, 1.0))
                .with_translation(Vec3::new(0.0, 0.0, 60.0)),
        ))
        .id();
    state.portrait_entity = Some(portrait);
    state.portrait_material = Some(material);

    // Three layered beam sprites — core / mid / halo. Hidden until
    // phase progresses past the first ramp.
    for layer in 0u8..3 {
        let (_w, color, z) = beam_layer_pose(layer, 0.0);
        commands.spawn((
            UltimateBeam { layer },
            Sprite::from_color(color, Vec2::splat(1.0)),
            Transform::from_translation(Vec3::new(0.0, 0.0, z)),
            Visibility::Hidden,
        ));
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

    let (phase_total, alpha, beam_visible) = match state.phase {
        UltimatePhase::ZoomingIn => {
            let p = (state.phase_timer_s / PHASE_ZOOM_IN_S).clamp(0.0, 1.0);
            (PHASE_ZOOM_IN_S, p, p > 0.35)
        }
        UltimatePhase::Unleashing => (PHASE_UNLEASH_S, 1.0, true),
        UltimatePhase::ZoomingOut => {
            let p = (state.phase_timer_s / PHASE_ZOOM_OUT_S).clamp(0.0, 1.0);
            (PHASE_ZOOM_OUT_S, 1.0 - p, p < 0.7)
        }
        UltimatePhase::Idle => unreachable!(),
    };

    if let Some(mat_handle) = state.portrait_material.clone() {
        if let Some(mat) = materials.get_mut(&mat_handle) {
            mat.params.x = alpha * 0.92;
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

    let spin = match state.phase {
        UltimatePhase::ZoomingIn => {
            let p = (state.phase_timer_s / PHASE_ZOOM_IN_S).clamp(0.0, 1.0);
            HYPER_SPIN_RAD_PER_S * p
        }
        UltimatePhase::Unleashing => HYPER_SPIN_RAD_PER_S,
        UltimatePhase::ZoomingOut => {
            let p = (state.phase_timer_s / PHASE_ZOOM_OUT_S).clamp(0.0, 1.0);
            HYPER_SPIN_RAD_PER_S * (1.0 - p)
        }
        UltimatePhase::Idle => 0.0,
    };
    if let Ok(mut av) = ships.get_mut(p1) {
        av.0 = spin;
    }
    let width_mult = match state.phase {
        UltimatePhase::ZoomingIn => {
            (state.phase_timer_s / PHASE_ZOOM_IN_S).clamp(0.0, 1.0)
        }
        UltimatePhase::Unleashing => 1.0,
        UltimatePhase::ZoomingOut => {
            1.0 - (state.phase_timer_s / PHASE_ZOOM_OUT_S).clamp(0.0, 1.0)
        }
        UltimatePhase::Idle => 0.0,
    };
    commands.entity(p1).insert(HyperActive {
        forced_ang_vel: spin,
        beam_width_mult: width_mult,
    });

    if state.phase_timer_s >= phase_total {
        state.phase_timer_s = 0.0;
        state.phase = match state.phase {
            UltimatePhase::ZoomingIn => UltimatePhase::Unleashing,
            UltimatePhase::Unleashing => UltimatePhase::ZoomingOut,
            UltimatePhase::ZoomingOut => {
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
        UltimatePhase::ZoomingIn => {
            let p = (state.phase_timer_s / PHASE_ZOOM_IN_S).clamp(0.0, 1.0);
            (p, state.orig_cam_scale * (1.0 - p) + HYPER_CAM_SCALE * p)
        }
        UltimatePhase::Unleashing => (1.0, HYPER_CAM_SCALE),
        UltimatePhase::ZoomingOut => {
            let p = (state.phase_timer_s / PHASE_ZOOM_OUT_S).clamp(0.0, 1.0);
            (1.0 - p, HYPER_CAM_SCALE * (1.0 - p) + state.orig_cam_scale * p)
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
            let off_x = 220.0 * scale;
            let off_y = -140.0 * scale;
            let z = portrait_xf.translation.z;
            portrait_xf.translation = cam_xf.translation + Vec3::new(off_x, off_y, 0.0);
            portrait_xf.translation.z = z;
            portrait_xf.scale = Vec3::new(380.0 * scale, 570.0 * scale, 1.0);
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
    if state.phase == UltimatePhase::Idle {
        return;
    }
    let dt = time.delta_secs();
    let spin = match state.phase {
        UltimatePhase::ZoomingIn => {
            let p = (state.phase_timer_s / PHASE_ZOOM_IN_S).clamp(0.0, 1.0);
            HYPER_SPIN_RAD_PER_S * p
        }
        UltimatePhase::Unleashing => HYPER_SPIN_RAD_PER_S,
        UltimatePhase::ZoomingOut => {
            let p = (state.phase_timer_s / PHASE_ZOOM_OUT_S).clamp(0.0, 1.0);
            HYPER_SPIN_RAD_PER_S * (1.0 - p)
        }
        UltimatePhase::Idle => 0.0,
    };
    for mut rot in &mut rotations {
        let new_angle = rot.as_radians() + spin * dt;
        *rot = Rotation::radians(new_angle);
    }
}

// ----------------------------------------------------------------
// Beam: pose each tick, deal damage, spawn smear trails.
// ----------------------------------------------------------------

fn tick_ultimate_beams(
    time: Res<Time<Real>>,
    state: Res<UltimateState>,
    spatial: SpatialQuery,
    ships: Query<(&Position, &Rotation), With<Ship>>,
    ship_lookup: Query<&Ship>,
    mut crews: Query<&mut Crew>,
    mut beams: Query<(&UltimateBeam, &mut Transform, &mut Sprite, &Visibility)>,
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

    let phase_alpha = match state.phase {
        UltimatePhase::ZoomingIn => {
            (state.phase_timer_s / PHASE_ZOOM_IN_S).clamp(0.0, 1.0)
        }
        UltimatePhase::Unleashing => 1.0,
        UltimatePhase::ZoomingOut => {
            1.0 - (state.phase_timer_s / PHASE_ZOOM_OUT_S).clamp(0.0, 1.0)
        }
        UltimatePhase::Idle => 0.0,
    };
    if phase_alpha <= 0.001 {
        return;
    }

    // Raycast to find the nearest collider (for damage). Visually
    // the blade always extends HYPER_BEAM_LEN — like a lightsaber
    // poking through whatever it touches.
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
    let midpoint = owner_pos + world_dir * (blade_len * 0.5);
    let angle = world_dir.y.atan2(world_dir.x) - std::f32::consts::FRAC_PI_2;

    for (beam, mut xf, mut sprite, vis) in &mut beams {
        let (w, color, z) = beam_layer_pose(beam.layer, phase_alpha);
        xf.translation = midpoint.extend(z);
        xf.rotation = Quat::from_rotation_z(angle);
        sprite.color = color;
        sprite.custom_size = Some(Vec2::new(w, blade_len));

        // Trail emission — only while fully visible and in the
        // Unleashing phase so the smear is dense and clearly
        // distinct from the ramp.
        if *vis == Visibility::Visible && state.phase == UltimatePhase::Unleashing && beam.layer >= 1 {
            let peak = 0.6 * phase_alpha;
            let lin = color.to_linear();
            let trail_color = Color::srgba(lin.red, lin.green, lin.blue, peak);
            commands.spawn((
                BeamTrail {
                    remaining_s: 0.5,
                    total_s: 0.5,
                    peak_alpha: peak,
                    base_color: trail_color,
                },
                Sprite::from_color(trail_color, Vec2::new(w * 1.15, blade_len)),
                Transform {
                    translation: midpoint.extend(z - 0.05),
                    rotation: Quat::from_rotation_z(angle),
                    scale: Vec3::ONE,
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
        // Cubic fade-out so the smear is bright at first then drops
        // off fast — feels more like a luminescent smoke wisp than
        // a linear fade.
        let alpha = trail.peak_alpha * frac * frac * frac;
        let lin = trail.base_color.to_linear();
        sprite.color = Color::srgba(lin.red, lin.green, lin.blue, alpha);
    }
}
