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
    INPUT_THRUST, INPUT_ULTIMATE,
};

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchAction {
    Left,
    Right,
    Thrust,
    Fire,
    Special,
    Ultimate,
    /// Cycle P1 to the previous / next ship class. Triggers a
    /// rematch reset (handled by `class_picker_input` in ship.rs).
    CyclePrev,
    CycleNext,
    /// Toggle visibility of every other touch button. Marker
    /// remains visible; used to show/hide the mobile-only buttons
    /// (default is hidden so the play area isn't covered).
    ToggleButtons,
}

impl TouchAction {
    fn mask(self) -> Option<u8> {
        match self {
            TouchAction::Left => Some(INPUT_LEFT),
            TouchAction::Right => Some(INPUT_RIGHT),
            TouchAction::Thrust => Some(INPUT_THRUST),
            TouchAction::Fire => Some(INPUT_FIRE),
            TouchAction::Special => Some(INPUT_SPECIAL),
            TouchAction::Ultimate => Some(INPUT_ULTIMATE),
            TouchAction::CyclePrev
            | TouchAction::CycleNext
            | TouchAction::ToggleButtons => None,
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
            TouchAction::CyclePrev => "<<",
            TouchAction::CycleNext => ">>",
            // Tiny knob — the user taps this to reveal/hide the
            // rest of the touch UI. Shorter than a word; reads as
            // a controller-y icon on small phone screens.
            TouchAction::ToggleButtons => "+",
        }
    }
}

/// Tracks the previous-frame `Interaction` per button so we can emit
/// just-pressed / just-released edges into `VirtualInput`.
#[derive(Component, Debug, Default)]
pub struct LastInteraction(pub Interaction);

/// Set to `false` by the on-screen toggle button to hide every
/// touch-input button (for desktop play where they're just clutter).
#[derive(Resource, Debug)]
pub struct TouchButtonsVisible(pub bool);

impl Default for TouchButtonsVisible {
    fn default() -> Self {
        // Default: HIDDEN. The play area shouldn't be covered by
        // big translucent buttons until the player asks for them.
        // Tap the small `+` knob in the top-left to reveal.
        Self(false)
    }
}

/// Marker on the cluster nodes that hold the input buttons. The
/// toggle button itself is NOT marked so it stays visible.
#[derive(Component)]
pub struct TouchButtonCluster;

pub struct MobileControlsPlugin;

impl Plugin for MobileControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TouchButtonsVisible>()
            .add_systems(Startup, spawn_touch_controls)
            .add_systems(Update, (drive_virtual_input, apply_visibility));
    }
}

// Compact "Nintendo gamepad" sizing: a small directional cross on
// the left, a face-button diamond on the right. Kept deliberately
// little so the buttons don't swallow the play area on a phone.
const DPAD_BTN: f32 = 46.0;
const FACE_BTN: f32 = 50.0;
const ULT_BTN: f32 = 42.0;
const CHIP_BTN: f32 = 34.0;
const BTN_MARGIN: f32 = 16.0;
const PAD_GAP: f32 = 10.0;
const BTN_ALPHA: f32 = 0.32;

fn spawn_touch_controls(mut commands: Commands) {
    let dpad_color = Color::srgba(0.20, 0.55, 0.95, BTN_ALPHA);
    let fire_color = Color::srgba(0.95, 0.45, 0.25, BTN_ALPHA);
    let spec_color = Color::srgba(0.65, 0.30, 0.95, BTN_ALPHA);
    let ult_color = Color::srgba(1.0, 0.85, 0.20, BTN_ALPHA);
    let cycle_color = Color::srgba(0.30, 0.85, 0.50, BTN_ALPHA);
    // Toggle knob: subtler and translucent — it sits on top of
    // the play area whether the rest of the controls are visible
    // or not, so make it as unobtrusive as possible.
    let toggle_color = Color::srgba(0.4, 0.4, 0.4, 0.22);

    // ---- Left: directional cross (D-pad) ----
    // Thrust on top, turn-left / turn-right below it, arranged as a
    // little cross so the thumb rocks between them like a real pad.
    let dpad_w = DPAD_BTN * 2.0 + PAD_GAP;
    let dpad_h = DPAD_BTN * 2.0 + PAD_GAP;
    commands
        .spawn((
            TouchButtonCluster,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(BTN_MARGIN),
                left: Val::Px(BTN_MARGIN),
                width: Val::Px(dpad_w),
                height: Val::Px(dpad_h),
                ..default()
            },
        ))
        .with_children(|pad| {
            // ▲ thrust — top-center
            spawn_gamepad_btn(
                pad,
                TouchAction::Thrust,
                dpad_color,
                DPAD_BTN,
                10.0,
                Val::Px(0.0),
                Val::Px((dpad_w - DPAD_BTN) * 0.5),
                Val::Auto,
                Val::Auto,
                20.0,
            );
            // ◀ turn-left — bottom-left
            spawn_gamepad_btn(
                pad,
                TouchAction::Left,
                dpad_color,
                DPAD_BTN,
                10.0,
                Val::Auto,
                Val::Px(0.0),
                Val::Auto,
                Val::Px(0.0),
                20.0,
            );
            // ▶ turn-right — bottom-right
            spawn_gamepad_btn(
                pad,
                TouchAction::Right,
                dpad_color,
                DPAD_BTN,
                10.0,
                Val::Auto,
                Val::Auto,
                Val::Px(0.0),
                Val::Px(0.0),
                20.0,
            );
        });

    // ---- Right: face-button diamond ----
    // FIRE (A) and SPEC (B) as the two round main buttons, ULT as a
    // smaller accent above them. Offset left of the HUD column
    // (~220 px on the right edge) so it isn't hidden behind it.
    let face_w = FACE_BTN * 2.0 + PAD_GAP;
    let face_h = FACE_BTN + ULT_BTN + PAD_GAP;
    commands
        .spawn((
            TouchButtonCluster,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(BTN_MARGIN),
                right: Val::Px(200.0),
                width: Val::Px(face_w),
                height: Val::Px(face_h),
                ..default()
            },
        ))
        .with_children(|face| {
            // ULT — accent, top-center (round)
            spawn_gamepad_btn(
                face,
                TouchAction::Ultimate,
                ult_color,
                ULT_BTN,
                ULT_BTN * 0.5,
                Val::Px(0.0),
                Val::Px((face_w - ULT_BTN) * 0.5),
                Val::Auto,
                Val::Auto,
                13.0,
            );
            // SPEC (B) — bottom-left (round)
            spawn_gamepad_btn(
                face,
                TouchAction::Special,
                spec_color,
                FACE_BTN,
                FACE_BTN * 0.5,
                Val::Auto,
                Val::Px(0.0),
                Val::Auto,
                Val::Px(0.0),
                15.0,
            );
            // FIRE (A) — bottom-right (round)
            spawn_gamepad_btn(
                face,
                TouchAction::Fire,
                fire_color,
                FACE_BTN,
                FACE_BTN * 0.5,
                Val::Auto,
                Val::Auto,
                Val::Px(0.0),
                Val::Px(0.0),
                15.0,
            );
        });

    // ---- Top-center: class-cycle chips (Tab / Shift+Tab) ----
    let chip_w = CHIP_BTN * 2.0 + PAD_GAP;
    commands
        .spawn((
            TouchButtonCluster,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(BTN_MARGIN),
                left: Val::Percent(50.0),
                margin: UiRect {
                    left: Val::Px(-chip_w * 0.5),
                    ..default()
                },
                width: Val::Px(chip_w),
                height: Val::Px(CHIP_BTN),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_gamepad_btn(
                row,
                TouchAction::CyclePrev,
                cycle_color,
                CHIP_BTN,
                8.0,
                Val::Px(0.0),
                Val::Px(0.0),
                Val::Auto,
                Val::Auto,
                15.0,
            );
            spawn_gamepad_btn(
                row,
                TouchAction::CycleNext,
                cycle_color,
                CHIP_BTN,
                8.0,
                Val::Px(0.0),
                Val::Auto,
                Val::Px(0.0),
                Val::Auto,
                15.0,
            );
        });

    // Toggle knob — tiny "+" button in the top-left that's ALWAYS
    // visible (not in any cluster). Tap to reveal/hide the rest.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(BTN_MARGIN),
            left: Val::Px(BTN_MARGIN),
            width: Val::Px(30.0),
            height: Val::Px(30.0),
            ..default()
        })
        .with_children(|root| {
            spawn_gamepad_btn(
                root,
                TouchAction::ToggleButtons,
                toggle_color,
                30.0,
                8.0,
                Val::Px(0.0),
                Val::Px(0.0),
                Val::Auto,
                Val::Auto,
                18.0,
            );
        });
}

/// Spawn a single absolutely-positioned gamepad button inside its
/// cluster. `radius` rounds the corners (set to half the size for a
/// circular face button); `top/left/right/bottom` are insets from the
/// cluster box (`Val::Auto` to leave an edge unpinned).
#[allow(clippy::too_many_arguments)]
fn spawn_gamepad_btn(
    parent: &mut ChildSpawnerCommands,
    action: TouchAction,
    color: Color,
    size: f32,
    radius: f32,
    top: Val,
    left: Val,
    right: Val,
    bottom: Val,
    font: f32,
) {
    parent
        .spawn((
            Button,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(size),
                height: Val::Px(size),
                top,
                left,
                right,
                bottom,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(radius)),
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
                TextFont::from_font_size(font),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
            ));
        });
}

/// Each frame, walk the touch buttons: OR pressed-ness into
/// `VirtualInput.held`, emit just-pressed / just-released edges
/// based on the per-button `LastInteraction` cache, AND flip
/// `TouchButtonsVisible` on a ToggleButtons press-edge.
///
/// The toggle handling lives in this same system because we're
/// the single writer of `LastInteraction`; running a separate
/// system after this one would never see a press edge (last.0
/// is already updated to the current frame's value).
fn drive_virtual_input(
    mut virt: ResMut<VirtualInput>,
    mut visible: ResMut<TouchButtonsVisible>,
    mut buttons: Query<(&Interaction, &TouchAction, &mut LastInteraction)>,
) {
    let mut held = PlayerInput::default();
    let mut pressed = PlayerInput::default();
    let mut released = PlayerInput::default();
    let mut ultimate_edge = false;
    let mut cycle_next_edge = false;
    let mut cycle_prev_edge = false;

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
        let edge_press = is_active && !was_active;
        match action {
            TouchAction::Ultimate if edge_press => ultimate_edge = true,
            TouchAction::CycleNext if edge_press => cycle_next_edge = true,
            TouchAction::CyclePrev if edge_press => cycle_prev_edge = true,
            TouchAction::ToggleButtons if edge_press => {
                visible.0 = !visible.0;
                info!("touch buttons: {}", if visible.0 { "shown" } else { "hidden" });
            }
            _ => {}
        }
        last.0 = *interaction;
    }

    virt.held = held;
    virt.just_pressed = pressed;
    virt.just_released = released;
    virt.ultimate_just_pressed = ultimate_edge;
    virt.cycle_next_just_pressed = cycle_next_edge;
    virt.cycle_prev_just_pressed = cycle_prev_edge;
}

/// Sync each cluster's Visibility with `TouchButtonsVisible`. The
/// hide/show toggle button itself is NOT in any cluster, so it
/// stays visible regardless.
fn apply_visibility(
    visible: Res<TouchButtonsVisible>,
    mut clusters: Query<&mut Visibility, With<TouchButtonCluster>>,
) {
    if !visible.is_changed() {
        return;
    }
    let target = if visible.0 {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut v in &mut clusters {
        *v = target;
    }
}
