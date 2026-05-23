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

/// Pulsating bluish-white halo used by the Earthling
/// jump-to-light-speed ultimate. Radial gradient + tint baked into
/// the shader; only `params.x` (intensity) is animated.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GlowMaterial {
    #[uniform(0)]
    pub params: Vec4,
}

impl Material2d for GlowMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/lightspeed_glow.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// Soft-edge blade triangle material. Used by the Arilou blade
/// layers and the BeamTrail / BlastTrail ghosts so each triangle
/// fades to transparent at its edges instead of reading as a hard
/// polygon. `color` is the full RGBA tint; the shader multiplies
/// the alpha by a barycentric-distance smoothstep.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SoftBladeMaterial {
    #[uniform(0)]
    pub color: LinearRgba,
}

impl Material2d for SoftBladeMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/soft_blade.wgsl".into()
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
    pub variant: UltimateVariant,
    pub player_entity: Option<Entity>,
    pub orig_cam_pos: Vec3,
    pub orig_cam_scale: f32,
    pub portrait_entity: Option<Entity>,
    pub portrait_material: Option<Handle<PortraitMaterial>>,
    /// Animated 0..1 zip-in fraction for the portrait sprite. 1.0
    /// = fully in (anchored to the screen-left, aligned with the
    /// player's status panel). 0.0 = fully off-screen left.
    /// Eased toward 1.0 during DramaticZoomIn and toward 0.0
    /// during every other phase, so the portrait "zips" in when
    /// time freezes and "zips" back out when time resumes.
    pub portrait_in_t: f32,
    pub was_paused: bool,
    /// Blade world-angle on the previous Update tick. We interpolate
    /// between this and the current angle when emitting trails, so
    /// the smear is continuous instead of stepped at 60 fps.
    pub last_blade_angle: Option<f32>,
    /// Per-layer `ColorMaterial` handles — [core, mid, halo]. We
    /// update each material's alpha each frame in `tick_ultimate_beams`
    /// so the layered blade fades in/out cleanly. Cleared on exit.
    pub beam_materials: Vec<Handle<SoftBladeMaterial>>,
    /// Spawned beam-entity ids — despawned on exit so repeated
    /// triggers don't leak entities.
    pub beam_entities: Vec<Entity>,
    // -- Earthling lightspeed jump --
    pub glow_entity: Option<Entity>,
    pub glow_material: Option<Handle<GlowMaterial>>,
    /// Ship's original Transform.scale, captured at the start of the
    /// Earthling cinematic so we can restore it after stretching.
    pub orig_ship_scale: Option<Vec3>,
    /// Forward world direction captured at the start of the blast —
    /// the ship can't steer mid-jump, so we lock the direction.
    pub blast_dir: Option<Vec2>,
    // -- Yehat --
    pub yehat_fighters: Vec<Entity>,
    // -- Chenjesu --
    pub chebr_ring_timer_s: f32,
    // -- Pkunk --
    pub pkunk_clones: Vec<Entity>,
    /// Width/height of the loaded portrait image. Detected lazily
    /// once the texture is decoded; zeroed on exit. Used by the
    /// camera follow / portrait positioning to scale the on-screen
    /// portrait without stretching it.
    pub portrait_aspect: f32,
    // -- Mmrnmhrm transform --
    pub mmrxf_overlay_entity: Option<Entity>,
    pub mmrxf_orig_scale: Option<Vec3>,
    pub mmrxf_laser_cooldown_s: f32,
    pub mmrxf_missile_cooldown_s: f32,
    /// Number of supershots fired so far in the current Druuge
    /// barrage. Used by `tick_druuge_barrage` to gate the
    /// per-shot interval. Reset on exit.
    pub druuge_shots_fired: usize,
    /// Snapshot of the Thraddash ship's pre-ultimate `speed_max`
    /// in `ShipPhysicsDerived`. Restored on cinematic exit.
    pub thraddash_orig_speed_max: Option<f32>,
    /// Time accumulator for emitting Thraddash flame puffs.
    pub thraddash_flame_timer_s: f32,
    /// Mycon orb entities spawned during MyconGathering — needed
    /// so MyconHurricane can convert each to a homing seeker
    /// without re-querying by marker (orbs may get despawned by
    /// the player ramming them, etc.).
    pub mycon_orbs: Vec<Entity>,
    /// Mycon orb spawn-time snapshot (paused-time elapsed) for
    /// the swirl-in animation.
    pub mycon_orb_t0: f32,
}

/// Marker on the Mmrnmhrm ship during MmrxfUnleashing. Normal
/// Mmrxf primary / special abilities check for this and skip so
/// the ultimate's weapons replace them rather than stacking on
/// top.
#[derive(Component, Debug)]
pub struct MmrxfActive;

/// Marker on the alt-form overlay sprite spawned during the
/// transform. Despawned on cinematic exit.
#[derive(Component, Debug)]
pub struct MmrxfOverlaySprite;

/// Component on the parent guided missile fired by Mmrnmhrm's
/// special during the ultimate. After `split_at_s` seconds it
/// despawns + spawns `child_count` smaller homing projectiles.
#[derive(Component, Debug)]
pub struct MmrxfSplitMissile {
    pub timer_s: f32,
    pub split_at_s: f32,
    pub child_count: usize,
    /// Carries the owner Entity so the children inherit the
    /// same firer for self-hit checks.
    pub owner: Entity,
}

/// One short-lived sprite segment of the tangled laser. Lifetime
/// is one FixedUpdate tick — the system redraws the segments
/// every tick to "animate" the wavy path. The fade lets each
/// segment stay visible for a couple of render frames between
/// physics ticks.
#[derive(Component, Debug)]
pub struct MmrxfLaserSegment {
    pub remaining_s: f32,
    pub total_s: f32,
    pub base_color: Color,
}

/// Ephemeral clone of a Pkunk ship spawned by its ultimate. Shares
/// the original's player_slot so `apply_player_input` /
/// `dispatch_primary` / `dispatch_special` all drive it as if it
/// were the original ship — the player effectively controls a
/// formation of three identical ships at once for `remaining_s`
/// seconds before the clone fades out and despawns.
///
/// `reveal_at_pan_t` is the PkunkPan timer fraction (0..1) at
/// which this clone should start warping into view. We hide the
/// clone (alpha = 0) until the camera reaches its position; once
/// the threshold is crossed, `warp_in_t` ramps from 0 → 1 over
/// ~0.15 s for a snap-in pop.
#[derive(Component, Debug)]
pub struct PkunkClone {
    pub remaining_s: f32,
    pub total_s: f32,
    pub reveal_at_pan_t: f32,
    pub warp_in_t: f32,
    /// Position offset relative to the leader's local frame at
    /// spawn. The aggressive-AI tick uses the SIGN of `.x` to bias
    /// which way the clone whirls (left clone spins left, right
    /// clone spins right) — the magnitude is otherwise unused now
    /// that clones no longer hold formation.
    pub formation_offset_local: Vec2,
    /// Aggro/Retreat state machine. `false` = charge in and fire;
    /// `true` = turn 180° away from the target and hold special to
    /// refill battery. Set by `tick_pkunk_aggressive_clones` based
    /// on Battery level with hysteresis.
    pub retreating: bool,
}

/// Soft halo entity glued to a Pkunk clone for the duration of
/// the cinematic. Makes it obvious which Pkunk on screen is the
/// "real" one (no aura) vs the ephemeral clones (auras). One
/// aura per clone — despawned in `exit_cinematic` alongside the
/// clones themselves.
#[derive(Component, Debug)]
pub struct PkunkAura {
    pub clone: Entity,
}

/// Marker on an asteroid that's been thrown by the Slylandro
/// ultimate. While present, the asteroid glows luminescent and any
/// CollisionStart with a ship that isn't the firer dispenses
/// `damage` crew. The marker self-removes when `remaining_s` hits
/// 0, at which point the asteroid returns to being an inert
/// drifting rock.
#[derive(Component, Debug)]
pub struct SlylandroLaunched {
    pub owner: Entity,
    pub damage: i32,
    pub remaining_s: f32,
    pub total_s: f32,
    /// Counts down from SLYP_HIT_FLASH_S on each crew hit so the
    /// asteroid flashes white briefly — visible confirmation that
    /// this rock just landed damage. Decremented in
    /// `tick_slylandro_glow`.
    pub hit_flash_remaining_s: f32,
}

/// Fading ghost of a SlylandroLaunched asteroid spawned each tick
/// behind it for the trail effect. Carries its source sprite handle
/// and an initial tint; tick_asteroid_ghosts decays alpha over the
/// total lifetime and despawns on zero.
#[derive(Component, Debug)]
pub struct AsteroidGhost {
    pub remaining_s: f32,
    pub total_s: f32,
    pub base_color: Color,
}

/// Yehat ultimate sub-entity — a fighter orbiting the parent
/// Terminator. Fires periodically at the nearest enemy ship.
#[derive(Component, Debug)]
pub struct YehatFighter {
    pub owner: Entity,
    pub angle_offset: f32,
    pub fire_cooldown_s: f32,
}

/// One of the Mycon plasma orbs that orbits the Podship during the
/// `MyconGathering` phase. Tracks its phase angle so the orbital
/// motion is deterministic; `radius_t` is 0..1 across the gather
/// (orb starts wide, spirals in to MYCON_ORBIT_R). On phase
/// transition to MyconHurricane the orbs convert into homing
/// seekers via `tick_mycon_hurricane_release`.
#[derive(Component, Debug)]
pub struct MyconOrbit {
    pub owner: Entity,
    pub theta: f32,
    pub omega: f32,
}

/// A spinning F.R.I.E.D. blade spawned by the Kohr-Ah ultimate.
/// Owned-projectile-ish: damages on contact, but also accelerates
/// outward radially over its lifetime so the kill zone expands.
#[derive(Component, Debug)]
pub struct KohrAhBlade {
    pub owner: Entity,
    pub dir: Vec2,
    pub speed: f32,
    pub lifetime_s: f32,
}

/// Which captain's ultimate is currently playing. Drives portrait /
/// voice asset selection and which behaviours run during the active
/// phases.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum UltimateVariant {
    #[default]
    None,
    /// Arilou: spin-and-slash lightsaber blade.
    Arilou,
    /// Earthling: jump-to-light-speed; ship charges, springs, then
    /// blasts forward at 4× top speed for 3 s, dealing massive
    /// contact damage.
    Earthling,
    /// Yehat: summon a battle fleet — three fighter sub-entities
    /// orbit the Terminator and auto-fire missiles at the nearest
    /// enemy for the duration.
    Yehat,
    /// Spathi: missile storm — fires a barrage of BUTT-style
    /// homing missiles in every direction at once.
    Spathi,
    /// Chenjesu: crystal tempest — emits ring after ring of
    /// crystal shards radiating outward.
    Chenjesu,
    /// Shofixti: nova sacrifice — supercharged Glory Device that
    /// detonates with a vast lethal radius and destroys the
    /// firer.
    Shofixti,
    /// Pkunk: two ephemeral clones spawn alongside the Fury in
    /// an equilateral formation; all three share the same input
    /// channel so the player commands three ships at once for a
    /// few seconds. Camera does a three-step pan over the trio
    /// during the paused reveal, then time resumes and the
    /// formation flies until the clones expire.
    Pkunk,
    /// Slylandro: every asteroid in the arena glows hot, then
    /// launches itself with a one-shot impulse toward the
    /// opponent at randomised chaotic speeds. Each hit on the
    /// opponent (or anything else) deducts crew.
    Slylandro,
    /// Mmrnmhrm: ship triples in size, both T-form and Y-form
    /// sprites superimposed; primary becomes a curvy "tangled
    /// rope" homing laser at 2× range; special fires guided
    /// missiles that split into smaller guided missiles
    /// mid-flight (Owa-style cluster).
    Mmrnmhrm,
    /// Druuge: Wrath of the Crimson Corporation. Six oversized
    /// cannon shots fired forward in rapid succession with
    /// extreme recoil — the Mauler's signature kickback amplified
    /// to "fling the ship halfway across the arena" levels.
    Druuge,
    /// Kohr-Ah: Sanctified Slaughter. Twelve F.R.I.E.D. saw
    /// blades emit outward in a 360° radial pattern from the
    /// spinning Marauder. The blades persist and accelerate so
    /// the kill zone keeps expanding.
    KohrAh,
    /// Mycon: Plasma Hurricane. Eight plasmoids orbit the Podship
    /// in a tightening spiral during the buildup, then launch as
    /// homing seekers in a synchronized swarm.
    Mycon,
    /// Thraddash: Afterburner Inferno. Speed cap quintuples for
    /// a brief duration and the exhaust trail behind the Torch
    /// becomes a lethal damage zone — the player flies through
    /// the arena painting kill streaks.
    Thraddash,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum UltimatePhase {
    #[default]
    Idle,
    /// Time is paused. Camera rams toward the ship, portrait fades
    /// in. No active behaviour yet — the held breath before the
    /// move. Shared by all variants.
    DramaticZoomIn,
    // ---- Arilou ----
    /// Time resumes. Camera pulls back to its original framing
    /// *while* the ship spins and the blade slashes — the zoom-out
    /// itself is the punch of the move.
    ArilouUnleashing,
    // ---- Earthling jump-to-light-speed ----
    /// Time paused. Ship is covered in a pulsating bluish-white
    /// glow that ramps up — the energy-buildup beat.
    EarthlingCharging,
    /// Time paused. Ship loses all velocity and stretches
    /// vertically like a spring loading up.
    EarthlingStretching,
    /// Time resumes. Ship snaps back to its normal scale and
    /// blasts forward at 4× top speed for 3 s. Anything in its
    /// path takes massive contact damage. Leaves a jagged trail.
    EarthlingBlasting,
    // ---- Yehat battle fleet ----
    /// Paused. Three fighter silhouettes materialise around the
    /// Terminator.
    YehatSummoning,
    /// Unpaused. Fighters orbit and auto-fire at the nearest enemy.
    YehatBattle,
    // ---- Spathi missile storm ----
    /// Paused. The Eluder visibly winds up — small reticles
    /// flicker around it as it locks every direction.
    SpathiLockOn,
    /// Unpaused single moment: 30 homing missiles launch outward,
    /// then the cinematic exits.
    SpathiBarrage,
    // ---- Chenjesu crystal tempest ----
    /// Paused. Broodhome glows crystal-blue, charging.
    ChenjesuCharging,
    /// Unpaused. Concentric rings of crystal shards emit from the
    /// ship every few hundred ms.
    ChenjesuTempest,
    // ---- Shofixti nova ----
    /// Paused. Scout glows hot-white pulsing — final pre-detonation
    /// beat.
    ShofixtiCharging,
    /// Unpaused single moment: massive damage zone detonates and
    /// the firer's crew goes to zero.
    ShofixtiNova,
    // ---- Pkunk clone formation ----
    /// Paused. Two clones spawn at the trailing vertices of an
    /// equilateral triangle. Their sprites fade in over the phase.
    PkunkSummoning,
    /// Paused. Camera pans original → clone-1 → clone-2 → all-3
    /// centroid (with a slow zoom-out by the final step) so the
    /// player sees who they're now commanding.
    PkunkPan,
    /// Unpaused. All three ships share the same player_slot so
    /// every input the player gives drives all three identically.
    /// Clones tick down their lifetime and fade as they expire.
    PkunkFormation,
    // ---- Slylandro asteroid storm ----
    /// Paused. Every asteroid in the field gets the
    /// `SlylandroLaunched` marker and starts glowing in place —
    /// energy buildup for the impending barrage. Camera stays
    /// tight on the firer.
    SlylandroCharging,
    /// Paused. Camera pulls back from the close-up to the
    /// original perspective — the player surveys the field of
    /// glowing asteroids before they fly. Time stays stopped.
    SlylandroPullback,
    /// Unpaused. Each asteroid receives a one-shot impulse along
    /// the predicted intercept of the nearest enemy ship and
    /// deals contact damage for the duration.
    SlylandroStorm,
    // ---- Mmrnmhrm transform ----
    /// Paused. Ship visually scales 3× and the alternate form's
    /// sprite is overlaid — the "X-form fusion" beat.
    MmrxfTransform,
    /// Unpaused. Primary = tangled-rope homing laser, special =
    /// splitting guided missiles. Normal Mmrxf abilities are
    /// suppressed for the duration via the MmrxfActive marker.
    MmrxfUnleashing,
    // ---- Druuge wrath ----
    /// Paused. Cannon glows red, recoil charges. Camera tight.
    DruugeCharging,
    /// Unpaused. Six super-sized cannon shots emit forward over
    /// the phase; each fires with an oversized recoil impulse on
    /// the ship.
    DruugeBarrage,
    // ---- Kohr-Ah sanctified slaughter ----
    /// Paused. Ship spins faster and faster as blades sharpen.
    KohrAhSharpening,
    /// Unpaused. Twelve saw projectiles emit in a 360° pattern,
    /// each accelerating outward over time.
    KohrAhSlaughter,
    // ---- Mycon plasma hurricane ----
    /// Paused. Eight plasmoids spawn orbiting the ship in a
    /// tightening spiral, glowing brighter as they close in.
    MyconGathering,
    /// Unpaused. The orbiting plasmoids release as homing
    /// seekers, locking the nearest enemy and pursuing.
    MyconHurricane,
    // ---- Thraddash afterburner ----
    /// Paused for a brief moment as the engine ignites — visual
    /// flare-up, no damage yet.
    ThraddashIgniting,
    /// Unpaused. Speed cap quintuples, exhaust trail becomes a
    /// lethal damage zone for the duration. Player keeps control.
    ThraddashBurning,
}

/// Marker on the ship while the cinematic is active.
/// `apply_player_input` reads this and stops touching the ship's
/// AngularVelocity / LinearVelocity / thrust. `cap_velocity` also
/// reads it so the Earthling jump isn't clamped to speed_max.
/// `tick_beams` reads it and fattens the regular beam visual.
#[derive(Component, Debug)]
pub struct HyperActive {
    pub forced_ang_vel: f32,
    pub beam_width_mult: f32,
}

/// Marker on the bluish glow halo that overlays the Earthling ship
/// during the charge / stretch phases. Despawned on exit.
#[derive(Component)]
pub struct LightspeedGlow;

/// Sticks on the Earthling ship for a few seconds AFTER the blast
/// ends, while it still has the over-cap velocity from the jump.
/// While present:
///   - `cap_velocity` doesn't clamp the ship to speed_max, so it
///     keeps coasting at whatever speed the blast left it at.
///   - `apply_player_input` interprets THRUST as a *brake* — applies
///     a force opposite the current velocity instead of forward —
///     so the player can shed the over-speed at will.
/// Auto-removes when the ship's speed drops at or below speed_max.
#[derive(Component, Debug)]
pub struct PostUltimateCoasting;

/// Spawned each frame during `EarthlingBlasting` behind the ship —
/// a stretched triangular streak with a chaotic alpha gradient.
/// Fades and despawns over `total_s`.
#[derive(Component, Debug)]
pub struct BlastTrail {
    pub remaining_s: f32,
    pub total_s: f32,
    pub peak_alpha: f32,
    pub base_color: Color,
    pub material: Handle<SoftBladeMaterial>,
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
    pub material: Handle<SoftBladeMaterial>,
    /// World-units / second the trail position drifts at. Mostly
    /// tangential to the blade so wisps "fling off" sideways.
    pub drift_vel: Vec2,
    /// Width grows by `1 + width_growth * age` over the lifetime.
    /// Randomised per-trail for chaotic flickering.
    pub width_growth: f32,
}

/// Strong-handle holders for the ultimate cinematic's meshes. Bevy
/// considers `uuid_handle!` constants to be `Handle::Uuid` — which
/// per the docs "does not necessarily reference a live asset, nor
/// will it keep assets alive". With our asset preload pushing
/// pressure on the GC, those weak handles dropped and the blade /
/// portrait stopped rendering. The fix is to keep strong handles
/// in a resource: insert the mesh assets at Startup, store the
/// strong handles here, hand out clones from inside the
/// cinematic code. Strong clones keep the asset alive.
#[derive(Resource, Default)]
pub struct UltimateMeshes {
    pub quad: Handle<Mesh>,
    pub blade: Handle<Mesh>,
}

#[derive(Component)]
pub struct UltimatePortraitTag;

/// Marker on the audio entity playing the Arilou ultimate voice
/// line. (Stinger SFX is no longer triggered on its despawn —
/// see `tick_arilou_stinger` for the new timing.)
#[derive(Component)]
pub struct ArilouVoicePlayer;

/// Stinger SFX entity spawned at the start of `ArilouUnleashing`.
/// `tick_arilou_stinger` fades its volume from 1.0 → 0.0 over the
/// attack window so the SFX matches the duration of the
/// blade-swing, then despawns at end.
#[derive(Component)]
pub struct ArilouStinger {
    pub total_s: f32,
    pub remaining_s: f32,
}

// ----------------------------------------------------------------
// Plugin.
// ----------------------------------------------------------------

pub struct UltimatePlugin;

impl Plugin for UltimatePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            Material2dPlugin::<PortraitMaterial>::default(),
            Material2dPlugin::<GlowMaterial>::default(),
            Material2dPlugin::<SoftBladeMaterial>::default(),
        ))
        .init_resource::<UltimateState>()
        .init_resource::<UltimateMeshes>()
        .add_systems(Startup, build_ultimate_meshes)
        .init_resource::<MmrxfUnleashedSprite>()
        // ---- GgrsSchedule: gameplay-affecting cinematic systems ----
        //
        // Anything that mutates game state DURING unpaused
        // phases must run in GgrsSchedule so rollback can
        // re-simulate it deterministically. We can't put
        // *paused-phase* systems here, because FixedUpdate (and
        // thus GgrsSchedule) halts when `Time<Virtual>` is
        // paused — those go in Update.
        .add_systems(
            bevy_ggrs::GgrsSchedule,
            (
                tick_ultimate_beams,
                tick_earthling_blast,
                tick_yehat_battle_fleet,
                tick_spathi_barrage,
                tick_chenjesu_tempest,
                tick_shofixti_nova,
                tick_pkunk_clones,
                handle_slylandro_asteroid_hits,
                tick_mycon_release,
                tick_thraddash_restore,
                tick_druuge_barrage,
                tick_kohrah_spawn,
                tick_kohrah_blades,
            ),
        )
        .add_systems(
            bevy_ggrs::GgrsSchedule,
            (
                tick_thraddash_burn,
                tick_mmrxf_tangled_laser,
                tick_mmrxf_split_launcher,
                tick_mmrxf_split_missiles,
            ),
        )
        // ---- Update: cinematic systems that MUST tick during
        // Time<Virtual> pause ----
        //
        // - tick_ultimate_phases drives phase transitions and
        //   has to advance the timer during the DramaticZoomIn
        //   pause (and other paused first-phases) so the
        //   cinematic actually progresses. If it lived in
        //   GgrsSchedule it'd freeze on pause.
        // - hyper_trigger reads INPUT_ULTIMATE and starts the
        //   cinematic; pre-cinematic the game is unpaused, but
        //   keeping it in Update avoids ordering oddities.
        // - abort_cinematic_if_ship_gone cleans up if the ship
        //   gets despawned mid-cinematic; must run during pause.
        // - tick_mycon_gather / tick_mycon_orbit animate the
        //   plasma orbs during the PAUSED MyconGathering beat.
        .add_systems(
            Update,
            (
                tick_ultimate_phases,
                hyper_trigger,
                abort_cinematic_if_ship_gone,
                tick_mycon_gather,
                tick_mycon_orbit,
                // tick_slylandro_storm has a fire-once arming
                // pass during the PAUSED SlylandroCharging
                // phase (it freezes every asteroid + adds the
                // SlylandroLaunched marker). The launch-
                // impulse pass during the UNPAUSED Storm phase
                // could live in GgrsSchedule, but keeping both
                // halves of the system in one schedule is
                // simpler. Wall-clock-driven timing for the
                // armed flight is acceptable since the asteroids
                // are rolled-back entities and their post-
                // impulse trajectories simulate in physics.
                tick_slylandro_storm,
            ),
        )
        // ---- Update: visual-only systems ----
        //
        // Cinematic eye-candy that just animates sprites /
        // cameras / materials. These read `Time<Real>` so they
        // tick smoothly at render-frame rate (even while
        // gameplay is paused via Time<Virtual>); their effects
        // don't feed back into game state, so peers showing
        // slightly different visuals don't desync.
        .add_systems(
            Update,
            (
                detect_portrait_aspect,
                drive_camera_during_ultimate,
                drive_ship_rotation_during_ultimate,
                tick_beam_trails,
                tick_lightspeed_glow,
                tick_blast_trails,
                tick_pkunk_clone_visual,
                tick_pkunk_auras,
                tick_slylandro_glow,
                tick_asteroid_ghosts,
                spawn_arilou_stinger_on_unleash,
                tick_arilou_stinger,
            ),
        )
        .add_systems(
            Update,
            (
                tick_mmrxf_transform,
                tick_mmrxf_laser_segments,
                tick_mmrxf_needs_restore,
                strip_white_background_once,
            ),
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

// -- Earthling jump-to-light-speed --
const EARTH_CHARGE_S: f32 = 0.55;
const EARTH_STRETCH_S: f32 = 0.30;
const EARTH_BLAST_S: f32 = 3.0;
/// How much faster than `speed_max` the ship goes during the blast.
const EARTH_BLAST_SPEED_MULT: f32 = 12.0;
/// Crew damage applied per second to anything overlapping the
/// blasting ship. Enormous on purpose — the move should one-shot
/// almost anything in its path.
const EARTH_BLAST_DMG_PER_SEC: f32 = 1200.0;
/// Stretch ratio at peak: ship sprite is this many × longer along
/// its forward axis at the moment of spring-release.
const EARTH_STRETCH_PEAK: f32 = 2.4;

// -- Yehat battle fleet --
const YEHAT_SUMMON_S: f32 = 0.6;
const YEHAT_BATTLE_S: f32 = 4.0;
const YEHAT_FIGHTER_RADIUS: f32 = 140.0;
const YEHAT_FIGHTER_ORBIT_RPS: f32 = 0.6;
const YEHAT_FIGHTER_FIRE_INTERVAL_S: f32 = 0.5;

// -- Spathi missile storm --
const SPATHI_LOCKON_S: f32 = 0.5;
const SPATHI_BARRAGE_S: f32 = 0.6;
const SPATHI_MISSILE_COUNT: usize = 30;

// -- Chenjesu crystal tempest --
const CHEBR_CHARGE_S: f32 = 0.6;
const CHEBR_TEMPEST_S: f32 = 2.5;
const CHEBR_RING_INTERVAL_S: f32 = 0.22;
const CHEBR_RING_SIZE: usize = 16;

// -- Shofixti nova --
const SHOSC_CHARGE_S: f32 = 0.8;
const SHOSC_NOVA_S: f32 = 0.5;
const SHOSC_NOVA_RADIUS: f32 = 1500.0;

// -- Pkunk formation --
const PKUNK_SUMMON_S: f32 = 0.5;
const PKUNK_PAN_S: f32 = 1.6;
/// Clones live this long in aggressive-AI mode after the pan ends.
/// Doubled from the original 5s — gives the swarm enough time to
/// actually pressure the opponent.
const PKUNK_FORMATION_S: f32 = 10.0;
/// Side length of the equilateral formation triangle (world units).
const PKUNK_FORMATION_SIDE: f32 = 240.0;
/// How wide the camera frames the three ships at the end of the pan.
const PKUNK_PAN_FAR_SCALE: f32 = 1.2;

// -- Slylandro asteroid storm --
const SLYP_CHARGE_S: f32 = 0.7;
const SLYP_PULLBACK_S: f32 = 0.7;
const SLYP_STORM_S: f32 = 4.0;
/// Min/max launch speed of each asteroid (world units / second).
/// Doubled from the first pass + narrower spread so the swarm
/// actually connects with the target instead of overshooting in
/// every direction.
const SLYP_LAUNCH_SPEED_MIN: f32 = 880.0;
const SLYP_LAUNCH_SPEED_MAX: f32 = 2480.0;
/// Max angular jitter applied to each asteroid's intercept aim,
/// in radians. ±0.08 ≈ ±4.6° — enough that the swarm spreads
/// visually but tight enough to actually hit a moving target.
const SLYP_AIM_JITTER_RAD: f32 = 0.08;
/// How long an asteroid stays "armed" — glowing and dealing contact
/// damage — after launch. Once it expires the rock returns to
/// being an inert physical obstacle.
const SLYP_ARMED_LIFE_S: f32 = 5.0;
/// Crew damage dealt per asteroid contact event.
const SLYP_ASTEROID_DAMAGE: i32 = 6;
/// On a damage hit the asteroid flashes white for this long so the
/// player can see which rocks are landing crew damage.
const SLYP_HIT_FLASH_S: f32 = 0.18;

// -- Mmrnmhrm transform --
const MMRXF_TRANSFORM_S: f32 = 0.6;
const MMRXF_UNLEASH_S: f32 = 5.0;
/// Tangled laser primary: range = 2× the canonical Mmrxf T-form
/// laser (8 SC2 units → 320 wu), damage per SC2-frame, and the
/// number of curve segments to render per tick.
const MMRXF_LASER_RANGE: f32 = 2.0 * 8.0 * crate::ship::SC2_VEL_SCALE * 5.0; // 768 wu  (2 * canon)
const MMRXF_LASER_DAMAGE: i32 = 1;
/// Refire interval for the tangled beam tick (damage application
/// rate cap, in SC2 frames).
const MMRXF_LASER_FIRE_INTERVAL_S: f32 = 0.05;
/// Special: spawn the parent missile this often (max 1 in flight
/// from this firer).
const MMRXF_MISSILE_COOLDOWN_S: f32 = 0.55;
/// Time before the parent missile splits into children.
const MMRXF_MISSILE_SPLIT_AT_S: f32 = 0.7;
/// Number of children produced when a split missile splits.
const MMRXF_MISSILE_CHILD_COUNT: usize = 5;

// -- Druuge Wrath --
const DRUUGE_CHARGE_S: f32 = 0.55;
const DRUUGE_BARRAGE_S: f32 = 1.6;
/// Number of supershots fired across the barrage.
const DRUUGE_SHOT_COUNT: usize = 6;
/// Time between successive supershots.
const DRUUGE_SHOT_INTERVAL_S: f32 = 0.22;
/// Backward kick (impulse) applied to the ship per supershot.
const DRUUGE_RECOIL_IMPULSE: f32 = 220.0;

// -- Kohr-Ah Sanctified Slaughter --
const KOHRAH_SHARPEN_S: f32 = 0.55;
const KOHRAH_SLAUGHTER_S: f32 = 2.4;
const KOHRAH_BLADE_COUNT: usize = 12;
/// Acceleration outward applied to each blade per second (wu/s²).
const KOHRAH_BLADE_ACCEL: f32 = 240.0;

// -- Mycon Plasma Hurricane --
const MYCON_GATHER_S: f32 = 0.95;
const MYCON_HURRICANE_S: f32 = 3.5;
const MYCON_ORB_COUNT: usize = 8;
/// Final orbit radius at the end of MyconGathering, before release.
const MYCON_ORBIT_R: f32 = 140.0;

// -- Thraddash Afterburner Inferno --
const THRADDASH_IGNITE_S: f32 = 0.40;
const THRADDASH_BURN_S: f32 = 4.5;
/// Speed-cap multiplier while burning.
const THRADDASH_SPEED_MULT: f32 = 5.0;
/// Damage per second dealt by a Thraddash flame puff in contact.
const THRADDASH_FLAME_DPS: f32 = 18.0;
/// Lifetime of each emitted flame puff.
const THRADDASH_FLAME_LIFE_S: f32 = 0.9;
/// Radius of each flame puff's damage zone.
const THRADDASH_FLAME_RADIUS: f32 = 38.0;
/// Interval between flame puff emissions.
const THRADDASH_FLAME_INTERVAL_S: f32 = 0.04;

fn portrait_path(variant: UltimateVariant) -> &'static str {
    match variant {
        UltimateVariant::Earthling => "ultimate/portrait_earcr.png",
        UltimateVariant::Yehat => "ultimate/portrait_yehte.png",
        UltimateVariant::Spathi => "ultimate/portrait_spael.png",
        UltimateVariant::Chenjesu => "ultimate/portrait_chebr.png",
        UltimateVariant::Shofixti => "ultimate/portrait_shosc.png",
        UltimateVariant::Pkunk => "ultimate/portrait_pkufu.png",
        UltimateVariant::Slylandro => "ultimate/portrait_slypr.png",
        UltimateVariant::Mmrnmhrm => "ultimate/portrait_mmrxf.png",
        _ => "ultimate/portrait_arisk.png",
    }
}

fn voice_path(variant: UltimateVariant) -> &'static str {
    // Per-class voice paths; falls back to Arilou's sample if a
    // class-specific file isn't present (Bevy silently fails to
    // load missing assets, the audio just plays nothing for that
    // ult). Drop wavs in assets/ultimate/ to wire each up.
    match variant {
        UltimateVariant::Earthling => "ultimate/earcr_voi.wav",
        UltimateVariant::Yehat => "ultimate/yehte_voi.wav",
        UltimateVariant::Spathi => "ultimate/spael_voi.wav",
        UltimateVariant::Chenjesu => "ultimate/chebr_voi.wav",
        UltimateVariant::Shofixti => "ultimate/shosc_voi.wav",
        UltimateVariant::Pkunk => "ultimate/pkufu_voi.wav",
        UltimateVariant::Slylandro => "ultimate/slypr_voi.wav",
        UltimateVariant::Mmrnmhrm => "ultimate/mmrxf_voi.wav",
        _ => "ultimate/arisk_voi.wav",
    }
}

fn variant_for_class(class: ShipClass) -> UltimateVariant {
    match class {
        ShipClass::Earcr => UltimateVariant::Earthling,
        ShipClass::Yehte => UltimateVariant::Yehat,
        ShipClass::Spael => UltimateVariant::Spathi,
        ShipClass::Chebr => UltimateVariant::Chenjesu,
        ShipClass::Shosc => UltimateVariant::Shofixti,
        ShipClass::Pkufu => UltimateVariant::Pkunk,
        ShipClass::Slypr => UltimateVariant::Slylandro,
        ShipClass::Mmrxf => UltimateVariant::Mmrnmhrm,
        ShipClass::Druma => UltimateVariant::Druuge,
        ShipClass::Kohma => UltimateVariant::KohrAh,
        ShipClass::Mycpo => UltimateVariant::Mycon,
        ShipClass::Thrto => UltimateVariant::Thraddash,
        _ => UltimateVariant::Arilou,
    }
}

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
    slot_inputs: Res<crate::input::SlotInputs>,
    mut state: ResMut<UltimateState>,
    cameras: Query<(&Transform, &Projection), With<Camera2d>>,
    ships: Query<(Entity, &Ship, &ShipClass, &Transform), Without<Camera2d>>,
    ship_pose: Query<(&Position, &Rotation), With<Ship>>,
    catalog: Res<crate::ship::ShipCatalog>,
    ship_colliders: Res<crate::collider::ShipColliders>,
    ultimate_meshes: Res<UltimateMeshes>,
    mut commands: Commands,
    mut materials: ResMut<Assets<PortraitMaterial>>,
    mut glow_materials: ResMut<Assets<GlowMaterial>>,
    mut color_mats: ResMut<Assets<SoftBladeMaterial>>,
    assets: Res<AssetServer>,
    mut virt: ResMut<Time<Virtual>>,
) {
    if state.phase != UltimatePhase::Idle {
        return;
    }
    // Pick the first ship whose slot pressed the ult button
    // this frame. Order is canonical (slot 0 first), so if
    // somehow two slots tap simultaneously, slot 0 wins. The
    // ult bit travels through SlotInputs so remote players
    // trigger their own slot's ult via the network.
    let Some((entity, ship, class, ship_xf)) = ships.iter().find(|(_, s, _, _)| {
        slot_inputs.just_pressed(s.player_slot, crate::input::INPUT_ULTIMATE)
    })
    else {
        return;
    };
    let Ok((cam_xf, projection)) = cameras.single() else {
        return;
    };

    state.variant = variant_for_class(*class);
    state.player_entity = Some(entity);
    state.orig_cam_pos = cam_xf.translation;
    state.orig_cam_scale = match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    };
    state.orig_ship_scale = Some(ship_xf.scale);
    state.phase = UltimatePhase::DramaticZoomIn;
    state.phase_timer_s = 0.0;
    // Portrait starts fully off-screen on the left; the camera-
    // drive system eases it in during DramaticZoomIn.
    state.portrait_in_t = 0.0;

    // Mesh handles come from the `UltimateMeshes` resource —
    // built once at Startup. Cloning a strong handle keeps the
    // asset alive while any entity holds it.
    let quad_mesh = ultimate_meshes.quad.clone();
    let blade_mesh = ultimate_meshes.blade.clone();

    // Portrait: Mesh2d + custom material with radial alpha fade.
    // Path is per-variant so each captain gets their own art.
    let material = materials.add(PortraitMaterial {
        image: assets.load(portrait_path(state.variant)),
        params: Vec4::new(0.0, 0.0, 0.0, 0.0),
    });
    let portrait = commands
        .spawn((
            UltimatePortraitTag,
            Mesh2d(quad_mesh.clone()),
            MeshMaterial2d(material.clone()),
            Transform::from_scale(Vec3::new(620.0, 930.0, 1.0))
                .with_translation(Vec3::new(0.0, 0.0, 60.0)),
        ))
        .id();
    state.portrait_entity = Some(portrait);
    state.portrait_material = Some(material);

    state.beam_materials.clear();
    state.beam_entities.clear();
    state.glow_entity = None;
    state.glow_material = None;
    state.blast_dir = None;

    match state.variant {
        UltimateVariant::Arilou => {
            // Three layered beam triangles — core / mid / halo. Each
            // layer gets its own ColorMaterial so alphas can be
            // animated independently. Hidden until Unleashing.
            for layer in 0u8..3 {
                let (_w, color, z) = beam_layer_pose(layer, 0.0);
                let mat_handle = color_mats
                    .add(SoftBladeMaterial { color: color.to_linear() });
                let id = commands
                    .spawn((
                        UltimateBeam { layer },
                        Mesh2d(blade_mesh.clone()),
                        MeshMaterial2d(mat_handle.clone()),
                        Transform::from_translation(Vec3::new(0.0, 0.0, z)),
                        Visibility::Hidden,
                    ))
                    .id();
                state.beam_materials.push(mat_handle);
                state.beam_entities.push(id);
            }
        }
        UltimateVariant::Earthling => {
            // Pulsating bluish-white halo overlay. Sits at z just
            // above the ship sprite; alpha is driven by
            // `tick_lightspeed_glow`. Despawned in
            // `exit_cinematic`.
            let glow_mat = glow_materials.add(GlowMaterial {
                params: Vec4::new(0.0, 0.0, 0.0, 0.0),
            });
            let id = commands
                .spawn((
                    LightspeedGlow,
                    Mesh2d(quad_mesh.clone()),
                    MeshMaterial2d(glow_mat.clone()),
                    // Roughly 3× ship's pre-rotated sprite footprint
                    // — big enough that the halo extends well past
                    // the hull.
                    Transform::from_scale(Vec3::new(240.0, 240.0, 1.0))
                        .with_translation(ship_xf.translation.truncate().extend(0.40)),
                ))
                .id();
            state.glow_entity = Some(id);
            state.glow_material = Some(glow_mat);
        }
        UltimateVariant::Pkunk => {
            // Spawn the two clones immediately, at the trailing
            // vertices of an equilateral triangle whose front
            // vertex is the player ship. All three share the same
            // `player_slot`, so apply_player_input /
            // dispatch_primary / dispatch_special drive all three
            // identically. Visual tint is applied each frame by
            // `tick_pkunk_clone_visual`.
            state.pkunk_clones.clear();
            if let Ok((pos, rot)) = ship_pose.get(entity) {
                let side = PKUNK_FORMATION_SIDE;
                // Local-frame offsets for the two trailing
                // vertices (the player's ship is the lead vertex).
                // The order matters: index 0 is revealed by the
                // first pan beat, index 1 by the second. Match
                // the sub-phase thresholds in
                // `drive_camera_during_ultimate`.
                let local_offsets = [
                    (Vec2::new(-side * 0.5, -side * 0.5 * 1.732_050_8), 0.20_f32),
                    (Vec2::new( side * 0.5, -side * 0.5 * 1.732_050_8), 0.45_f32),
                ];
                let rot_angle = rot.sin.atan2(rot.cos);
                let cos = rot.cos;
                let sin = rot.sin;
                for (offset, reveal_at) in local_offsets {
                    let world_offset = Vec2::new(
                        offset.x * cos - offset.y * sin,
                        offset.x * sin + offset.y * cos,
                    );
                    let clone_pos = pos.0 + world_offset;
                    if let Some(clone_entity) = crate::ship::spawn_class(
                        &mut commands,
                        &catalog,
                        &assets,
                        *class,
                        clone_pos,
                        rot_angle,
                        ship.player_slot, // SAME slot — shares input
                        &ship_colliders,
                    ) {
                        commands.entity(clone_entity).insert(PkunkClone {
                            remaining_s: PKUNK_FORMATION_S,
                            total_s: PKUNK_FORMATION_S,
                            reveal_at_pan_t: reveal_at,
                            warp_in_t: 0.0,
                            formation_offset_local: offset,
                            retreating: false,
                        });
                        // Half-HP clones — they're aggressive but
                        // fragile. crew_max from catalog; if the
                        // class is missing, fall back to a small
                        // value so the clone still spawns alive.
                        let half = catalog
                            .ships
                            .get(class.code())
                            .map(|s| (s.crew_max / 2).max(1))
                            .unwrap_or(5);
                        commands.entity(clone_entity).insert(crate::ship::Crew {
                            current: half,
                            max: half,
                        });
                        // Ghostly aura: soft glow disc behind the
                        // clone so the player can immediately tell
                        // clones from the real Pkunk. Position-
                        // tracked in `tick_pkunk_clone_visual`.
                        let aura_mat = glow_materials.add(GlowMaterial {
                            params: Vec4::new(1.0, 0.0, 0.0, 0.0),
                        });
                        commands.spawn((
                            PkunkAura { clone: clone_entity },
                            Mesh2d(quad_mesh.clone()),
                            MeshMaterial2d(aura_mat),
                            // Soft halo ~2× ship footprint. The
                            // ship is 80–100 px depending on class.
                            Transform::from_scale(Vec3::new(180.0, 180.0, 1.0))
                                .with_translation(clone_pos.extend(0.10)),
                        ));
                        state.pkunk_clones.push(clone_entity);
                    }
                }
            }
        }
        // The remaining variants do their spawning later, in
        // their first active phase. Nothing pre-spawned here.
        UltimateVariant::Mmrnmhrm => {
            // Snapshot the ship's current Transform.scale so the
            // exit path can restore it cleanly.
            if let Ok((_, _, _, ship_xf)) = ships.get(entity) {
                state.mmrxf_orig_scale = Some(ship_xf.scale);
            }
            // Spawn the unified "unleashed" overlay sprite. This
            // replaces the previous stack-two-sprites approach
            // which read as ghostly and unclear. The PNG at
            // ultimate/mmrxf_unleashed.png is loaded once and has
            // its white background stripped to alpha by
            // `strip_white_background_once`. Sized to ~2× a
            // normal ship sprite (mmrxf ship_p00 = 100 px tall),
            // so the unleashed form reads as a clear power-up
            // rather than the previous 10× scale wipeout.
            if let Ok((_, _, _, ship_xf)) = ships.get(entity) {
                let overlay = commands
                    .spawn((
                        MmrxfOverlaySprite,
                        Sprite {
                            image: assets.load("ultimate/mmrxf_unleashed.png"),
                            color: Color::srgba(1.0, 1.0, 1.0, 1.0),
                            custom_size: Some(Vec2::splat(200.0)),
                            ..default()
                        },
                        Transform::from_translation(
                            ship_xf.translation.truncate().extend(0.45),
                        ),
                    ))
                    .id();
                state.mmrxf_overlay_entity = Some(overlay);
            }
            // Mark the ship as Mmrxf-ultimate-active so the normal
            // dispatch path skips its primary + special.
            commands.entity(entity).insert(MmrxfActive);
            state.mmrxf_laser_cooldown_s = 0.0;
            state.mmrxf_missile_cooldown_s = 0.0;
        }
        UltimateVariant::Yehat
        | UltimateVariant::Spathi
        | UltimateVariant::Chenjesu
        | UltimateVariant::Shofixti
        | UltimateVariant::Slylandro
        | UltimateVariant::Druuge
        | UltimateVariant::KohrAh
        | UltimateVariant::Mycon
        | UltimateVariant::Thraddash
        | UltimateVariant::None => {}
    }

    // Pause everything else.
    virt.pause();
    state.was_paused = true;

    // Vocal sample. `PlaybackSettings::DESPAWN` removes the
    // AudioPlayer entity when the clip finishes so we don't
    // accumulate. The Arilou variant additionally tags the
    // entity with `ArilouVoicePlayer` — `watch_arilou_voice_end`
    // notices when that marker is removed (because the entity
    // despawned) and fires the stinger SFX in the same tick.
    let mut voice = commands.spawn((
        AudioPlayer::<AudioSource>(assets.load(voice_path(state.variant))),
        PlaybackSettings::DESPAWN,
    ));
    if state.variant == UltimateVariant::Arilou {
        voice.insert(ArilouVoicePlayer);
    }

    info!("ULTIMATE: P{} {}", ship.player_slot + 1, class.code());
}

// ----------------------------------------------------------------
// Per-phase tweens.
// ----------------------------------------------------------------

fn tick_ultimate_phases(
    // Must use `Time<Real>` because the cinematic pauses
    // `Time<Virtual>` during DramaticZoomIn (and other paused
    // phases) — and offline, `Res<Time>` inside GgrsSchedule
    // still resolves to the paused Virtual time (bevy_ggrs only
    // swaps it to GgrsTime when actually driving rollback).
    // If this read delta = 0 during pause, the phase timer
    // would never advance and the cinematic would freeze.
    // Wall-clock divergence between peers is acceptable for
    // cinematic timing — the gameplay state at the moment
    // unpause resumes is what's consistent.
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
    //   `beam_visible`       — Arilou blade visibility
    //   `spin`               — angular velocity (Arilou only)
    //   `should_be_paused`   — Time<Virtual> pause state
    let (phase_total, portrait_alpha, beam_visible, spin, should_be_paused) =
        match state.phase {
            UltimatePhase::DramaticZoomIn => {
                let p = (state.phase_timer_s / PHASE_ZOOM_IN_S).clamp(0.0, 1.0);
                let eased = 1.0 - (1.0 - p).powi(3);
                (PHASE_ZOOM_IN_S, eased, false, 0.0, true)
            }
            UltimatePhase::ArilouUnleashing => {
                let p = (state.phase_timer_s / PHASE_UNLEASH_S).clamp(0.0, 1.0);
                let portrait_p = (p / 0.6).clamp(0.0, 1.0);
                (PHASE_UNLEASH_S, 1.0 - portrait_p, true, HYPER_SPIN_RAD_PER_S, false)
            }
            UltimatePhase::EarthlingCharging => {
                // Portrait holds full alpha through the charge.
                (EARTH_CHARGE_S, 1.0, false, 0.0, true)
            }
            UltimatePhase::EarthlingStretching => {
                // Portrait still full while the spring loads.
                (EARTH_STRETCH_S, 1.0, false, 0.0, true)
            }
            UltimatePhase::EarthlingBlasting => {
                // Portrait fades over the first 35% of the blast.
                let p = (state.phase_timer_s / EARTH_BLAST_S).clamp(0.0, 1.0);
                let portrait_p = (p / 0.35).clamp(0.0, 1.0);
                (EARTH_BLAST_S, 1.0 - portrait_p, false, 0.0, false)
            }
            UltimatePhase::YehatSummoning => {
                (YEHAT_SUMMON_S, 1.0, false, 0.0, true)
            }
            UltimatePhase::YehatBattle => {
                let p = (state.phase_timer_s / YEHAT_BATTLE_S).clamp(0.0, 1.0);
                let portrait_p = (p / 0.4).clamp(0.0, 1.0);
                (YEHAT_BATTLE_S, 1.0 - portrait_p, false, 0.0, false)
            }
            UltimatePhase::SpathiLockOn => (SPATHI_LOCKON_S, 1.0, false, 0.0, true),
            UltimatePhase::SpathiBarrage => {
                let p = (state.phase_timer_s / SPATHI_BARRAGE_S).clamp(0.0, 1.0);
                (SPATHI_BARRAGE_S, 1.0 - p, false, 0.0, false)
            }
            UltimatePhase::ChenjesuCharging => (CHEBR_CHARGE_S, 1.0, false, 0.0, true),
            UltimatePhase::ChenjesuTempest => {
                let p = (state.phase_timer_s / CHEBR_TEMPEST_S).clamp(0.0, 1.0);
                let portrait_p = (p / 0.5).clamp(0.0, 1.0);
                (CHEBR_TEMPEST_S, 1.0 - portrait_p, false, 0.0, false)
            }
            UltimatePhase::ShofixtiCharging => (SHOSC_CHARGE_S, 1.0, false, 0.0, true),
            UltimatePhase::ShofixtiNova => {
                let p = (state.phase_timer_s / SHOSC_NOVA_S).clamp(0.0, 1.0);
                (SHOSC_NOVA_S, 1.0 - p, false, 0.0, false)
            }
            UltimatePhase::PkunkSummoning => (PKUNK_SUMMON_S, 1.0, false, 0.0, true),
            UltimatePhase::PkunkPan => (PKUNK_PAN_S, 1.0, false, 0.0, true),
            UltimatePhase::PkunkFormation => {
                let p = (state.phase_timer_s / PKUNK_FORMATION_S).clamp(0.0, 1.0);
                let portrait_p = (p / 0.35).clamp(0.0, 1.0);
                (PKUNK_FORMATION_S, 1.0 - portrait_p, false, 0.0, false)
            }
            UltimatePhase::SlylandroCharging => (SLYP_CHARGE_S, 1.0, false, 0.0, true),
            UltimatePhase::SlylandroPullback => (SLYP_PULLBACK_S, 1.0, false, 0.0, true),
            UltimatePhase::SlylandroStorm => {
                let p = (state.phase_timer_s / SLYP_STORM_S).clamp(0.0, 1.0);
                let portrait_p = (p / 0.5).clamp(0.0, 1.0);
                (SLYP_STORM_S, 1.0 - portrait_p, false, 0.0, false)
            }
            UltimatePhase::MmrxfTransform => (MMRXF_TRANSFORM_S, 1.0, false, 0.0, true),
            UltimatePhase::MmrxfUnleashing => {
                let p = (state.phase_timer_s / MMRXF_UNLEASH_S).clamp(0.0, 1.0);
                let portrait_p = (p / 0.5).clamp(0.0, 1.0);
                (MMRXF_UNLEASH_S, 1.0 - portrait_p, false, 0.0, false)
            }
            UltimatePhase::DruugeCharging => (DRUUGE_CHARGE_S, 1.0, false, 0.0, true),
            UltimatePhase::DruugeBarrage => {
                let p = (state.phase_timer_s / DRUUGE_BARRAGE_S).clamp(0.0, 1.0);
                (DRUUGE_BARRAGE_S, 1.0 - p, false, 0.0, false)
            }
            UltimatePhase::KohrAhSharpening => (KOHRAH_SHARPEN_S, 1.0, false, 0.0, true),
            UltimatePhase::KohrAhSlaughter => {
                let p = (state.phase_timer_s / KOHRAH_SLAUGHTER_S).clamp(0.0, 1.0);
                (KOHRAH_SLAUGHTER_S, 1.0 - p, false, 0.0, false)
            }
            UltimatePhase::MyconGathering => (MYCON_GATHER_S, 1.0, false, 0.0, true),
            UltimatePhase::MyconHurricane => {
                let p = (state.phase_timer_s / MYCON_HURRICANE_S).clamp(0.0, 1.0);
                (MYCON_HURRICANE_S, 1.0 - p, false, 0.0, false)
            }
            UltimatePhase::ThraddashIgniting => (THRADDASH_IGNITE_S, 1.0, false, 0.0, true),
            UltimatePhase::ThraddashBurning => {
                let p = (state.phase_timer_s / THRADDASH_BURN_S).clamp(0.0, 1.0);
                (THRADDASH_BURN_S, 1.0 - p, false, 0.0, false)
            }
            UltimatePhase::Idle => unreachable!(),
        };

    // Unpause Time<Virtual> the first tick of an unpaused phase.
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

    // Some variants want the player to keep *normal* input
    // control of their ship during the active phases:
    //   - Pkunk: player flies a three-ship formation, so the
    //     original needs to respond to input alongside the clones.
    //   - Slylandro: the asteroids do the work; the ship just
    //     flies around dodging the chaos like normal.
    // For everything else we lock the input via HyperActive + a
    // forced ang_vel write (the cinematic owns the ship's motion).
    let needs_lock = !matches!(
        state.variant,
        UltimateVariant::Pkunk
            | UltimateVariant::Slylandro
            | UltimateVariant::Mmrnmhrm
            // Thraddash keeps the player at the helm — the whole
            // point of the ultimate is to fly around painting kill
            // streaks with the lethal exhaust trail.
            | UltimateVariant::Thraddash
    );
    if needs_lock {
        if let Ok(mut av) = ships.get_mut(p1) {
            av.0 = match state.variant {
                UltimateVariant::Arilou => spin,
                // Earthling locks orientation so the blast goes
                // straight.
                _ => 0.0,
            };
        }
        commands.entity(p1).insert(HyperActive {
            forced_ang_vel: spin,
            beam_width_mult: if beam_visible { 1.0 } else { 0.0 },
        });
    }

    if state.phase_timer_s >= phase_total {
        state.phase_timer_s = 0.0;
        state.phase = match (state.phase, state.variant) {
            // Shared entry: variant decides what comes next.
            (UltimatePhase::DramaticZoomIn, UltimateVariant::Arilou) => {
                UltimatePhase::ArilouUnleashing
            }
            (UltimatePhase::DramaticZoomIn, UltimateVariant::Earthling) => {
                UltimatePhase::EarthlingCharging
            }
            (UltimatePhase::DramaticZoomIn, UltimateVariant::Yehat) => {
                UltimatePhase::YehatSummoning
            }
            (UltimatePhase::DramaticZoomIn, UltimateVariant::Spathi) => {
                UltimatePhase::SpathiLockOn
            }
            (UltimatePhase::DramaticZoomIn, UltimateVariant::Chenjesu) => {
                UltimatePhase::ChenjesuCharging
            }
            (UltimatePhase::DramaticZoomIn, UltimateVariant::Shofixti) => {
                UltimatePhase::ShofixtiCharging
            }
            (UltimatePhase::DramaticZoomIn, UltimateVariant::Pkunk) => {
                UltimatePhase::PkunkSummoning
            }
            (UltimatePhase::DramaticZoomIn, UltimateVariant::Slylandro) => {
                UltimatePhase::SlylandroCharging
            }
            (UltimatePhase::EarthlingCharging, _) => UltimatePhase::EarthlingStretching,
            (UltimatePhase::EarthlingStretching, _) => UltimatePhase::EarthlingBlasting,
            (UltimatePhase::YehatSummoning, _) => UltimatePhase::YehatBattle,
            (UltimatePhase::SpathiLockOn, _) => UltimatePhase::SpathiBarrage,
            (UltimatePhase::ChenjesuCharging, _) => UltimatePhase::ChenjesuTempest,
            (UltimatePhase::ShofixtiCharging, _) => UltimatePhase::ShofixtiNova,
            (UltimatePhase::PkunkSummoning, _) => UltimatePhase::PkunkPan,
            (UltimatePhase::PkunkPan, _) => UltimatePhase::PkunkFormation,
            (UltimatePhase::SlylandroCharging, _) => UltimatePhase::SlylandroPullback,
            (UltimatePhase::SlylandroPullback, _) => UltimatePhase::SlylandroStorm,
            (UltimatePhase::DramaticZoomIn, UltimateVariant::Mmrnmhrm) => {
                UltimatePhase::MmrxfTransform
            }
            (UltimatePhase::MmrxfTransform, _) => UltimatePhase::MmrxfUnleashing,
            (UltimatePhase::DramaticZoomIn, UltimateVariant::Druuge) => {
                UltimatePhase::DruugeCharging
            }
            (UltimatePhase::DruugeCharging, _) => UltimatePhase::DruugeBarrage,
            (UltimatePhase::DramaticZoomIn, UltimateVariant::KohrAh) => {
                UltimatePhase::KohrAhSharpening
            }
            (UltimatePhase::KohrAhSharpening, _) => UltimatePhase::KohrAhSlaughter,
            (UltimatePhase::DramaticZoomIn, UltimateVariant::Mycon) => {
                UltimatePhase::MyconGathering
            }
            (UltimatePhase::MyconGathering, _) => UltimatePhase::MyconHurricane,
            (UltimatePhase::DramaticZoomIn, UltimateVariant::Thraddash) => {
                UltimatePhase::ThraddashIgniting
            }
            (UltimatePhase::ThraddashIgniting, _) => UltimatePhase::ThraddashBurning,
            // Final phases: exit.
            (UltimatePhase::ArilouUnleashing, _)
            | (UltimatePhase::EarthlingBlasting, _)
            | (UltimatePhase::YehatBattle, _)
            | (UltimatePhase::SpathiBarrage, _)
            | (UltimatePhase::ChenjesuTempest, _)
            | (UltimatePhase::ShofixtiNova, _)
            | (UltimatePhase::PkunkFormation, _)
            | (UltimatePhase::SlylandroStorm, _)
            | (UltimatePhase::MmrxfUnleashing, _)
            | (UltimatePhase::DruugeBarrage, _)
            | (UltimatePhase::KohrAhSlaughter, _)
            | (UltimatePhase::MyconHurricane, _)
            | (UltimatePhase::ThraddashBurning, _) => {
                exit_cinematic(&mut state, &mut commands, &mut virt, &mut zoom_state);
                return;
            }
            _ => {
                exit_cinematic(&mut state, &mut commands, &mut virt, &mut zoom_state);
                return;
            }
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
        // get_entity returns Err if the entity was despawned out
        // from under us (rematch reset / class switch mid-cinematic).
        if let Ok(mut e) = commands.get_entity(p1) {
            e.remove::<HyperActive>();
            // Earthling keeps its blast velocity after the
            // cinematic; PostUltimateCoasting lets cap_velocity
            // and apply_player_input know not to snap it back.
            if state.variant == UltimateVariant::Earthling {
                e.insert(PostUltimateCoasting);
            }
            // Drop the Mmrnmhrm-active marker so normal abilities
            // resume + restore the ship's original scale on the
            // next tick via MmrxfNeedsRestore.
            if state.variant == UltimateVariant::Mmrnmhrm {
                e.remove::<MmrxfActive>();
                if let Some(orig) = state.mmrxf_orig_scale {
                    e.insert(MmrxfNeedsRestore { orig });
                }
            }
        }
    }
    if let Some(p) = state.portrait_entity.take() {
        if let Ok(mut e) = commands.get_entity(p) {
            e.try_despawn();
        }
    }
    for e in state.beam_entities.drain(..) {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
    }
    if let Some(g) = state.glow_entity.take() {
        if let Ok(mut e) = commands.get_entity(g) {
            e.try_despawn();
        }
    }
    for f in state.yehat_fighters.drain(..) {
        if let Ok(mut ec) = commands.get_entity(f) {
            ec.try_despawn();
        }
    }
    for clone in state.pkunk_clones.drain(..) {
        if let Ok(mut ec) = commands.get_entity(clone) {
            ec.try_despawn();
        }
    }
    state.glow_material = None;
    state.orig_ship_scale = None;
    state.blast_dir = None;
    state.chebr_ring_timer_s = 0.0;
    // Mmrnmhrm cleanup: despawn the overlay, drop the active
    // marker, and restore the ship's original Transform.scale.
    if let Some(overlay) = state.mmrxf_overlay_entity.take() {
        if let Ok(mut ec) = commands.get_entity(overlay) {
            ec.try_despawn();
        }
    }
    state.mmrxf_orig_scale = None;
    state.mmrxf_laser_cooldown_s = 0.0;
    state.mmrxf_missile_cooldown_s = 0.0;
    state.druuge_shots_fired = 0;
    state.thraddash_flame_timer_s = 0.0;
    // Restore the Thraddash ship's speed_max if we had bumped it.
    // The ship may already be despawned (rematch reset); ignore.
    if let Some(orig) = state.thraddash_orig_speed_max.take() {
        // We can't query in here (no system params), so store it
        // for the next `tick_thraddash_restore` poll. The marker
        // component approach is simpler: insert a marker on the
        // ship in the tick system itself when the phase ends.
        let _ = orig;
    }
    // Despawn any leftover Mycon orbs that weren't released.
    for orb in state.mycon_orbs.drain(..) {
        if let Ok(mut ec) = commands.get_entity(orb) {
            ec.try_despawn();
        }
    }
    state.mycon_orb_t0 = 0.0;
    state.variant = UltimateVariant::None;
    state.beam_materials.clear();
    state.portrait_material = None;
    state.portrait_aspect = 0.0;
    state.portrait_in_t = 0.0;
    zoom_state.target_scale = state.orig_cam_scale;
    state.phase = UltimatePhase::Idle;
    state.phase_timer_s = 0.0;
}

/// If the player's ship gets despawned mid-cinematic (rematch reset,
/// Tab to switch class), tear down the cinematic gracefully on the
/// next frame so we don't try to access a dead entity. Without this
/// the next phase tick hits `commands.entity(p1)` on a despawned id
/// and panics.
fn abort_cinematic_if_ship_gone(
    mut state: ResMut<UltimateState>,
    mut commands: Commands,
    mut virt: ResMut<Time<Virtual>>,
    mut zoom_state: ResMut<ZoomState>,
    ships: Query<(), With<Ship>>,
) {
    if state.phase == UltimatePhase::Idle {
        return;
    }
    if let Some(p1) = state.player_entity {
        if ships.get(p1).is_err() {
            exit_cinematic(&mut state, &mut commands, &mut virt, &mut zoom_state);
        }
    }
}

// ----------------------------------------------------------------
// Camera follow + portrait positioning.
// ----------------------------------------------------------------

fn drive_camera_during_ultimate(
    time: Res<Time<Real>>,
    mut state: ResMut<UltimateState>,
    ships: Query<&Position, With<Ship>>,
    ship_lookup: Query<&Ship>,
    windows: Query<&Window>,
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
        UltimatePhase::ArilouUnleashing => {
            let p = (state.phase_timer_s / PHASE_UNLEASH_S).clamp(0.0, 1.0);
            // Ease-out cubic — fast pull-back at the start so the
            // player sees the whole arena while the blade swings,
            // then smooth into the final framing.
            let eased = 1.0 - (1.0 - p).powi(3);
            (
                1.0 - eased,
                HYPER_CAM_SCALE * (1.0 - eased) + state.orig_cam_scale * eased,
            )
        }
        // Earthling: stay fully zoomed in through charge + stretch,
        // then rapidly pull back during the blast so the action
        // returns to game-scale.
        UltimatePhase::EarthlingCharging | UltimatePhase::EarthlingStretching => {
            (1.0, HYPER_CAM_SCALE)
        }
        UltimatePhase::EarthlingBlasting => {
            let p = (state.phase_timer_s / 0.45).clamp(0.0, 1.0);
            let eased = 1.0 - (1.0 - p).powi(3);
            (
                1.0 - eased,
                HYPER_CAM_SCALE * (1.0 - eased) + state.orig_cam_scale * eased,
            )
        }
        // Yehat: stay zoomed on the firer through the brief
        // summoning beat, then pull back so the player can see
        // the fighters orbit + fight.
        UltimatePhase::YehatSummoning => (1.0, HYPER_CAM_SCALE),
        UltimatePhase::YehatBattle => {
            let p = (state.phase_timer_s / 0.6).clamp(0.0, 1.0);
            let eased = 1.0 - (1.0 - p).powi(3);
            (
                1.0 - eased,
                HYPER_CAM_SCALE * (1.0 - eased) + state.orig_cam_scale * eased,
            )
        }
        // Spathi / Chenjesu / Shofixti: tight on the firer during
        // wind-up, fast pull-back during the action so the player
        // sees the full barrage / ring expansion / nova blast.
        UltimatePhase::SpathiLockOn
        | UltimatePhase::ChenjesuCharging
        | UltimatePhase::ShofixtiCharging => (1.0, HYPER_CAM_SCALE),
        UltimatePhase::SpathiBarrage
        | UltimatePhase::ChenjesuTempest
        | UltimatePhase::ShofixtiNova => {
            let phase_dur = match state.phase {
                UltimatePhase::SpathiBarrage => SPATHI_BARRAGE_S,
                UltimatePhase::ChenjesuTempest => CHEBR_TEMPEST_S,
                _ => SHOSC_NOVA_S,
            };
            let p = (state.phase_timer_s / (phase_dur * 0.6)).clamp(0.0, 1.0);
            let eased = 1.0 - (1.0 - p).powi(3);
            // Pull back farther than the original scale so the
            // big-radius effects (nova, tempest) fit on screen.
            let zoom_far = state.orig_cam_scale.max(1.4);
            (
                1.0 - eased,
                HYPER_CAM_SCALE * (1.0 - eased) + zoom_far * eased,
            )
        }
        // Pkunk: tight on the player ship through Summoning;
        // PkunkPan + PkunkFormation use a custom target (handled
        // below by overriding the lerp target).
        UltimatePhase::PkunkSummoning => (1.0, HYPER_CAM_SCALE),
        UltimatePhase::PkunkPan => (1.0, HYPER_CAM_SCALE),
        UltimatePhase::PkunkFormation => {
            let p = (state.phase_timer_s / 0.5).clamp(0.0, 1.0);
            let eased = 1.0 - (1.0 - p).powi(3);
            let zoom_far = state.orig_cam_scale.max(PKUNK_PAN_FAR_SCALE);
            (
                1.0 - eased,
                PKUNK_PAN_FAR_SCALE * (1.0 - eased) + zoom_far * eased,
            )
        }
        // Slylandro: tight on the firer through Charging; the
        // dedicated Pullback phase eases the camera back to the
        // player's original framing (time still paused), then
        // Storm fires the impulse the moment time resumes.
        UltimatePhase::SlylandroCharging => (1.0, HYPER_CAM_SCALE),
        UltimatePhase::SlylandroPullback => {
            let p = (state.phase_timer_s / SLYP_PULLBACK_S).clamp(0.0, 1.0);
            // Ease-out cubic — quick at the start, settling
            // into the wide framing.
            let eased = 1.0 - (1.0 - p).powi(3);
            let zoom_far = state.orig_cam_scale.max(1.4);
            (
                1.0 - eased,
                HYPER_CAM_SCALE * (1.0 - eased) + zoom_far * eased,
            )
        }
        UltimatePhase::SlylandroStorm => {
            // Pullback already eased us to the wide framing before
            // time resumed; stay there so the impulse fires
            // against a stable view.
            let zoom_far = state.orig_cam_scale.max(1.4);
            (0.0, zoom_far)
        }
        // Mmrnmhrm: tight zoom during the transform beat, then
        // ease back to a wide playable view for the unleashing.
        UltimatePhase::MmrxfTransform => (1.0, HYPER_CAM_SCALE),
        UltimatePhase::MmrxfUnleashing => {
            let p = (state.phase_timer_s / 0.5).clamp(0.0, 1.0);
            let eased = 1.0 - (1.0 - p).powi(3);
            let zoom_far = state.orig_cam_scale.max(1.4);
            (
                1.0 - eased,
                HYPER_CAM_SCALE * (1.0 - eased) + zoom_far * eased,
            )
        }
        // New ultimates: tight during the wind-up, fast pull-back
        // to original framing so the action fits on screen.
        UltimatePhase::DruugeCharging
        | UltimatePhase::KohrAhSharpening
        | UltimatePhase::MyconGathering
        | UltimatePhase::ThraddashIgniting => (1.0, HYPER_CAM_SCALE),
        UltimatePhase::DruugeBarrage
        | UltimatePhase::KohrAhSlaughter
        | UltimatePhase::MyconHurricane
        | UltimatePhase::ThraddashBurning => {
            let phase_dur = match state.phase {
                UltimatePhase::DruugeBarrage => DRUUGE_BARRAGE_S,
                UltimatePhase::KohrAhSlaughter => KOHRAH_SLAUGHTER_S,
                UltimatePhase::MyconHurricane => MYCON_HURRICANE_S,
                _ => THRADDASH_BURN_S,
            };
            let p = (state.phase_timer_s / (phase_dur * 0.4)).clamp(0.0, 1.0);
            let eased = 1.0 - (1.0 - p).powi(3);
            let zoom_far = state.orig_cam_scale.max(1.4);
            (
                1.0 - eased,
                HYPER_CAM_SCALE * (1.0 - eased) + zoom_far * eased,
            )
        }
        UltimatePhase::Idle => return,
    };

    if let Ok((mut cam_xf, mut projection)) = cameras.single_mut() {
        // Pkunk gets a custom multi-keyframe target during PkunkPan
        // so the camera quickly snaps between the three ships
        // before settling on the centroid.
        let mut custom_target: Option<Vec2> = None;
        let mut custom_scale: Option<f32> = None;
        if state.variant == UltimateVariant::Pkunk {
            // Gather clone positions (filter out despawned ones).
            let mut positions: Vec<Vec2> = Vec::with_capacity(3);
            positions.push(ship_pos.0);
            for &c in &state.pkunk_clones {
                if let Ok(p) = ships.get(c) {
                    positions.push(p.0);
                }
            }
            let centroid = if positions.is_empty() {
                ship_pos.0
            } else {
                positions.iter().copied().sum::<Vec2>() / positions.len() as f32
            };
            match state.phase {
                UltimatePhase::PkunkPan => {
                    // Four sub-segments across PKUNK_PAN_S:
                    //   0   .. 0.20: hold on player
                    //   0.20.. 0.45: snap-pan to clone 1
                    //   0.45.. 0.70: snap-pan to clone 2
                    //   0.70.. 1.00: pull back to centroid + zoom out
                    let t = (state.phase_timer_s / PKUNK_PAN_S).clamp(0.0, 1.0);
                    let p1 = positions.first().copied().unwrap_or(ship_pos.0);
                    let c1 = positions.get(1).copied().unwrap_or(centroid);
                    let c2 = positions.get(2).copied().unwrap_or(centroid);
                    let (target, scale_t) = if t < 0.20 {
                        (p1, 0.0)
                    } else if t < 0.45 {
                        let sub = ((t - 0.20) / 0.25).clamp(0.0, 1.0);
                        let eased = 1.0 - (1.0 - sub).powi(3);
                        (p1.lerp(c1, eased), 0.0)
                    } else if t < 0.70 {
                        let sub = ((t - 0.45) / 0.25).clamp(0.0, 1.0);
                        let eased = 1.0 - (1.0 - sub).powi(3);
                        (c1.lerp(c2, eased), 0.0)
                    } else {
                        let sub = ((t - 0.70) / 0.30).clamp(0.0, 1.0);
                        let eased = 1.0 - (1.0 - sub).powi(3);
                        (c2.lerp(centroid, eased), sub)
                    };
                    custom_target = Some(target);
                    // During the first 70% of pan, stay zoomed in.
                    // In the last 30%, ease out to PKUNK_PAN_FAR_SCALE
                    // so the player sees the full formation.
                    custom_scale = Some(
                        HYPER_CAM_SCALE * (1.0 - scale_t)
                            + PKUNK_PAN_FAR_SCALE * scale_t,
                    );
                }
                UltimatePhase::PkunkFormation => {
                    // Follow the centroid of the trio.
                    custom_target = Some(centroid);
                }
                _ => {}
            }
        }

        let blended_default = state.orig_cam_pos.lerp(
            ship_pos.0.extend(cam_xf.translation.z),
            blend,
        );
        if let Some(target) = custom_target {
            // For Pkunk pan, snap directly to the computed target
            // (the per-segment ease is baked into the lerp above).
            cam_xf.translation =
                Vec3::new(target.x, target.y, cam_xf.translation.z);
        } else {
            cam_xf.translation = blended_default;
        }
        let final_scale = custom_scale.unwrap_or(scale);
        if let Projection::Orthographic(ref mut ortho) = *projection {
            ortho.scale = final_scale;
        }
        let scale = final_scale;

        // Portrait zip animation: ease portrait_in_t toward 1.0
        // during DramaticZoomIn (time is frozen — portrait is on
        // screen), toward 0.0 in every other phase (time has
        // resumed — portrait zips back out). 0.12s in / 0.18s out
        // is fast enough to read as a snap but slow enough that
        // the eye sees the motion.
        const ZIP_IN_S: f32 = 0.12;
        const ZIP_OUT_S: f32 = 0.18;
        let target = if matches!(state.phase, UltimatePhase::DramaticZoomIn) {
            1.0
        } else {
            0.0
        };
        let rate = if target > state.portrait_in_t {
            1.0 / ZIP_IN_S
        } else {
            1.0 / ZIP_OUT_S
        };
        let dt = time.delta_secs();
        let step = (target - state.portrait_in_t).signum() * rate * dt;
        // Don't overshoot the target.
        if (target - state.portrait_in_t).abs() <= step.abs() {
            state.portrait_in_t = target;
        } else {
            state.portrait_in_t += step;
        }
        let in_t = state.portrait_in_t.clamp(0.0, 1.0);
        // Cubic ease-out so the slide-in lands gently.
        let eased = 1.0 - (1.0 - in_t).powi(3);

        if let Ok(mut portrait_xf) = portraits.single_mut() {
            // Portrait sized for the side-panel layout — smaller
            // than the old centre-stage placement so it fits
            // vertically alongside one of the two status panels.
            const MAX_SCREEN_DIM_PX: f32 = 320.0;
            let aspect = if state.portrait_aspect > 0.0 {
                state.portrait_aspect
            } else {
                620.0 / 930.0
            };
            let (w_px, h_px) = if aspect >= 1.0 {
                (MAX_SCREEN_DIM_PX, MAX_SCREEN_DIM_PX / aspect)
            } else {
                (MAX_SCREEN_DIM_PX * aspect, MAX_SCREEN_DIM_PX)
            };

            // Window size for screen-edge anchoring. Fall back to
            // the canonical 1280×720 if we can't read the window.
            let (win_w, win_h) = match windows.single() {
                Ok(w) => (w.width().max(1.0), w.height().max(1.0)),
                Err(_) => (1280.0, 720.0),
            };
            let half_w_px = win_w * 0.5;
            let half_h_px = win_h * 0.5;
            /// Pixel inset from the window's left edge to the
            /// portrait's left edge when fully zipped in. Roughly
            /// matches the 12 px panel padding on the right-side
            /// HUD column.
            const LEFT_MARGIN_PX: f32 = 12.0;
            // World-space conversion: 1 screen px = `scale` world units.
            let rest_x_world = (-half_w_px + LEFT_MARGIN_PX + w_px * 0.5) * scale;
            let off_x_world = (-half_w_px - w_px * 0.5 - LEFT_MARGIN_PX) * scale;
            let x_world = off_x_world + (rest_x_world - off_x_world) * eased;

            // Vertical: align the portrait's centre with the
            // centre of the player's status panel. Right-edge HUD
            // splits the column SpaceBetween; for a 720 px window
            // each panel is centred around y = ±half_h * 0.49.
            // Slot 0 (P1) → top → +y in world. Slot 1 (P2) → bottom.
            let slot = state
                .player_entity
                .and_then(|e| ship_lookup.get(e).ok())
                .map(|s| s.player_slot)
                .unwrap_or(0);
            let y_anchor_frac: f32 = 0.49;
            let y_world = if slot == 0 {
                half_h_px * y_anchor_frac * scale
            } else {
                -half_h_px * y_anchor_frac * scale
            };

            let z = portrait_xf.translation.z;
            portrait_xf.translation = cam_xf.translation
                + Vec3::new(x_world, y_world, 0.0);
            portrait_xf.translation.z = z;
            portrait_xf.scale = Vec3::new(w_px * scale, h_px * scale, 1.0);
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
    if state.phase != UltimatePhase::ArilouUnleashing {
        return;
    }
    let dt = time.delta_secs();
    let _ = (dt, &mut rotations);
}

// ----------------------------------------------------------------
// Beam: pose each tick, deal damage, spawn smear trails.
// ----------------------------------------------------------------

fn tick_ultimate_beams(
    time: Res<Time>,
    mut state: ResMut<UltimateState>,
    spatial: SpatialQuery,
    ships: Query<(&Position, &Rotation), With<Ship>>,
    ship_lookup: Query<&Ship>,
    mut crews: Query<&mut Crew>,
    mut beams: Query<(&UltimateBeam, &mut Transform, &Visibility), Without<BeamTrail>>,
    mut color_mats: ResMut<Assets<SoftBladeMaterial>>,
    ultimate_meshes: Res<UltimateMeshes>,
    asteroids: Query<(Entity, &Position), With<crate::ship::Asteroid>>,
    assets: Res<AssetServer>,
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
        UltimatePhase::ArilouUnleashing => {
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

    // Asteroids in the blade's sweep arc explode. The blade spins
    // fast (HYPER_SPIN_RAD_PER_S ≈ 14 rad/s ≈ 0.23 rad/frame at
    // 60 fps); a ±0.18 rad window catches everything the sweep
    // passes through without false positives behind the ship.
    let blade_angle = world_dir.y.atan2(world_dir.x);
    const ARILOU_ASTEROID_ARC: f32 = 0.18;
    for (ast_e, ast_pos) in &asteroids {
        let v = ast_pos.0 - owner_pos;
        let d2 = v.length_squared();
        if d2 > HYPER_BEAM_LEN * HYPER_BEAM_LEN || d2 < 1.0 {
            continue;
        }
        let a = v.y.atan2(v.x);
        let mut delta = a - blade_angle;
        while delta > std::f32::consts::PI {
            delta -= std::f32::consts::TAU;
        }
        while delta < -std::f32::consts::PI {
            delta += std::f32::consts::TAU;
        }
        if delta.abs() > ARILOU_ASTEROID_ARC {
            continue;
        }
        crate::ship::spawn_asteroid_explosion(&mut commands, &assets, ast_pos.0, 24.0);
        if let Ok(mut ec) = commands.get_entity(ast_e) {
            ec.try_despawn();
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
                mat.color = color.to_linear();
            }
        }
    }

    // Trail emission — interpolate between last frame's blade angle
    // and this frame's so the smear is continuous at any frame rate.
    // Spawn TRAIL_SAMPLES_PER_FRAME * 3 layers of ghost copies (one
    // set per layer), each at a sub-frame interpolated angle. Result:
    // at 60 fps + 42 rad/s the blade moves ~12° per frame; we emit
    // 6 copies across that arc, so the trail looks smoothly swept.
    if state.phase != UltimatePhase::ArilouUnleashing {
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
                color_mats.add(SoftBladeMaterial { color: trail_color.to_linear() });
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
                Mesh2d(ultimate_meshes.blade.clone()),
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
    mut color_mats: ResMut<Assets<SoftBladeMaterial>>,
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
            mat.color = LinearRgba::new(lin.red, lin.green, lin.blue, alpha);
        }
    }
}

// ----------------------------------------------------------------
// Earthling jump-to-light-speed — glow, stretch, blast, trail.
// ----------------------------------------------------------------

/// Pulsating glow that overlays the Earthling ship through the
/// Charging and Stretching phases. Sets the glow material's alpha
/// each tick to a `sin`-modulated breathing curve, scales the halo
/// up during the buildup, follows the ship's world position, and
/// stretches the ship sprite during the spring-load phase.
fn tick_lightspeed_glow(
    time: Res<Time<Real>>,
    state: Res<UltimateState>,
    mut glow_mats: ResMut<Assets<GlowMaterial>>,
    ships: Query<(&Position, &Rotation), With<Ship>>,
    mut transforms: Query<&mut Transform, Without<Ship>>,
    mut ship_transforms: Query<&mut Transform, With<Ship>>,
    mut lin_vels: Query<&mut LinearVelocity, With<Ship>>,
) {
    if state.variant != UltimateVariant::Earthling {
        return;
    }
    let Some(p1) = state.player_entity else {
        return;
    };
    let Ok((pos, rot)) = ships.get(p1) else { return };

    // Pulse alpha intensity: ramps up during Charging, holds during
    // Stretching, vanishes at Blasting. The high-frequency `sin`
    // gives the breathing "energy buildup" feel.
    let (base_intensity, halo_scale) = match state.phase {
        UltimatePhase::EarthlingCharging => {
            let p = (state.phase_timer_s / EARTH_CHARGE_S).clamp(0.0, 1.0);
            // Quadratic ramp-up so the build feels like it
            // accelerates toward release.
            (p * p, 200.0 + 80.0 * p)
        }
        UltimatePhase::EarthlingStretching => {
            // Full intensity, halo pulses bigger as the ship
            // stretches — about to release.
            let p = (state.phase_timer_s / EARTH_STRETCH_S).clamp(0.0, 1.0);
            (1.0, 280.0 + 80.0 * (p * p))
        }
        UltimatePhase::EarthlingBlasting => {
            // Quick fade-out as the ship springs forward.
            let p = (state.phase_timer_s / 0.12).clamp(0.0, 1.0);
            (1.0 - p, 360.0 + 200.0 * p)
        }
        _ => (0.0, 200.0),
    };
    let pulse = 0.75 + 0.25 * (time.elapsed_secs() * 14.0).sin();
    let intensity = base_intensity * pulse;

    if let Some(mat_handle) = state.glow_material.clone() {
        if let Some(mat) = glow_mats.get_mut(&mat_handle) {
            mat.params.x = intensity.clamp(0.0, 1.0);
        }
    }
    if let Some(glow_entity) = state.glow_entity {
        if let Ok(mut xf) = transforms.get_mut(glow_entity) {
            xf.translation.x = pos.0.x;
            xf.translation.y = pos.0.y;
            // Rotate the halo to match ship facing and elongate
            // 40% along the forward axis so the glow visibly
            // points where the jump will go — fixes the "I don't
            // understand the orientation of the ellipse" confusion.
            let angle = rot.sin.atan2(rot.cos);
            xf.rotation = Quat::from_rotation_z(angle);
            xf.scale = Vec3::new(halo_scale * 0.7, halo_scale, 1.0);
        }
    }

    // Spring-load: ship swells then snaps back. We use a uniform
    // scale rather than a directional stretch because the ship
    // renders via pre-rotated sprite frames whose texture-Y axis
    // is world-Y, not the ship's facing direction — a directional
    // stretch would only point "correctly" for the up-facing frame.
    // Uniform scale reads as "loading energy" instead of literal
    // elongation but lands the same beat.
    let stretch = match state.phase {
        UltimatePhase::EarthlingCharging => 1.0,
        UltimatePhase::EarthlingStretching => {
            let p = (state.phase_timer_s / EARTH_STRETCH_S).clamp(0.0, 1.0);
            // Ease-in cubic — wind-up accelerates toward release.
            1.0 + (EARTH_STRETCH_PEAK - 1.0) * (p * p * p)
        }
        UltimatePhase::EarthlingBlasting => 1.0,
        _ => 1.0,
    };
    if let Some(orig_scale) = state.orig_ship_scale {
        if let Ok(mut ship_xf) = ship_transforms.get_mut(p1) {
            let _ = rot;
            ship_xf.scale = orig_scale * stretch;
        }
    }

    // Ship loses all inertia at the start of Charging and stays
    // pinned to zero velocity until Blasting starts.
    if matches!(
        state.phase,
        UltimatePhase::EarthlingCharging | UltimatePhase::EarthlingStretching
    ) {
        if let Ok(mut lv) = lin_vels.get_mut(p1) {
            lv.0 = Vec2::ZERO;
        }
    }
}

/// Earthling blast: ship slams forward at 4× speed_max along the
/// direction it was facing at the start of the blast, deals massive
/// contact damage to anything overlapping it each tick, and spawns
/// a jagged streak trail behind itself.
fn tick_earthling_blast(
    time: Res<Time>,
    mut state: ResMut<UltimateState>,
    spatial: SpatialQuery,
    ships: Query<(&Position, &Rotation), With<Ship>>,
    ship_lookup: Query<&Ship>,
    derived_q: Query<&crate::ship::ShipPhysicsDerived>,
    mut lin_vels: Query<&mut LinearVelocity, With<Ship>>,
    mut crews: Query<&mut Crew>,
    mut color_mats: ResMut<Assets<SoftBladeMaterial>>,
    ultimate_meshes: Res<UltimateMeshes>,
    mut commands: Commands,
) {
    if state.variant != UltimateVariant::Earthling
        || state.phase != UltimatePhase::EarthlingBlasting
    {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok((pos, rot)) = ships.get(p1) else { return };

    // Lock the blast direction the first tick — the ship's
    // orientation at the moment of spring-release. After that the
    // ship can't be steered.
    if state.blast_dir.is_none() {
        let world_dir = Vec2::new(-rot.sin, rot.cos);
        state.blast_dir = Some(world_dir);
    }
    let dir = state.blast_dir.unwrap();

    // Override LinearVelocity each frame so cap_velocity / damping
    // can't slow us. (`cap_velocity` is also patched to skip
    // HyperActive ships as a belt-and-suspenders.)
    let speed_max = derived_q
        .get(p1)
        .map(|d| d.speed_max)
        .unwrap_or(200.0);
    let target_vel = dir * speed_max * EARTH_BLAST_SPEED_MULT;
    if let Ok(mut lv) = lin_vels.get_mut(p1) {
        lv.0 = target_vel;
    }

    // Damage everything overlapping the ship's circle this tick.
    // We use a circle shape-intersection ahead of the ship covering
    // the swept area for this frame, so a tiny enemy doesn't
    // tunnel through between physics steps.
    let frame_travel = target_vel.length() * time.delta_secs();
    // Probe radius scales with per-frame travel so a 2x-faster
    // blast still catches small enemies between physics steps.
    let probe_radius = 80.0_f32.max(frame_travel * 0.8);
    let probe_center = pos.0 + dir * frame_travel * 0.5;
    let filter = SpatialQueryFilter::default().with_excluded_entities([p1]);
    let hits = spatial.shape_intersections(
        &Collider::circle(probe_radius),
        probe_center,
        0.0,
        &filter,
    );
    let dmg = (EARTH_BLAST_DMG_PER_SEC * time.delta_secs()).round() as i32;
    if dmg > 0 {
        for hit_entity in hits {
            if let (Ok(target_ship), Ok(owner_ship)) =
                (ship_lookup.get(hit_entity), ship_lookup.get(p1))
            {
                if target_ship.player_slot != owner_ship.player_slot {
                    if let Ok(mut crew) = crews.get_mut(hit_entity) {
                        crew.current = (crew.current - dmg).max(0);
                    }
                }
            }
        }
    }

    // Emit jagged streak trails behind the ship. 3 ghosts per
    // frame, each with random lateral offset and angle jitter so
    // the trail reads as "broken / shattering" rather than a clean
    // line.
    const TRAILS_PER_FRAME: usize = 3;
    for i in 0..TRAILS_PER_FRAME {
        let t = (i as f32 + 0.5) / TRAILS_PER_FRAME as f32;
        let lateral = (fastrand::f32() - 0.5) * 22.0;
        let along = -t * frame_travel * 0.7;
        let tangent = Vec2::new(-dir.y, dir.x);
        let origin = pos.0 + dir * along + tangent * lateral;

        // Random jagged angle offset so segments fork at angles.
        let angle_jit = (fastrand::f32() - 0.5) * 0.30;
        let trail_dir = Vec2::new(
            dir.x * angle_jit.cos() - dir.y * angle_jit.sin(),
            dir.x * angle_jit.sin() + dir.y * angle_jit.cos(),
        );
        let local_angle = trail_dir.y.atan2(trail_dir.x) - std::f32::consts::FRAC_PI_2;

        // Length-jittered streak, oriented BACKWARDS along the
        // blast direction (apex AT the ship, base extending behind).
        // To flip the triangle (apex behind, base toward us), we
        // simply rotate by +π so local +Y points opposite to dir.
        let streak_len = 70.0 + fastrand::f32() * 140.0;
        let streak_width = 8.0 + fastrand::f32() * 14.0;
        let lifetime = 0.45 + fastrand::f32() * 0.40;
        let peak = 0.85 + fastrand::f32() * 0.15;

        // Gradient: hot-white center fading toward blue. The
        // shader-less version just uses one solid colour per trail,
        // varied by the ghost — gives the gradient feel across the
        // population of ghosts.
        let hot = fastrand::f32();
        let color = Color::srgba(
            0.55 + 0.45 * hot,
            0.75 + 0.25 * hot,
            1.0,
            peak,
        );
        let mat_handle =
            color_mats.add(SoftBladeMaterial { color: color.to_linear() });
        commands.spawn((
            BlastTrail {
                remaining_s: lifetime,
                total_s: lifetime,
                peak_alpha: peak,
                base_color: color,
                material: mat_handle.clone(),
            },
            Mesh2d(ultimate_meshes.blade.clone()),
            MeshMaterial2d(mat_handle),
            Transform {
                translation: origin.extend(0.28),
                // Rotate by π extra so the triangle's apex sits at
                // the ship side and the base trails behind.
                rotation: Quat::from_rotation_z(local_angle + std::f32::consts::PI),
                scale: Vec3::new(streak_width, streak_len, 1.0),
            },
        ));
    }
}

/// Tick + fade out the jagged blast trails. Same shape as
/// `tick_beam_trails` but with a faster, more violent decay.
fn tick_blast_trails(
    time: Res<Time<Real>>,
    mut commands: Commands,
    mut color_mats: ResMut<Assets<SoftBladeMaterial>>,
    mut q: Query<(Entity, &mut BlastTrail, &mut Transform)>,
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
        // Width contracts as it fades — opposite of the beam smoke
        // billow; here we want the streak to thin out as it dies.
        xf.scale.x *= 0.985;
        // Length stretches slightly so the streak "leans" away.
        xf.scale.y *= 1.004;
        let alpha = trail.peak_alpha * frac.powf(0.8) * (1.0 - 0.4 * age);
        if let Some(mat) = color_mats.get_mut(&trail.material) {
            let lin = trail.base_color.to_linear();
            mat.color = LinearRgba::new(lin.red, lin.green, lin.blue, alpha.max(0.0));
        }
    }
}

// ----------------------------------------------------------------
// Yehat — battle fleet ultimate
// ----------------------------------------------------------------

/// Three fighter sprites spawn around the Terminator at the start
/// of `YehatSummoning`, orbit the parent at YEHAT_FIGHTER_RADIUS
/// during the `YehatBattle` phase, and auto-fire missiles at the
/// nearest enemy every YEHAT_FIGHTER_FIRE_INTERVAL_S seconds.
fn tick_yehat_battle_fleet(
    time: Res<Time>,
    mut state: ResMut<UltimateState>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    ships: Query<(&Position, &Rotation), With<Ship>>,
    other_ships: Query<(Entity, &Ship, &Position), (With<Ship>, Without<crate::ship::Invisible>)>,
    mut fighters: Query<
        (Entity, &mut YehatFighter, &mut Transform),
        Without<Ship>,
    >,
    owner_ship: Query<&Ship>,
) {
    if state.variant != UltimateVariant::Yehat {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok((ship_pos, _)) = ships.get(p1) else { return };

    // Spawn the three fighters once, on entry to Summoning.
    if state.phase == UltimatePhase::YehatSummoning && state.yehat_fighters.is_empty() {
        use std::f32::consts::TAU;
        for i in 0..3 {
            let angle_offset = (i as f32) * TAU / 3.0;
            let pos = ship_pos.0
                + Vec2::new(angle_offset.cos(), angle_offset.sin())
                    * YEHAT_FIGHTER_RADIUS;
            let id = commands
                .spawn((
                    YehatFighter {
                        owner: p1,
                        angle_offset,
                        fire_cooldown_s: 0.5 + i as f32 * 0.15,
                    },
                    Sprite {
                        image: assets.load("ships/yehte/sprites/ship_p00.png"),
                        color: Color::srgba(1.0, 0.9, 0.55, 1.0),
                        custom_size: Some(Vec2::splat(36.0)),
                        ..default()
                    },
                    Transform::from_translation(pos.extend(0.45)),
                ))
                .id();
            state.yehat_fighters.push(id);
        }
    }

    let active =
        matches!(state.phase, UltimatePhase::YehatSummoning | UltimatePhase::YehatBattle);
    if !active {
        return;
    }
    let in_battle = matches!(state.phase, UltimatePhase::YehatBattle);
    let dt = time.delta_secs();
    let owner_slot = owner_ship.get(p1).map(|s| s.player_slot).unwrap_or(0);

    for (_e, mut fighter, mut xf) in &mut fighters {
        // Orbit. Angle advances in real time so the fighters keep
        // moving even while time<Virtual> is paused (during the
        // summon phase).
        fighter.angle_offset += YEHAT_FIGHTER_ORBIT_RPS * std::f32::consts::TAU * dt;
        let off = Vec2::new(fighter.angle_offset.cos(), fighter.angle_offset.sin())
            * YEHAT_FIGHTER_RADIUS;
        let world = ship_pos.0 + off;
        xf.translation.x = world.x;
        xf.translation.y = world.y;
        // Face the orbit-tangent so the sprite "leans" into the path.
        let tangent_angle = fighter.angle_offset + std::f32::consts::FRAC_PI_2;
        xf.rotation = Quat::from_rotation_z(tangent_angle - std::f32::consts::FRAC_PI_2);

        if !in_battle {
            continue;
        }

        // Fire cooldown.
        fighter.fire_cooldown_s -= dt;
        if fighter.fire_cooldown_s > 0.0 {
            continue;
        }
        fighter.fire_cooldown_s = YEHAT_FIGHTER_FIRE_INTERVAL_S;

        // Pick nearest enemy ship in range.
        let mut best: Option<(Vec2, f32)> = None;
        for (e, s, p) in &other_ships {
            if e == p1 || s.player_slot == owner_slot {
                continue;
            }
            let d2 = (p.0 - world).length_squared();
            if best.map_or(true, |(_, b)| d2 < b) {
                best = Some((p.0, d2));
            }
        }
        let Some((target, _)) = best else { continue };
        let delta = target - world;
        let d = delta.length();
        if d < 0.5 {
            continue;
        }
        let dir = delta / d;
        let speed = 80.0 * crate::ship::SC2_VEL_SCALE;
        let init_angle = dir.y.atan2(dir.x) - std::f32::consts::FRAC_PI_2;

        // Standard Projectile via Avian: handle_projectile_hits
        // picks it up for damage on contact, projectile_lifetime
        // despawns it after `lifetime` seconds.
        commands.spawn((
            crate::ship::Projectile {
                owner: p1,
                damage: 4,
                lifetime: 2.5,
            },
            Sprite {
                image: assets.load("ships/yehte/sprites/shot_a01.png"),
                color: Color::srgb(1.0, 0.85, 0.4),
                custom_size: Some(Vec2::splat(10.0)),
                ..default()
            },
            Transform::from_translation(world.extend(0.5)),
            RigidBody::Dynamic,
            Collider::circle(5.0),
            Sensor,
            Mass(0.5),
            Position(world),
            Rotation::radians(init_angle),
            LinearVelocity(dir * speed),
            AngularVelocity::ZERO,
            LinearDamping(0.0),
            AngularDamping(0.0),
            CollisionEventsEnabled,
        ));
    }
}

// ----------------------------------------------------------------
// Spathi — missile storm ultimate
// ----------------------------------------------------------------

/// Spawn `SPATHI_MISSILE_COUNT` homing missiles in every direction
/// once, at the start of the Barrage phase. They use the existing
/// projectile + Homing pipeline.
fn tick_spathi_barrage(
    mut state: ResMut<UltimateState>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut rng: ResMut<crate::rng::GameRng>,
    ships: Query<&Position, With<Ship>>,
) {
    if state.variant != UltimateVariant::Spathi
        || state.phase != UltimatePhase::SpathiBarrage
    {
        return;
    }
    // Fire-once gate: use phase_timer to detect the entry frame.
    // The phase enters with phase_timer_s == 0; we tick it once
    // and then it grows positive. Fire on the first tick only.
    if state.phase_timer_s > 0.02 {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok(ship_pos) = ships.get(p1) else { return };
    let world = ship_pos.0;
    let speed = 110.0 * crate::ship::SC2_VEL_SCALE;

    for i in 0..SPATHI_MISSILE_COUNT {
        let theta = (i as f32) * std::f32::consts::TAU / SPATHI_MISSILE_COUNT as f32;
        // Determinism-critical: each missile's launch direction
        // feeds homing physics for the duration of its life.
        let jitter = rng.signed_unit() * 0.1;
        let dir = Vec2::new((theta + jitter).cos(), (theta + jitter).sin());
        let init_angle = dir.y.atan2(dir.x) - std::f32::consts::FRAC_PI_2;
        commands.spawn((
            crate::ship::Projectile {
                owner: p1,
                damage: 5,
                lifetime: 3.5,
            },
            crate::ship::Homing {
                target: None,
                turn_rate: crate::ship::sc2_turning(2.5),
            },
            Sprite {
                image: assets.load("ships/spael/sprites/shot_a01.png"),
                color: Color::srgb(1.0, 0.7, 0.85),
                custom_size: Some(Vec2::splat(10.0)),
                ..default()
            },
            Transform::from_translation(world.extend(0.5)),
            RigidBody::Dynamic,
            Collider::circle(6.0),
            Sensor,
            Mass(0.5),
            Position(world + dir * 30.0),
            Rotation::radians(init_angle),
            LinearVelocity(dir * speed),
            AngularVelocity::ZERO,
            LinearDamping(0.0),
            AngularDamping(0.0),
            CollisionEventsEnabled,
        ));
    }
    // Bump phase_timer past the gate so we don't fire again next tick.
    state.phase_timer_s = 0.05;
}

// ----------------------------------------------------------------
// Chenjesu — crystal tempest ultimate
// ----------------------------------------------------------------

/// Each tick during ChenjesuTempest, accumulate time; when the
/// accumulator crosses CHEBR_RING_INTERVAL_S, emit a ring of
/// CHEBR_RING_SIZE crystal shards radiating outward.
fn tick_chenjesu_tempest(
    time: Res<Time>,
    mut state: ResMut<UltimateState>,
    mut commands: Commands,
    mut rng: ResMut<crate::rng::GameRng>,
    ships: Query<&Position, With<Ship>>,
) {
    if state.variant != UltimateVariant::Chenjesu
        || state.phase != UltimatePhase::ChenjesuTempest
    {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok(ship_pos) = ships.get(p1) else { return };
    let dt = time.delta_secs();
    state.chebr_ring_timer_s += dt;
    if state.chebr_ring_timer_s < CHEBR_RING_INTERVAL_S {
        return;
    }
    state.chebr_ring_timer_s -= CHEBR_RING_INTERVAL_S;

    let world = ship_pos.0;
    let speed = 80.0 * crate::ship::SC2_VEL_SCALE;
    // All draws are gameplay-critical (drive projectile
    // trajectories / colliders / spin).
    let theta_offset = rng.f32() * std::f32::consts::TAU;
    for i in 0..CHEBR_RING_SIZE {
        let theta = theta_offset
            + (i as f32) * std::f32::consts::TAU / CHEBR_RING_SIZE as f32;
        let dir = Vec2::new(theta.cos(), theta.sin());
        let init_angle = dir.y.atan2(dir.x) - std::f32::consts::FRAC_PI_2;
        let speed_mult = 0.7 + rng.f32() * 0.6;
        let mut verts = [Vec2::ZERO; 3];
        for (j, v) in verts.iter_mut().enumerate() {
            let base_a = j as f32 * std::f32::consts::TAU / 3.0;
            let a = base_a + rng.signed_unit() * 0.30;
            let r = 4.0 + rng.f32() * 5.0;
            *v = Vec2::new(a.cos() * r, a.sin() * r);
        }
        // Colour draws stay on the seeded RNG too so the
        // stream advances the same number of slots per shard.
        let cr = 0.7 + rng.f32() * 0.3;
        let cg = 0.8 + rng.f32() * 0.2;
        let ang_v = rng.signed_unit() * 6.0;
        commands.spawn((
            crate::ship::Projectile {
                owner: p1,
                damage: 2,
                lifetime: 2.4,
            },
            Sprite {
                color: Color::srgb(cr, cg, 1.0),
                custom_size: Some(Vec2::splat(10.0)),
                ..default()
            },
            Transform::from_translation((world + dir * 24.0).extend(0.5)),
            RigidBody::Dynamic,
            Collider::triangle(verts[0], verts[1], verts[2]),
            Sensor,
            Mass(0.6),
            Position(world + dir * 24.0),
            Rotation::radians(init_angle),
            LinearVelocity(dir * speed * speed_mult),
            AngularVelocity(ang_v),
            LinearDamping(0.0),
            AngularDamping(0.0),
            CollisionEventsEnabled,
        ));
    }
}

// ----------------------------------------------------------------
// Shofixti — nova sacrifice ultimate
// ----------------------------------------------------------------

/// At entry to ShofixtiNova: spawn a DamageZone with a huge radius
/// that wipes everything within blast distance, including the
/// firer (set its crew to 0). Standard tick_damage_zones handles
/// per-tick damage application + lifetime decay.
fn tick_shofixti_nova(
    mut state: ResMut<UltimateState>,
    mut commands: Commands,
    mut crews: Query<&mut crate::ship::Crew>,
    ships: Query<&Position, With<Ship>>,
) {
    if state.variant != UltimateVariant::Shofixti
        || state.phase != UltimatePhase::ShofixtiNova
    {
        return;
    }
    if state.phase_timer_s > 0.02 {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok(ship_pos) = ships.get(p1) else { return };
    let world = ship_pos.0;

    // The nova damage zone — covers a huge radius, ticks for a
    // brief moment. damage_per_sec is enormous so anything caught
    // in the radius dies within the half-second lifetime.
    // Reuse the existing spawn_damage_zone helper so the bundle
    // shape matches what tick_damage_zones expects (manual spawns
    // ran into Bevy `#[require]` conflicts with Avian).
    crate::ship::spawn_damage_zone(
        &mut commands,
        None, // friendly-fire ON — canon Glory kills self
        world,
        SHOSC_NOVA_RADIUS,
        2000.0,
        SHOSC_NOVA_S,
        Color::srgba(1.0, 0.95, 0.6, 0.55),
    );

    // The Scout sacrifices itself in canon — set crew to 0 so the
    // post-match flow picks up the loss.
    if let Ok(mut crew) = crews.get_mut(p1) {
        crew.current = 0;
    }
    state.phase_timer_s = 0.05;
}

// ----------------------------------------------------------------
// Pkunk — clone-formation ultimate
// ----------------------------------------------------------------

/// Tick PkunkClone lifetimes during PkunkFormation; despawn when
/// the timer expires. The clones share the player's input by
/// virtue of having the same player_slot, so no input plumbing is
/// needed here — they just fly with the same controls until the
/// timer runs out.
fn tick_pkunk_clones(
    time: Res<Time>,
    state: Res<UltimateState>,
    mut commands: Commands,
    mut clones: Query<(Entity, &mut PkunkClone)>,
) {
    // Only count down while the formation is active. During earlier
    // paused phases the clones exist but their timer doesn't run.
    if state.phase != UltimatePhase::PkunkFormation {
        return;
    }
    let dt = time.delta_secs();
    for (e, mut clone) in &mut clones {
        clone.remaining_s -= dt;
        if clone.remaining_s <= 0.0 {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_despawn();
            }
        }
    }
}

/// Pulsing magenta-cyan tint on PkunkClone sprites so the player
/// can tell the ephemeral copies from the real ship. Also fades
/// alpha as the clone's remaining_s drops, so the visual countdown
/// communicates "you're about to lose your wingmen".
///
/// Runs in Update after swap_rotation_frame so the per-frame sprite
/// image swap doesn't overwrite our color. swap_rotation_frame
/// touches `image` only, not `color`, so they coexist fine — this
/// is just a write to a different field.
/// Position-track and fade each `PkunkAura` to its clone each
/// frame. Despawns the aura if its clone has been destroyed
/// (crew damage → `destroy_zero_crew_ships`).
fn tick_pkunk_auras(
    time: Res<Time<Real>>,
    state: Res<UltimateState>,
    mut commands: Commands,
    clone_pose: Query<(&Position, &PkunkClone), With<crate::ship::Ship>>,
    mut auras: Query<(Entity, &PkunkAura, &mut Transform, &MeshMaterial2d<GlowMaterial>)>,
    mut glow_mats: ResMut<Assets<GlowMaterial>>,
) {
    let t = time.elapsed_secs();
    let cinematic_active = matches!(
        state.phase,
        UltimatePhase::PkunkSummoning
            | UltimatePhase::PkunkPan
            | UltimatePhase::PkunkFormation
    );
    for (aura_e, aura, mut xf, mat_handle) in &mut auras {
        // Clone gone → aura gone.
        let Ok((pos, clone)) = clone_pose.get(aura.clone) else {
            if let Ok(mut ec) = commands.get_entity(aura_e) {
                ec.try_despawn();
            }
            continue;
        };
        xf.translation.x = pos.0.x;
        xf.translation.y = pos.0.y;
        // Pulse the glow intensity. Slightly higher while
        // retreating so the "recharging" state reads visually.
        let base = if clone.retreating { 1.4 } else { 1.0 };
        let pulse = 0.7 + 0.3 * (t * 5.0).sin();
        let life = (clone.remaining_s / clone.total_s).clamp(0.0, 1.0);
        let visible = if cinematic_active {
            clone.warp_in_t * life
        } else {
            0.0
        };
        if let Some(mat) = glow_mats.get_mut(mat_handle.id()) {
            mat.params.x = base * pulse * visible;
        }
    }
}

fn tick_pkunk_clone_visual(
    time: Res<Time<Real>>,
    state: Res<UltimateState>,
    mut clones: Query<(&mut PkunkClone, &mut Sprite)>,
) {
    let t = time.elapsed_secs();
    // PkunkPan timer fraction (0..1) — clones use this against
    // their own `reveal_at_pan_t` to decide if they should be
    // warping into view yet. Before PkunkPan: stay hidden.
    // After PkunkPan ends: all clones are revealed.
    let pan_t = match state.phase {
        UltimatePhase::PkunkSummoning => 0.0,
        UltimatePhase::PkunkPan => {
            (state.phase_timer_s / PKUNK_PAN_S).clamp(0.0, 1.0)
        }
        UltimatePhase::PkunkFormation => 1.0,
        _ => 0.0,
    };
    let dt = time.delta_secs();
    for (mut clone, mut sprite) in &mut clones {
        // Ramp warp_in_t toward the target (1 once the camera has
        // arrived at this clone's spot, 0 before). 0.15 s to fully
        // pop in — fast enough to feel like a snap-warp, slow
        // enough that the eye registers it.
        let target = if pan_t >= clone.reveal_at_pan_t { 1.0 } else { 0.0 };
        let direction = (target - clone.warp_in_t).signum();
        clone.warp_in_t =
            (clone.warp_in_t + direction * (1.0 / 0.15) * dt).clamp(0.0, 1.0);

        // Pulsing magenta ↔ cyan tint for the ephemeral feel.
        let phase = (t * 4.0).sin() * 0.5 + 0.5;
        let r = 1.0;
        let g = 0.45 + 0.4 * phase;
        let b = 0.85 + 0.15 * (1.0 - phase);
        // Alpha fades with both warp-in (snap-pop) and remaining
        // lifetime (visibly wears thin as the timer runs out).
        let life = (clone.remaining_s / clone.total_s).clamp(0.0, 1.0);
        let life_alpha = 0.45 + 0.45 * life;
        let alpha = life_alpha * clone.warp_in_t;
        sprite.color = Color::srgba(r, g, b, alpha);
    }
}

/// Each frame while the cinematic is active, check whether the
/// portrait image has finished decoding; if so, snapshot its
/// width/height ratio so `drive_camera_during_ultimate` can scale
/// the portrait without stretching. The probe is cheap (a couple
/// of asset lookups) and stops the moment the aspect is known.
fn detect_portrait_aspect(
    mut state: ResMut<UltimateState>,
    images: Res<Assets<Image>>,
    materials: Res<Assets<PortraitMaterial>>,
) {
    if state.portrait_aspect > 0.0 || state.phase == UltimatePhase::Idle {
        return;
    }
    let Some(mat_handle) = state.portrait_material.clone() else { return };
    let Some(mat) = materials.get(&mat_handle) else { return };
    let Some(image) = images.get(&mat.image) else { return };
    let size = image.size();
    if size.x > 0 && size.y > 0 {
        state.portrait_aspect = size.x as f32 / size.y as f32;
    }
}

// ----------------------------------------------------------------
// Slylandro — asteroid storm ultimate
// ----------------------------------------------------------------

/// Slylandro flow split across two phases:
///   - On entry to SlylandroCharging: zero every Asteroid's
///     linear + angular velocity ("dead stop") and stamp the
///     SlylandroLaunched marker so they immediately start
///     glowing. No impulse yet — the rocks just freeze + light up.
///   - On entry to SlylandroStorm: pick the nearest enemy ship,
///     compute a *predictive* intercept for each asteroid using
///     the target's current velocity, and write that intercept
///     velocity as a one-shot impulse. Speeds are randomised in
///     [SLYP_LAUNCH_SPEED_MIN, _MAX], aim is jittered by at most
///     SLYP_AIM_JITTER_RAD on either side.
fn tick_slylandro_storm(
    mut state: ResMut<UltimateState>,
    mut commands: Commands,
    mut rng: ResMut<crate::rng::GameRng>,
    ships: Query<(Entity, &crate::ship::Ship, &Position, &LinearVelocity)>,
    mut asteroids: Query<
        (
            Entity,
            &mut LinearVelocity,
            &mut AngularVelocity,
            &Position,
            Option<&SlylandroLaunched>,
        ),
        (With<crate::ship::Asteroid>, Without<crate::ship::Ship>),
    >,
) {
    if state.variant != UltimateVariant::Slylandro {
        return;
    }
    let Some(firer) = state.player_entity else { return };

    // Phase 1: freeze + arm. Fire once on entry to Charging.
    if state.phase == UltimatePhase::SlylandroCharging && state.phase_timer_s <= 0.02 {
        let mut stopped = 0;
        for (asteroid, mut lin, mut ang, _pos, marker) in &mut asteroids {
            if marker.is_some() {
                continue;
            }
            lin.0 = Vec2::ZERO;
            ang.0 = 0.0;
            commands.entity(asteroid).insert(SlylandroLaunched {
                owner: firer,
                damage: SLYP_ASTEROID_DAMAGE,
                remaining_s: SLYP_ARMED_LIFE_S,
                total_s: SLYP_ARMED_LIFE_S,
                hit_flash_remaining_s: 0.0,
            });
            stopped += 1;
        }
        info!("Slylandro storm: froze {} asteroids", stopped);
        // Bump the timer just enough that we don't re-trigger.
        state.phase_timer_s = 0.05;
        return;
    }

    // Phase 2: launch impulse. Fire once on entry to Storm.
    if state.phase != UltimatePhase::SlylandroStorm || state.phase_timer_s > 0.02 {
        return;
    }

    let Ok((_, firer_ship, firer_pos, _)) = ships.get(firer) else { return };
    let firer_slot = firer_ship.player_slot;

    // Pick the nearest non-friendly ship to aim the swarm at.
    let mut target_state: Option<(Vec2, Vec2)> = None;
    let mut best_d2 = f32::INFINITY;
    for (e, s, p, v) in &ships {
        if e == firer || s.player_slot == firer_slot {
            continue;
        }
        let d2 = (p.0 - firer_pos.0).length_squared();
        if d2 < best_d2 {
            best_d2 = d2;
            target_state = Some((p.0, v.0));
        }
    }
    // No enemy → aim toward the world centre. Better than nothing
    // (and won't happen during a normal match).
    let (target_pos, target_vel) =
        target_state.unwrap_or((Vec2::ZERO, Vec2::ZERO));

    // Determinism-critical: iterate asteroids in a stable
    // order so the RNG stream consumed in the loop body
    // assigns the same draws to the same asteroids on every
    // peer. Bevy `Query` iteration order is not stable across
    // machines; sort by Entity index.
    let mut entries: Vec<(Entity, _, _, _, _)> = asteroids.iter_mut().collect();
    entries.sort_by_key(|(e, _, _, _, _)| e.index());

    let mut count = 0;
    for (_asteroid, mut vel, _ang, pos, marker) in entries {
        if marker.is_none() {
            // Asteroid wasn't armed during charging — skip.
            continue;
        }
        // Per-asteroid random speed across the launch range.
        // Determinism-critical: this drives the impulse that
        // moves the asteroid, and a wrong direction means peers
        // disagree about who got hit.
        let speed = SLYP_LAUNCH_SPEED_MIN
            + rng.f32() * (SLYP_LAUNCH_SPEED_MAX - SLYP_LAUNCH_SPEED_MIN);
        // Predictive aim: solve the quadratic for the time τ at
        // which a projectile launched from `pos.0` at `speed` will
        // intercept a target at `target_pos` moving with
        // `target_vel`. If the target is faster than the projectile
        // and moving away, or no positive root exists, fall back
        // to a direct aim at the current target position.
        let delta = target_pos - pos.0;
        let intercept_dir = match intercept_time(delta, target_vel, speed) {
            Some(tau) => {
                let intercept = target_pos + target_vel * tau;
                let aim = intercept - pos.0;
                if aim.length_squared() > 1.0 {
                    aim.normalize()
                } else {
                    delta.normalize_or_zero()
                }
            }
            None => delta.normalize_or_zero(),
        };
        // Apply a small random jitter around the intercept aim so
        // the swarm doesn't read as 8 lines converging to a point.
        let jitter = rng.signed_unit() * SLYP_AIM_JITTER_RAD;
        let (cj, sj) = (jitter.cos(), jitter.sin());
        let dir = Vec2::new(
            intercept_dir.x * cj - intercept_dir.y * sj,
            intercept_dir.x * sj + intercept_dir.y * cj,
        );
        vel.0 = dir * speed;
        count += 1;
    }
    info!(
        "Slylandro storm: launched {} asteroids (predictive aim, target_vel=({:.0}, {:.0}))",
        count, target_vel.x, target_vel.y
    );
    state.phase_timer_s = 0.05;
}

/// Classic intercept-time solver for a projectile launching from
/// the origin at speed `s` toward a target moving with velocity
/// `tv` and current relative position `d`. Returns the smaller
/// positive root of `(tv·tv - s²)·t² + 2(d·tv)·t + d·d = 0`, or
/// `None` if no positive intercept exists (target outrunning a
/// slow projectile is the typical failure case).
fn intercept_time(d: Vec2, tv: Vec2, s: f32) -> Option<f32> {
    let a = tv.length_squared() - s * s;
    let b = 2.0 * d.dot(tv);
    let c = d.length_squared();
    if a.abs() < 1e-6 {
        if b.abs() < 1e-6 {
            return None;
        }
        let t = -c / b;
        return (t > 0.0).then_some(t);
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let sd = disc.sqrt();
    let t1 = (-b + sd) / (2.0 * a);
    let t2 = (-b - sd) / (2.0 * a);
    // Smallest positive root (= soonest intercept).
    let candidates = [t1, t2];
    candidates
        .iter()
        .copied()
        .filter(|t| t.is_finite() && *t > 0.0)
        .fold(None, |acc: Option<f32>, t| match acc {
            None => Some(t),
            Some(prev) => Some(prev.min(t)),
        })
}

/// While `SlylandroLaunched` is on an asteroid, tick its remaining
/// armed time + pulse a bright cyan-white tint on the sprite so
/// the eye reads it as glowing-hot. Expiry removes the marker and
/// restores the asteroid's normal grey tint so it goes back to
/// being a benign drifting rock.
fn tick_slylandro_glow(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(
        Entity,
        &mut SlylandroLaunched,
        &mut Sprite,
        &Position,
        &Transform,
    )>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (e, mut launched, mut sprite, pos, xf) in &mut q {
        launched.remaining_s -= dt;
        launched.hit_flash_remaining_s =
            (launched.hit_flash_remaining_s - dt).max(0.0);
        if launched.remaining_s <= 0.0 {
            // Done glowing — restore default white tint so the
            // asteroid sprite renders at its native colour.
            sprite.color = Color::WHITE;
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.remove::<SlylandroLaunched>();
            }
            continue;
        }

        // Tint: hot-flash white briefly after a damage hit,
        // otherwise a pulsing cyan-white glow.
        let life_frac = (launched.remaining_s / launched.total_s).clamp(0.0, 1.0);
        let flash = (launched.hit_flash_remaining_s / SLYP_HIT_FLASH_S).clamp(0.0, 1.0);
        if flash > 0.0 {
            sprite.color = Color::srgba(1.0, 1.0, 1.0, 1.0);
        } else {
            let pulse = (t * 18.0).sin() * 0.5 + 0.5;
            let r = 0.65 + 0.35 * pulse * life_frac;
            let g = 0.85 + 0.15 * pulse;
            let b = 1.0;
            let a = 0.6 + 0.4 * pulse * life_frac;
            sprite.color = Color::srgba(r, g, b, a);
        }

        // Emit one fading ghost copy per asteroid per frame — the
        // "comet trail" look. Reuses the asteroid's current sprite
        // image so the trail visually matches.
        let trail_color =
            Color::srgba(0.55, 0.85, 1.0, 0.55 * life_frac);
        commands.spawn((
            AsteroidGhost {
                remaining_s: 0.40,
                total_s: 0.40,
                base_color: trail_color,
            },
            Sprite {
                image: sprite.image.clone(),
                color: trail_color,
                custom_size: sprite.custom_size,
                ..default()
            },
            Transform {
                translation: pos.0.extend(0.05),
                // Inherit a snapshot of the current rotation so
                // the trail "ghost" looks like the rock at that
                // moment.
                rotation: xf.rotation,
                scale: xf.scale,
            },
        ));
    }
}

/// Decay each AsteroidGhost's alpha + slight scale over its life
/// and despawn at zero. Same shape as BeamTrail but textured.
fn tick_asteroid_ghosts(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut AsteroidGhost, &mut Sprite, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut ghost, mut sprite, mut xf) in &mut q {
        ghost.remaining_s -= dt;
        if ghost.remaining_s <= 0.0 {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_despawn();
            }
            continue;
        }
        let frac = (ghost.remaining_s / ghost.total_s).clamp(0.0, 1.0);
        let lin = ghost.base_color.to_linear();
        let alpha = lin.alpha * frac.powf(0.6);
        sprite.color = Color::srgba(lin.red, lin.green, lin.blue, alpha);
        // Slight scale shrink so the ghost "thins out" as it fades.
        xf.scale *= 0.985;
    }
}

/// CollisionStart handler for launched asteroids. On any contact
/// between a SlylandroLaunched asteroid and a Ship that isn't the
/// firer, deduct `damage` crew (shield-multiplied). The asteroid
/// itself isn't despawned — it keeps bouncing through the field
/// for the rest of its glow timer, potentially hitting the same
/// target multiple times.
fn handle_slylandro_asteroid_hits(
    mut reader: MessageReader<CollisionStart>,
    mut launched: Query<&mut SlylandroLaunched>,
    ships: Query<&crate::ship::Ship>,
    shields: Query<&crate::ship::ShieldActive>,
    mut crews: Query<&mut crate::ship::Crew>,
) {
    for event in reader.read() {
        let (asteroid, ship_e) = if launched.get(event.collider1).is_ok() {
            (event.collider1, event.collider2)
        } else if launched.get(event.collider2).is_ok() {
            (event.collider2, event.collider1)
        } else {
            continue;
        };
        let damage_amt = {
            let Ok(launch) = launched.get(asteroid) else { continue };
            if launch.owner == ship_e {
                continue;
            }
            launch.damage
        };
        // Only damage ships — bounces off other asteroids or
        // projectiles are physics-only.
        if ships.get(ship_e).is_err() {
            continue;
        }
        let factor = shields
            .get(ship_e)
            .map(|s| s.damage_factor)
            .unwrap_or(1.0);
        let dmg = ((damage_amt as f32 * factor).round() as i32).max(0);
        if dmg > 0 {
            if let Ok(mut crew) = crews.get_mut(ship_e) {
                crew.current = (crew.current - dmg).max(0);
            }
            // Flash the asteroid white so the player can see
            // which rocks landed crew damage this frame.
            if let Ok(mut launch) = launched.get_mut(asteroid) {
                launch.hit_flash_remaining_s = SLYP_HIT_FLASH_S;
            }
        }
    }
}

/// Spawn the Arilou stinger SFX the instant the cinematic enters
/// `ArilouUnleashing` (i.e. exactly when the blade swings start)
/// and tag it with `ArilouStinger` carrying the attack duration.
/// `tick_arilou_stinger` then fades the AudioSink volume linearly
/// over that window so the SFX matches the attack's lifetime.
fn spawn_arilou_stinger_on_unleash(
    state: Res<UltimateState>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut local_started: Local<bool>,
) {
    let active = state.variant == UltimateVariant::Arilou
        && state.phase == UltimatePhase::ArilouUnleashing;
    if !active {
        // Reset for next cinematic.
        *local_started = false;
        return;
    }
    if *local_started {
        return;
    }
    *local_started = true;
    commands.spawn((
        AudioPlayer::<AudioSource>(assets.load("ultimate/arisk_stinger.mp3")),
        PlaybackSettings::DESPAWN,
        ArilouStinger {
            total_s: PHASE_UNLEASH_S,
            remaining_s: PHASE_UNLEASH_S,
        },
    ));
}

/// Fade the Arilou stinger SFX volume linearly from 1.0 → 0.0 over
/// its attack window; despawn the entity at zero so the cinematic
/// finishes clean. AudioSink is added by bevy_audio once playback
/// actually starts, so we look it up via the Option<&AudioSink>
/// query and only call .set_volume() once it's present.
fn tick_arilou_stinger(
    time: Res<Time<Real>>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut ArilouStinger, Option<&mut AudioSink>)>,
) {
    let dt = time.delta_secs();
    for (e, mut stinger, sink) in &mut q {
        stinger.remaining_s -= dt;
        if stinger.remaining_s <= 0.0 {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_despawn();
            }
            continue;
        }
        if let Some(mut sink) = sink {
            let frac = (stinger.remaining_s / stinger.total_s).clamp(0.0, 1.0);
            sink.set_volume(bevy::audio::Volume::Linear(frac));
        }
    }
}

/// Builds the quad and triangle meshes at Startup, inserts them
/// into the Mesh asset registry, and stashes STRONG handles in
/// `UltimateMeshes` so they survive asset GC. The cinematic code
/// then hands out clones of those strong handles instead of
/// relying on the (now-deprecated) `uuid_handle!` consts, which
/// were `Handle::Uuid` and didn't keep their referent alive.
pub fn build_ultimate_meshes(
    mut meshes: ResMut<Assets<Mesh>>,
    mut ultimate_meshes: ResMut<UltimateMeshes>,
) {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};

    // 1×1 quad for the portrait + glow halo. Width/height come
    // from `Transform.scale`.
    ultimate_meshes.quad = meshes.add(Rectangle::new(1.0, 1.0));

    // Isosceles triangle with apex at local (0, 0), base at
    // (±0.5, 1). Custom UVs encode the per-vertex barycentric
    // weights for the soft-edge shader to read.
    let mut tri = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    tri.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[0.0, 0.0, 0.0], [-0.5, 1.0, 0.0], [0.5, 1.0, 0.0]],
    );
    tri.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[1.0_f32, 0.0_f32], [0.0_f32, 0.0_f32], [0.0_f32, 1.0_f32]],
    );
    tri.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        vec![[0.0_f32, 0.0, 1.0]; 3],
    );
    tri.insert_indices(Indices::U32(vec![0, 1, 2]));
    ultimate_meshes.blade = meshes.add(tri);
}

/// Aggressive Pkunk clone AI with charge/retreat hysteresis.
///
/// State machine:
///   - CHARGING (default): steer toward the nearest enemy, thrust
///     in, whirl through the kill. `dispatch_primary` force-fires
///     the lightning each tick, so the clone shoots non-stop.
///   - RETREATING: triggered when battery drops below
///     `RETREAT_BATT_LOW`. Turn 180° from the enemy, thrust away,
///     and `dispatch_special` force-presses the Pkunk's
///     RefillBattery special. Held until battery climbs back to
///     `RESUME_BATT_HIGH`, then back to CHARGING.
///
/// Hysteresis prevents thrash near the threshold. The two
/// thresholds also give the clone enough battery in reserve to
/// actually USE the special (special_drain costs 2 — if we let
/// battery hit zero we'd lock ourselves out of recharging).
///
/// Runs `.after(apply_player_input)` so its overrides win over
/// the leader's input on the shared slot.
pub fn tick_pkunk_aggressive_clones(
    state: Res<crate::ultimate::UltimateState>,
    enemies: Query<(Entity, &crate::ship::Ship, &Position), With<crate::ship::Ship>>,
    mut clones: Query<
        (
            Entity,
            &crate::ship::Ship,
            &mut PkunkClone,
            &Position,
            &Rotation,
            &crate::ship::Battery,
            &crate::ship::ShipPhysicsDerived,
            &mut ConstantLocalForce,
            &mut AngularVelocity,
        ),
        With<crate::ship::Ship>,
    >,
) {
    if state.variant != UltimateVariant::Pkunk
        || state.phase != UltimatePhase::PkunkFormation
    {
        return;
    }

    use std::f32::consts::{FRAC_PI_2, PI, TAU};

    /// Whirl overlay (rad/s) blended in once the clone is roughly
    /// on-bearing — the death-spiral signature.
    const PKUNK_AGGRO_SPIN: f32 = 6.0;
    /// Switch CHARGING → RETREATING when battery falls at/below
    /// this. Leaves enough headroom (≥ special_drain=2) to
    /// actually trigger the refill special.
    const RETREAT_BATT_LOW: i32 = 4;
    /// Switch RETREATING → CHARGING when battery climbs back at
    /// or above this. Wider band = less thrash.
    const RESUME_BATT_HIGH: i32 = 10;

    for (clone_e, clone_ship, mut clone, pos, rot, batt, derived, mut thrust, mut ang_vel) in &mut clones {
        // Update state with hysteresis.
        if clone.retreating {
            if batt.current >= RESUME_BATT_HIGH {
                clone.retreating = false;
            }
        } else if batt.current <= RETREAT_BATT_LOW {
            clone.retreating = true;
        }

        // Pick nearest non-friendly ship.
        let mut best: Option<(Vec2, f32)> = None;
        for (e, s, p) in &enemies {
            if e == clone_e || s.player_slot == clone_ship.player_slot {
                continue;
            }
            let d2 = (p.0 - pos.0).length_squared();
            if best.map_or(true, |(_, bd2)| d2 < bd2) {
                best = Some((p.0, d2));
            }
        }
        let Some((target_pos, _)) = best else { continue };

        let cur_heading = rot.sin.atan2(rot.cos);
        let to_target = target_pos - pos.0;
        // Bearing toward the target if charging, 180° away if
        // retreating. The clone always thrusts forward (+Y in
        // local frame), so flipping the bearing flips the run.
        let desired_world = if clone.retreating {
            -to_target
        } else {
            to_target
        };
        let bearing = desired_world.y.atan2(desired_world.x) - FRAC_PI_2;
        let mut steer_err = bearing - cur_heading;
        while steer_err > PI {
            steer_err -= TAU;
        }
        while steer_err < -PI {
            steer_err += TAU;
        }

        if clone.retreating {
            // Just turn-and-burn away. No whirl — the spin is the
            // aggro signature, not the recharge one.
            ang_vel.0 = steer_err.signum() * derived.target_omega;
        } else {
            // Charge: steering term dominates when off-bearing,
            // constant whirl takes over once nearly on-course so
            // the clone whirls through the kill. Whirl sign biased
            // by spawn-side so the two clones spin opposite ways.
            let spin_sign = if clone.formation_offset_local.x < 0.0 {
                -1.0
            } else {
                1.0
            };
            let steer_omega = steer_err.signum() * derived.target_omega;
            let blend = (steer_err.abs() / FRAC_PI_2).clamp(0.0, 1.0);
            ang_vel.0 = steer_omega * blend + spin_sign * PKUNK_AGGRO_SPIN * (1.0 - blend);
        }

        // Always thrust forward — clones close in on or run from
        // the enemy with the same physical action.
        thrust.0 = Vec2::new(0.0, derived.thrust_force);
    }
}

// ----------------------------------------------------------------
// Mmrnmhrm — transform ultimate
// ----------------------------------------------------------------

/// During MmrxfTransform + MmrxfUnleashing:
///   - Hide the underlying ship sprite so we don't see two ships
///     stacked. The ship entity itself stays alive (collider,
///     weapons, AI) — only the renderer is suppressed.
///   - Glue the unified overlay sprite to the ship's world
///     position and rotate it to match the ship's heading.
///
/// The ship's Visibility is restored on exit by
/// `tick_mmrxf_needs_restore`, which now also flips Visibility
/// back to Inherited alongside the (now no-op) scale restore.
pub fn tick_mmrxf_transform(
    state: Res<UltimateState>,
    leader: Query<(&Position, &Rotation), With<crate::ship::Ship>>,
    mut ship_vis: Query<&mut Visibility, (With<crate::ship::Ship>, Without<MmrxfOverlaySprite>)>,
    mut overlay_q: Query<
        &mut Transform,
        (With<MmrxfOverlaySprite>, Without<crate::ship::Ship>),
    >,
) {
    if state.variant != UltimateVariant::Mmrnmhrm {
        return;
    }
    let active = matches!(
        state.phase,
        UltimatePhase::MmrxfTransform | UltimatePhase::MmrxfUnleashing
    );
    if !active {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok((pos, rot)) = leader.get(p1) else { return };

    // Hide the underlying ship sprite. The overlay (the unified
    // "unleashed" sprite) is what the player sees instead.
    if let Ok(mut vis) = ship_vis.get_mut(p1) {
        if *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        }
    }

    // Update the overlay: glued to ship position and rotated to
    // match the ship's heading. The sprite is a single unified
    // image (not pre-rotated frames), so a direct Quat from the
    // ship's angle does the right thing. The ship sprite art is
    // drawn facing +Y (up) — same convention as ship_p00 frames —
    // so the angle math just subtracts π/2 to align the sprite.
    if let Some(overlay) = state.mmrxf_overlay_entity {
        if let Ok(mut overlay_xf) = overlay_q.get_mut(overlay) {
            overlay_xf.translation.x = pos.0.x;
            overlay_xf.translation.y = pos.0.y;
            let angle = rot.sin.atan2(rot.cos) - std::f32::consts::FRAC_PI_2;
            overlay_xf.rotation = Quat::from_rotation_z(angle);
            overlay_xf.scale = Vec3::ONE;
        }
    }
}

/// Strong handle to `ultimate/mmrxf_unleashed.png`. Held by a
/// resource so the asset stays loaded once we've stripped its
/// white background — without it, the GC would drop the image
/// between cinematics and the next ultimate would see a blank
/// sprite (or a re-load that needs another strip pass).
#[derive(Resource, Default)]
pub struct MmrxfUnleashedSprite {
    pub handle: Option<Handle<Image>>,
    pub stripped: bool,
}

/// Post-load post-process for `ultimate/mmrxf_unleashed.png`. The
/// source PNG has a white (RGB ≈ 255,255,255) studio background
/// that the user didn't manually clip; instead of doing it in an
/// editor we walk the loaded image data once, set alpha → 0 on
/// near-white pixels, and fade alpha down on near-white edge
/// pixels to keep antialiasing.
///
/// Runs every Update until the image actually appears in
/// `Assets<Image>` (loads happen asynchronously). On the first
/// successful pass, sets `stripped = true` so subsequent ticks
/// no-op. If the file doesn't exist at runtime the system just
/// keeps polling — harmless.
pub fn strip_white_background_once(
    asset_server: Res<AssetServer>,
    mut res: ResMut<MmrxfUnleashedSprite>,
    mut images: ResMut<Assets<Image>>,
) {
    if res.stripped {
        return;
    }
    // Lazily load on first call. assets.load returns the same
    // strong handle as the overlay spawn's, so they share asset
    // identity.
    if res.handle.is_none() {
        res.handle = Some(asset_server.load("ultimate/mmrxf_unleashed.png"));
    }
    let Some(handle) = res.handle.as_ref() else { return };
    let Some(img) = images.get_mut(handle) else { return };
    // Bail safely on unexpected formats. Bevy decodes PNG with
    // alpha into Rgba8UnormSrgb by default.
    use bevy::render::render_resource::TextureFormat;
    let fmt = img.texture_descriptor.format;
    if fmt != TextureFormat::Rgba8UnormSrgb && fmt != TextureFormat::Rgba8Unorm {
        // Mark as stripped to stop polling — we can't help here.
        res.stripped = true;
        return;
    }
    let Some(data) = img.data.as_mut() else {
        // Data isn't accessible (CPU side not retained). Mark
        // stripped so we don't churn forever.
        res.stripped = true;
        return;
    };
    for chunk in data.chunks_exact_mut(4) {
        let r = chunk[0];
        let g = chunk[1];
        let b = chunk[2];
        // Treat "white" as the channel min — anti-aliased edges
        // where one channel drops while the other two stay high
        // still count as not-quite-white.
        let m = r.min(g).min(b);
        if m >= 245 {
            chunk[3] = 0;
        } else if m >= 210 {
            // Partial fade so anti-aliased edges blend instead of
            // cutting hard. Linear ramp from 245 → 210 maps alpha
            // 0 → ~255.
            let frac = ((245 - m) as u32 * 7).min(255) as u8;
            chunk[3] = chunk[3].min(frac);
        }
    }
    res.stripped = true;
}

/// Marker dropped onto the ship at cinematic exit when the
/// Mmrnmhrm variant was active. `tick_mmrxf_needs_restore` reads
/// it, writes Transform.scale back to `orig`, and removes the
/// marker. Done in a follow-up tick because `exit_cinematic`
/// doesn't have Transform access.
#[derive(Component, Debug)]
pub struct MmrxfNeedsRestore {
    pub orig: Vec3,
}

/// Consume `MmrxfNeedsRestore`: restore the ship's Transform.scale
/// (kept as a safety net for older code paths that may still have
/// scaled the ship) and flip Visibility back to Inherited so the
/// ship sprite re-appears now that the overlay is gone.
pub fn tick_mmrxf_needs_restore(
    mut commands: Commands,
    mut q: Query<(Entity, &MmrxfNeedsRestore, &mut Transform, &mut Visibility)>,
) {
    for (e, restore, mut xf, mut vis) in &mut q {
        xf.scale = restore.orig;
        *vis = Visibility::Inherited;
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.remove::<MmrxfNeedsRestore>();
        }
    }
}

/// While MmrxfUnleashing is active and the player holds FIRE,
/// the tangled-rope homing laser always renders three chaotic
/// sinusoidal streams. Damage is only applied when a non-friendly
/// target is within range; otherwise the curves still fire,
/// pointing along the ship's forward direction.
pub fn tick_mmrxf_tangled_laser(
    time: Res<Time<Physics>>,
    mut state: ResMut<UltimateState>,
    slot_inputs: Res<crate::input::SlotInputs>,
    mut commands: Commands,
    ships: Query<(Entity, &crate::ship::Ship, &Position, &Rotation)>,
    target_ships: Query<
        (Entity, &crate::ship::Ship, &Position),
        Without<crate::ship::Invisible>,
    >,
    shields: Query<&crate::ship::ShieldActive>,
    mut crews: Query<&mut crate::ship::Crew>,
) {
    if state.variant != UltimateVariant::Mmrnmhrm
        || state.phase != UltimatePhase::MmrxfUnleashing
    {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok((_, firer_ship, firer_pos, firer_rot)) = ships.get(p1) else { return };
    let fire_held =
        slot_inputs.pressed(firer_ship.player_slot, crate::input::INPUT_FIRE);
    let dt = time.delta_secs();
    state.mmrxf_laser_cooldown_s = (state.mmrxf_laser_cooldown_s - dt).max(0.0);
    if !fire_held {
        return;
    }
    if state.mmrxf_laser_cooldown_s > 0.0 {
        return;
    }
    state.mmrxf_laser_cooldown_s = MMRXF_LASER_FIRE_INTERVAL_S;

    // Pick nearest non-friendly non-invisible ship within range,
    // if any. Damage is gated on a hit; rendering is not.
    let firer_slot = firer_ship.player_slot;
    let mut best: Option<(Vec2, Entity, f32)> = None;
    for (e, s, p) in &target_ships {
        if e == p1 || s.player_slot == firer_slot {
            continue;
        }
        let d2 = (p.0 - firer_pos.0).length_squared();
        if d2 > MMRXF_LASER_RANGE * MMRXF_LASER_RANGE {
            continue;
        }
        if best.map_or(true, |(_, _, b)| d2 < b) {
            best = Some((p.0, e, d2));
        }
    }

    // Apply damage if we have a target.
    if let Some((_, target_e, _)) = best {
        let factor = shields
            .get(target_e)
            .map(|s| s.damage_factor)
            .unwrap_or(1.0);
        let dmg = ((MMRXF_LASER_DAMAGE as f32 * factor).round() as i32).max(0);
        if dmg > 0 {
            if let Ok(mut crew) = crews.get_mut(target_e) {
                crew.current = (crew.current - dmg).max(0);
            }
        }
    }

    // Slylandro-style chaotic lightning. The bolt is a random
    // walk that takes biased steps toward `end`: each step picks
    // a heading roughly along the remaining-to-target vector but
    // perturbed by a wide random angle, so the bolt zig-zags
    // wildly. Multiple bolts in distinct colors fire each tick,
    // and some steps fork short dead-end branches off into space.
    let start = firer_pos.0;
    let end = match best {
        Some((tp, _, _)) => tp,
        None => {
            let fwd = Vec2::new(-firer_rot.sin, firer_rot.cos);
            start + fwd * MMRXF_LASER_RANGE
        }
    };

    /// Bolts per tick (one per color).
    const BOLT_COUNT: usize = 3;
    /// Steps in each main bolt. More = more jaggedness.
    const STEPS: usize = 18;
    /// Max angular deviation per step (radians). The bolt picks a
    /// new heading uniformly in [-DEV, DEV] off the current
    /// remaining-to-target bearing. ~1.0 rad ≈ 57° per step =>
    /// very jagged.
    const STEP_ANGLE_DEV: f32 = 1.0;
    /// Probability per main-bolt step of forking a short
    /// dead-end branch off into space.
    const FORK_PROB: f32 = 0.18;
    /// Steps in a fork branch (short).
    const FORK_STEPS: usize = 5;
    /// Step length as fraction of remaining distance, floored.
    const STEP_FRAC: f32 = 0.10;
    const STEP_MIN: f32 = 20.0;

    let colors: [Color; BOLT_COUNT] = [
        Color::srgba(0.55, 0.95, 1.00, 0.95), // cyan-white core
        Color::srgba(1.00, 0.35, 1.00, 0.85), // magenta arc
        Color::srgba(1.00, 1.00, 0.55, 0.80), // yellow arc
    ];

    for color in colors {
        let mut cursor = start;
        for _ in 0..STEPS {
            let remaining = end - cursor;
            let remaining_len = remaining.length();
            if remaining_len < 4.0 {
                break;
            }
            let dir = remaining / remaining_len;
            let base_angle = dir.y.atan2(dir.x);
            let dev = (fastrand::f32() - 0.5) * 2.0 * STEP_ANGLE_DEV;
            let step_angle = base_angle + dev;
            let step_len = (remaining_len * STEP_FRAC)
                .max(STEP_MIN)
                .min(remaining_len);
            let next = cursor
                + Vec2::new(step_angle.cos(), step_angle.sin()) * step_len;
            spawn_lightning_segment(&mut commands, cursor, next, color, 5.0, 0.10);

            // Dead-end fork: shoots roughly perpendicular for
            // FORK_STEPS short steps. Adds the "tangled rope"
            // fingers without changing where the main bolt ends.
            if fastrand::f32() < FORK_PROB {
                let mut fork_cursor = cursor;
                let fork_perp_sign = if fastrand::bool() { 1.0 } else { -1.0 };
                let mut fork_angle =
                    base_angle + fork_perp_sign * std::f32::consts::FRAC_PI_2;
                for _ in 0..FORK_STEPS {
                    fork_angle += (fastrand::f32() - 0.5) * 2.0 * STEP_ANGLE_DEV;
                    let fork_step = step_len * 0.6;
                    let fork_next = fork_cursor
                        + Vec2::new(fork_angle.cos(), fork_angle.sin()) * fork_step;
                    spawn_lightning_segment(
                        &mut commands,
                        fork_cursor,
                        fork_next,
                        color,
                        3.5,
                        0.07,
                    );
                    fork_cursor = fork_next;
                }
            }

            cursor = next;
        }
    }
}

/// Spawn one short bright line segment that fades alpha → 0 over
/// `lifetime_s` via `tick_mmrxf_laser_segments`. Helper for the
/// lightning random-walk above.
fn spawn_lightning_segment(
    commands: &mut Commands,
    from: Vec2,
    to: Vec2,
    color: Color,
    width: f32,
    lifetime_s: f32,
) {
    let mid = (from + to) * 0.5;
    let seg = to - from;
    let seg_len = seg.length().max(1.0);
    let angle = seg.y.atan2(seg.x) - std::f32::consts::FRAC_PI_2;
    commands.spawn((
        MmrxfLaserSegment {
            remaining_s: lifetime_s,
            total_s: lifetime_s,
            base_color: color,
        },
        Sprite::from_color(color, Vec2::new(width, seg_len)),
        Transform {
            translation: mid.extend(0.32),
            rotation: Quat::from_rotation_z(angle),
            scale: Vec3::ONE,
        },
    ));
}

/// Fade laser-segment alpha out over their short lifetime.
fn tick_mmrxf_laser_segments(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut MmrxfLaserSegment, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    for (e, mut seg, mut sprite) in &mut q {
        seg.remaining_s -= dt;
        if seg.remaining_s <= 0.0 {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_despawn();
            }
            continue;
        }
        let frac = (seg.remaining_s / seg.total_s).clamp(0.0, 1.0);
        let lin = seg.base_color.to_linear();
        sprite.color = Color::srgba(lin.red, lin.green, lin.blue, lin.alpha * frac);
    }
}

/// While MmrxfUnleashing is active and the player taps SPECIAL,
/// spawn one Owa-style guided missile (Homing) that after
/// `MMRXF_MISSILE_SPLIT_AT_S` despawns + spawns 5 smaller homing
/// children radially outward — each child is also Homing toward
/// the nearest enemy, so the cluster scatters then re-aims.
pub fn tick_mmrxf_split_launcher(
    time: Res<Time<Physics>>,
    mut state: ResMut<UltimateState>,
    slot_inputs: Res<crate::input::SlotInputs>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    ships: Query<(&crate::ship::Ship, &Position, &Rotation, &LinearVelocity), With<crate::ship::Ship>>,
) {
    if state.variant != UltimateVariant::Mmrnmhrm
        || state.phase != UltimatePhase::MmrxfUnleashing
    {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok((firer_ship, firer_pos, firer_rot, firer_vel)) = ships.get(p1) else { return };
    let special_held =
        slot_inputs.pressed(firer_ship.player_slot, crate::input::INPUT_SPECIAL);
    let dt = time.delta_secs();
    state.mmrxf_missile_cooldown_s = (state.mmrxf_missile_cooldown_s - dt).max(0.0);
    if !special_held || state.mmrxf_missile_cooldown_s > 0.0 {
        return;
    }
    state.mmrxf_missile_cooldown_s = MMRXF_MISSILE_COOLDOWN_S;

    // Spawn the parent missile from the ship's nose, inheriting
    // its velocity. The Homing component steers it toward the
    // nearest enemy; the MmrxfSplitMissile marker tells the tick
    // system to split it after `split_at_s`.
    let forward = Vec2::new(-firer_rot.sin, firer_rot.cos);
    let muzzle = firer_pos.0 + forward * 38.0;
    let parent_speed = 80.0 * crate::ship::SC2_VEL_SCALE;
    let proj_vel = firer_vel.0 + forward * parent_speed;
    let init_angle = forward.y.atan2(forward.x) - std::f32::consts::FRAC_PI_2;
    commands.spawn((
        (
            crate::ship::Projectile {
                owner: p1,
                damage: 5,
                lifetime: MMRXF_MISSILE_SPLIT_AT_S + 1.5,
            },
            crate::ship::Homing {
                target: None,
                turn_rate: crate::ship::sc2_turning(3.0),
            },
            MmrxfSplitMissile {
                timer_s: 0.0,
                split_at_s: MMRXF_MISSILE_SPLIT_AT_S,
                child_count: MMRXF_MISSILE_CHILD_COUNT,
                owner: p1,
            },
            Sprite {
                image: assets.load("ships/mmrxf/sprites/shot_a01.png"),
                color: Color::srgb(0.85, 0.9, 1.0),
                custom_size: Some(Vec2::splat(14.0)),
                ..default()
            },
            Transform::from_translation(muzzle.extend(0.5)),
        ),
        (
            RigidBody::Dynamic,
            Collider::circle(7.0),
            Sensor,
            Mass(1.2),
            Position(muzzle),
            Rotation::radians(init_angle),
            LinearVelocity(proj_vel),
            AngularVelocity::ZERO,
            LinearDamping(0.0),
            AngularDamping(0.0),
            CollisionEventsEnabled,
        ),
    ));
}

/// Each FixedUpdate, tick every MmrxfSplitMissile; when its
/// `timer_s` exceeds `split_at_s`, despawn the parent and spawn
/// `child_count` smaller Homing projectiles radially around the
/// parent's last known position.
pub fn tick_mmrxf_split_missiles(
    time: Res<Time<Physics>>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut rng: ResMut<crate::rng::GameRng>,
    mut parents: Query<(
        Entity,
        &mut MmrxfSplitMissile,
        &Position,
        &LinearVelocity,
    )>,
) {
    let dt = time.delta_secs();
    // Determinism-critical: stable iteration order so the same
    // parent missile splits with the same child speeds on
    // every peer. Bevy Query iteration isn't cross-machine
    // stable — sort by Entity index.
    let mut entries: Vec<_> = parents.iter_mut().collect();
    entries.sort_by_key(|(e, _, _, _)| e.index());
    for (e, mut split, pos, vel) in entries {
        split.timer_s += dt;
        if split.timer_s < split.split_at_s {
            continue;
        }
        // SPLIT. Spawn child_count smaller homing projectiles
        // outward from the parent.
        let world = pos.0;
        let base_vel = vel.0;
        let base_speed = base_vel.length().max(50.0 * crate::ship::SC2_VEL_SCALE);
        let owner = split.owner;
        let count = split.child_count;
        for i in 0..count {
            let theta = (i as f32) * std::f32::consts::TAU / count as f32;
            let dir = Vec2::new(theta.cos(), theta.sin());
            // Determinism-critical: child speed drives where
            // they end up under homing curves.
            let speed_jitter = 0.7 + rng.f32() * 0.5;
            let child_vel = dir * base_speed * speed_jitter;
            let init_angle = dir.y.atan2(dir.x) - std::f32::consts::FRAC_PI_2;
            commands.spawn((
                crate::ship::Projectile {
                    owner,
                    damage: 3,
                    lifetime: 2.5,
                },
                crate::ship::Homing {
                    target: None,
                    turn_rate: crate::ship::sc2_turning(2.0),
                },
                Sprite {
                    image: assets.load("ships/mmrxf/sprites/shot_a01.png"),
                    color: Color::srgb(0.7, 0.85, 1.0),
                    custom_size: Some(Vec2::splat(9.0)),
                    ..default()
                },
                Transform::from_translation((world + dir * 14.0).extend(0.5)),
                RigidBody::Dynamic,
                Collider::circle(4.5),
                Sensor,
                Mass(0.5),
                Position(world + dir * 14.0),
                Rotation::radians(init_angle),
                LinearVelocity(child_vel),
                AngularVelocity::ZERO,
                LinearDamping(0.0),
                AngularDamping(0.0),
                CollisionEventsEnabled,
            ));
        }
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
    }
}


// ----------------------------------------------------------------
// Druuge — Wrath of the Crimson Corporation
// ----------------------------------------------------------------

/// During DruugeBarrage, fire DRUUGE_SHOT_COUNT oversized cannon
/// shots forward in series, each at DRUUGE_SHOT_INTERVAL_S
/// intervals. Every shot applies a substantial backward recoil
/// impulse to the firer — the canonical Druuge kickback amped
/// up to "fling the ship across the arena" intensity.
fn tick_druuge_barrage(
    mut state: ResMut<UltimateState>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut ships: Query<(&Position, &Rotation, &mut LinearVelocity), With<crate::ship::Ship>>,
) {
    if state.variant != UltimateVariant::Druuge
        || state.phase != UltimatePhase::DruugeBarrage
    {
        return;
    }
    let due_at = state.druuge_shots_fired as f32 * DRUUGE_SHOT_INTERVAL_S;
    if state.druuge_shots_fired >= DRUUGE_SHOT_COUNT || state.phase_timer_s < due_at {
        return;
    }
    let Some(p1) = state.player_entity else {
        info!("Druuge: no player_entity, skipping shot");
        return;
    };
    let Ok((pos, rot, mut vel)) = ships.get_mut(p1) else {
        info!(
            "Druuge: ships.get_mut({p1:?}) failed (ship despawned?), skipping shot"
        );
        return;
    };
    info!(
        "Druuge: firing shot {} at t={:.2}s (due_at={:.2}s)",
        state.druuge_shots_fired,
        state.phase_timer_s,
        due_at
    );
    let fwd = Vec2::new(-rot.sin, rot.cos);
    let back = -fwd;
    let world = pos.0;
    let speed = 130.0 * crate::ship::SC2_VEL_SCALE;
    let init_angle = fwd.y.atan2(fwd.x) - std::f32::consts::FRAC_PI_2;
    commands.spawn((
        crate::ship::Projectile {
            owner: p1,
            damage: 8,
            lifetime: 2.2,
        },
        Sprite {
            image: assets.load("ships/druma/sprites/shot_a01.png"),
            color: Color::srgb(1.0, 0.65, 0.20),
            custom_size: Some(Vec2::splat(28.0)),
            ..default()
        },
        Transform::from_translation(world.extend(0.5)),
        RigidBody::Dynamic,
        Collider::circle(14.0),
        Sensor,
        Mass(1.4),
        Position(world + fwd * 28.0),
        Rotation::radians(init_angle),
        LinearVelocity(fwd * speed),
        AngularVelocity::ZERO,
        LinearDamping(0.0),
        AngularDamping(0.0),
        CollisionEventsEnabled,
    ));
    // Recoil: write directly to LinearVelocity so we get a clean
    // hard kick even with damping on the ship.
    vel.0 += back * DRUUGE_RECOIL_IMPULSE;
    state.druuge_shots_fired += 1;
}

// ----------------------------------------------------------------
// Kohr-Ah — Sanctified Slaughter
// ----------------------------------------------------------------

/// On the first tick of KohrAhSlaughter, fan KOHRAH_BLADE_COUNT
/// saw-blade projectiles out in a 360° pattern. Each is tagged
/// with `KohrAhBlade` so `tick_kohrah_blades` accelerates them
/// outward each frame for a continuously-expanding kill ring.
fn tick_kohrah_spawn(
    mut state: ResMut<UltimateState>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut rng: ResMut<crate::rng::GameRng>,
    ships: Query<&Position, With<crate::ship::Ship>>,
) {
    if state.variant != UltimateVariant::KohrAh
        || state.phase != UltimatePhase::KohrAhSlaughter
    {
        return;
    }
    if state.phase_timer_s > 0.02 {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok(pos) = ships.get(p1) else { return };
    let world = pos.0;
    let speed = 80.0 * crate::ship::SC2_VEL_SCALE;
    for i in 0..KOHRAH_BLADE_COUNT {
        let theta = (i as f32) * std::f32::consts::TAU / KOHRAH_BLADE_COUNT as f32;
        // Determinism-critical: blade trajectories.
        let theta = theta + rng.signed_unit() * 0.05;
        let dir = Vec2::new(theta.cos(), theta.sin());
        let init_angle = dir.y.atan2(dir.x) - std::f32::consts::FRAC_PI_2;
        commands.spawn((
            crate::ship::Projectile {
                owner: p1,
                damage: 4,
                lifetime: KOHRAH_SLAUGHTER_S * 1.1,
            },
            KohrAhBlade {
                owner: p1,
                dir,
                speed,
                lifetime_s: KOHRAH_SLAUGHTER_S * 1.1,
            },
            Sprite {
                image: assets.load("ships/kohma/sprites/shot_a01.png"),
                color: Color::srgb(1.0, 0.85, 0.40),
                custom_size: Some(Vec2::splat(28.0)),
                ..default()
            },
            Transform::from_translation(world.extend(0.5)),
            RigidBody::Dynamic,
            Collider::circle(12.0),
            Sensor,
            Mass(0.4),
            Position(world + dir * 30.0),
            Rotation::radians(init_angle),
            LinearVelocity(dir * speed),
            // Visual: blade spins fast in place even as it
            // translates outward.
            AngularVelocity(18.0),
            LinearDamping(0.0),
            AngularDamping(0.0),
            CollisionEventsEnabled,
        ));
    }
    // Past the gate so we don't re-fire.
    state.phase_timer_s = 0.05;
    // Suppress the unused-import warning for state (otherwise
    // rustc thinks ResMut is unused after the gate).
    let _ = &mut state;
}

/// Each tick, accelerate every existing `KohrAhBlade` outward
/// along its initial direction. Blades that have outlived their
/// lifetime are despawned (the normal projectile lifetime tick
/// also handles them, but this is a belt-and-braces cleanup).
fn tick_kohrah_blades(
    time: Res<Time<Physics>>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut KohrAhBlade, &mut LinearVelocity)>,
) {
    let dt = time.delta_secs();
    for (e, mut blade, mut vel) in &mut q {
        blade.lifetime_s -= dt;
        if blade.lifetime_s <= 0.0 {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_despawn();
            }
            continue;
        }
        blade.speed += KOHRAH_BLADE_ACCEL * dt;
        vel.0 = blade.dir * blade.speed;
    }
}

// ----------------------------------------------------------------
// Mycon — Plasma Hurricane
// ----------------------------------------------------------------

/// On the first tick of MyconGathering, spawn MYCON_ORB_COUNT
/// orbs around the ship at a wide initial radius. Each tick the
/// orbs are re-positioned along a tightening spiral via
/// `tick_mycon_orbit`. On phase transition to MyconHurricane the
/// orbs convert to homing projectiles in `tick_mycon_release`.
fn tick_mycon_gather(
    // Time<Real> so the gather animation ticks during the
    // paused MyconGathering phase — same reason as
    // tick_ultimate_phases.
    time: Res<Time<Real>>,
    mut state: ResMut<UltimateState>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    ships: Query<&Position, With<crate::ship::Ship>>,
) {
    if state.variant != UltimateVariant::Mycon
        || state.phase != UltimatePhase::MyconGathering
    {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok(pos) = ships.get(p1) else { return };
    let world = pos.0;

    // First-tick gate: spawn the orbs once.
    if state.mycon_orbs.is_empty() {
        state.mycon_orb_t0 = time.elapsed_secs();
        for i in 0..MYCON_ORB_COUNT {
            let theta = (i as f32) * std::f32::consts::TAU / MYCON_ORB_COUNT as f32;
            let r = MYCON_ORBIT_R * 2.4;
            let off = Vec2::new(theta.cos(), theta.sin()) * r;
            let id = commands
                .spawn((
                    MyconOrbit {
                        owner: p1,
                        theta,
                        // Counter-clockwise sweep at a brisk rate.
                        omega: 3.0,
                    },
                    Sprite {
                        image: assets.load("ships/mycpo/sprites/shot_a01.png"),
                        color: Color::srgba(0.85, 0.45, 1.0, 1.0),
                        custom_size: Some(Vec2::splat(28.0)),
                        ..default()
                    },
                    Transform::from_translation((world + off).extend(0.4)),
                ))
                .id();
            state.mycon_orbs.push(id);
        }
    }
}

/// While MyconGathering or MyconHurricane is active, advance each
/// `MyconOrbit`'s phase and lerp its radius down toward
/// `MYCON_ORBIT_R` over the gather phase. The ship's position is
/// the orbit center.
fn tick_mycon_orbit(
    // Time<Real> so the spiraling-in animation keeps moving
    // during the paused gather beat.
    time: Res<Time<Real>>,
    state: Res<UltimateState>,
    mut orbs: Query<(&mut MyconOrbit, &mut Transform), Without<crate::ship::Ship>>,
    ships: Query<&Position, With<crate::ship::Ship>>,
) {
    if state.phase != UltimatePhase::MyconGathering {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok(pos) = ships.get(p1) else { return };
    let world = pos.0;
    let dt = time.delta_secs();
    // Tightening factor: 0 at start → 1 at end of gather.
    let elapsed = (time.elapsed_secs() - state.mycon_orb_t0).max(0.0);
    let tighten = (elapsed / MYCON_GATHER_S).clamp(0.0, 1.0);
    let r = MYCON_ORBIT_R * 2.4 * (1.0 - tighten) + MYCON_ORBIT_R * tighten;
    for (mut orb, mut xf) in &mut orbs {
        orb.theta += orb.omega * dt;
        let dir = Vec2::new(orb.theta.cos(), orb.theta.sin());
        let p = world + dir * r;
        xf.translation = p.extend(0.4);
    }
}

/// On the first tick of MyconHurricane, convert each orb entity
/// into a homing projectile. We rebuild the entity rather than
/// patch components because the orbs were spawned as visuals (no
/// Projectile / Homing / collider). Despawn the visual and spawn
/// a new one with the full projectile bundle at the same place.
fn tick_mycon_release(
    mut state: ResMut<UltimateState>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    orbs: Query<(Entity, &MyconOrbit, &Transform)>,
) {
    if state.variant != UltimateVariant::Mycon
        || state.phase != UltimatePhase::MyconHurricane
    {
        return;
    }
    if state.phase_timer_s > 0.02 {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let speed = 110.0 * crate::ship::SC2_VEL_SCALE;
    for (e, orb, xf) in &orbs {
        if orb.owner != p1 {
            continue;
        }
        let pos2 = xf.translation.truncate();
        // Initial velocity: tangent to the orbit (perpendicular
        // to the radial direction), giving the swarm a brief
        // outward spiral before the homing kicks in.
        let radial = Vec2::new(orb.theta.cos(), orb.theta.sin());
        let tangent = Vec2::new(-radial.y, radial.x);
        let init_dir = (tangent + radial * 0.5).normalize_or_zero();
        let init_angle = init_dir.y.atan2(init_dir.x) - std::f32::consts::FRAC_PI_2;
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
        commands.spawn((
            crate::ship::Projectile {
                owner: p1,
                damage: 5,
                lifetime: 4.0,
            },
            crate::ship::Homing {
                target: None,
                turn_rate: crate::ship::sc2_turning(3.0),
            },
            Sprite {
                image: assets.load("ships/mycpo/sprites/shot_a01.png"),
                color: Color::srgba(0.95, 0.50, 1.0, 1.0),
                custom_size: Some(Vec2::splat(32.0)),
                ..default()
            },
            Transform::from_translation(pos2.extend(0.5)),
            RigidBody::Dynamic,
            Collider::circle(14.0),
            Sensor,
            Mass(0.5),
            Position(pos2),
            Rotation::radians(init_angle),
            LinearVelocity(init_dir * speed),
            AngularVelocity::ZERO,
            LinearDamping(0.0),
            AngularDamping(0.0),
            CollisionEventsEnabled,
        ));
    }
    state.mycon_orbs.clear();
    state.phase_timer_s = 0.05;
}

// ----------------------------------------------------------------
// Thraddash — Afterburner Inferno
// ----------------------------------------------------------------

/// While ThraddashBurning:
///   - Bump the ship's `derived.speed_max` to 5× its pre-ultimate
///     value the first tick of the phase; snapshot the original
///     so `tick_thraddash_restore` can put it back on exit.
///   - Every THRADDASH_FLAME_INTERVAL_S, spawn a lethal damage
///     zone at the ship's current rear (just behind the engine
///     nozzle). The zone has THRADDASH_FLAME_LIFE_S lifetime and
///     applies THRADDASH_FLAME_DPS while overlapping a hostile
///     ship — turning the entire trail behind the Torch into a
///     kill streak the player paints with their own flight path.
fn tick_thraddash_burn(
    time: Res<Time<Physics>>,
    mut state: ResMut<UltimateState>,
    mut commands: Commands,
    mut ships: Query<(&Position, &Rotation, &mut crate::ship::ShipPhysicsDerived), With<crate::ship::Ship>>,
) {
    if state.variant != UltimateVariant::Thraddash
        || state.phase != UltimatePhase::ThraddashBurning
    {
        return;
    }
    let Some(p1) = state.player_entity else { return };
    let Ok((pos, rot, mut derived)) = ships.get_mut(p1) else { return };
    if state.thraddash_orig_speed_max.is_none() {
        state.thraddash_orig_speed_max = Some(derived.speed_max);
    }
    if let Some(orig) = state.thraddash_orig_speed_max {
        derived.speed_max = orig * THRADDASH_SPEED_MULT;
    }
    let dt = time.delta_secs();
    state.thraddash_flame_timer_s += dt;
    if state.thraddash_flame_timer_s < THRADDASH_FLAME_INTERVAL_S {
        return;
    }
    state.thraddash_flame_timer_s = 0.0;
    let fwd = Vec2::new(-rot.sin, rot.cos);
    let back = -fwd;
    // Spawn point: behind the engine nozzle by ~24 wu so the
    // puff sits in the visible exhaust plume.
    let spawn = pos.0 + back * 24.0;
    crate::ship::spawn_damage_zone(
        &mut commands,
        Some(p1),
        spawn,
        THRADDASH_FLAME_RADIUS,
        THRADDASH_FLAME_DPS,
        THRADDASH_FLAME_LIFE_S,
        Color::srgba(1.0, 0.55, 0.20, 0.55),
    );
}

/// If we left ThraddashBurning while holding a speed_max
/// override, restore the original speed_max so the ship doesn't
/// permanently fly at 5× speed.
fn tick_thraddash_restore(
    mut state: ResMut<UltimateState>,
    mut ships: Query<&mut crate::ship::ShipPhysicsDerived, With<crate::ship::Ship>>,
) {
    let burning = state.variant == UltimateVariant::Thraddash
        && state.phase == UltimatePhase::ThraddashBurning;
    if burning {
        return;
    }
    let Some(orig) = state.thraddash_orig_speed_max.take() else { return };
    let Some(p1) = state.player_entity else { return };
    if let Ok(mut derived) = ships.get_mut(p1) {
        derived.speed_max = orig;
    }
}
