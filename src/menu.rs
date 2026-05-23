//! Title screen + match-setup buttons.
//!
//! Drawn while `AppState == MainMenu`. Three options:
//!   - Local 2-Player: populate `MatchConfig` with the default
//!     pair and jump straight to InMatch.
//!   - Local 4-Player: same but with four classes, for hotseat
//!     testing on a single keyboard (slots 0..3 use Arrow/WASD/
//!     IJKL/NumPad keymaps respectively).
//!   - Online (2-4): hand off to the `LobbyOnline` state which
//!     drives the matchbox WebRTC connection.
//!
//! Everything is plain Bevy UI — no shaders, no extra crates,
//! just nodes and Interaction-based buttons. Despawned on
//! OnExit(MainMenu).

use bevy::prelude::*;

use crate::ship::{MatchConfig, ShipClass, SlotConfig};
use crate::AppState;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::MainMenu), spawn_menu)
            .add_systems(OnExit(AppState::MainMenu), despawn_menu)
            .add_systems(
                Update,
                handle_menu_buttons.run_if(in_state(AppState::MainMenu)),
            );
    }
}

/// Marker on the root menu node so OnExit can despawn the whole
/// UI tree with a single recursive call.
#[derive(Component)]
struct MenuRoot;

/// Per-button discriminator. The interaction handler reads this
/// to decide what to do on press.
#[derive(Component, Clone, Copy)]
enum MenuAction {
    LocalTwo,
    LocalThree,
    LocalFour,
    SoloVsAi,
    Online,
}

fn spawn_menu(mut commands: Commands) {
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
                row_gap: Val::Px(18.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.02, 0.06, 0.94)),
        ))
        .with_children(|root| {
            // Title text.
            root.spawn((
                Text::new("STARCON"),
                TextFont::from_font_size(64.0),
                TextColor(Color::srgb(0.85, 0.95, 1.0)),
            ));
            root.spawn((
                Text::new("Modern Star Control: TimeWarp"),
                TextFont::from_font_size(18.0),
                TextColor(Color::srgba(0.65, 0.75, 0.90, 0.85)),
                Node {
                    margin: UiRect::bottom(Val::Px(24.0)),
                    ..default()
                },
            ));

            spawn_button(root, MenuAction::LocalTwo, "Local 2-Player");
            spawn_button(root, MenuAction::LocalThree, "Local 3-Player (hotseat)");
            spawn_button(root, MenuAction::LocalFour, "Local 4-Player (hotseat)");
            spawn_button(root, MenuAction::SoloVsAi, "Solo vs 3 AI");
            spawn_button(root, MenuAction::Online, "Online (2-4 players)");

            root.spawn((
                Text::new("[1..0] cycle P1 class    [F1..F10] cycle P2"),
                TextFont::from_font_size(13.0),
                TextColor(Color::srgba(0.55, 0.62, 0.75, 0.75)),
                Node {
                    margin: UiRect::top(Val::Px(28.0)),
                    ..default()
                },
            ));
        });
}

fn spawn_button(parent: &mut ChildSpawnerCommands, action: MenuAction, label: &str) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Px(320.0),
                height: Val::Px(54.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.15, 0.25, 0.45, 0.85)),
            BorderColor::all(Color::srgba(0.55, 0.75, 1.0, 0.80)),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(label.to_string()),
                TextFont::from_font_size(22.0),
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
    mut config: ResMut<MatchConfig>,
    mut next: ResMut<NextState<AppState>>,
    mut lobby_req: ResMut<crate::netplay::LobbyRequest>,
    interactions: Query<(&Interaction, &MenuAction), Changed<Interaction>>,
) {
    for (interaction, action) in &interactions {
        if !matches!(interaction, Interaction::Pressed) {
            continue;
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
                next.set(AppState::InMatch);
            }
            MenuAction::LocalFour => {
                config.slots = vec![
                    SlotConfig::human(ShipClass::Earcr),
                    SlotConfig::human(ShipClass::Spael),
                    SlotConfig::human(ShipClass::Yehte),
                    SlotConfig::human(ShipClass::Chmav),
                ];
                next.set(AppState::InMatch);
            }
            MenuAction::SoloVsAi => {
                // P1 local + three AI opponents. Picked classes
                // give visual variety — Earcr (cruiser),
                // Druma (cannon kickback), Pkufu (lightning),
                // Kohma (saw blade).
                config.slots = vec![
                    SlotConfig::human(ShipClass::Earcr),
                    SlotConfig::ai(ShipClass::Druma),
                    SlotConfig::ai(ShipClass::Pkufu),
                    SlotConfig::ai(ShipClass::Kohma),
                ];
                next.set(AppState::InMatch);
            }
            MenuAction::Online => {
                // The lobby setup screen owns the slot config —
                // it'll mutate `MatchConfig` once the user picks
                // their humans/AI mix and a matchbox session
                // connects. Don't pre-populate here.
                lobby_req.requested = true;
                next.set(AppState::LobbyOnline);
            }
        }
    }
}
