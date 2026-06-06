//! Title screen + match-setup buttons.
//!
//! Drawn while `AppState == MainMenu`. The user picks a match mode
//! (Local 2-4 hotseat, Solo vs 1 or 3 AI, Online) and optionally
//! flips control prefs (steering scheme, AI difficulty) without
//! having to dig into the in-match Settings panel first.
//!
//! Everything is plain Bevy UI — no shaders, no extra crates. Despawned
//! on OnExit(MainMenu).

use bevy::prelude::*;

use crate::ai::AiDifficulty;
use crate::ship::{
    AngularControl, AngularControlOverride, MatchConfig, ShipClass, SlotConfig,
};
use crate::AppState;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::MainMenu), spawn_menu)
            .add_systems(OnExit(AppState::MainMenu), despawn_menu)
            .add_systems(
                Update,
                (handle_menu_buttons, update_menu_setting_labels)
                    .run_if(in_state(AppState::MainMenu)),
            );
    }
}

/// Marker on the root menu node so OnExit can despawn the whole
/// UI tree with a single recursive call.
#[derive(Component)]
struct MenuRoot;

/// Per-button discriminator. The interaction handler reads this
/// to decide what to do on press.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    LocalTwo,
    LocalThree,
    LocalFour,
    /// Local fleet-melee: build two teams, then fight them ship-by-ship.
    MeleeLocal,
    SoloVsOneAi,
    SoloVsThreeAi,
    Online,
    /// Cycle the global Classic/Inertial/Default steering override.
    CycleSteering,
    /// Cycle Easy/Medium/Hard AI difficulty.
    CycleDifficulty,
    /// Show/hide the on-screen touch controls (virtual stick + buttons)
    /// from the menu, so a mobile player can turn them on *before* the
    /// match starts instead of fumbling for the in-game `+` knob.
    ToggleControls,
    /// Open the key-rebinding panel (`keyconfig`).
    OpenKeys,
}

/// Marks a label whose text mirrors the live value of a setting,
/// updated each frame by `update_menu_setting_labels`.
#[derive(Component, Clone, Copy)]
struct SettingButtonLabel(MenuAction);

fn spawn_menu(mut commands: Commands, windows: Query<&Window>) {
    // Scale every dimension by the window height so the column fits in
    // short landscape viewports (see the earlier landscape-fit fix).
    let win_h = windows.single().map(|w| w.height()).unwrap_or(720.0);
    let s = (win_h / 640.0).clamp(0.5, 1.0);

    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(14.0 * s),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.02, 0.06, 0.94)),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("STARCON"),
                TextFont::from_font_size(64.0 * s),
                TextColor(Color::srgb(0.85, 0.95, 1.0)),
            ));
            root.spawn((
                Text::new("Modern Star Control: TimeWarp"),
                TextFont::from_font_size(18.0 * s),
                TextColor(Color::srgba(0.65, 0.75, 0.90, 0.85)),
                Node {
                    margin: UiRect::bottom(Val::Px(18.0 * s)),
                    ..default()
                },
            ));

            spawn_button(root, MenuAction::LocalTwo, "Local 2-Player", s);
            spawn_button(root, MenuAction::LocalThree, "Local 3-Player (hotseat)", s);
            spawn_button(root, MenuAction::LocalFour, "Local 4-Player (hotseat)", s);
            spawn_button(root, MenuAction::MeleeLocal, "Melee 2P (build fleets)", s);
            spawn_button(root, MenuAction::SoloVsOneAi, "Solo vs 1 AI", s);
            spawn_button(root, MenuAction::SoloVsThreeAi, "Solo vs 3 AI", s);
            spawn_button(root, MenuAction::Online, "Online (2-4 players)", s);

            // Compact controls row — cycle the most-changed prefs from
            // here so the player can set them without entering the match.
            root.spawn(Node {
                margin: UiRect::top(Val::Px(14.0 * s)),
                column_gap: Val::Px(10.0 * s),
                row_gap: Val::Px(10.0 * s),
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                justify_content: JustifyContent::Center,
                max_width: Val::Percent(96.0),
                ..default()
            })
            .with_children(|row| {
                spawn_setting_button(row, MenuAction::CycleSteering, s);
                spawn_setting_button(row, MenuAction::CycleDifficulty, s);
                spawn_setting_button(row, MenuAction::ToggleControls, s);
                spawn_setting_button(row, MenuAction::OpenKeys, s);
            });

            root.spawn((
                Text::new("[1..0] cycle P1 class    [F1..F10] cycle P2"),
                TextFont::from_font_size(13.0 * s),
                TextColor(Color::srgba(0.55, 0.62, 0.75, 0.75)),
                Node {
                    margin: UiRect::top(Val::Px(14.0 * s)),
                    ..default()
                },
            ));
        });
}

fn spawn_button(parent: &mut ChildSpawnerCommands, action: MenuAction, label: &str, s: f32) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Px(320.0 * s),
                height: Val::Px(50.0 * s),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(10.0 * s)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.15, 0.25, 0.45, 0.85)),
            BorderColor::all(Color::srgba(0.55, 0.75, 1.0, 0.80)),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(label.to_string()),
                TextFont::from_font_size(22.0 * s),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
            ));
        });
}

/// Compact cycle-button for a setting (Steering / Difficulty). Half
/// the width of a normal button so two fit in a row, with a label that
/// auto-updates from `update_menu_setting_labels`.
fn spawn_setting_button(parent: &mut ChildSpawnerCommands, action: MenuAction, s: f32) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Px(155.0 * s),
                height: Val::Px(40.0 * s),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(8.0 * s)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.12, 0.18, 0.32, 0.85)),
            BorderColor::all(Color::srgba(0.55, 0.75, 1.0, 0.70)),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(""),
                SettingButtonLabel(action),
                TextFont::from_font_size(16.0 * s),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
            ));
        });
}

fn despawn_menu(mut commands: Commands, q: Query<Entity, With<MenuRoot>>) {
    for e in &q {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
    }
}

fn handle_menu_buttons(
    mut commands: Commands,
    mut config: ResMut<MatchConfig>,
    mut next: ResMut<NextState<AppState>>,
    mut lobby_req: ResMut<crate::netplay::LobbyRequest>,
    mut angular: ResMut<AngularControlOverride>,
    mut difficulty: ResMut<AiDifficulty>,
    mut touch_visible: ResMut<crate::mobile_controls::TouchButtonsVisible>,
    mut key_config_open: ResMut<crate::keyconfig::KeyConfigOpen>,
    mut team_builder: ResMut<crate::melee::TeamBuilder>,
    interactions: Query<(&Interaction, &MenuAction), Changed<Interaction>>,
) {
    for (interaction, action) in &interactions {
        if !matches!(interaction, Interaction::Pressed) {
            continue;
        }
        // Starting any LOCAL match clears stale netplay identity. After a
        // netplay session `NetRole`/`LocalHandle` persist (e.g. Guest in
        // slot 1); without this reset `apply_player_input`'s guest gate
        // would skip the solo player's own ship — controls would appear
        // dead. The lobby's Online flow sets these itself, so only reset
        // for the local-match actions.
        if matches!(
            action,
            MenuAction::LocalTwo
                | MenuAction::LocalThree
                | MenuAction::LocalFour
                | MenuAction::SoloVsOneAi
                | MenuAction::SoloVsThreeAi
        ) {
            commands.insert_resource(crate::netcode::NetRole::Solo);
            commands.insert_resource(crate::netcode::LocalHandle(0));
            commands.remove_resource::<crate::netcode::NetSocket>();
        }
        match action {
            MenuAction::LocalTwo => {
                *config = MatchConfig::local_two(ShipClass::Earcr, ShipClass::Spael);
                next.set(AppState::InMatch);
            }
            MenuAction::LocalThree => {
                config.slots = vec![
                    SlotConfig::human(ShipClass::Earcr),
                    SlotConfig::human(ShipClass::Spael),
                    SlotConfig::human(ShipClass::Yehte),
                ];
                config.melee = false;
                next.set(AppState::InMatch);
            }
            MenuAction::LocalFour => {
                config.slots = vec![
                    SlotConfig::human(ShipClass::Earcr),
                    SlotConfig::human(ShipClass::Spael),
                    SlotConfig::human(ShipClass::Yehte),
                    SlotConfig::human(ShipClass::Chmav),
                ];
                config.melee = false;
                next.set(AppState::InMatch);
            }
            MenuAction::MeleeLocal => {
                // Build two local fleets, then play them as a melee.
                team_builder.begin(2, false);
                next.set(AppState::TeamSelect);
            }
            MenuAction::SoloVsOneAi => {
                // Straight 1v1 — player vs a single AI opponent.
                // Default opponent is the Spathi for the classic
                // "fast and slippery" tutorial fight; the user can
                // cycle it with F1..F10 on the main menu.
                config.slots = vec![
                    SlotConfig::human(ShipClass::Earcr),
                    SlotConfig::ai(ShipClass::Spael),
                ];
                config.melee = false;
                next.set(AppState::InMatch);
            }
            MenuAction::SoloVsThreeAi => {
                config.slots = vec![
                    SlotConfig::human(ShipClass::Earcr),
                    SlotConfig::ai(ShipClass::Druma),
                    SlotConfig::ai(ShipClass::Pkufu),
                    SlotConfig::ai(ShipClass::Kohma),
                ];
                config.melee = false;
                next.set(AppState::InMatch);
            }
            MenuAction::Online => {
                lobby_req.requested = true;
                next.set(AppState::LobbyOnline);
            }
            MenuAction::CycleSteering => {
                angular.0 = match angular.0 {
                    None => Some(AngularControl::Classic),
                    Some(AngularControl::Classic) => Some(AngularControl::Inertial),
                    Some(AngularControl::Inertial) => None,
                };
            }
            MenuAction::CycleDifficulty => {
                *difficulty = match *difficulty {
                    AiDifficulty::Easy => AiDifficulty::Medium,
                    AiDifficulty::Medium => AiDifficulty::Hard,
                    AiDifficulty::Hard => AiDifficulty::Easy,
                };
            }
            MenuAction::ToggleControls => {
                touch_visible.0 = !touch_visible.0;
            }
            MenuAction::OpenKeys => {
                key_config_open.0 = true;
            }
        }
    }
}

/// Refresh each cycle-button's label so it reflects the live resource
/// value (e.g. "Steering: Inertial", "AI: Hard"). Runs every frame
/// while the menu is up; cheap (at most two writes per tick).
fn update_menu_setting_labels(
    angular: Res<AngularControlOverride>,
    difficulty: Res<AiDifficulty>,
    touch_visible: Res<crate::mobile_controls::TouchButtonsVisible>,
    mut labels: Query<(&SettingButtonLabel, &mut Text)>,
) {
    for (label, mut text) in &mut labels {
        let s = match label.0 {
            MenuAction::CycleSteering => {
                let v = match angular.0 {
                    None => "Default",
                    Some(AngularControl::Classic) => "Classic",
                    Some(AngularControl::Inertial) => "Inertial",
                };
                format!("Angular: {v}")
            }
            MenuAction::CycleDifficulty => {
                let v = match *difficulty {
                    AiDifficulty::Easy => "Easy",
                    AiDifficulty::Medium => "Medium",
                    AiDifficulty::Hard => "Hard",
                };
                format!("AI: {v}")
            }
            MenuAction::ToggleControls => {
                format!("Controls: {}", if touch_visible.0 { "On" } else { "Off" })
            }
            MenuAction::OpenKeys => "Keys".to_string(),
            _ => continue,
        };
        *text = Text::new(s);
    }
}
