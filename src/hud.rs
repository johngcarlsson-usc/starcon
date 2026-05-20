//! In-match HUD: per-player crew readout, ship-destroyed handling, and
//! winner detection. Kept deliberately barebones — it's playtest scaffolding,
//! not the final UI.
//!
//! Future: per-ship battery bar, weapon-charge indicator, fleet roster
//! once we have more than one ship per player.

use bevy::prelude::*;

use crate::ship::{Crew, Ship};

#[derive(Component)]
struct CrewReadout {
    slot: usize,
}

#[derive(Resource, Default)]
struct MatchOutcome {
    winner_announced: bool,
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MatchOutcome>()
            .add_systems(Startup, setup_hud)
            .add_systems(
                Update,
                (
                    update_crew_readouts,
                    destroy_zero_crew_ships,
                    detect_winner,
                ),
            );
    }
}

fn setup_hud(mut commands: Commands) {
    // Root node spanning the screen so the two readouts can pin to corners.
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::all(Val::Px(12.0)),
            ..default()
        })
        .with_children(|root| {
            for slot in [0usize, 1usize] {
                root.spawn((
                    Text::new("P? crew --"),
                    TextFont::from_font_size(20.0),
                    TextColor(if slot == 0 {
                        Color::srgb(0.6, 0.9, 1.0)
                    } else {
                        Color::srgb(1.0, 0.7, 0.6)
                    }),
                    CrewReadout { slot },
                ));
            }
        });
}

fn update_crew_readouts(
    ships: Query<(&Ship, &Crew)>,
    mut readouts: Query<(&mut Text, &CrewReadout)>,
) {
    for (mut text, readout) in &mut readouts {
        let label = ships
            .iter()
            .find(|(s, _)| s.player_slot == readout.slot)
            .map(|(_, c)| format!("P{} crew {:>2}/{:>2}", readout.slot + 1, c.current, c.max))
            .unwrap_or_else(|| format!("P{} —", readout.slot + 1));
        text.0 = label;
    }
}

fn destroy_zero_crew_ships(
    mut commands: Commands,
    q: Query<(Entity, &Ship, &Crew), Changed<Crew>>,
) {
    for (entity, ship, crew) in &q {
        if crew.current <= 0 {
            info!("ship destroyed: P{} ({})", ship.player_slot + 1, ship.stats.name);
            commands.entity(entity).despawn();
        }
    }
}

fn detect_winner(ships: Query<&Ship>, mut outcome: ResMut<MatchOutcome>) {
    if outcome.winner_announced {
        return;
    }
    // Need to wait until at least one ship has spawned before we can
    // declare a winner — the HUD's Startup runs before the match scene.
    let mut have_p1 = false;
    let mut have_p2 = false;
    for ship in &ships {
        match ship.player_slot {
            0 => have_p1 = true,
            1 => have_p2 = true,
            _ => {}
        }
    }
    if have_p1 && !have_p2 {
        info!("WINNER: Player 1");
        outcome.winner_announced = true;
    } else if have_p2 && !have_p1 {
        info!("WINNER: Player 2");
        outcome.winner_announced = true;
    } else if !have_p1 && !have_p2 {
        // Both gone (probably ramming both to zero) — call it a draw.
        // Skip until we've definitely seen ships spawn.
    }
}
