//! Per-ship collider polygon extraction + debug visualization.
//!
//! At startup we kick off a load of each ship's frame-1 sprite (the
//! "north-facing" pose). When the asset is ready, we compute a convex
//! hull from its non-transparent pixels, simplify it with
//! Douglas-Peucker to keep vertex counts low, and store the result in
//! a `ShipColliders` resource. `spawn_ship` reads this resource and
//! builds an Avian `Collider::convex_hull` from the vertices — or
//! falls back to a circle if the polygon isn't ready yet (early
//! startup race).
//!
//! Press **F3** in-game to toggle the debug-render overlay, which
//! draws each ship's collider outline + facing arrow as Gizmos lines.
//! Useful for verifying the auto-extracted shapes are sane before
//! relying on them for gameplay.

use std::collections::HashMap;

use avian2d::prelude::*;
use bevy::prelude::*;

use crate::ship::{ShipClass, ALL_CLASSES};

/// Convex-hull polygons per ship class, in ship-local coordinates
/// centred on the sprite (origin = sprite centre, +Y up). Avian
/// rotates the collider with the ship's `Rotation`, so we only ever
/// need one polygon per class — for the "north-facing" pose.
#[derive(Resource, Default, Debug)]
pub struct ShipColliders {
    pub polys: HashMap<ShipClass, Vec<Vec2>>,
}

/// Sprite handles we've requested but not yet processed. Drained as
/// each asset finishes loading and its polygon is computed.
#[derive(Resource, Default)]
pub struct PendingPolygons {
    pub handles: HashMap<ShipClass, Handle<Image>>,
}

/// F3 toggle — draw collider outlines + facing arrows as gizmos.
#[derive(Resource, Default)]
pub struct DebugCollider(pub bool);

pub struct ColliderPlugin;

impl Plugin for ColliderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShipColliders>()
            .init_resource::<PendingPolygons>()
            .init_resource::<DebugCollider>()
            .add_systems(Startup, kick_off_polygon_loads)
            .add_systems(
                Update,
                (process_pending_polygons, toggle_debug, draw_debug_gizmos),
            );
    }
}

/// Request all ship sprites at startup so the asset loader has them
/// in flight by the time we need polygons.
fn kick_off_polygon_loads(assets: Res<AssetServer>, mut pending: ResMut<PendingPolygons>) {
    for class in ALL_CLASSES.iter().copied() {
        // Most ships use `ship_s01.png` directly; three classes
        // (chebr/orzne/druma) kept the dat-extractor's `_bmp`/`_tga`
        // suffix on their filenames. We pick the right name per
        // class so polygon extraction works for the full roster.
        let filename = sprite_filename_for(class);
        let path = format!("ships/{}/sprites/{}", class.code(), filename);
        let handle: Handle<Image> = assets.load(path);
        pending.handles.insert(class, handle);
    }
}

/// Maps a class to the filename of its "north-facing" frame-1 sprite.
/// Centralised so the per-class oddities of the legacy dat extractor
/// don't leak into multiple loaders.
fn sprite_filename_for(class: ShipClass) -> &'static str {
    match class {
        ShipClass::Chebr | ShipClass::Orzne => "ship_s_01_tga.png",
        ShipClass::Druma => "ship_s01_bmp.png",
        _ => "ship_s01.png",
    }
}

/// Each frame, check every still-pending sprite. As they finish
/// loading, extract a polygon and stash it in `ShipColliders`, then
/// drop the pending entry.
fn process_pending_polygons(
    mut pending: ResMut<PendingPolygons>,
    mut colliders: ResMut<ShipColliders>,
    images: Res<Assets<Image>>,
) {
    if pending.handles.is_empty() {
        return;
    }
    let mut done = Vec::new();
    for (class, handle) in &pending.handles {
        let Some(image) = images.get(handle) else {
            continue; // not loaded yet, try next frame
        };
        let size = image.texture_descriptor.size;
        let data_len = image.data.as_ref().map(|d| d.len()).unwrap_or(0);
        match compute_polygon(image) {
            Some(poly) => {
                info!(
                    "collider polygon ready for {:?}: {} verts (image {}x{}, {} bytes)",
                    class, poly.len(), size.width, size.height, data_len
                );
                colliders.polys.insert(*class, poly);
            }
            None => {
                warn!(
                    "could not extract polygon for {:?}: image {}x{}, data {} bytes, format {:?}",
                    class, size.width, size.height, data_len,
                    image.texture_descriptor.format,
                );
            }
        }
        done.push(*class);
    }
    for class in done {
        pending.handles.remove(&class);
    }
}

/// Extract a simplified outline polygon from a sprite. Returns the
/// vertices of the ship's actual silhouette (concavities preserved),
/// not a convex hull — fed to `Collider::convex_decomposition` at
/// spawn time so concave shapes are decomposed into multiple convex
/// pieces internally by Avian.
///
/// Returns `None` if the image's CPU-side pixel buffer isn't
/// available (some platforms drop it after GPU upload) or no
/// non-transparent pixels were found.
pub fn compute_polygon(image: &Image) -> Option<Vec<Vec2>> {
    let data = image.data.as_ref()?;
    let size = image.texture_descriptor.size;
    let width = size.width as i32;
    let height = size.height as i32;
    let total = (width * height) as usize;
    if data.len() < total * 4 {
        return None;
    }
    // Binary alpha mask. Threshold > 16/255 ignores the soft AA halo
    // at the sprite edge so the boundary tracks the visible hull, not
    // its blurred outline.
    let mask: Vec<bool> = (0..total)
        .map(|i| data[i * 4 + 3] > 16)
        .collect();
    let contour = trace_boundary(&mask, width, height)?;
    if contour.len() < 3 {
        return None;
    }
    // Douglas-Peucker simplification — drops collinear / near-collinear
    // vertices. epsilon = 1 pixel keeps the polygon faithful to the
    // sprite while dropping a ~60-vert contour to ~16-24 verts (which
    // gives Avian's decomposition pleasant pieces to work with).
    let simplified = douglas_peucker_closed(&contour, 1.0);
    if simplified.len() < 3 {
        return None;
    }
    // Sprite-space (origin top-left, +Y down) → ship-local (origin
    // sprite centre, +Y up) so the collider lines up with the
    // ship's render-space orientation.
    let cx = width as f32 * 0.5;
    let cy = height as f32 * 0.5;
    Some(
        simplified
            .into_iter()
            .map(|p| Vec2::new(p.x - cx, cy - p.y))
            .collect(),
    )
}

/// Moore-Neighbor boundary tracing. Returns the outer contour of the
/// first foreground component in CW order (no duplicate start at end).
///
/// `mask` is row-major (`mask[y * width + x]`). The contour follows
/// pixel centres — anti-aliased edges should be pre-thresholded.
fn trace_boundary(mask: &[bool], width: i32, height: i32) -> Option<Vec<Vec2>> {
    // 8-neighbor offsets in clockwise order starting at "west".
    const CW: [(i32, i32); 8] = [
        (-1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
    ];
    let is_fg = |x: i32, y: i32| -> bool {
        if x < 0 || y < 0 || x >= width || y >= height {
            return false;
        }
        mask[(y * width + x) as usize]
    };
    // Find topmost-leftmost foreground pixel as the starting point.
    let mut start: Option<(i32, i32)> = None;
    'outer: for y in 0..height {
        for x in 0..width {
            if is_fg(x, y) {
                start = Some((x, y));
                break 'outer;
            }
        }
    }
    let (sx, sy) = start?;
    let mut boundary: Vec<(i32, i32)> = vec![(sx, sy)];
    let mut p = (sx, sy);
    // entry_idx is the direction we came FROM, expressed as an index
    // into CW. The topmost-leftmost start has no NW/N/NE/W foreground
    // neighbours, so entering "from west" (CW[0]) is always safe.
    let mut entry_idx: usize = 0;

    let safety_cap = (width * height) as usize + 8;
    loop {
        let mut found = false;
        for k in 1..=8 {
            let i = (entry_idx + k) % 8;
            let (dx, dy) = CW[i];
            let q = (p.0 + dx, p.1 + dy);
            if is_fg(q.0, q.1) {
                // Moving p → q in direction CW[i]; from q's frame we
                // arrived from the opposite direction, CW[(i + 4) % 8].
                p = q;
                entry_idx = (i + 4) % 8;
                if p == (sx, sy) {
                    // Loop closed.
                    return Some(
                        boundary
                            .into_iter()
                            .map(|(x, y)| Vec2::new(x as f32, y as f32))
                            .collect(),
                    );
                }
                boundary.push(p);
                found = true;
                break;
            }
        }
        if !found {
            // Isolated foreground pixel.
            break;
        }
        if boundary.len() > safety_cap {
            warn!("trace_boundary: safety cap hit ({} pts), shape may be malformed", boundary.len());
            break;
        }
    }
    Some(
        boundary
            .into_iter()
            .map(|(x, y)| Vec2::new(x as f32, y as f32))
            .collect(),
    )
}

/// Douglas-Peucker simplification for *closed* polygons. The standard
/// DP is for open polylines; for a closed contour we anchor the two
/// most-distant vertices (which act as the polyline endpoints) so the
/// algorithm doesn't accidentally collapse one side of the loop.
fn douglas_peucker_closed(points: &[Vec2], epsilon: f32) -> Vec<Vec2> {
    if points.len() <= 3 {
        return points.to_vec();
    }
    // Find the vertex farthest from points[0] — use that as the second
    // anchor so DP runs on two open chains that cover the full loop.
    let mut far_idx = 0;
    let mut far_d2 = 0.0;
    for (i, p) in points.iter().enumerate() {
        let d2 = (*p - points[0]).length_squared();
        if d2 > far_d2 {
            far_d2 = d2;
            far_idx = i;
        }
    }
    let mut out = Vec::new();
    let first_chain = douglas_peucker_open(&points[0..=far_idx], epsilon);
    let second_chain = douglas_peucker_open(&points[far_idx..], epsilon);
    out.extend_from_slice(&first_chain[..first_chain.len() - 1]);
    out.extend_from_slice(&second_chain[..second_chain.len() - 1]);
    out
}

fn douglas_peucker_open(points: &[Vec2], epsilon: f32) -> Vec<Vec2> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0_usize, points.len() - 1)];
    while let Some((start, end)) = stack.pop() {
        let mut max_dist = 0.0_f32;
        let mut max_idx = start;
        for i in (start + 1)..end {
            let d = perp_dist(points[i], points[start], points[end]);
            if d > max_dist {
                max_dist = d;
                max_idx = i;
            }
        }
        if max_dist > epsilon {
            keep[max_idx] = true;
            stack.push((start, max_idx));
            stack.push((max_idx, end));
        }
    }
    points
        .iter()
        .enumerate()
        .filter(|(i, _)| keep[*i])
        .map(|(_, p)| *p)
        .collect()
}

fn perp_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq < 1e-6 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    let proj = a + ab * t;
    (p - proj).length()
}

fn toggle_debug(keys: Res<ButtonInput<KeyCode>>, mut debug_on: ResMut<DebugCollider>) {
    if keys.just_pressed(KeyCode::F3) {
        debug_on.0 = !debug_on.0;
        info!(
            "collider debug overlay: {}",
            if debug_on.0 { "ON" } else { "OFF" }
        );
    }
}

fn draw_debug_gizmos(
    debug_on: Res<DebugCollider>,
    mut gizmos: Gizmos,
    ships: Query<(&Position, &Rotation, &ShipClass)>,
    colliders: Res<ShipColliders>,
) {
    if !debug_on.0 {
        return;
    }
    for (pos, rot, class) in &ships {
        // Facing arrow — short line out the nose of the ship.
        let forward = Vec2::new(-rot.sin, rot.cos);
        gizmos.line_2d(pos.0, pos.0 + forward * 40.0, Color::srgb(0.4, 1.0, 0.4));
        // Centre dot.
        gizmos.circle_2d(pos.0, 2.0, Color::srgb(1.0, 1.0, 0.4));

        // Collider outline — rotated polygon if we have one, else a
        // fallback circle so the user knows we're using a circle.
        if let Some(poly) = colliders.polys.get(class) {
            let rotated: Vec<Vec2> = poly
                .iter()
                .map(|p| {
                    Vec2::new(
                        p.x * rot.cos - p.y * rot.sin,
                        p.x * rot.sin + p.y * rot.cos,
                    ) + pos.0
                })
                .collect();
            for i in 0..rotated.len() {
                let a = rotated[i];
                let b = rotated[(i + 1) % rotated.len()];
                gizmos.line_2d(a, b, Color::srgb(1.0, 0.5, 0.0));
            }
        } else {
            gizmos.circle_2d(pos.0, 22.0, Color::srgb(1.0, 0.3, 0.3));
        }
    }
}
