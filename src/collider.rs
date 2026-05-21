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
        if let Some(image) = images.get(handle) {
            if let Some(poly) = compute_polygon(image) {
                info!(
                    "collider polygon ready for {:?}: {} vertices",
                    class,
                    poly.len()
                );
                colliders.polys.insert(*class, poly);
            }
            // Whether or not we extracted a polygon, treat the handle
            // as resolved so we don't retry every frame.
            done.push(*class);
        }
    }
    for class in done {
        pending.handles.remove(&class);
    }
}

/// Extract a simplified convex-hull polygon from a sprite. Returns
/// `None` if the image's CPU-side pixel buffer isn't available (some
/// platforms drop it after GPU upload) or no non-transparent pixels
/// were found.
pub fn compute_polygon(image: &Image) -> Option<Vec<Vec2>> {
    let data = image.data.as_ref()?;
    let size = image.texture_descriptor.size;
    let width = size.width as usize;
    let height = size.height as usize;
    if data.len() < width * height * 4 {
        return None;
    }
    // Collect every non-transparent pixel. Threshold > 16 to ignore
    // the soft anti-aliased halo at the sprite edge (which would
    // bloat the hull by a few pixels in every direction).
    let mut points: Vec<Vec2> = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) * 4;
            let alpha = data[i + 3];
            if alpha > 16 {
                points.push(Vec2::new(x as f32, y as f32));
            }
        }
    }
    if points.is_empty() {
        return None;
    }
    // Convex hull, then Douglas-Peucker simplification. epsilon=1.0
    // means we drop any hull vertex whose perpendicular distance to
    // the polyline is under one pixel — generally cuts a 64-pixel
    // hull from ~30 verts to ~12 without visible loss.
    let hull = convex_hull(&points);
    let simplified = douglas_peucker(&hull, 1.0);
    // Sprite space → ship-local space: origin at sprite centre, +Y
    // up (sprite source is +Y down).
    let cx = width as f32 * 0.5;
    let cy = height as f32 * 0.5;
    Some(
        simplified
            .into_iter()
            .map(|p| Vec2::new(p.x - cx, cy - p.y))
            .collect(),
    )
}

/// Andrew's monotone chain convex hull. Returns vertices in CCW order
/// with no duplicate start/end vertex.
fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut sorted: Vec<Vec2> = points.to_vec();
    sorted.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut hull: Vec<Vec2> = Vec::with_capacity(2 * sorted.len());
    // Lower hull.
    for &p in &sorted {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }
    let lower_count = hull.len() + 1;
    // Upper hull.
    for &p in sorted.iter().rev().skip(1) {
        while hull.len() >= lower_count
            && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
        {
            hull.pop();
        }
        hull.push(p);
    }
    hull.pop(); // last point is the same as the first
    hull
}

fn cross(o: Vec2, a: Vec2, b: Vec2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Iterative Douglas-Peucker — drops any vertex whose perpendicular
/// distance to the polyline is below `epsilon`.
fn douglas_peucker(points: &[Vec2], epsilon: f32) -> Vec<Vec2> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
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
