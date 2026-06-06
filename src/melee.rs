//! Fleet/team melee mode.
//!
//! Two flavours of UI:
//!  - **Team builder** (`AppState::TeamSelect`): each player assembles a
//!    team of ships from the full roster. There's no point budget (honor
//!    system) — just a live counter of ship count and total TWCost. Local
//!    melee builds every player's team in sequence on one screen.
//!  - **Mid-match pick** (`AppState::InMatch`, melee only): when your
//!    active ship dies and you still have ships in reserve, the sim pauses
//!    and you pick your next ship from the survivors (SC2 super-melee
//!    style). Picking deploys it and removes it from your pool; a player
//!    whose pool empties is eliminated. Last team standing wins.
//!
//! Works on touch (tap the ship buttons / on-screen turn+fire) and on the
//! keyboard (arrows + Space/Enter in the builder; each player's own
//! turn/fire keys drive their mid-match pick).

use bevy::prelude::*;

use crate::collider::ShipColliders;
use crate::input::{
    read_local_just_pressed_with_virtual, VirtualInput, INPUT_FIRE, INPUT_LEFT, INPUT_RIGHT,
    INPUT_SPECIAL,
};
use crate::ship::{
    spawn_class, MatchConfig, PlayerKind, Ship, ShipCatalog, ShipClass, SlotConfig, ALL_CLASSES,
};
use crate::AppState;

pub struct MeleePlugin;

impl Plugin for MeleePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TeamBuilder>()
            .init_resource::<FleetMatch>()
            // Team builder screen.
            .add_systems(OnEnter(AppState::TeamSelect), spawn_team_builder_ui)
            .add_systems(OnExit(AppState::TeamSelect), despawn_team_builder_ui)
            .add_systems(
                Update,
                (tb_handle_buttons, tb_handle_keyboard, tb_update_ui)
                    .run_if(in_state(AppState::TeamSelect)),
            )
            // In-match fleet logic.
            .add_systems(OnEnter(AppState::InMatch), init_fleet_pools)
            .add_systems(
                Update,
                (melee_manage_respawn, melee_pick_input, melee_picker_ui)
                    .chain()
                    .run_if(in_state(AppState::InMatch)),
            )
            .add_systems(OnExit(AppState::InMatch), melee_cleanup);
    }
}

// ===========================================================================
//  Resources
// ===========================================================================

/// State of the team-builder screen.
#[derive(Resource, Default)]
pub struct TeamBuilder {
    pub active: bool,
    /// Number of teams to build (local melee builds them in sequence).
    pub num_players: usize,
    /// Online melee: only the local team is built here (phase 2).
    pub online: bool,
    /// Which player (0-based) is currently building.
    pub current: usize,
    /// The assembled fleet for each player.
    pub fleets: Vec<Vec<ShipClass>>,
    /// Keyboard highlight into `ALL_CLASSES`.
    pub cursor: usize,
}

impl TeamBuilder {
    /// Begin a fresh local team-build for `num_players` teams.
    pub fn begin(&mut self, num_players: usize, online: bool) {
        self.active = true;
        self.num_players = num_players.max(1);
        self.online = online;
        self.current = 0;
        self.fleets = vec![Vec::new(); self.num_players];
        self.cursor = 0;
    }
}

/// Runtime state of a melee match: each slot's not-yet-deployed pool plus
/// the per-slot "choosing next ship" / "eliminated" flags.
#[derive(Resource, Default)]
pub struct FleetMatch {
    pub active: bool,
    /// Ships still in reserve per slot (the deployed ship is not here).
    pub pool: [Vec<ShipClass>; 4],
    /// Slot is currently choosing its next ship (sim is paused).
    pub choosing: [bool; 4],
    /// Selection cursor into `pool[slot]` for the choosing player.
    pub cursor: [usize; 4],
    /// Slot has run out of ships (its last ship died with an empty pool).
    pub eliminated: [bool; 4],
    /// We've observed a living ship for this slot at least once — guards
    /// the startup frame before `spawn_match`'s ships exist.
    pub seen_alive: [bool; 4],
    /// A ship was just deployed for this slot; ignore its momentary
    /// absence until the new entity actually appears.
    pub deploying: [bool; 4],
}

// ===========================================================================
//  Spawn geometry (mirror of spawn_match's compass table)
// ===========================================================================

fn compass(slot: usize) -> (Vec2, f32) {
    use std::f32::consts::{FRAC_PI_2, PI};
    match slot % 4 {
        0 => (Vec2::new(-740.0, 0.0), -FRAC_PI_2),
        1 => (Vec2::new(740.0, 0.0), FRAC_PI_2),
        2 => (Vec2::new(0.0, -740.0), 0.0),
        _ => (Vec2::new(0.0, 740.0), PI),
    }
}

// ===========================================================================
//  Team-builder UI
// ===========================================================================

#[derive(Component)]
struct TeamBuilderRoot;
#[derive(Component)]
struct TbTitle;
#[derive(Component)]
struct TbStatus;
#[derive(Component)]
struct TbFleetList;
#[derive(Component, Clone, Copy)]
struct PickShip(ShipClass);
#[derive(Component, Clone, Copy)]
enum TbAction {
    Done,
    Clear,
}

const PANEL_BG: Color = Color::srgba(0.02, 0.03, 0.07, 0.97);
const TILE_BG: Color = Color::srgba(0.12, 0.18, 0.32, 0.9);
const TILE_HL: Color = Color::srgba(0.85, 0.55, 0.15, 0.95);
const BORDER: Color = Color::srgba(0.55, 0.75, 1.0, 0.7);

fn spawn_team_builder_ui(mut commands: Commands, catalog: Res<ShipCatalog>, windows: Query<&Window>) {
    let win_h = windows.single().map(|w| w.height()).unwrap_or(720.0);
    let s = (win_h / 720.0).clamp(0.55, 1.0);

    commands
        .spawn((
            TeamBuilderRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexStart,
                row_gap: Val::Px(8.0 * s),
                padding: UiRect::all(Val::Px(16.0 * s)),
                ..default()
            },
            BackgroundColor(PANEL_BG),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("Build your fleet"),
                TbTitle,
                TextFont::from_font_size(30.0 * s),
                TextColor(Color::srgb(0.85, 0.95, 1.0)),
            ));
            root.spawn((
                Text::new(""),
                TbStatus,
                TextFont::from_font_size(18.0 * s),
                TextColor(Color::srgb(1.0, 0.9, 0.5)),
            ));
            root.spawn((
                Text::new(""),
                TbFleetList,
                TextFont::from_font_size(15.0 * s),
                TextColor(Color::srgba(0.8, 0.9, 1.0, 0.9)),
                Node {
                    max_width: Val::Percent(92.0),
                    ..default()
                },
            ));

            // Ship roster grid.
            root.spawn((
                Node {
                    margin: UiRect::top(Val::Px(8.0 * s)),
                    width: Val::Percent(96.0),
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    justify_content: JustifyContent::Center,
                    column_gap: Val::Px(5.0 * s),
                    row_gap: Val::Px(5.0 * s),
                    ..default()
                },
            ))
            .with_children(|grid| {
                for &class in ALL_CLASSES.iter() {
                    spawn_ship_tile(grid, class, &catalog, s);
                }
            });

            // Footer.
            root.spawn(Node {
                margin: UiRect::top(Val::Px(12.0 * s)),
                column_gap: Val::Px(12.0 * s),
                ..default()
            })
            .with_children(|row| {
                spawn_footer_button(row, TbAction::Clear, "Clear", s);
                spawn_footer_button(row, TbAction::Done, "Done", s);
            });

            root.spawn((
                Text::new(
                    "Tap a ship to add it  ·  arrows + Space to add, Backspace to remove, Enter = Done",
                ),
                TextFont::from_font_size(13.0 * s),
                TextColor(Color::srgba(0.55, 0.62, 0.75, 0.8)),
                Node {
                    margin: UiRect::top(Val::Px(8.0 * s)),
                    ..default()
                },
            ));
        });
}

fn spawn_ship_tile(
    parent: &mut ChildSpawnerCommands,
    class: ShipClass,
    catalog: &ShipCatalog,
    s: f32,
) {
    let cost = catalog.cost_of(class);
    let name = catalog.name_of(class);
    parent
        .spawn((
            Button,
            PickShip(class),
            Node {
                width: Val::Px(108.0 * s),
                height: Val::Px(46.0 * s),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Px(6.0 * s)),
                padding: UiRect::horizontal(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(TILE_BG),
            BorderColor::all(BORDER),
        ))
        .with_children(|t| {
            t.spawn((
                Text::new(name),
                TextFont::from_font_size(12.0 * s),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
            ));
            t.spawn((
                Text::new(format!("{cost} pts")),
                TextFont::from_font_size(11.0 * s),
                TextColor(Color::srgb(1.0, 0.85, 0.45)),
            ));
        });
}

fn spawn_footer_button(parent: &mut ChildSpawnerCommands, action: TbAction, label: &str, s: f32) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Px(130.0 * s),
                height: Val::Px(40.0 * s),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(8.0 * s)),
                ..default()
            },
            BackgroundColor(TILE_BG),
            BorderColor::all(BORDER),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.to_string()),
                TextFont::from_font_size(18.0 * s),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
            ));
        });
}

fn despawn_team_builder_ui(
    mut commands: Commands,
    q: Query<Entity, With<TeamBuilderRoot>>,
) {
    for e in &q {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
    }
}

fn tb_handle_buttons(
    picks: Query<(&Interaction, &PickShip), Changed<Interaction>>,
    actions: Query<(&Interaction, &TbAction), Changed<Interaction>>,
    mut tb: ResMut<TeamBuilder>,
    mut config: ResMut<MatchConfig>,
    mut next: ResMut<NextState<AppState>>,
) {
    for (interaction, pick) in &picks {
        if matches!(interaction, Interaction::Pressed) {
            add_ship(&mut tb, pick.0);
        }
    }
    for (interaction, action) in &actions {
        if !matches!(interaction, Interaction::Pressed) {
            continue;
        }
        match action {
            TbAction::Clear => {
                let cur = tb.current;
                if let Some(f) = tb.fleets.get_mut(cur) {
                    f.clear();
                }
            }
            TbAction::Done => confirm_player(&mut tb, &mut config, &mut next),
        }
    }
}

fn tb_handle_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut tb: ResMut<TeamBuilder>,
    mut config: ResMut<MatchConfig>,
    mut next: ResMut<NextState<AppState>>,
) {
    let n = ALL_CLASSES.len();
    // Roughly 9 tiles per row at full width — close enough for up/down.
    const COLS: usize = 9;
    if keys.just_pressed(KeyCode::ArrowRight) {
        tb.cursor = (tb.cursor + 1) % n;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        tb.cursor = (tb.cursor + n - 1) % n;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        tb.cursor = (tb.cursor + COLS) % n;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        tb.cursor = (tb.cursor + n - COLS) % n;
    }
    if keys.just_pressed(KeyCode::Space) {
        let class = ALL_CLASSES[tb.cursor.min(n - 1)];
        add_ship(&mut tb, class);
    }
    if keys.just_pressed(KeyCode::Backspace) {
        let cur = tb.current;
        if let Some(f) = tb.fleets.get_mut(cur) {
            f.pop();
        }
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        confirm_player(&mut tb, &mut config, &mut next);
    }
}

fn add_ship(tb: &mut TeamBuilder, class: ShipClass) {
    let cur = tb.current;
    if let Some(f) = tb.fleets.get_mut(cur) {
        f.push(class);
    }
}

/// Finish the current player's fleet: advance to the next player, or
/// start the match once everyone has a team. A fleet must have at least
/// one ship.
fn confirm_player(
    tb: &mut TeamBuilder,
    config: &mut MatchConfig,
    next: &mut NextState<AppState>,
) {
    if tb.fleets.get(tb.current).map(|f| f.is_empty()).unwrap_or(true) {
        return;
    }
    if tb.current + 1 < tb.num_players {
        tb.current += 1;
        tb.cursor = 0;
        return;
    }
    // All teams built — assemble the match.
    let slots: Vec<SlotConfig> = tb
        .fleets
        .iter()
        .map(|f| {
            let fleet = if f.is_empty() {
                vec![ShipClass::Earcr]
            } else {
                f.clone()
            };
            SlotConfig::fleet_human(fleet)
        })
        .collect();
    *config = MatchConfig { slots, melee: true };
    tb.active = false;
    info!("melee: starting match with {} fleets", config.slots.len());
    next.set(AppState::InMatch);
}

fn tb_update_ui(
    tb: Res<TeamBuilder>,
    catalog: Res<ShipCatalog>,
    mut titles: Query<&mut Text, (With<TbTitle>, Without<TbStatus>, Without<TbFleetList>)>,
    mut status: Query<&mut Text, (With<TbStatus>, Without<TbTitle>, Without<TbFleetList>)>,
    mut fleet_list: Query<&mut Text, (With<TbFleetList>, Without<TbTitle>, Without<TbStatus>)>,
    mut tiles: Query<(&PickShip, &mut BackgroundColor)>,
) {
    let fleet = tb.fleets.get(tb.current);
    let count = fleet.map(|f| f.len()).unwrap_or(0);
    let points: i32 = fleet
        .map(|f| f.iter().map(|c| catalog.cost_of(*c)).sum())
        .unwrap_or(0);

    if let Ok(mut t) = titles.single_mut() {
        *t = Text::new(format!("Player {} — build your fleet", tb.current + 1));
    }
    if let Ok(mut t) = status.single_mut() {
        *t = Text::new(format!("Ships: {count}    Points: {points}"));
    }
    if let Ok(mut t) = fleet_list.single_mut() {
        let names: Vec<String> = fleet
            .map(|f| f.iter().map(|c| catalog.name_of(*c)).collect())
            .unwrap_or_default();
        *t = Text::new(if names.is_empty() {
            "(empty — add at least one ship)".to_string()
        } else {
            format!("Fleet: {}", names.join(", "))
        });
    }
    let highlit = ALL_CLASSES.get(tb.cursor).copied();
    for (pick, mut bg) in &mut tiles {
        bg.0 = if Some(pick.0) == highlit { TILE_HL } else { TILE_BG };
    }
}

// ===========================================================================
//  In-match fleet logic
// ===========================================================================

fn init_fleet_pools(config: Res<MatchConfig>, mut fleet: ResMut<FleetMatch>) {
    *fleet = FleetMatch::default();
    fleet.active = config.melee;
    if !config.melee {
        return;
    }
    for (slot, cfg) in config.slots.iter().enumerate().take(4) {
        // The first ship is deployed by spawn_match; the rest are reserve.
        fleet.pool[slot] = cfg.fleet.iter().skip(1).copied().collect();
    }
    info!(
        "melee: pools = {:?}",
        fleet.pool.iter().map(|p| p.len()).collect::<Vec<_>>()
    );
}

/// Detect a slot whose ship has died and either start its next-ship
/// choice (pool non-empty) or mark it eliminated (pool empty).
fn melee_manage_respawn(
    ships: Query<&Ship>,
    config: Res<MatchConfig>,
    mut fleet: ResMut<FleetMatch>,
) {
    if !fleet.active {
        return;
    }
    let mut has = [false; 4];
    for s in &ships {
        if s.player_slot < 4 {
            has[s.player_slot] = true;
        }
    }
    let n = config.slot_count().min(4);
    for slot in 0..n {
        if has[slot] {
            fleet.seen_alive[slot] = true;
            fleet.deploying[slot] = false;
            continue;
        }
        // No living ship in this slot right now.
        if !fleet.seen_alive[slot]
            || fleet.deploying[slot]
            || fleet.eliminated[slot]
            || fleet.choosing[slot]
        {
            continue;
        }
        if fleet.pool[slot].is_empty() {
            fleet.eliminated[slot] = true;
            info!("melee: P{} eliminated (fleet wiped)", slot + 1);
        } else {
            fleet.choosing[slot] = true;
            fleet.cursor[slot] = 0;
            info!("melee: P{} choosing next ship", slot + 1);
        }
    }
}

/// Drive the next-ship pick for the lowest-index choosing slot. Human
/// slots steer with their own turn keys + fire to deploy; AI auto-picks.
#[allow(clippy::too_many_arguments)]
fn melee_pick_input(
    keys: Res<ButtonInput<KeyCode>>,
    virt: Res<VirtualInput>,
    config: Res<MatchConfig>,
    catalog: Res<ShipCatalog>,
    assets: Res<AssetServer>,
    colliders: Res<ShipColliders>,
    mut fleet: ResMut<FleetMatch>,
    mut commands: Commands,
) {
    if !fleet.active {
        return;
    }
    let Some(slot) = fleet.choosing.iter().position(|c| *c) else {
        return;
    };
    let len = fleet.pool[slot].len();
    if len == 0 {
        fleet.choosing[slot] = false;
        return;
    }

    let is_ai = matches!(config.kind(slot), Some(PlayerKind::Ai));
    let mut deploy: Option<usize> = None;
    if is_ai {
        deploy = Some(0);
    } else {
        let jp = read_local_just_pressed_with_virtual(&keys, Some(&virt), slot);
        if jp.pressed(INPUT_LEFT) {
            fleet.cursor[slot] = (fleet.cursor[slot] + len - 1) % len;
        }
        if jp.pressed(INPUT_RIGHT) {
            fleet.cursor[slot] = (fleet.cursor[slot] + 1) % len;
        }
        if jp.pressed(INPUT_FIRE) || jp.pressed(INPUT_SPECIAL) {
            deploy = Some(fleet.cursor[slot].min(len - 1));
        }
    }

    if let Some(idx) = deploy {
        let class = fleet.pool[slot].remove(idx);
        let (pos, rot) = compass(slot);
        if let Some(entity) =
            spawn_class(&mut commands, &catalog, &assets, class, pos, rot, slot, &colliders)
        {
            if is_ai {
                commands
                    .entity(entity)
                    .try_insert((crate::ai::AiControlled, crate::ai::AiBrain::default()));
            }
        }
        fleet.choosing[slot] = false;
        fleet.deploying[slot] = true;
        fleet.cursor[slot] = 0;
        info!("melee: P{} deployed {:?}", slot + 1, class);
    }
}

fn melee_cleanup(
    mut commands: Commands,
    mut fleet: ResMut<FleetMatch>,
    ui: Query<Entity, With<MeleePickerRoot>>,
) {
    *fleet = FleetMatch::default();
    for e in &ui {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
    }
}

// ===========================================================================
//  Mid-match picker overlay
// ===========================================================================

#[derive(Component)]
struct MeleePickerRoot;

/// Rebuild the picker overlay to match the current choosing slot + cursor.
/// Cheap to rebuild (only happens while paused), so we despawn + respawn
/// whenever the displayed (slot, cursor, pool-len) changes.
fn melee_picker_ui(
    mut commands: Commands,
    fleet: Res<FleetMatch>,
    catalog: Res<ShipCatalog>,
    existing: Query<Entity, With<MeleePickerRoot>>,
    mut last: Local<Option<(usize, usize, usize)>>,
) {
    let active = if fleet.active {
        fleet.choosing.iter().position(|c| *c)
    } else {
        None
    };

    let Some(slot) = active else {
        // Not choosing — tear the overlay down.
        if !existing.is_empty() {
            for e in &existing {
                if let Ok(mut ec) = commands.get_entity(e) {
                    ec.try_despawn();
                }
            }
            *last = None;
        }
        return;
    };

    let pool = &fleet.pool[slot];
    let cursor = fleet.cursor[slot].min(pool.len().saturating_sub(1));
    let key = (slot, cursor, pool.len());
    if *last == Some(key) {
        return; // unchanged
    }
    *last = Some(key);

    for e in &existing {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
    }

    commands
        .spawn((
            MeleePickerRoot,
            GlobalZIndex(200),
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(40.0),
                left: Val::Percent(50.0),
                margin: UiRect {
                    left: Val::Px(-300.0),
                    ..default()
                },
                width: Val::Px(600.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(12.0)),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(BORDER),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new(format!("Player {} — choose your next ship", slot + 1)),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgb(0.85, 0.95, 1.0)),
            ));
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    justify_content: JustifyContent::Center,
                    column_gap: Val::Px(6.0),
                    row_gap: Val::Px(6.0),
                    max_width: Val::Percent(100.0),
                    ..default()
                },
            ))
            .with_children(|row| {
                for (i, &class) in pool.iter().enumerate() {
                    let hl = i == cursor;
                    row.spawn((
                        Node {
                            padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                            border: UiRect::all(Val::Px(1.5)),
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            ..default()
                        },
                        BackgroundColor(if hl { TILE_HL } else { TILE_BG }),
                        BorderColor::all(BORDER),
                    ))
                    .with_children(|tile| {
                        tile.spawn((
                            Text::new(format!(
                                "{} ({})",
                                catalog.name_of(class),
                                catalog.cost_of(class)
                            )),
                            TextFont::from_font_size(14.0),
                            TextColor(if hl {
                                Color::BLACK
                            } else {
                                Color::srgba(1.0, 1.0, 1.0, 0.95)
                            }),
                        ));
                    });
                }
            });
            root.spawn((
                Text::new("turn keys: select   ·   fire: deploy"),
                TextFont::from_font_size(13.0),
                TextColor(Color::srgba(0.6, 0.7, 0.85, 0.85)),
            ));
        });
}
