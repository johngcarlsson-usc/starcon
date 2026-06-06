//! Key-rebinding panel, reachable from the title screen.
//!
//! Standard "click an action, press a key" remapper. A "Keys" button on
//! the main menu opens this overlay; it lists the five gameplay actions
//! for Player 1 and Player 2 (online play uses the P1 map). Clicking a
//! row arms it — the next key you press becomes that action's binding.
//! Edits the `input::KeyBindings` resource; `input::sync_key_bindings`
//! pushes the change into the live keymap the readers use.
//!
//! Bindings are session-scoped (no on-disk persistence yet). P3/P4
//! hotseat slots keep their defaults.

use bevy::prelude::*;

use crate::input::{KeyBindings, BINDABLE_ACTIONS};
use crate::AppState;

/// Whether the rebind panel is showing (toggled by the menu "Keys"
/// button and the panel's own Close button).
#[derive(Resource, Default)]
pub struct KeyConfigOpen(pub bool);

/// The (slot, action-index) currently waiting for a key press, if any.
#[derive(Resource, Default)]
struct Rebinding(Option<(usize, usize)>);

#[derive(Component)]
struct KeyConfigRoot;

#[derive(Component)]
struct KeyConfigPanel;

/// A clickable row that arms rebinding of `(slot, action)`.
#[derive(Component, Clone, Copy)]
struct RebindButton {
    slot: usize,
    action: usize,
}

/// Text node showing a row's current key (or "press a key…" while armed).
#[derive(Component, Clone, Copy)]
struct RebindLabel {
    slot: usize,
    action: usize,
}

#[derive(Component, Clone, Copy)]
enum KeyConfigAction {
    Close,
    ResetDefaults,
}

/// Slots exposed in the UI. P1 + P2 cover hotseat-2P and online (which
/// uses the P1 map for every peer).
const UI_SLOTS: usize = 2;

const PANEL_BG: Color = Color::srgba(0.05, 0.07, 0.12, 0.96);
const ROW_BG: Color = Color::srgba(0.15, 0.25, 0.45, 0.9);
const ARMED_BG: Color = Color::srgba(0.85, 0.55, 0.15, 0.95);
const BORDER: Color = Color::srgba(0.55, 0.75, 1.0, 0.8);

pub struct KeyConfigPlugin;

impl Plugin for KeyConfigPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KeyConfigOpen>()
            .init_resource::<Rebinding>()
            .add_systems(OnEnter(AppState::MainMenu), spawn_key_config_ui)
            .add_systems(OnExit(AppState::MainMenu), despawn_key_config_ui)
            .add_systems(
                Update,
                (
                    handle_rebind_clicks,
                    handle_panel_actions,
                    capture_rebind_key,
                    update_rebind_labels,
                    apply_panel_visibility,
                )
                    .run_if(in_state(AppState::MainMenu)),
            );
    }
}

/// Pretty key name. `KeyCode`'s Debug ("ArrowLeft", "Slash", …) is good
/// enough; trim the common `Key`/`Arrow` noise for readability.
fn key_name(k: KeyCode) -> String {
    let raw = format!("{k:?}");
    let trimmed = raw
        .strip_prefix("Key")
        .or_else(|| raw.strip_prefix("Arrow"))
        .unwrap_or(&raw);
    trimmed.to_string()
}

fn spawn_key_config_ui(mut commands: Commands, mut open: ResMut<KeyConfigOpen>) {
    open.0 = false;
    commands
        .spawn((
            KeyConfigRoot,
            KeyConfigPanel,
            Visibility::Hidden,
            // Above the full-screen menu background.
            GlobalZIndex(100),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(8.0),
                left: Val::Percent(50.0),
                margin: UiRect {
                    left: Val::Px(-200.0),
                    ..default()
                },
                width: Val::Px(400.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(14.0)),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(BORDER),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("Key Bindings"),
                TextFont::from_font_size(22.0),
                TextColor(Color::srgb(0.85, 0.95, 1.0)),
                Node {
                    margin: UiRect::bottom(Val::Px(8.0)),
                    ..default()
                },
            ));
            for slot in 0..UI_SLOTS {
                panel.spawn((
                    Text::new(format!("Player {}", slot + 1)),
                    TextFont::from_font_size(16.0),
                    TextColor(Color::srgba(0.7, 0.8, 0.95, 0.9)),
                    Node {
                        margin: UiRect::top(Val::Px(6.0)),
                        ..default()
                    },
                ));
                for (action, _) in BINDABLE_ACTIONS.iter().enumerate() {
                    spawn_rebind_row(panel, slot, action);
                }
            }
            // Footer buttons.
            panel
                .spawn(Node {
                    margin: UiRect::top(Val::Px(12.0)),
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|row| {
                    spawn_action_button(row, KeyConfigAction::ResetDefaults, "Reset");
                    spawn_action_button(row, KeyConfigAction::Close, "Close");
                });
        });
}

fn spawn_rebind_row(parent: &mut ChildSpawnerCommands, slot: usize, action: usize) {
    parent
        .spawn((
            Button,
            RebindButton { slot, action },
            Node {
                width: Val::Px(340.0),
                height: Val::Px(32.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(12.0)),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(ROW_BG),
            BorderColor::all(BORDER),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(BINDABLE_ACTIONS[action].1),
                TextFont::from_font_size(15.0),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
            ));
            b.spawn((
                Text::new(""),
                RebindLabel { slot, action },
                TextFont::from_font_size(15.0),
                TextColor(Color::srgb(1.0, 0.9, 0.5)),
            ));
        });
}

fn spawn_action_button(parent: &mut ChildSpawnerCommands, action: KeyConfigAction, label: &str) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Px(110.0),
                height: Val::Px(34.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(ROW_BG),
            BorderColor::all(BORDER),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.to_string()),
                TextFont::from_font_size(16.0),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
            ));
        });
}

fn despawn_key_config_ui(
    mut commands: Commands,
    q: Query<Entity, With<KeyConfigRoot>>,
    mut rebinding: ResMut<Rebinding>,
) {
    rebinding.0 = None;
    for e in &q {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
    }
}

fn handle_rebind_clicks(
    interactions: Query<(&Interaction, &RebindButton), Changed<Interaction>>,
    mut rebinding: ResMut<Rebinding>,
) {
    for (interaction, btn) in &interactions {
        if matches!(interaction, Interaction::Pressed) {
            rebinding.0 = Some((btn.slot, btn.action));
        }
    }
}

fn handle_panel_actions(
    interactions: Query<(&Interaction, &KeyConfigAction), Changed<Interaction>>,
    mut open: ResMut<KeyConfigOpen>,
    mut bindings: ResMut<KeyBindings>,
    mut rebinding: ResMut<Rebinding>,
) {
    for (interaction, action) in &interactions {
        if !matches!(interaction, Interaction::Pressed) {
            continue;
        }
        match action {
            KeyConfigAction::Close => {
                open.0 = false;
                rebinding.0 = None;
            }
            KeyConfigAction::ResetDefaults => {
                *bindings = KeyBindings::default();
                rebinding.0 = None;
            }
        }
    }
}

/// While a row is armed, the next key press (other than Escape, which
/// cancels) becomes that binding.
fn capture_rebind_key(
    keys: Res<ButtonInput<KeyCode>>,
    mut rebinding: ResMut<Rebinding>,
    mut bindings: ResMut<KeyBindings>,
) {
    let Some((slot, action)) = rebinding.0 else {
        return;
    };
    if keys.just_pressed(KeyCode::Escape) {
        rebinding.0 = None;
        return;
    }
    if let Some(&key) = keys.get_just_pressed().next() {
        bindings.slots[slot][action] = key;
        rebinding.0 = None;
    }
}

fn update_rebind_labels(
    bindings: Res<KeyBindings>,
    rebinding: Res<Rebinding>,
    mut labels: Query<(&RebindLabel, &mut Text)>,
    mut rows: Query<(&RebindButton, &mut BackgroundColor)>,
) {
    for (label, mut text) in &mut labels {
        let armed = rebinding.0 == Some((label.slot, label.action));
        *text = Text::new(if armed {
            "press a key...".to_string()
        } else {
            key_name(bindings.slots[label.slot][label.action])
        });
    }
    for (btn, mut bg) in &mut rows {
        let armed = rebinding.0 == Some((btn.slot, btn.action));
        bg.0 = if armed { ARMED_BG } else { ROW_BG };
    }
}

fn apply_panel_visibility(
    open: Res<KeyConfigOpen>,
    mut panels: Query<&mut Visibility, With<KeyConfigPanel>>,
) {
    if !open.is_changed() {
        return;
    }
    let target = if open.0 {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut v in &mut panels {
        *v = target;
    }
}
