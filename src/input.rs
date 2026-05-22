//! Player input encoded as a fixed-size POD struct so GGRS can ship it
//! across the wire and roll it back deterministically.

use bevy::prelude::*;
use bytemuck::{Pod, Zeroable};

pub const INPUT_LEFT: u8 = 1 << 0;
pub const INPUT_RIGHT: u8 = 1 << 1;
pub const INPUT_THRUST: u8 = 1 << 2;
pub const INPUT_FIRE: u8 = 1 << 3;
pub const INPUT_SPECIAL: u8 = 1 << 4;

#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Pod, Zeroable, Default, Debug)]
pub struct PlayerInput {
    pub buttons: u8,
}

impl PlayerInput {
    pub fn pressed(&self, mask: u8) -> bool {
        self.buttons & mask != 0
    }
}

/// Synthetic input state populated by the touch-controls module
/// (`mobile_controls.rs`). Bits set here are OR'd into player 1's
/// keyboard input each tick so the touch buttons "are" the keyboard.
/// Also drives the ultimate-cinematic trigger on touch devices where
/// there's no Space bar to press.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct VirtualInput {
    pub held: PlayerInput,
    pub just_pressed: PlayerInput,
    pub just_released: PlayerInput,
    /// Edge-triggered: true the tick the on-screen ULT button is hit.
    pub ultimate_just_pressed: bool,
    /// Edge-triggered: true the tick the on-screen "next class" /
    /// "prev class" cycle button is hit. Drives P1's roster cycle
    /// without a keyboard Tab. `class_picker_input` consumes both
    /// edges each frame.
    pub cycle_next_just_pressed: bool,
    pub cycle_prev_just_pressed: bool,
}

fn keymap(slot: usize) -> &'static [(KeyCode, u8)] {
    match slot {
        0 => &[
            (KeyCode::ArrowLeft, INPUT_LEFT),
            (KeyCode::ArrowRight, INPUT_RIGHT),
            (KeyCode::ArrowUp, INPUT_THRUST),
            (KeyCode::KeyZ, INPUT_FIRE),
            (KeyCode::KeyX, INPUT_SPECIAL),
        ],
        _ => &[
            (KeyCode::KeyA, INPUT_LEFT),
            (KeyCode::KeyD, INPUT_RIGHT),
            (KeyCode::KeyW, INPUT_THRUST),
            (KeyCode::KeyG, INPUT_FIRE),
            (KeyCode::KeyH, INPUT_SPECIAL),
        ],
    }
}

/// Read local keyboard state into a `PlayerInput`. Player 1 uses arrows + Z/X;
/// player 2 (couch co-op) uses WASD + G/H. This returns the *held* state — a
/// bit is set as long as the key is down.
///
/// On touch devices the virtual-controls overlay (see
/// `mobile_controls.rs`) writes into a `VirtualInput` resource; we OR
/// those bits into player 1's input each tick. Player 2 is unaffected
/// because there's only one set of touch buttons on screen.
pub fn read_local_input(keys: &ButtonInput<KeyCode>, slot: usize) -> PlayerInput {
    let mut buttons = 0u8;
    for (key, mask) in keymap(slot) {
        if keys.pressed(*key) {
            buttons |= mask;
        }
    }
    PlayerInput { buttons }
}

/// Same as `read_local_input` but also OR's in the virtual touch
/// controls for player 1. Pass an `Option` so callers in test paths
/// can pass `None` and keep deterministic behaviour.
pub fn read_local_input_with_virtual(
    keys: &ButtonInput<KeyCode>,
    virt: Option<&VirtualInput>,
    slot: usize,
) -> PlayerInput {
    let mut input = read_local_input(keys, slot);
    if slot == 0 {
        if let Some(v) = virt {
            input.buttons |= v.held.buttons;
        }
    }
    input
}

/// Like `read_local_input` but only sets a bit on the *rising edge* —
/// the tick the key transitioned from up to down. Used by abilities
/// that should fire once per press regardless of how long the player
/// holds the button (e.g. `ToggleMode`), so a 100ms key press doesn't
/// flip the mode 3 times in the cooldown window.
pub fn read_local_just_pressed(keys: &ButtonInput<KeyCode>, slot: usize) -> PlayerInput {
    let mut buttons = 0u8;
    for (key, mask) in keymap(slot) {
        if keys.just_pressed(*key) {
            buttons |= mask;
        }
    }
    PlayerInput { buttons }
}

pub fn read_local_just_pressed_with_virtual(
    keys: &ButtonInput<KeyCode>,
    virt: Option<&VirtualInput>,
    slot: usize,
) -> PlayerInput {
    let mut input = read_local_just_pressed(keys, slot);
    if slot == 0 {
        if let Some(v) = virt {
            input.buttons |= v.just_pressed.buttons;
        }
    }
    input
}

/// Like `read_local_just_pressed` but for the *falling edge* — the
/// tick the key transitioned from down to up. Used by Inertial-mode
/// steering to detect "player just released their turn key" so the
/// player-induced spin can be cancelled without affecting any
/// collision-induced spin that arrived while the key was held.
pub fn read_local_just_released(keys: &ButtonInput<KeyCode>, slot: usize) -> PlayerInput {
    let mut buttons = 0u8;
    for (key, mask) in keymap(slot) {
        if keys.just_released(*key) {
            buttons |= mask;
        }
    }
    PlayerInput { buttons }
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VirtualInput>();
    }
}
