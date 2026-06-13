//! Boss co-op minimap — a little radar in the bottom-right corner that
//! plots *everything* in the arena as tiny coloured blips: the player
//! fighters, the dreadnought's hull / turrets / core, and the drifting
//! power-ups. It's a fixed full-arena view (the whole `[-1500, 1500]`
//! torus mapped into one square), so it never pans — you read the dots,
//! not the camera.
//!
//! Pure UI/visual: runs in `Update`, reads `Position`, never touches
//! gameplay. The blips are a pre-spawned pool of square `Node`s; each
//! frame we fill as many as we need (set colour / size / position /
//! show) and hide the rest, so there's no per-frame spawn churn.
//!
//! Boss-only: the whole panel hides outside a boss match.

use avian2d::prelude::Position;
use bevy::prelude::*;

use crate::physics::ARENA_HALF_EXTENT;
use crate::ship::{CapitalCore, CapitalShip, MatchConfig, PowerUp, Ship, Turret};

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_minimap)
            .add_systems(Update, update_minimap);
    }
}

/// On-screen size of the square radar (logical px).
const MAP_SIZE: f32 = 156.0;
/// Distance from the window corner (logical px).
const MARGIN: f32 = 14.0;
/// How many blips the pool can show at once. The arena holds at most a
/// handful of fighters + 1 core + 4 turrets + 3 power-ups + 4 hull
/// outline marks, so 48 is comfortable headroom.
const POOL: usize = 48;

/// Marker for the radar panel container.
#[derive(Component)]
struct MinimapRoot;

/// Marker for one pooled blip node.
#[derive(Component)]
struct MinimapBlip;

/// Player accent colour — mirrors `hud`/`indicator`.
fn slot_color(slot: usize) -> Color {
    match slot {
        0 => Color::srgb(0.6, 0.9, 1.0),
        1 => Color::srgb(1.0, 0.7, 0.6),
        2 => Color::srgb(0.6, 1.0, 0.7),
        _ => Color::srgb(1.0, 0.6, 1.0),
    }
}

fn spawn_minimap(mut commands: Commands) {
    commands
        .spawn((
            MinimapRoot,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(MARGIN),
                bottom: Val::Px(MARGIN),
                width: Val::Px(MAP_SIZE),
                height: Val::Px(MAP_SIZE),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.04, 0.08, 0.55)),
            BorderColor::all(Color::srgba(0.5, 0.7, 0.9, 0.45)),
            Visibility::Hidden,
            ZIndex(45),
        ))
        .with_children(|parent| {
            for _ in 0..POOL {
                parent.spawn((
                    MinimapBlip,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(0.0),
                        width: Val::Px(4.0),
                        height: Val::Px(4.0),
                        ..default()
                    },
                    BackgroundColor(Color::WHITE),
                    Visibility::Hidden,
                ));
            }
        });
}

/// One thing to plot: arena position, tint, and pixel size.
struct Blip {
    pos: Vec2,
    color: Color,
    size: f32,
}

fn update_minimap(
    config: Res<MatchConfig>,
    ships: Query<(&Position, &Ship)>,
    cores: Query<&Position, With<CapitalCore>>,
    turrets: Query<&Position, With<Turret>>,
    powerups: Query<(&Position, &PowerUp)>,
    hull: Query<(), With<CapitalShip>>,
    mut root: Query<&mut Visibility, (With<MinimapRoot>, Without<MinimapBlip>)>,
    mut blips: Query<(&mut Node, &mut BackgroundColor, &mut Visibility), With<MinimapBlip>>,
) {
    // Radar is a boss-co-op fixture; hide it everywhere else.
    if let Ok(mut vis) = root.single_mut() {
        *vis = if config.boss {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if !config.boss {
        for (_, _, mut vis) in &mut blips {
            *vis = Visibility::Hidden;
        }
        return;
    }

    // Build the plot list. Hull outline first (dim, drawn under the live
    // markers since the pool fills in order), then the live entities.
    let mut wanted: Vec<Blip> = Vec::with_capacity(POOL);

    if !hull.is_empty() {
        // Wedge corners + centre, in hull-local = world coords (the hull
        // sits at the origin, unrotated — see `spawn_capital_ship`). Just
        // enough dots to read the dreadnought's bulk and heading.
        let dim = Color::srgba(0.45, 0.50, 0.62, 0.8);
        for corner in [
            Vec2::new(0.0, 850.0),
            Vec2::new(-360.0, -700.0),
            Vec2::new(360.0, -700.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(-180.0, 75.0),
            Vec2::new(180.0, 75.0),
        ] {
            wanted.push(Blip { pos: corner, color: dim, size: 3.0 });
        }
    }

    for tpos in &turrets {
        wanted.push(Blip {
            pos: tpos.0,
            color: Color::srgb(0.95, 0.75, 0.25),
            size: 5.0,
        });
    }
    for cpos in &cores {
        wanted.push(Blip {
            pos: cpos.0,
            color: Color::srgb(1.0, 0.35, 0.30),
            size: 8.0,
        });
    }
    for (ppos, pu) in &powerups {
        wanted.push(Blip { pos: ppos.0, color: pu.kind.color(), size: 5.0 });
    }
    for (spos, ship) in &ships {
        wanted.push(Blip {
            pos: spos.0,
            color: slot_color(ship.player_slot),
            size: 6.0,
        });
    }

    // Map an arena position into a pixel offset inside the panel. World y
    // is up, screen y is down, so v is flipped. Positions are already
    // wrapped into [-half, half] by `wrap_arena` each physics tick.
    let to_px = |p: Vec2| -> Vec2 {
        let u = (p.x + ARENA_HALF_EXTENT) / (ARENA_HALF_EXTENT * 2.0);
        let v = (p.y + ARENA_HALF_EXTENT) / (ARENA_HALF_EXTENT * 2.0);
        Vec2::new(u.clamp(0.0, 1.0) * MAP_SIZE, (1.0 - v.clamp(0.0, 1.0)) * MAP_SIZE)
    };

    let mut it = wanted.into_iter();
    for (mut node, mut bg, mut vis) in &mut blips {
        match it.next() {
            Some(b) => {
                let c = to_px(b.pos);
                node.left = Val::Px(c.x - b.size * 0.5);
                node.top = Val::Px(c.y - b.size * 0.5);
                node.width = Val::Px(b.size);
                node.height = Val::Px(b.size);
                bg.0 = b.color;
                *vis = Visibility::Inherited;
            }
            None => {
                *vis = Visibility::Hidden;
            }
        }
    }
}
