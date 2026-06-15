//! Dynamic split-screen camera (work in progress).
//!
//! Goal: in local hotseat, give each player their own pane — a vertical
//! split for 2, quadrants for 3-4 — and (later) blend seamlessly into one
//! shared view when players are close, like the LEGO/platformer dynamic
//! split.
//!
//! This is built in stages and **gated behind `SplitScreen.enabled`**
//! (default OFF, toggle with F8) so it never disturbs the proven
//! single-camera path until it's solid. While a split is active the
//! `PrimaryCamera` is deactivated and the toroidal re-image is suspended
//! (panes render the world at canonical coordinates); the seamless
//! torus-tiling that makes the wrap perfect in every pane lands in a
//! later stage.
//!
//! Status: Stage 2 — viewport panes + per-pane follow. Known gaps still
//! to come: torus-tiled wrap, per-pane starfield/HUD/minimap, and the
//! 2-player seamless merge.

use avian2d::prelude::Position;
use bevy::prelude::*;
use bevy::camera::Viewport;

use crate::ship::{MatchConfig, PlayerKind, Ship};
use crate::starfield::PrimaryCamera;

/// Master toggle for the split-screen experiment. OFF by default so the
/// single-camera renderer is untouched; F8 flips it.
#[derive(Resource, Default)]
pub struct SplitScreen {
    pub enabled: bool,
}

/// A split-screen pane camera, bound to one player slot's ship.
#[derive(Component, Debug)]
pub struct PaneCamera {
    pub slot: usize,
}

/// Fixed orthographic scale each pane renders at (world-units per pixel-
/// ish). Tunable; smaller = more zoomed in.
const PANE_SCALE: f32 = 1.3;

pub struct SplitScreenPlugin;

impl Plugin for SplitScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SplitScreen>().add_systems(
            Update,
            (
                toggle_split_screen,
                manage_split_cameras,
                follow_pane_cameras,
            )
                .chain(),
        );
    }
}

/// True while a split is actually being rendered (toggle on + a local
/// hotseat with ≥2 human slots). Other systems gate on this.
pub fn split_active(split: Res<SplitScreen>, config: Res<MatchConfig>) -> bool {
    split.enabled && hotseat_human_slots(&config) >= 2
}

/// Number of local human-controlled slots (couch hotseat). Online and
/// AI-only matches don't get a split.
fn hotseat_human_slots(config: &MatchConfig) -> usize {
    config
        .slots
        .iter()
        .filter(|s| s.kind == PlayerKind::Human)
        .count()
}

fn toggle_split_screen(keys: Res<ButtonInput<KeyCode>>, mut split: ResMut<SplitScreen>) {
    if keys.just_pressed(KeyCode::F8) {
        split.enabled = !split.enabled;
        info!("split-screen: {}", if split.enabled { "ON" } else { "OFF" });
    }
}

/// Compute each pane's pixel viewport rect for `n` panes in a `win`-sized
/// window: 1 = full, 2 = left/right, 3-4 = quadrants (3 leaves the
/// bottom-right empty).
fn pane_viewport(index: usize, n: usize, win: UVec2) -> Viewport {
    let (w, h) = (win.x.max(2), win.y.max(2));
    let (pos, size) = match n {
        0 | 1 => (UVec2::ZERO, UVec2::new(w, h)),
        2 => {
            let half = UVec2::new(w / 2, h);
            if index == 0 {
                (UVec2::ZERO, half)
            } else {
                (UVec2::new(w / 2, 0), UVec2::new(w - w / 2, h))
            }
        }
        _ => {
            // Quadrants. index 0=TL 1=TR 2=BL 3=BR.
            let (hw, hh) = (w / 2, h / 2);
            let col = (index % 2) as u32;
            let row = (index / 2) as u32;
            let px = col * hw;
            let py = row * hh;
            let sw = if col == 0 { hw } else { w - hw };
            let sh = if row == 0 { hh } else { h - hh };
            (UVec2::new(px, py), UVec2::new(sw, sh))
        }
    };
    Viewport {
        physical_position: pos,
        physical_size: size.max(UVec2::splat(1)),
        ..default()
    }
}

/// Spawn/despawn pane cameras to match the split state, and toggle the
/// primary camera on/off so the two render paths never overlap.
fn manage_split_cameras(
    mut commands: Commands,
    split: Res<SplitScreen>,
    config: Res<MatchConfig>,
    windows: Query<&Window>,
    mut primary: Query<&mut Camera, (With<PrimaryCamera>, Without<PaneCamera>)>,
    panes: Query<(Entity, &PaneCamera)>,
) {
    let active = split.enabled && hotseat_human_slots(&config) >= 2;

    if !active {
        // Tear down any panes and re-activate the single camera.
        for (e, _) in &panes {
            commands.entity(e).despawn();
        }
        if let Ok(mut cam) = primary.single_mut() {
            cam.is_active = true;
        }
        return;
    }

    // Split is on: the primary steps aside.
    if let Ok(mut cam) = primary.single_mut() {
        cam.is_active = false;
    }

    let n = hotseat_human_slots(&config).min(4);
    let win = windows
        .single()
        .map(|w| UVec2::new(w.physical_width(), w.physical_height()))
        .unwrap_or(UVec2::new(1280, 720));

    let existing: Vec<usize> = panes.iter().map(|(_, p)| p.slot).collect();
    // Human slots in order.
    let human_slots: Vec<usize> = config
        .slots
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == PlayerKind::Human)
        .map(|(i, _)| i)
        .take(4)
        .collect();

    for (index, &slot) in human_slots.iter().enumerate() {
        if existing.contains(&slot) {
            continue;
        }
        commands.spawn((
            Camera2d,
            Camera {
                order: (index as isize) + 1, // above the (inactive) primary
                viewport: Some(pane_viewport(index, n, win)),
                ..default()
            },
            Projection::Orthographic(OrthographicProjection {
                scale: PANE_SCALE,
                ..OrthographicProjection::default_2d()
            }),
            PaneCamera { slot },
        ));
    }
}

/// Keep each pane centred on its player's ship at canonical coordinates,
/// and refresh viewports on window resize / pane-count change.
fn follow_pane_cameras(
    config: Res<MatchConfig>,
    windows: Query<&Window>,
    ships: Query<(&Ship, &Position)>,
    mut panes: Query<(&PaneCamera, &mut Transform, &mut Camera)>,
) {
    let n = hotseat_human_slots(&config).min(4);
    let win = windows
        .single()
        .map(|w| UVec2::new(w.physical_width(), w.physical_height()))
        .unwrap_or(UVec2::new(1280, 720));
    // Stable order of human slots → pane index.
    let order: Vec<usize> = config
        .slots
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == PlayerKind::Human)
        .map(|(i, _)| i)
        .collect();

    for (pane, mut xf, mut cam) in &mut panes {
        // Refresh viewport (cheap; handles resize + count change).
        if let Some(index) = order.iter().position(|&s| s == pane.slot) {
            cam.viewport = Some(pane_viewport(index, n, win));
        }
        // Centre on this slot's ship if it's alive.
        if let Some((_, pos)) = ships.iter().find(|(s, _)| s.player_slot == pane.slot) {
            xf.translation.x = pos.0.x;
            xf.translation.y = pos.0.y;
        }
    }
}
