//! In-match HUD: per-player crew + battery readouts pinned to the
//! right edge of the screen, plus a centred status banner that shows
//! the running score and the rematch prompt between rounds.
//!
//! Both the bars and the numeric readouts are kept simple Bevy UI
//! nodes (no custom shaders). Each frame the relevant `update_*`
//! system measures the current/max ratio for the ship in that slot
//! and resizes the bar's foreground Node accordingly.

use bevy::prelude::*;

use crate::ship::{Battery, Crew, Ship};

#[derive(Component)]
struct CrewBarFill {
    slot: usize,
}

#[derive(Component)]
struct BatteryBarFill {
    slot: usize,
}

#[derive(Component)]
struct StatLabel {
    slot: usize,
    kind: StatKind,
}

#[derive(Clone, Copy)]
enum StatKind {
    Name,
    Crew,
    Battery,
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

/// Per-slot match scores. Index = player_slot (0..=3). The
/// length is fixed at 4 even when fewer players are in the
/// match — unused slots just stay at 0.
#[derive(Resource, Default, Clone)]
pub struct MatchOutcome {
    pub winner: Option<usize>,
    pub wins: [u32; 4],
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MatchOutcome>()
            .init_resource::<MatchPhase>()
            // The HUD is rebuilt every time we enter a match so
            // the per-slot panel count tracks the current
            // `MatchConfig.slot_count()` — going from 2-player
            // local to a 4-player online match without restarting
            // the app needs to add two more panels.
            .add_systems(OnEnter(crate::AppState::InMatch), setup_hud)
            .add_systems(OnExit(crate::AppState::InMatch), despawn_hud)
            // Death + winner detection ride in `GgrsSchedule` so they
            // tick once per confirmed GGRS frame on BOTH peers — same
            // schedule that mutates `Crew`. In `Update` they fired at
            // render-rate (different on each peer) with a non-rollback
            // `commands.try_despawn` that left the ship dead even when
            // rollback restored its `Crew>0`, so the local death felt
            // permanent and the remote peer's later despawn produced
            // a divergent world.
            // Both run on every peer in netplay. Crew is reconciled
            // from the host's snapshot, so the guest's `Changed<Crew>`
            // listener fires the same tick as the host's ship dies —
            // both peers reach the same despawn + winner decision
            // independently from the same authoritative data. No
            // `role_is_authoritative` gate: gating despawn would leave
            // a zombie ship on the guest (there's no
            // "ship row missing from snapshot → despawn" reconcile
            // sweep), and gating winner detection would leave the
            // guest stuck in `MatchPhase::Live` forever (which would
            // also break the post-match Ready toggle).
            .add_systems(
                FixedUpdate,
                (destroy_zero_crew_ships, detect_winner)
                    .chain()
                    .run_if(in_state(crate::AppState::InMatch)),
            )
            // Pure visuals — keep in Update so the HUD stays responsive
            // at render-rate without burdening the deterministic loop.
            .add_systems(
                Update,
                (
                    update_stat_labels,
                    update_bar_fills,
                    update_status_banner,
                    compact_hud_for_touch,
                )
                    .run_if(in_state(crate::AppState::InMatch)),
            );
    }
}

const BAR_HEIGHT_PX: f32 = 12.0;
const PANEL_BG: Color = Color::srgba(0.08, 0.10, 0.16, 0.85);
/// HUD column width: full when playing with keyboard, compact when the
/// on-screen touch controls are showing (so it doesn't crowd a phone).
const HUD_WIDTH_FULL: f32 = 220.0;
const HUD_WIDTH_COMPACT: f32 = 132.0;
/// Panel background when compact — more transparent so it stays out of
/// the way of the play area on mobile.
const PANEL_BG_COMPACT: Color = Color::srgba(0.08, 0.10, 0.16, 0.45);
const BAR_BG: Color = Color::srgb(0.12, 0.14, 0.18);
const CREW_COLOR: Color = Color::srgb(0.35, 0.95, 0.50);
const BATT_COLOR: Color = Color::srgb(0.45, 0.75, 1.00);

/// Marker on the root HUD nodes so OnExit can despawn them in
/// one recursive pass without having to remember each child id.
#[derive(Component)]
struct HudNode;

/// The right-edge stat column; `compact_hud_for_touch` resizes it.
#[derive(Component)]
struct HudColumn;

/// A per-player stat panel; goes more transparent in compact mode.
#[derive(Component)]
struct HudPanel;

/// Shrink + fade the stat HUD whenever the on-screen touch controls are
/// showing (i.e. the player is on a phone), so the bars don't eat the
/// right edge of a small screen. Reverts to the full panel for
/// keyboard play.
fn compact_hud_for_touch(
    touch: Res<crate::mobile_controls::TouchButtonsVisible>,
    mut columns: Query<&mut Node, With<HudColumn>>,
    mut panels: Query<&mut BackgroundColor, With<HudPanel>>,
) {
    // Runs every frame but only writes on an actual change, so it picks
    // up both toggles and the initial state when the HUD first spawns
    // without re-triggering a layout pass each frame.
    let width = Val::Px(if touch.0 { HUD_WIDTH_COMPACT } else { HUD_WIDTH_FULL });
    for mut node in &mut columns {
        if node.width != width {
            node.width = width;
        }
    }
    let bg = if touch.0 { PANEL_BG_COMPACT } else { PANEL_BG };
    for mut color in &mut panels {
        if color.0 != bg {
            color.0 = bg;
        }
    }
}

fn setup_hud(mut commands: Commands, config: Res<crate::ship::MatchConfig>) {
    // Right-edge column with one panel per active slot stacked
    // vertically. Slot count comes from `MatchConfig` — the
    // current match's player count.
    let n = config.slot_count().clamp(2, 4);
    commands
        .spawn((
            HudNode,
            HudColumn,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(HUD_WIDTH_FULL),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::all(Val::Px(12.0)),
                row_gap: Val::Px(12.0),
                ..default()
            },
        ))
        .with_children(|column| {
            for slot in 0..n {
                spawn_player_panel(column, slot);
            }
        });

    // Centred banner overlay used between rounds.
    commands
        .spawn((
            HudNode,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                Text::new(""),
                TextFont::from_font_size(40.0),
                TextColor(Color::srgb(1.0, 1.0, 1.0)),
                StatusBanner,
            ));
        });
}

fn despawn_hud(mut commands: Commands, q: Query<Entity, With<HudNode>>) {
    for e in &q {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
    }
}

fn spawn_player_panel(parent: &mut ChildSpawnerCommands, slot: usize) {
    let player_label = match slot {
        0 => Color::srgb(0.6, 0.9, 1.0), // cyan
        1 => Color::srgb(1.0, 0.7, 0.6), // orange
        2 => Color::srgb(0.6, 1.0, 0.7), // green
        _ => Color::srgb(1.0, 0.6, 1.0), // magenta
    };

    parent
        .spawn((
            HudPanel,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(PANEL_BG),
        ))
        .with_children(|panel| {
            // Heading: e.g. "P1  Earthling Cruiser"
            panel.spawn((
                Text::new(format!("P{} —", slot + 1)),
                TextFont::from_font_size(16.0),
                TextColor(player_label),
                StatLabel {
                    slot,
                    kind: StatKind::Name,
                },
            ));

            // Crew row
            spawn_stat_row(
                panel,
                slot,
                StatKind::Crew,
                "Crew",
                CREW_COLOR,
                |bar| CrewBarFill { slot }.insert_marker(bar),
            );

            // Battery row
            spawn_stat_row(
                panel,
                slot,
                StatKind::Battery,
                "Batt",
                BATT_COLOR,
                |bar| BatteryBarFill { slot }.insert_marker(bar),
            );
        });
}

fn spawn_stat_row<F>(
    panel: &mut ChildSpawnerCommands,
    slot: usize,
    kind: StatKind,
    label_text: &str,
    fill_color: Color,
    mut tag_fill: F,
) where
    F: FnMut(&mut EntityCommands),
{
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            ..default()
        })
        .with_children(|row| {
            // Top: "Crew  18/18"
            row.spawn(Node {
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|line| {
                line.spawn((
                    Text::new(label_text),
                    TextFont::from_font_size(13.0),
                    TextColor(Color::srgb(0.75, 0.82, 0.92)),
                ));
                line.spawn((
                    Text::new("--/--"),
                    TextFont::from_font_size(13.0),
                    TextColor(Color::srgb(0.92, 0.95, 1.00)),
                    StatLabel { slot, kind },
                ));
            });

            // Bottom: the bar (BG + fill child)
            row.spawn((
                Node {
                    // Track the column width (it shrinks in compact mode)
                    // instead of a fixed pixel width.
                    width: Val::Percent(100.0),
                    height: Val::Px(BAR_HEIGHT_PX),
                    ..default()
                },
                BackgroundColor(BAR_BG),
            ))
            .with_children(|bar| {
                let mut fill = bar.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(fill_color),
                ));
                tag_fill(&mut fill);
            });
        });
}

// Tiny trait so each marker can be inserted via a closure that
// receives an EntityCommands. Reduces boilerplate at the call site.
trait InsertMarker: Component + Sized {
    fn insert_marker(self, ec: &mut EntityCommands) {
        ec.insert(self);
    }
}
impl InsertMarker for CrewBarFill {}
impl InsertMarker for BatteryBarFill {}

fn update_stat_labels(
    ships: Query<(&Ship, &Crew, &Battery)>,
    mut labels: Query<(&mut Text, &StatLabel)>,
) {
    for (mut text, label) in &mut labels {
        let ship_data = ships.iter().find(|(s, _, _)| s.player_slot == label.slot);
        text.0 = match (ship_data, label.kind) {
            (Some((ship, _, _)), StatKind::Name) => {
                format!("P{}  {}", label.slot + 1, ship.stats.name)
            }
            (Some((_, crew, _)), StatKind::Crew) => {
                format!("{:>2}/{:>2}", crew.current, crew.max)
            }
            (Some((_, _, batt)), StatKind::Battery) => {
                format!("{:>2}/{:>2}", batt.current, batt.max)
            }
            (None, StatKind::Name) => format!("P{}  —", label.slot + 1),
            (None, _) => "—".into(),
        };
    }
}

fn update_bar_fills(
    ships: Query<(&Ship, &Crew, &Battery)>,
    mut crew_fills: Query<(&mut Node, &CrewBarFill), Without<BatteryBarFill>>,
    mut batt_fills: Query<(&mut Node, &BatteryBarFill), Without<CrewBarFill>>,
) {
    for (mut node, fill) in &mut crew_fills {
        let ratio = ships
            .iter()
            .find(|(s, _, _)| s.player_slot == fill.slot)
            .map(|(_, c, _)| {
                if c.max > 0 {
                    (c.current as f32 / c.max as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);
        node.width = Val::Percent(ratio * 100.0);
    }
    for (mut node, fill) in &mut batt_fills {
        let ratio = ships
            .iter()
            .find(|(s, _, _)| s.player_slot == fill.slot)
            .map(|(_, _, b)| {
                if b.max > 0 {
                    (b.current as f32 / b.max as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);
        node.width = Val::Percent(ratio * 100.0);
    }
}

fn destroy_zero_crew_ships(
    mut commands: Commands,
    q: Query<(Entity, &Ship, &Crew), Changed<Crew>>,
) {
    for (entity, ship, crew) in &q {
        if crew.current <= 0 {
            info!("ship destroyed: P{} ({})", ship.player_slot + 1, ship.stats.name);
            commands.entity(entity).try_despawn();
        }
    }
}

fn detect_winner(
    ships: Query<&Ship>,
    config: Res<crate::ship::MatchConfig>,
    mut outcome: ResMut<MatchOutcome>,
    mut phase: ResMut<MatchPhase>,
) {
    if *phase == MatchPhase::PostMatch {
        return;
    }
    // Track which slots still have a living ship.
    let active_slots = config.slot_count().min(4);
    let mut alive = [false; 4];
    for ship in &ships {
        if ship.player_slot < 4 {
            alive[ship.player_slot] = true;
        }
    }
    let live_count = alive[..active_slots].iter().filter(|a| **a).count();
    // Match ends when at most one slot has a ship left. Sole
    // survivor wins; double-KO (zero alive) is a draw.
    if live_count > 1 {
        return;
    }
    let winner = alive[..active_slots].iter().position(|a| *a);
    outcome.winner = winner;
    if let Some(w) = winner {
        outcome.wins[w] = outcome.wins[w].saturating_add(1);
        info!(
            "WINNER: Player {} (scores {:?})",
            w + 1,
            &outcome.wins[..active_slots]
        );
    } else {
        info!("DRAW (scores {:?})", &outcome.wins[..active_slots]);
    }
    *phase = MatchPhase::PostMatch;
}

fn update_status_banner(
    phase: Res<MatchPhase>,
    outcome: Res<MatchOutcome>,
    config: Res<crate::ship::MatchConfig>,
    slot_inputs: Res<crate::input::SlotInputs>,
    session: Option<Res<crate::netcode::NetSocket>>,
    mut q: Query<&mut Text, With<StatusBanner>>,
) {
    let n = config.slot_count().min(4);
    let score_str = outcome.wins[..n]
        .iter()
        .map(|w| w.to_string())
        .collect::<Vec<_>>()
        .join("-");
    for mut text in &mut q {
        text.0 = match *phase {
            // Hide the running score during play — it sat in the
            // middle of the screen and got in the way. The
            // post-match banner still shows the winner + score.
            MatchPhase::Live => String::new(),
            MatchPhase::PostMatch => {
                let head = match outcome.winner {
                    Some(w) => format!("P{} WINS  ({})", w + 1, score_str),
                    None => format!("DRAW  ({})", score_str),
                };
                if session.is_some() {
                    // Netplay: show the per-slot Ready vote + class
                    // pick so each peer can see what the OTHER side
                    // has chosen and whether they're ready. The class
                    // index must be read from `SlotInputs` (the
                    // GGRS-synced source) not `MatchConfig` — the
                    // local peer's Tab cycle updates `MatchConfig`
                    // locally, but the REMOTE peer's Tab presses only
                    // arrive through `SlotInputs.held[remote].class`.
                    // `MatchConfig` is the post-match-restart target,
                    // not the live picker view.
                    let mut lines = String::new();
                    for (i, _) in config.slots.iter().enumerate().take(n) {
                        let ready = slot_inputs.flag(i, crate::input::FLAG_READY);
                        let mark = if ready { "READY" } else { "..." };
                        let class = crate::ship::class_from_index(
                            slot_inputs.held[i].class,
                        );
                        lines.push_str(&format!(
                            "\nP{}: {:?}  {}",
                            i + 1,
                            class,
                            mark
                        ));
                    }
                    format!(
                        "{head}{lines}\n[Tab] change ship   [R] toggle ready"
                    )
                } else {
                    format!("{head}\n[R] rematch")
                }
            }
        };
    }
}
