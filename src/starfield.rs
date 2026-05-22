//! Background starfield + mouse-wheel zoom + a subtle zoom-burst
//! effect that fades in a handful of "moving" stars during active
//! scroll, as a hyperspace-lite visual cue.
//!
//! Pieces:
//!   - `setup_starfield` (Startup) — spawns ~200 faint static stars
//!     scattered across the arena, sitting at z = -10 behind
//!     everything else.
//!   - `handle_zoom_input` (Update) — reads `MouseWheel` events,
//!     adjusts the Camera2d's orthographic scale (clamped), and on
//!     every non-zero scroll delta spawns a handful of `ZoomStar`s.
//!   - `tick_zoom_stars` (Update) — moves each ZoomStar radially
//!     (outward when zooming in, inward when zooming out), fades it
//!     in then out over its short lifetime, despawns it.
//!
//! No physics on any of this — pure visual. Doesn't go through
//! Avian (documented in docs/SHIP_AUDIT.md as an expected
//! non-physics system: background visuals).

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

/// Persistent background star. Currently only carries brightness so
/// we could later modulate it (twinkle, gradient, etc).
#[derive(Component, Debug)]
pub struct BackgroundStar {
    pub brightness: f32,
}

/// Short-lived "you're moving through space" star spawned each time
/// the player scrolls the wheel. Moves radially relative to world
/// origin, fades in and out across its lifetime.
#[derive(Component, Debug)]
pub struct ZoomStar {
    pub remaining_s: f32,
    pub total_s: f32,
    /// World units / second along the star's outward direction.
    /// Positive = outward (zooming in feels like flying forward
    /// → stars whoosh past); negative = inward.
    pub radial_speed: f32,
    /// Peak alpha; the fade curve below scales between 0 and this.
    pub peak_alpha: f32,
}

/// Scratch state for the zoom system — we want to know whether the
/// player is *currently* scrolling vs just stopped, so we can hold
/// off spawning new ZoomStars during quiet moments.
#[derive(Resource, Default)]
pub struct ZoomState {
    pub last_scroll_dir: f32,
}

pub struct StarfieldPlugin;

impl Plugin for StarfieldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ZoomState>()
            .add_systems(Startup, setup_starfield)
            .add_systems(Update, (handle_zoom_input, tick_zoom_stars));
    }
}

const STAR_COUNT: usize = 250;
const STAR_AREA_HALF: f32 = 2000.0;
const STAR_Z_BACKGROUND: f32 = -10.0;
const STAR_Z_ZOOM_BURST: f32 = -5.0;

fn setup_starfield(mut commands: Commands) {
    for _ in 0..STAR_COUNT {
        let x = (fastrand::f32() - 0.5) * STAR_AREA_HALF * 2.0;
        let y = (fastrand::f32() - 0.5) * STAR_AREA_HALF * 2.0;
        // Faint stars: 4-22% alpha. Adds visible texture without
        // competing with the foreground sprites for attention.
        let brightness = 0.04 + fastrand::f32() * 0.18;
        // Star size jitter (1-3 px). Larger stars feel "closer."
        let size = 1.0 + fastrand::f32() * 2.0;
        // Slight blue/white colour jitter so the field doesn't read
        // as a flat grid of identical white dots.
        let r = 0.85 + fastrand::f32() * 0.15;
        let g = 0.85 + fastrand::f32() * 0.15;
        let b = 0.9 + fastrand::f32() * 0.1;
        commands.spawn((
            BackgroundStar { brightness },
            Sprite::from_color(
                Color::srgba(r, g, b, brightness),
                Vec2::splat(size),
            ),
            Transform::from_translation(Vec3::new(x, y, STAR_Z_BACKGROUND)),
        ));
    }
}

/// Scroll wheel handler. Each notch multiplies the orthographic
/// scale by ZOOM_STEP (inverse for zoom-in). Spawns a few
/// `ZoomStar`s per scrolled notch as the "you're moving" cue.
fn handle_zoom_input(
    mut commands: Commands,
    mut scroll: MessageReader<MouseWheel>,
    mut camera_q: Query<&mut Projection, With<Camera2d>>,
    mut zoom_state: ResMut<ZoomState>,
) {
    const ZOOM_STEP: f32 = 1.15;
    const SCALE_MIN: f32 = 0.25;
    const SCALE_MAX: f32 = 6.0;

    let mut delta_total = 0.0_f32;
    for ev in scroll.read() {
        // Pixel-mode wheels (trackpads) report many small deltas;
        // line-mode (mouse wheels) reports ±1 per notch. Normalise
        // by clamping the per-event contribution so a trackpad
        // fling doesn't shoot the zoom to the floor.
        delta_total += ev.y.clamp(-3.0, 3.0);
    }
    if delta_total.abs() < 0.001 {
        zoom_state.last_scroll_dir = 0.0;
        return;
    }

    let zoom_dir = delta_total.signum();
    zoom_state.last_scroll_dir = zoom_dir;

    // Apply the scale change.
    for mut projection in &mut camera_q {
        if let Projection::Orthographic(ref mut ortho) = *projection {
            let factor = ZOOM_STEP.powf(delta_total.abs());
            if delta_total > 0.0 {
                ortho.scale /= factor; // zoom in
            } else {
                ortho.scale *= factor; // zoom out
            }
            ortho.scale = ortho.scale.clamp(SCALE_MIN, SCALE_MAX);
        }
    }

    // Spawn a handful of zoom-burst stars. Count scales with the
    // scroll magnitude — a single notch gets 4 stars; a fling gets
    // a dozen or so.
    let n = (3.0 + delta_total.abs() * 2.0).round() as i32;
    for _ in 0..n {
        // Start radius: closer to the origin = more visible motion.
        // Larger range keeps the effect from looking like a tight
        // bullseye.
        let start_r = 60.0 + fastrand::f32() * 240.0;
        let theta = fastrand::f32() * std::f32::consts::TAU;
        let pos = Vec2::new(theta.cos() * start_r, theta.sin() * start_r);
        // Outward when zooming in (feels like flying forward),
        // inward when zooming out.
        let radial_speed =
            zoom_dir * (80.0 + fastrand::f32() * 200.0);
        let total_s = 0.5 + fastrand::f32() * 0.4;
        let peak_alpha = 0.25 + fastrand::f32() * 0.35;
        let size = 1.0 + fastrand::f32() * 2.0;
        commands.spawn((
            ZoomStar {
                remaining_s: total_s,
                total_s,
                radial_speed,
                peak_alpha,
            },
            Sprite::from_color(
                Color::srgba(1.0, 1.0, 1.0, 0.0),
                Vec2::splat(size),
            ),
            Transform::from_translation(pos.extend(STAR_Z_ZOOM_BURST)),
        ));
    }
}

/// Move each `ZoomStar` radially, fade it in then out over its
/// short lifetime, despawn at end.
fn tick_zoom_stars(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut ZoomStar, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    for (entity, mut star, mut xf, mut sprite) in &mut q {
        star.remaining_s -= dt;
        if star.remaining_s <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        // Move radially. With AngularDamping=0 / no physics we just
        // integrate the position directly.
        let pos = xf.translation.truncate();
        let r = pos.length();
        if r > 0.1 {
            let dir = pos / r;
            let new_pos = pos + dir * star.radial_speed * dt;
            xf.translation = new_pos.extend(xf.translation.z);
        }
        // Fade curve: in then out via 4·f·(1-f) — peaks at 0.5,
        // zero at both ends.
        let elapsed = star.total_s - star.remaining_s;
        let f = (elapsed / star.total_s).clamp(0.0, 1.0);
        let alpha = (4.0 * f * (1.0 - f)).clamp(0.0, 1.0) * star.peak_alpha;
        sprite.color = Color::srgba(1.0, 1.0, 1.0, alpha);
    }
}
