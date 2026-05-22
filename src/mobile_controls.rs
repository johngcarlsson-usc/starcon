//! On-screen virtual controls for touch devices (phones / tablets).
//! Drives the same `VirtualInput` resource that `input.rs` OR's into
//! player 1's keyboard input each tick, so the touch buttons "are"
//! the keyboard from the rest of the game's perspective.
//!
//! Layout (anchored to viewport edges, so it works in any orientation):
//!   Bottom-left  : ◀  ▶  ▲     — turn-left, turn-right, thrust
//!   Bottom-right : FIRE  SPC   — primary, special
//!   Top-center   : ★ULT★       — ultimate cinematic trigger
//!
//! The buttons are also rendered on desktop so they can be tested
//! with the mouse — they're translucent and sit in the corners so
//! they don't obscure gameplay. On a desktop install they're a no-op
//! relative to the keyboard; on a phone they're the only way to play.

use bevy::prelude::*;

use crate::input::{
    PlayerInput, VirtualInput, INPUT_FIRE, INPUT_LEFT, INPUT_RIGHT, INPUT_SPECIAL,
    INPUT_THRUST,
};

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchAction {
    Left,
    Right,
    Thrust,
    Fire,
    Special,
    Ultimate,
}

impl TouchAction {
    fn mask(self) -> Option<u8> {
        match self {
            TouchAction::Left => Some(INPUT_LEFT),
            TouchAction::Right => Some(INPUT_RIGHT),
            TouchAction::Thrust => Some(INPUT_THRUST),
            TouchAction::Fire => Some(INPUT_FIRE),
            TouchAction::Special => Some(INPUT_SPECIAL),
            TouchAction::Ultimate => None,
        }
    }
    fn label(self) -> &'static str {
        match self {
            TouchAction::Left => "<",
            TouchAction::Right => ">",
            TouchAction::Thrust => "^",
            TouchAction::Fire => "FIRE",
            TouchAction::Special => "SPEC",
            TouchAction::Ultimate => "ULT",
        }
    }
}

/// Tracks the previous-frame `Interaction` per button so we can emit
/// just-pressed / just-released edges into `VirtualInput`.
#[derive(Component, Debug, Default)]
pub struct LastInteraction(pub Interaction);

pub struct MobileControlsPlugin;

impl Plugin for MobileControlsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_touch_controls)
            .add_systems(Update, drive_virtual_input);
    }
}

const BTN_SIZE: f32 = 78.0;
const BTN_MARGIN: f32 = 14.0;
const BTN_ALPHA: f32 = 0.32;

fn spawn_touch_controls(mut commands: Commands) {
    let dpad_color = Color::srgba(0.20, 0.55, 0.95, BTN_ALPHA);
    let fire_color = Color::srgba(0.95, 0.45, 0.25, BTN_ALPHA);
    let spec_color = Color::srgba(0.65, 0.30, 0.95, BTN_ALPHA);
    let ult_color = Color::srgba(1.0, 0.85, 0.20, BTN_ALPHA);

    // Bottom-left cluster: turn-left, turn-right, thrust.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(BTN_MARGIN),
            left: Val::Px(BTN_MARGIN),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(10.0),
            ..default()
        })
        .with_children(|row| {
            for action in [TouchAction::Left, TouchAction::Right, TouchAction::Thrust] {
                spawn_btn(row, action, dpad_color);
            }
        });

    // Bottom-right cluster: fire, special. Offset further left so it
    // clears the HUD column (220 px wide on the right edge).
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(BTN_MARGIN),
            right: Val::Px(230.0),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(10.0),
            ..default()
        })
        .with_children(|row| {
            spawn_btn(row, TouchAction::Fire, fire_color);
            spawn_btn(row, TouchAction::Special, spec_color);
        });

    // Top-center ULTIMATE button — bigger, brighter, harder to miss.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(BTN_MARGIN),
            left: Val::Percent(50.0),
            margin: UiRect {
                left: Val::Px(-60.0),
                ..default()
            },
            ..default()
        })
        .with_children(|root| {
            spawn_btn_sized(root, TouchAction::Ultimate, ult_color, 120.0, 58.0);
        });
}

fn spawn_btn(parent: &mut ChildSpawnerCommands, action: TouchAction, color: Color) {
    spawn_btn_sized(parent, action, color, BTN_SIZE, BTN_SIZE);
}

fn spawn_btn_sized(
    parent: &mut ChildSpawnerCommands,
    action: TouchAction,
    color: Color,
    w: f32,
    h: f32,
) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Px(w),
                height: Val::Px(h),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(14.0)),
                ..default()
            },
            BackgroundColor(color),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.55)),
            action,
            LastInteraction::default(),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(action.label()),
                TextFont::from_font_size(22.0),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
            ));
        });
}

/// Each frame, walk the touch buttons; OR pressed-ness into
/// `VirtualInput.held` and emit just-pressed / just-released edges
/// based on the per-button `LastInteraction` cache.
fn drive_virtual_input(
    mut virt: ResMut<VirtualInput>,
    mut buttons: Query<(&Interaction, &TouchAction, &mut LastInteraction)>,
) {
    let mut held = PlayerInput::default();
    let mut pressed = PlayerInput::default();
    let mut released = PlayerInput::default();
    let mut ultimate_edge = false;

    for (interaction, action, mut last) in &mut buttons {
        let is_active = matches!(interaction, Interaction::Pressed);
        let was_active = matches!(last.0, Interaction::Pressed);
        if let Some(mask) = action.mask() {
            if is_active {
                held.buttons |= mask;
            }
            if is_active && !was_active {
                pressed.buttons |= mask;
            }
            if !is_active && was_active {
                released.buttons |= mask;
            }
        }
        if matches!(action, TouchAction::Ultimate) && is_active && !was_active {
            ultimate_edge = true;
        }
        last.0 = *interaction;
    }

    virt.held = held;
    virt.just_pressed = pressed;
    virt.just_released = released;
    virt.ultimate_just_pressed = ultimate_edge;
}
