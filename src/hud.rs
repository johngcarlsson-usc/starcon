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

#[derive(Component)]
struct StatusBanner;

/// Whose turn it is in the match loop. Restarts go Live → PostMatch
/// (waiting for R) → Live.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatchPhase {
    #[default]
    Live,
    PostMatch,
}

#[derive(Resource, Default)]
pub struct MatchOutcome {
    pub winner: Option<usize>,
    pub p1_wins: u32,
    pub p2_wins: u32,
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MatchOutcome>()
            .init_resource::<MatchPhase>()
            .add_systems(Startup, setup_hud)
            .add_systems(
                Update,
                (
                    update_crew_readouts,
                    destroy_zero_crew_ships,
                    detect_winner,
                    update_status_banner,
                ),
            );
    }
}

fn setup_hud(mut commands: Commands) {
    // Top row: per-player crew readouts pinned to corners.
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

    // Centred banner that lights up between rounds.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|root| {
            root.spawn((
                Text::new(""),
                TextFont::from_font_size(40.0),
                TextColor(Color::srgb(1.0, 1.0, 1.0)),
                StatusBanner,
            ));
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

fn detect_winner(
    ships: Query<&Ship>,
    mut outcome: ResMut<MatchOutcome>,
    mut phase: ResMut<MatchPhase>,
) {
    if *phase == MatchPhase::PostMatch {
        return;
    }
    let mut have_p1 = false;
    let mut have_p2 = false;
    for ship in &ships {
        match ship.player_slot {
            0 => have_p1 = true,
            1 => have_p2 = true,
            _ => {}
        }
    }
    // Need at least one ship to ever have existed before declaring an
    // outcome — Startup runs before the match scene spawns ships.
    let winner = if have_p1 && !have_p2 {
        Some(0usize)
    } else if have_p2 && !have_p1 {
        Some(1usize)
    } else {
        None
    };
    if let Some(w) = winner {
        outcome.winner = Some(w);
        if w == 0 {
            outcome.p1_wins += 1;
        } else {
            outcome.p2_wins += 1;
        }
        *phase = MatchPhase::PostMatch;
        info!(
            "WINNER: Player {} (score {}-{})",
            w + 1,
            outcome.p1_wins,
            outcome.p2_wins
        );
    }
}

fn update_status_banner(
    phase: Res<MatchPhase>,
    outcome: Res<MatchOutcome>,
    mut q: Query<&mut Text, With<StatusBanner>>,
) {
    for mut text in &mut q {
        text.0 = match *phase {
            MatchPhase::Live => format!("{} — {}", outcome.p1_wins, outcome.p2_wins),
            MatchPhase::PostMatch => match outcome.winner {
                Some(0) => format!(
                    "P1 WINS  ({}-{})\n[R] rematch",
                    outcome.p1_wins, outcome.p2_wins
                ),
                Some(1) => format!(
                    "P2 WINS  ({}-{})\n[R] rematch",
                    outcome.p1_wins, outcome.p2_wins
                ),
                _ => "DRAW\n[R] rematch".to_string(),
            },
        };
    }
}
