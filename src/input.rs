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

/// Read local keyboard state into a `PlayerInput`. Player 1 uses arrows + Z/X;
/// player 2 (couch co-op) uses WASD + G/H.
pub fn read_local_input(keys: &ButtonInput<KeyCode>, slot: usize) -> PlayerInput {
    let mut buttons = 0u8;
    let map: &[(KeyCode, u8)] = match slot {
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
    };
    for (key, mask) in map {
        if keys.pressed(*key) {
            buttons |= mask;
        }
    }
    PlayerInput { buttons }
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, _app: &mut App) {}
}
