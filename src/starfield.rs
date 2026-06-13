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

use avian2d::prelude::Position;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use crate::ship::Ship;

/// Persistent background star. `anchor` is the star's world position
/// in the "frozen" frame (i.e. where it would be with no camera
/// movement). `parallax` is a 0..1 depth — 0 is far background (the
/// star is locked to the camera, appears stationary on screen) and
/// 1 is foreground (star is fixed in world, slides past as camera
/// moves). Intermediate values give the classic multi-layer
/// parallax slide.
#[derive(Component, Debug)]
pub struct BackgroundStar {
    pub brightness: f32,
    pub anchor: Vec2,
    pub parallax: f32,
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

/// Scratch state for the zoom system. Tracks the player's desired
/// scale (`target_scale`) separately from the camera's actual
/// rendered scale — `smooth_zoom_scale` tweens between them each
/// frame so a scroll notch produces an *animated* zoom rather than
/// an instant snap.
#[derive(Resource)]
pub struct ZoomState {
    pub target_scale: f32,
    pub last_scroll_dir: f32,
    /// Pinch baseline: distance between the two active touches on
    /// the previous frame, in screen-pixel units. `None` when fewer
    /// than two fingers are down — re-baselined each time a pinch
    /// gesture starts so quick re-pinches don't snap the scale.
    pub last_pinch_dist: Option<f32>,
    /// Mouse-pivot for scroll-wheel zoom. When set, `smooth_zoom_scale`
    /// adjusts the camera each frame so that the world point captured
    /// at scroll time stays under the same screen-pixel position
    /// throughout the tween — i.e. the zoom centres on the cursor,
    /// not on screen-centre. Cleared once the scale tween settles.
    pub pivot: Option<ZoomPivot>,
    /// Last frame's ship count. The follow system uses this to
    /// detect a 0→N transition (match start / first spawn) and
    /// snap the camera + scale to the tight bounding box instead
    /// of slowly easing in.
    pub last_ship_count: usize,
    /// `Time<Real>` instant at which `CameraFollowMode::Manual`
    /// should revert to `Auto`. Wheel / pinch input flips to
    /// Manual + sets this to (now + REVERT_DELAY_S). Press `C`
    /// to flip permanently (sets this to f32::INFINITY).
    pub manual_revert_at: f32,
    /// Set true by `reset_for_new_match` (OnEnter(InMatch)).
    /// The next Update tick of `follow_ships_with_camera` reads
    /// it, snaps the camera + scale to the tight bounding box,
    /// and clears the flag. This is the only reliable way to
    /// fit the bbox on the FIRST rendered frame of a match —
    /// OnEnter's commands aren't flushed by the time other
    /// OnEnter systems run, so reading ship positions in
    /// OnEnter sometimes finds zero ships.
    pub pending_initial_snap: bool,
}

/// Captured at the moment of a scroll event: `offset_px` is the
/// mouse position relative to window centre in pixels (Y already
/// flipped to world convention), `world` is the world point that
/// was under the cursor at scroll-time.
#[derive(Clone, Copy, Debug)]
pub struct ZoomPivot {
    pub offset_px: Vec2,
    pub world: Vec2,
}

impl Default for ZoomState {
    fn default() -> Self {
        Self {
            target_scale: 1.0,
            last_scroll_dir: 0.0,
            last_pinch_dist: None,
            pivot: None,
            last_ship_count: 0,
            manual_revert_at: 0.0,
            pending_initial_snap: false,
        }
    }
}

/// Camera follow behaviour. `Auto` (default) is the canonical SC2-
/// style follow: the camera centres on the midpoint of the two ships
/// and zooms to fit them with some padding. `Manual` lets the user
/// pan/zoom freely (wheel/pinch). Use `C` to toggle.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CameraFollowMode {
    #[default]
    Auto,
    Manual,
}

/// Per-ship "which periodic image are we framing this ship in" memory,
/// so the auto-follow camera's unwrap choice is sticky frame-to-frame.
/// Without it, two ships near the half-arena separation flip the
/// minimum-image centroid back and forth every frame; with it, the
/// camera holds one framing (tolerating a slightly larger bounding box)
/// until a clearly tighter one is available. Keyed by ship `Entity`;
/// rebuilt every frame so dead ships drop out automatically.
#[derive(Resource, Default)]
pub struct CameraUnwrap {
    images: bevy::platform::collections::HashMap<Entity, Vec2>,
}

pub struct StarfieldPlugin;

impl Plugin for StarfieldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ZoomState>()
            .init_resource::<CameraFollowMode>()
            .init_resource::<CameraUnwrap>()
            .add_systems(Startup, setup_starfield)
            .add_systems(
                Update,
                (
                    toggle_camera_follow_mode,
                    follow_ships_with_camera,
                    handle_zoom_input,
                    handle_pinch_zoom,
                    smooth_zoom_scale,
                    tick_starfield_parallax,
                    tick_zoom_stars,
                )
                    .chain(),
            )
            // The render offset runs in PostUpdate, *after* every
            // camera-moving system (the auto-follow AND the Ultimate
            // cinematic driver, which lives in another plugin) has had
            // its say, and before Bevy propagates GlobalTransforms.
            // That ordering is what keeps bodies glued to whichever
            // camera is in charge this frame.
            .add_systems(
                PostUpdate,
                apply_toroidal_render_offset
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

const STAR_COUNT: usize = 480;
const STAR_AREA_HALF: f32 = 3500.0;
const STAR_Z_BACKGROUND: f32 = -10.0;
const STAR_Z_ZOOM_BURST: f32 = -5.0;

fn setup_starfield(mut commands: Commands) {
    for _ in 0..STAR_COUNT {
        let x = (fastrand::f32() - 0.5) * STAR_AREA_HALF * 2.0;
        let y = (fastrand::f32() - 0.5) * STAR_AREA_HALF * 2.0;
        // Per-star depth — 0 = far background (locked to camera,
        // appears stationary), 1 = foreground (world-fixed, slides
        // past at full camera speed). Most stars are far away;
        // a tail of closer ones gives the eye motion cues.
        let parallax = (fastrand::f32() * fastrand::f32()).clamp(0.0, 1.0);
        // Closer stars (high parallax) are brighter and bigger
        // because they "occupy more pixels per square parsec".
        let brightness = 0.04 + 0.05 * parallax + fastrand::f32() * 0.18;
        let size = 1.0 + 2.0 * parallax + fastrand::f32() * 1.5;
        let r = 0.85 + fastrand::f32() * 0.15;
        let g = 0.85 + fastrand::f32() * 0.15;
        let b = 0.9 + fastrand::f32() * 0.1;
        let anchor = Vec2::new(x, y);
        commands.spawn((
            BackgroundStar {
                brightness,
                anchor,
                parallax,
            },
            Sprite::from_color(
                Color::srgba(r, g, b, brightness),
                Vec2::splat(size),
            ),
            Transform::from_translation(anchor.extend(STAR_Z_BACKGROUND)),
        ));
    }
}

/// Slide each star's rendered Transform based on the camera's
/// position. World-fixed stars (parallax = 1) stay put; far-distance
/// stars (parallax ≈ 0) follow the camera so they appear nearly
/// stationary on screen. The remaining stars slide at intermediate
/// rates → classic multi-layer parallax.
fn tick_starfield_parallax(
    cameras: Query<&Transform, (With<Camera2d>, Without<BackgroundStar>)>,
    mut stars: Query<(&BackgroundStar, &mut Transform), Without<Camera2d>>,
) {
    let Ok(cam) = cameras.single() else { return };
    let cam_xy = cam.translation.truncate();
    for (star, mut xf) in &mut stars {
        // Screen offset from the camera = anchor - cam·parallax:
        //   parallax=0 → offset = anchor (star locked to camera).
        //   parallax=1 → offset = anchor - cam (star world-fixed).
        // The camera focus is now continuous (it can drift far past
        // the arena as ships lap the torus), so tile the offset into
        // a window around the camera — each star wraps individually
        // at the tile edge (off-screen for normal zoom), keeping the
        // field populated no matter how far the focus has travelled.
        let offset = star.anchor - cam_xy * star.parallax;
        let tiled = wrap_to_tile(offset, STAR_TILE);
        let render = cam_xy + tiled;
        xf.translation.x = render.x;
        xf.translation.y = render.y;
    }
}

/// Tile size for the parallax starfield — wraps each star into
/// `[-STAR_TILE/2, STAR_TILE/2]` around the camera. Matches the
/// spawn spread (`2 * STAR_AREA_HALF`) so density is unchanged.
const STAR_TILE: f32 = STAR_AREA_HALF * 2.0;

/// Wrap a vector into `[-tile/2, tile/2]` on each axis.
fn wrap_to_tile(v: Vec2, tile: f32) -> Vec2 {
    Vec2::new(
        v.x - tile * (v.x / tile).round(),
        v.y - tile * (v.y / tile).round(),
    )
}

const ZOOM_STEP: f32 = 1.05;
const SCALE_MIN: f32 = 0.15;
const SCALE_MAX: f32 = 6.0;
/// How many seconds of zoom-input silence before the camera
/// auto-reverts from Manual back to Auto follow. The follow's
/// existing lerp then gradually slides the framing back to the
/// bounding box — no jarring snap.
const MANUAL_REVERT_DELAY_S: f32 = 3.5;
/// Higher = snappier tween (1/seconds). At 8.0 the actual scale
/// reaches ~95% of target in ~0.4 s — smooth but not laggy.
const SMOOTHING_RATE: f32 = 8.0;

/// Read scroll wheel, adjust the *target* scale only. The actual
/// camera scale is tweened toward this target by `smooth_zoom_scale`
/// each frame, so a scroll notch produces a smooth animated zoom
/// instead of an instant snap.
fn handle_zoom_input(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut scroll: MessageReader<MouseWheel>,
    mut zoom_state: ResMut<ZoomState>,
    mut follow_mode: ResMut<CameraFollowMode>,
    windows: Query<&Window>,
    cameras: Query<(&Transform, &Projection), With<Camera2d>>,
) {
    let mut delta_total = 0.0_f32;
    for ev in scroll.read() {
        // Pixel-mode wheels (trackpads) report many small deltas;
        // line-mode (mouse wheels) reports ±1 per notch. Normalise
        // by clamping the per-event contribution.
        delta_total += ev.y.clamp(-3.0, 3.0);
    }
    if delta_total.abs() < 0.001 {
        zoom_state.last_scroll_dir = 0.0;
        return;
    }

    // Wheel input switches the camera to Manual mode briefly so
    // the auto-follow doesn't immediately undo the zoom — but
    // schedule a revert to Auto after REVERT_DELAY_S of stillness
    // so the player isn't permanently locked out of bbox-follow.
    *follow_mode = CameraFollowMode::Manual;
    zoom_state.manual_revert_at = time.elapsed_secs() + MANUAL_REVERT_DELAY_S;

    let zoom_dir = delta_total.signum();
    zoom_state.last_scroll_dir = zoom_dir;

    // Capture the mouse-pivot *before* changing the target scale.
    // `smooth_zoom_scale` uses it each frame to keep the world point
    // under the cursor pinned during the tween.
    if let (Ok(window), Ok((cam_xf, projection))) = (windows.single(), cameras.single())
    {
        if let Some(cursor) = window.cursor_position() {
            let scale = match projection {
                Projection::Orthographic(o) => o.scale,
                _ => 1.0,
            };
            // Pixel offset from window centre. Bevy window Y points
            // down; flip so +Y matches world up.
            let offset_px = Vec2::new(
                cursor.x - window.width() * 0.5,
                -(cursor.y - window.height() * 0.5),
            );
            let world = cam_xf.translation.truncate() + offset_px * scale;
            zoom_state.pivot = Some(ZoomPivot { offset_px, world });
        }
    }

    // Adjust the target scale only. Small step per scroll notch
    // (5%) keeps each "tick" feeling like one smooth nudge rather
    // than a big jump. The visible animation comes from the tween.
    let factor = ZOOM_STEP.powf(delta_total.abs());
    if delta_total > 0.0 {
        zoom_state.target_scale /= factor; // zoom in
    } else {
        zoom_state.target_scale *= factor; // zoom out
    }
    zoom_state.target_scale = zoom_state.target_scale.clamp(SCALE_MIN, SCALE_MAX);

    // Spawn a handful of zoom-burst stars. Count scales with the
    // scroll magnitude — a single notch gets 4 stars; a fling gets
    // a dozen or so. These animate in their own system and live
    // through the duration of the tween.
    let n = (3.0 + delta_total.abs() * 2.0).round() as i32;
    // Spawn around the camera, not the world origin — the focus
    // follows the ships and can sit far from (0,0).
    let cam_xy = cameras
        .single()
        .map(|(t, _)| t.translation.truncate())
        .unwrap_or(Vec2::ZERO);
    for _ in 0..n {
        let start_r = 60.0 + fastrand::f32() * 240.0;
        let theta = fastrand::f32() * std::f32::consts::TAU;
        let pos = cam_xy + Vec2::new(theta.cos() * start_r, theta.sin() * start_r);
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

/// Each frame, ease the camera's actual orthographic scale toward
/// the `ZoomState.target_scale`. Exponential decay with rate
/// `SMOOTHING_RATE` — the higher the rate the snappier the tween.
///
/// If a mouse `pivot` is set, also slide the camera each frame so
/// that the captured world-anchor stays under the cursor pixel-
/// offset for the entire tween — i.e. scroll-wheel zoom centres on
/// the cursor, not on screen-centre. Pivot clears when the scale
/// settles.
fn smooth_zoom_scale(
    time: Res<Time>,
    mut zoom_state: ResMut<ZoomState>,
    mut camera_q: Query<(&mut Projection, &mut Transform), With<Camera2d>>,
) {
    let dt = time.delta_secs();
    let blend = (SMOOTHING_RATE * dt).min(1.0);
    let mut settled = true;
    let pivot = zoom_state.pivot;
    for (mut projection, mut transform) in &mut camera_q {
        if let Projection::Orthographic(ref mut ortho) = *projection {
            let current = ortho.scale;
            let target = zoom_state.target_scale;
            let new_scale = if (current - target).abs() < 1e-4 {
                target
            } else {
                settled = false;
                current + (target - current) * blend
            };
            ortho.scale = new_scale;

            if let Some(p) = pivot {
                // Keep the world point captured at scroll-time at
                // the same pixel offset from the camera. Solves to
                // `cam = world_anchor - offset_px * scale`.
                let new_xy = p.world - p.offset_px * new_scale;
                transform.translation.x = new_xy.x;
                transform.translation.y = new_xy.y;
            }
        }
    }
    if settled && zoom_state.pivot.is_some() {
        zoom_state.pivot = None;
    }
}

/// Move each `ZoomStar` radially, fade it in then out over its
/// short lifetime, despawn at end.
fn tick_zoom_stars(
    mut commands: Commands,
    time: Res<Time>,
    cameras: Query<&Transform, (With<Camera2d>, Without<ZoomStar>)>,
    mut q: Query<(Entity, &mut ZoomStar, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    let cam_xy = cameras
        .single()
        .map(|t| t.translation.truncate())
        .unwrap_or(Vec2::ZERO);
    for (entity, mut star, mut xf, mut sprite) in &mut q {
        star.remaining_s -= dt;
        if star.remaining_s <= 0.0 {
            commands.entity(entity).try_despawn();
            continue;
        }
        // Move radially out from the camera centre. With no physics
        // we integrate the position directly.
        let pos = xf.translation.truncate();
        let rel = pos - cam_xy;
        let r = rel.length();
        if r > 0.1 {
            let dir = rel / r;
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

/// Two-finger pinch-to-zoom. When exactly two fingers are on the
/// screen, the ratio between the current and previous inter-finger
/// distance drives `target_scale` — pinching out (fingers spread)
/// shrinks the orthographic scale (zoom in), pinching in (fingers
/// close) grows it. Re-baselines each time a new gesture starts so
/// finger-lifts don't snap the scale.
///
/// Sits alongside `handle_zoom_input` and writes to the same
/// `ZoomState.target_scale`, so the existing `smooth_zoom_scale`
/// tween picks up the change automatically.
fn handle_pinch_zoom(
    time: Res<Time<Real>>,
    touches: Res<Touches>,
    touch_visible: Res<crate::mobile_controls::TouchButtonsVisible>,
    mut zoom_state: ResMut<ZoomState>,
    mut follow_mode: ResMut<CameraFollowMode>,
) {
    // While the on-screen controls are up, the player's two fingers are
    // the virtual stick + a fire/special button — NOT a pinch. Treating
    // them as one used to flip the camera to Manual (it stopped following,
    // the opponent slid off-screen) until the revert timer snapped it
    // back. Auto-follow frames the fight during touch play anyway; a real
    // pinch-zoom is still available once the controls are hidden (the `+`
    // toggle).
    if touch_visible.0 {
        zoom_state.last_pinch_dist = None;
        return;
    }
    // Collect up to two active touches. If there's a third we still
    // pinch on the first two — common mobile-browser idiom and
    // tolerates accidental third-finger taps.
    let mut iter = touches.iter();
    let (Some(t0), Some(t1)) = (iter.next(), iter.next()) else {
        zoom_state.last_pinch_dist = None;
        return;
    };
    let dist = t0.position().distance(t1.position());
    if dist < 1.0 {
        return;
    }
    let Some(prev) = zoom_state.last_pinch_dist else {
        // First frame of the gesture — set the baseline, no scaling
        // yet (avoids a jump on the first sample).
        zoom_state.last_pinch_dist = Some(dist);
        return;
    };
    let ratio = dist / prev;
    if (ratio - 1.0).abs() < 0.0005 {
        // Sub-pixel jitter; ignore so the scale doesn't drift while
        // fingers are still.
        zoom_state.last_pinch_dist = Some(dist);
        return;
    }
    // Active pinch implies intentional zooming — switch to Manual
    // briefly, but schedule a revert so the user isn't locked out
    // of the bbox follow forever.
    *follow_mode = CameraFollowMode::Manual;
    zoom_state.manual_revert_at = time.elapsed_secs() + MANUAL_REVERT_DELAY_S;
    // Spread fingers (ratio>1) = zoom in = smaller ortho scale.
    let new_scale = (zoom_state.target_scale / ratio).clamp(SCALE_MIN, SCALE_MAX);
    zoom_state.target_scale = new_scale;
    zoom_state.last_pinch_dist = Some(dist);
}

/// Toggle camera-follow mode with `C`. Once switched to Manual,
/// wheel / pinch zoom works as before and the camera stays where
/// the user left it. Switching back to Auto re-centres on the
/// midpoint of all live ships.
fn toggle_camera_follow_mode(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<CameraFollowMode>,
    mut zoom_state: ResMut<ZoomState>,
) {
    if keys.just_pressed(KeyCode::KeyC) {
        *mode = match *mode {
            CameraFollowMode::Auto => {
                // Permanent Manual — `manual_revert_at` of INFINITY
                // disables the auto-revert.
                zoom_state.manual_revert_at = f32::INFINITY;
                CameraFollowMode::Manual
            }
            CameraFollowMode::Manual => {
                // Back to Auto immediately.
                zoom_state.manual_revert_at = 0.0;
                CameraFollowMode::Auto
            }
        };
        info!("camera follow: {:?}", *mode);
    }

    // Auto-revert: any non-permanent Manual mode flips back to
    // Auto once the player has stopped zooming for a few seconds.
    if *mode == CameraFollowMode::Manual
        && zoom_state.manual_revert_at != f32::INFINITY
        && time.elapsed_secs() >= zoom_state.manual_revert_at
    {
        *mode = CameraFollowMode::Auto;
        zoom_state.manual_revert_at = 0.0;
    }
}

/// Canonical SC2-style "fit both ships in frame" camera.
///
/// Each frame (when mode is `Auto` and a cinematic isn't running):
///   - compute the centroid + half-extents of every live ship,
///   - lerp the camera position toward the centroid,
///   - lerp `ZoomState.target_scale` toward the scale that fits
///     the bounding box plus a generous padding.
///
/// The lerps make the follow feel weighty rather than snapping.
/// The cinematic-aware skip is so the Ultimate's dramatic zoom-in
/// isn't fought by this system.
fn follow_ships_with_camera(
    time: Res<Time>,
    mode: Res<CameraFollowMode>,
    ultimate: Option<Res<crate::ultimate::UltimateState>>,
    mut zoom_state: ResMut<ZoomState>,
    mut unwrap: ResMut<CameraUnwrap>,
    ships: Query<(Entity, &Position), With<Ship>>,
    windows: Query<&Window>,
    config: Res<crate::ship::MatchConfig>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    if *mode != CameraFollowMode::Auto {
        return;
    }
    if let Some(u) = ultimate {
        // Only the dramatic zoom-in / zoom-out beats lock the camera to
        // the cinematic driver. During the paused wind-ups the virtual
        // clock is stopped so our lerps below are no-ops anyway (the
        // camera simply holds wherever the zoom-out left it), and during
        // the unpaused action we deliberately resume ordinary follow.
        if crate::ultimate::is_cinematic_camera_phase(u.phase) {
            return;
        }
    }

    let Ok((mut cam_xf, mut projection)) = cameras.single_mut() else {
        return;
    };

    // The camera position doubles as a CONTINUOUS focus point that we
    // unwrap every ship around (minimum-image). Keeping it continuous
    // — never re-wrapped into the canonical arena — is what makes the
    // wrap seamless: two ships flying together stay in one unwrapped
    // frame, so when one crosses the edge its image stays right next
    // to the other and the camera glides instead of jerking. The
    // render-offset pass (`apply_toroidal_render_offset`) draws every
    // body in this same frame, and the starfield tiles around it, so
    // the unbounded focus is invisible.
    //
    // EXCEPT at a match-start snap: a new match's ships spawn in the
    // canonical cell (±900), but the focus may have drifted whole
    // arenas during the previous match. Imaging the fresh spawns around
    // that stale focus picks wrapped copies (the spawn pair is >½ arena
    // apart, so the nearest images land the wrong way round) — that's
    // the "P1 ends up on the right, ships back-to-back" bug. On snap we
    // re-anchor the focus to the canonical origin so the ships image at
    // their true spawn positions.
    let count = ships.iter().count();
    let snap_transition = zoom_state.last_ship_count == 0 && count > 0;
    let snap = snap_transition || zoom_state.pending_initial_snap;
    let focus = if snap {
        Vec2::ZERO
    } else {
        cam_xf.translation.truncate()
    };
    if snap {
        unwrap.images.clear();
    }

    // For every ship we hold two candidate images: the "sticky" one
    // (continuous with last frame's choice) and the "fresh" minimum-
    // image one (tightest possible around the focus). We commit to the
    // sticky framing — even when its bounding box is a little larger —
    // and only switch to fresh when fresh is tighter by a clear margin.
    // That deliberate tolerance is the fix for the half-arena flip-flop:
    // when two ships hover near maximum separation, the camera stops
    // snapping back and forth every frame and instead holds one framing,
    // re-jumping once only when the zoom payoff is genuinely worth it.
    let mut entries: Vec<(Entity, Vec2, Vec2)> = Vec::new(); // (e, sticky, fresh)
    let mut s_min = Vec2::splat(f32::INFINITY);
    let mut s_max = Vec2::splat(f32::NEG_INFINITY);
    let mut f_min = Vec2::splat(f32::INFINITY);
    let mut f_max = Vec2::splat(f32::NEG_INFINITY);
    // Anchor the "fresh" framing on the first ship's raw position and image
    // every ship as its minimum-image RELATIVE TO THAT ANCHOR. This is the
    // genuine tightest cluster (the true shortest-path pairing), independent
    // of where the continuous focus has drifted. Imaging around the focus
    // instead (the old bug) meant that when you flew one ship toward the wrap
    // edge — chasing the camera with it — `fresh` tracked the same stretched
    // framing as `sticky`, so the camera never noticed the ships were actually
    // closing from the other side and only reframed when they nearly touched.
    let anchor = ships.iter().next().map(|(_, p)| p.0).unwrap_or(focus);
    for (e, p) in &ships {
        // Pull any remembered image into the current focus cell first.
        // For normal continuity this is a no-op (last frame's image is
        // already near the focus); but if the focus was rebased while
        // we weren't looking — e.g. the Ultimate cinematic snapped it
        // into the ship's canonical cell — this discards the stale,
        // arenas-away memory so the camera doesn't pan all the way back.
        let prev = unwrap
            .images
            .get(&e)
            .copied()
            .map(|img| crate::physics::nearest_image(img, focus))
            .unwrap_or_else(|| crate::physics::nearest_image(p.0, focus));
        let sticky = crate::physics::nearest_image(p.0, prev);
        let fresh = crate::physics::nearest_image(p.0, anchor);
        s_min = s_min.min(sticky);
        s_max = s_max.max(sticky);
        f_min = f_min.min(fresh);
        f_max = f_max.max(fresh);
        entries.push((e, sticky, fresh));
    }
    if entries.is_empty() {
        return;
    }
    let sticky_extent = (s_max - s_min).max_element();
    let fresh_extent = (f_max - f_min).max_element();

    // Adopt the tighter fresh framing only when it helps by more than
    // HYSTERESIS_WU — otherwise tolerate the slightly bigger sticky box
    // for continuity. The safety valve forces fresh if the sticky box
    // has grown so wide that a body could approach the half-arena point
    // (beyond which the render offset's own image would flip).
    //
    // The dead-band is the visible cost of every reframe: when we finally
    // cross it, the framing tightens by ~HYSTERESIS_WU in one snap (and
    // the camera centroid hops half an arena, re-imaging both ships). So
    // we want this as SMALL as possible while still killing the flip-flop.
    // The framing is "switch once, then stable" (after a switch the new
    // sticky == fresh, so it won't bounce back until a ship re-crosses the
    // boundary the other way), which means even a tiny dead-band defeats
    // per-frame oscillation at the exact half-arena point. 700 was wildly
    // oversized — it let the pair drift to opposite screen edges (true sep
    // ~1150) before snapping them together with a jarring ~700 WU jolt.
    // 150 trips the reframe at true sep ~1425, so the ships never get more
    // than slightly-wide before the view tightens, and the snap is small.
    const HYSTERESIS_WU: f32 = 150.0;
    // Window pixel size — used both to decide whether the sticky framing
    // still fits on screen and (below) to convert the fit span to scale.
    let Ok(window) = windows.single() else { return };
    let win = Vec2::new(window.width().max(1.0), window.height().max(1.0));
    // If the sticky (continuous) framing has stretched so far it can no
    // longer fit even at max zoom-out, a ship is about to slide off the
    // edge — reframe to the tighter wrapped images NOW instead of waiting
    // on the hysteresis margin. This is the mobile "opponent off-screen
    // until it clicks back" fix: a narrow viewport hits SCALE_MAX much
    // sooner, so the stale unwrapped framing used to persist far too long.
    // Only fires when a genuinely tighter wrapped framing actually exists.
    let sticky_fits = {
        let sspan = (s_max - s_min).max(Vec2::splat(200.0));
        let spad = (sspan.max_element() * 0.35).max(150.0);
        let sneeded = sspan + Vec2::splat(spad * 2.0);
        (sneeded.x / win.x).max(sneeded.y / win.y) <= SCALE_MAX
    };
    let overflow_reframe = !sticky_fits && fresh_extent + 1.0 < sticky_extent;
    let force_fresh =
        sticky_extent > crate::physics::ARENA_SIZE * 0.85 || overflow_reframe;
    let use_fresh = force_fresh || fresh_extent + HYSTERESIS_WU < sticky_extent;
    // A genuine reframe (the images actually move to the other side) —
    // we snap the camera straight there rather than pan, so it can't
    // glide through intermediate poses where a ship pops across.
    let reframed = use_fresh && (sticky_extent - fresh_extent) > 1.0;

    let mut center = Vec2::ZERO;
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    unwrap.images.clear();
    for (e, sticky, fresh) in &entries {
        let img = if use_fresh { *fresh } else { *sticky };
        unwrap.images.insert(*e, img);
        center += img;
        min = min.min(img);
        max = max.max(img);
    }
    center /= count as f32;

    // Boss co-op: the capital ship is a fixed landmark at the origin.
    // It isn't a `Ship` (so it's absent from the loop above), but the
    // fight only reads if the whole dreadnought stays framed alongside
    // the fleet. Fold the static hull's bounding box into the framing
    // and recentre on the box midpoint so the camera holds both the
    // boss to the north and the player fleet to the south in view.
    if config.boss {
        // Hull corners must track `spawn_capital_ship`.
        const HULL_MIN: Vec2 = Vec2::new(-360.0, -700.0);
        const HULL_MAX: Vec2 = Vec2::new(360.0, 850.0);
        min = min.min(HULL_MIN);
        max = max.max(HULL_MAX);
        center = (min + max) * 0.5;
    }

    let span = (max - min).max(Vec2::splat(200.0));

    // (`win` was computed above, before the sticky-fit check.)

    // Pad the bounding box so the ships sit in the central zone, not
    // at the screen edges. The padding is *proportional* to the
    // bounding box (with a small floor) so close-quarters fights zoom
    // in tight — the padding shrinks with the ships — while a wide
    // separation still leaves breathing room around the pair.
    let pad = (span.max_element() * 0.35).max(150.0);
    let needed = span + Vec2::splat(pad * 2.0);
    let raw_scale = (needed.x / win.x).max(needed.y / win.y).clamp(SCALE_MIN, SCALE_MAX);

    // `snap` was determined up top (it gates the focus re-anchor).
    // Consume the flags now.
    zoom_state.last_ship_count = count;
    zoom_state.pending_initial_snap = false;

    // On snap, use a *tight* framing (small padding) because the
    // player wants the smallest bounding box at match start.
    let snap_scale = {
        let pad = (span.max_element() * 0.25).max(120.0);
        let needed_tight = span + Vec2::splat(pad * 2.0);
        (needed_tight.x / win.x)
            .max(needed_tight.y / win.y)
            .clamp(SCALE_MIN, SCALE_MAX)
    };

    let dt = time.delta_secs();
    let blend = (4.0 * dt).min(1.0);

    if snap {
        // Direct assign — no lerp — so the very first rendered frame
        // already has camera + scale fitted to the bbox. Re-wrap the
        // centroid into the canonical arena cell here (only on snap)
        // so the continuous focus resets each match instead of
        // accumulating drift over a long session, shifting the stored
        // images by the same whole-arena step so they stay consistent
        // with the wrapped focus.
        let c = crate::physics::min_image(center);
        let shift = c - center;
        for img in unwrap.images.values_mut() {
            *img += shift;
        }
        cam_xf.translation.x = c.x;
        cam_xf.translation.y = c.y;
        if let Projection::Orthographic(ref mut ortho) = *projection {
            ortho.scale = snap_scale;
        }
        zoom_state.target_scale = snap_scale;
    } else if reframed {
        // Deliberate reframe: jump the camera to the new centroid in one
        // step (the "significant zoom improvement" the framing committed
        // to), but still ease the scale so the zoom-in glides.
        cam_xf.translation.x = center.x;
        cam_xf.translation.y = center.y;
        let cur = zoom_state.target_scale;
        zoom_state.target_scale = cur + (raw_scale - cur) * blend;
    } else {
        cam_xf.translation.x += (center.x - cam_xf.translation.x) * blend;
        cam_xf.translation.y += (center.y - cam_xf.translation.y) * blend;
        // Continuous zoom: track the exact fit-scale every frame (no
        // hysteresis dead-band) so the view tightens smoothly as the
        // ships close and widens as they separate. The two lerps
        // (here + `smooth_zoom_scale`) keep it from feeling twitchy.
        let cur = zoom_state.target_scale;
        zoom_state.target_scale = cur + (raw_scale - cur) * blend;
    }
}

/// Draw every physics body at its periodic image nearest the camera,
/// so wraparound is seamless. Runs in `PostUpdate` after every
/// camera-moving system (auto-follow AND the Ultimate cinematic) and
/// before transform propagation, so it always images bodies around the
/// camera that's actually in charge this frame. During the cinematic
/// the camera is rebased into the acting ship's canonical cell (see
/// `hyper_trigger`), so this keeps the ship and its effects framed
/// correctly there too.
fn apply_toroidal_render_offset(
    camera: Query<&Transform, With<Camera2d>>,
    mut bodies: Query<(&Position, &mut Transform), Without<Camera2d>>,
) {
    let Ok(cam) = camera.single() else { return };
    let focus = cam.translation.truncate();
    for (pos, mut xf) in &mut bodies {
        let img = crate::physics::nearest_image(pos.0, focus);
        xf.translation.x = img.x;
        xf.translation.y = img.y;
    }
}

/// Set the `pending_initial_snap` flag so the very next Update
/// tick of `follow_ships_with_camera` writes camera position and
/// orthographic scale directly to fit the bounding box, regardless
/// of the current ZoomState. Doing the actual write here is
/// fragile — OnEnter(InMatch)'s commands haven't been flushed when
/// this runs, so a ships-query may return zero. Deferring to the
/// next Update guarantees commands have been applied and ships
/// are visible.
pub fn reset_for_new_match(mut zoom_state: ResMut<ZoomState>) {
    zoom_state.last_ship_count = 0;
    zoom_state.manual_revert_at = 0.0;
    zoom_state.pending_initial_snap = true;
}
