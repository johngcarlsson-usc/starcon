//! In-match settings overlay. A small always-visible "Set" gear in the
//! top-left toggles a panel of toggles that until now were only reachable
//! by keyboard (handy on touch devices, where there's no keyboard):
//!
//!   - Steering : per-class default / Classic / Inertial
//!     (mirrors the `M` key — `AngularControlOverride`)
//!   - Colliders: debug collider overlay on/off (mirrors `F3`)
//!   - Camera   : auto-follow / manual (mirrors `C`)
//!
//! Spawned on entering a match and torn down on exit. Pure UI — clicking
//! a row mutates the relevant resource; the keyboard shortcuts keep
//! working in parallel and this panel reflects whatever they set.

use bevy::prelude::*;

use crate::collider::DebugCollider;
use crate::ship::{AngularControl, AngularControlOverride};
use crate::starfield::CameraFollowMode;
use crate::AppState;

#[derive(Resource, Default)]
struct SettingsMenuOpen(bool);

#[derive(Component)]
struct SettingsUiRoot;

#[derive(Component)]
struct SettingsPanel;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum SettingButton {
    /// The gear — opens/closes the panel.
    Toggle,
    Steering,
    Colliders,
    Camera,
    /// Mobile "tilt + absolute aim" control scheme.
    TiltAim,
    /// AI difficulty: Easy / Medium / Hard. Affects every AI-controlled
    /// ship in the current match (and the next one).
    Difficulty,
}

/// Marks a text node whose contents reflect the live value of a setting.
#[derive(Component, Clone, Copy)]
struct SettingValueText(SettingButton);

pub struct SettingsMenuPlugin;

impl Plugin for SettingsMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SettingsMenuOpen>()
            .add_systems(OnEnter(AppState::InMatch), spawn_settings_ui)
            .add_systems(OnExit(AppState::InMatch), despawn_settings_ui)
            .add_systems(
                Update,
                (
                    handle_settings_buttons,
                    apply_panel_visibility,
                    update_setting_labels,
                )
                    .run_if(in_state(AppState::InMatch)),
            );
    }
}

const PANEL_BG: Color = Color::srgba(0.05, 0.07, 0.12, 0.92);
const ROW_BG: Color = Color::srgba(0.15, 0.25, 0.45, 0.85);
const BORDER: Color = Color::srgba(0.55, 0.75, 1.0, 0.80);

fn spawn_settings_ui(mut commands: Commands, mut open: ResMut<SettingsMenuOpen>) {
    // Always start closed so a match doesn't open behind the panel.
    open.0 = false;

    commands
        .spawn((SettingsUiRoot, Node::default()))
        .with_children(|root| {
            // Gear toggle — small, top-left, just right of the touch
            // "+" knob so they don't collide.
            root.spawn((
                Button,
                SettingButton::Toggle,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(16.0),
                    left: Val::Px(54.0),
                    width: Val::Px(40.0),
                    height: Val::Px(30.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.4, 0.4, 0.4, 0.30)),
                BorderColor::all(BORDER),
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new("Set"),
                    TextFont::from_font_size(15.0),
                    TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
                ));
            });

            // The panel itself (hidden until the gear is tapped).
            root.spawn((
                SettingsPanel,
                Visibility::Hidden,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(54.0),
                    left: Val::Px(16.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::all(Val::Px(12.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(PANEL_BG),
                BorderColor::all(BORDER),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("Settings"),
                    TextFont::from_font_size(18.0),
                    TextColor(Color::srgba(0.8, 0.9, 1.0, 0.95)),
                ));
                spawn_row(panel, SettingButton::Steering);
                spawn_row(panel, SettingButton::Colliders);
                spawn_row(panel, SettingButton::Camera);
                spawn_row(panel, SettingButton::TiltAim);
                spawn_row(panel, SettingButton::Difficulty);
            });
        });
}

fn spawn_row(parent: &mut ChildSpawnerCommands, kind: SettingButton) {
    parent
        .spawn((
            Button,
            kind,
            Node {
                width: Val::Px(260.0),
                height: Val::Px(40.0),
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
                Text::new(""),
                SettingValueText(kind),
                TextFont::from_font_size(17.0),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
            ));
        });
}

fn despawn_settings_ui(mut commands: Commands, q: Query<Entity, With<SettingsUiRoot>>) {
    for e in &q {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
    }
}

fn handle_settings_buttons(
    interactions: Query<(&Interaction, &SettingButton), Changed<Interaction>>,
    mut open: ResMut<SettingsMenuOpen>,
    mut angular: ResMut<AngularControlOverride>,
    mut debug_collider: ResMut<DebugCollider>,
    mut camera_mode: ResMut<CameraFollowMode>,
    mut zoom: ResMut<crate::starfield::ZoomState>,
    mut scheme: ResMut<crate::mobile_controls::MobileScheme>,
    mut difficulty: ResMut<crate::ai::AiDifficulty>,
) {
    use crate::mobile_controls::MobileScheme;
    for (interaction, button) in &interactions {
        if !matches!(interaction, Interaction::Pressed) {
            continue;
        }
        match button {
            SettingButton::Toggle => open.0 = !open.0,
            SettingButton::TiltAim => {
                *scheme = match *scheme {
                    MobileScheme::Normal => MobileScheme::AbsoluteTilt,
                    MobileScheme::AbsoluteTilt => MobileScheme::Absolute,
                    MobileScheme::Absolute => MobileScheme::Normal,
                };
            }
            SettingButton::Steering => {
                angular.0 = match angular.0 {
                    None => Some(AngularControl::Classic),
                    Some(AngularControl::Classic) => Some(AngularControl::Inertial),
                    Some(AngularControl::Inertial) => None,
                };
            }
            SettingButton::Colliders => debug_collider.0 = !debug_collider.0,
            SettingButton::Difficulty => {
                use crate::ai::AiDifficulty;
                *difficulty = match *difficulty {
                    AiDifficulty::Easy => AiDifficulty::Medium,
                    AiDifficulty::Medium => AiDifficulty::Hard,
                    AiDifficulty::Hard => AiDifficulty::Easy,
                };
            }
            SettingButton::Camera => {
                // Mirror the `C` key exactly: switching to Manual must
                // also pin the auto-revert timer to infinity, otherwise
                // the follow system reverts to Auto on the next frame.
                *camera_mode = match *camera_mode {
                    CameraFollowMode::Auto => {
                        zoom.manual_revert_at = f32::INFINITY;
                        CameraFollowMode::Manual
                    }
                    CameraFollowMode::Manual => {
                        zoom.manual_revert_at = 0.0;
                        CameraFollowMode::Auto
                    }
                };
            }
        }
    }
}

fn apply_panel_visibility(
    open: Res<SettingsMenuOpen>,
    mut panels: Query<&mut Visibility, With<SettingsPanel>>,
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

fn update_setting_labels(
    angular: Res<AngularControlOverride>,
    debug_collider: Res<DebugCollider>,
    camera_mode: Res<CameraFollowMode>,
    scheme: Res<crate::mobile_controls::MobileScheme>,
    difficulty: Res<crate::ai::AiDifficulty>,
    mut labels: Query<(&SettingValueText, &mut Text)>,
) {
    for (label, mut text) in &mut labels {
        let s = match label.0 {
            SettingButton::Steering => {
                let v = match angular.0 {
                    None => "Default",
                    Some(AngularControl::Classic) => "Classic",
                    Some(AngularControl::Inertial) => "Inertial",
                };
                format!("Steering: {v}")
            }
            SettingButton::Colliders => {
                format!("Colliders: {}", if debug_collider.0 { "On" } else { "Off" })
            }
            SettingButton::Camera => {
                let v = match *camera_mode {
                    CameraFollowMode::Auto => "Auto",
                    CameraFollowMode::Manual => "Manual",
                };
                format!("Camera: {v}")
            }
            SettingButton::TiltAim => {
                use crate::mobile_controls::MobileScheme;
                let v = match *scheme {
                    MobileScheme::Normal => "Off",
                    MobileScheme::AbsoluteTilt => "Tilt+Aim",
                    MobileScheme::Absolute => "Aim only",
                };
                format!("Steer mode: {v}")
            }
            SettingButton::Difficulty => {
                use crate::ai::AiDifficulty;
                let v = match *difficulty {
                    AiDifficulty::Easy => "Easy",
                    AiDifficulty::Medium => "Medium",
                    AiDifficulty::Hard => "Hard",
                };
                format!("AI: {v}")
            }
            SettingButton::Toggle => continue,
        };
        *text = Text::new(s);
    }
}
