//! Off-screen opponent indicators.
//!
//! When a ship drifts off the edge of the visible viewport — e.g. the two
//! ships spread far apart, or one is off chasing around the torus while the
//! camera is zoomed in on the other — we pin a small "P2 ↗" style chip to
//! the screen edge nearest that ship, coloured to match the HUD panel, with
//! an arrow glyph pointing the way.
//!
//! The direction respects the toroidal wrap: we point at the ship's periodic
//! image NEAREST the camera (the same image the renderer draws), so two ships
//! flying "apart" on screen — actually closing from the other side of the
//! torus — get arrows pointing off the wrap edge toward each other, not into
//! empty space.
//!
//! Pure UI/visual: runs in `Update`, reads `Position` + camera, never touches
//! gameplay state. Four chips are pre-spawned (one per player slot) and
//! toggled visible / repositioned each frame.

use avian2d::prelude::Position;
use bevy::prelude::*;

use crate::ship::Ship;

pub struct IndicatorPlugin;

impl Plugin for IndicatorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_indicators)
            .add_systems(Update, update_indicators);
    }
}

/// Marker for a per-slot off-screen indicator chip.
#[derive(Component)]
struct OffscreenIndicator {
    slot: usize,
}

/// Player accent colour — matches `hud::spawn_player_panel`.
fn slot_color(slot: usize) -> Color {
    match slot {
        0 => Color::srgb(0.6, 0.9, 1.0), // cyan
        1 => Color::srgb(1.0, 0.7, 0.6), // orange
        2 => Color::srgb(0.6, 1.0, 0.7), // green
        _ => Color::srgb(1.0, 0.6, 1.0), // magenta
    }
}

/// How far (logical px) from the window edge the chip is pinned.
const EDGE_MARGIN: f32 = 36.0;

fn spawn_indicators(mut commands: Commands) {
    for slot in 0..4usize {
        commands
            .spawn((
                OffscreenIndicator { slot },
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                Visibility::Hidden,
                ZIndex(50),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 20.0,
                        ..default()
                    },
                    TextColor(slot_color(slot)),
                ));
            });
    }
}

fn update_indicators(
    windows: Query<&Window>,
    cameras: Query<(&Transform, &Projection), With<crate::starfield::PrimaryCamera>>,
    ships: Query<(&Position, &Ship)>,
    mut indicators: Query<(&OffscreenIndicator, &mut Node, &mut Visibility, &Children)>,
    mut texts: Query<&mut Text>,
) {
    let Ok(window) = windows.single() else { return };
    let Ok((cam_xf, projection)) = cameras.single() else {
        return;
    };
    let scale = match projection {
        Projection::Orthographic(o) => o.scale,
        _ => 1.0,
    };
    let focus = cam_xf.translation.truncate();
    let win = Vec2::new(window.width().max(1.0), window.height().max(1.0));
    let half = win * 0.5;
    // Inner box the chip is clamped to (keep it off the very corner).
    let bx = (half.x - EDGE_MARGIN).max(1.0);
    let by = (half.y - EDGE_MARGIN).max(1.0);

    // Compute the on/off-screen state + edge placement per occupied slot.
    // (slot -> (screen_top_left, glyph)). Absent = hide that slot's chip.
    let mut placement: [Option<(Vec2, &'static str)>; 4] = [None; 4];

    for (pos, ship) in &ships {
        let slot = ship.player_slot.min(3);
        // The image of this ship nearest the camera — same one the renderer
        // draws, so the arrow agrees with what's on screen / across the wrap.
        let world = crate::physics::nearest_image(pos.0, focus);
        // Screen-space offset from centre, y flipped (world up -> screen down).
        let rel = world - focus;
        let screen_delta = Vec2::new(rel.x / scale, -rel.y / scale);

        let on_screen = screen_delta.x.abs() <= bx && screen_delta.y.abs() <= by;
        if on_screen {
            continue; // visible — no indicator needed
        }

        // Clamp the offset to the inner box, preserving direction, so the
        // chip sits on the edge pointing at the ship.
        let sx = if screen_delta.x.abs() > f32::EPSILON {
            bx / screen_delta.x.abs()
        } else {
            f32::INFINITY
        };
        let sy = if screen_delta.y.abs() > f32::EPSILON {
            by / screen_delta.y.abs()
        } else {
            f32::INFINITY
        };
        let k = sx.min(sy);
        let clamped = screen_delta * k;
        // Window pixel coords (origin top-left).
        let screen_pos = half + clamped;

        // Direction glyph (math convention: +x right, +y up).
        let glyph = arrow_glyph(clamped.x, -clamped.y);
        placement[slot] = Some((screen_pos, glyph));
    }

    for (ind, mut node, mut vis, children) in &mut indicators {
        match placement[ind.slot] {
            Some((screen_pos, glyph)) => {
                // Anchor roughly centred on the edge point.
                node.left = Val::Px((screen_pos.x - 24.0).clamp(0.0, win.x - 8.0));
                node.top = Val::Px((screen_pos.y - 14.0).clamp(0.0, win.y - 8.0));
                *vis = Visibility::Visible;
                if let Some(&child) = children.first() {
                    if let Ok(mut text) = texts.get_mut(child) {
                        **text = format!("P{} {}", ind.slot + 1, glyph);
                    }
                }
            }
            None => {
                *vis = Visibility::Hidden;
            }
        }
    }
}

/// Map a direction vector (math convention, +y up) to one of eight
/// compass labels. We use ASCII letters (N/NE/E/…) rather than Unicode
/// arrow glyphs: the bundled font has no arrow glyphs, so those rendered
/// as blank "tofu" squares. The chip is also pinned to the screen edge
/// in the ship's direction, so the label just reinforces that.
fn arrow_glyph(dx: f32, dy: f32) -> &'static str {
    use std::f32::consts::FRAC_PI_4;
    const GLYPHS: [&str; 8] = ["E", "NE", "N", "NW", "W", "SW", "S", "SE"];
    let ang = dy.atan2(dx); // -PI..PI
    let octant = (((ang / FRAC_PI_4).round() as i32) + 8) % 8;
    GLYPHS[octant as usize]
}
