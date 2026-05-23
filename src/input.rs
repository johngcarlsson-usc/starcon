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
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Pod,
    Zeroable,
    Default,
    Debug,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct PlayerInput {
    pub buttons: u8,
}

impl PlayerInput {
    pub fn pressed(&self, mask: u8) -> bool {
        self.buttons & mask != 0
    }
}

/// Per-slot input state for the current tick. Populated once
/// per `FixedUpdate` by `gather_slot_inputs`; consumed by every
/// gameplay system that previously called
/// `read_local_input_with_virtual` directly.
///
/// Source per slot:
///   - **Local hotseat** (no `Session`): each slot reads its
///     keymap (slot 0 = arrows, slot 1 = WASD, …).
///   - **Online** (`Session` present):
///     - Local handle's slot: read the slot-0 kbd keymap
///       (online players always use arrow keys regardless of
///       which slot they own).
///     - Other slots: read from `NetInputs` for that slot's
///       handle (latest received from GGRS).
///
/// `just_pressed` / `just_released` are edge-triggered: a bit
/// is set only on the tick a button transitioned. For local
/// inputs we use `keys.just_pressed` directly; for network
/// inputs we compute the edge as `current & !previous` since
/// remote peers only ship the current held state.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct SlotInputs {
    pub held: [PlayerInput; 4],
    pub just_pressed: [PlayerInput; 4],
    pub just_released: [PlayerInput; 4],
}

impl SlotInputs {
    /// Convenience for the very common "is the player holding
    /// this button on slot N?" question.
    pub fn pressed(&self, slot: usize, mask: u8) -> bool {
        slot < 4 && self.held[slot].pressed(mask)
    }
    pub fn just_pressed(&self, slot: usize, mask: u8) -> bool {
        slot < 4 && self.just_pressed[slot].pressed(mask)
    }
}

/// Latest network-received inputs per slot (handle), kept
/// across frames so edge detection works. Written by the
/// `net_inputs_bridge` system inside `GgrsSchedule`; read by
/// `gather_slot_inputs` next `FixedUpdate`.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct NetInputs {
    pub current: [PlayerInput; 4],
    pub previous: [PlayerInput; 4],
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
    // Local hotseat keymaps. Online play uses network inputs for
    // slots that aren't the local player — these tables only
    // matter when more than one human is at the same keyboard.
    match slot {
        0 => &[
            (KeyCode::ArrowLeft, INPUT_LEFT),
            (KeyCode::ArrowRight, INPUT_RIGHT),
            (KeyCode::ArrowUp, INPUT_THRUST),
            (KeyCode::KeyZ, INPUT_FIRE),
            (KeyCode::KeyX, INPUT_SPECIAL),
        ],
        1 => &[
            (KeyCode::KeyA, INPUT_LEFT),
            (KeyCode::KeyD, INPUT_RIGHT),
            (KeyCode::KeyW, INPUT_THRUST),
            (KeyCode::KeyG, INPUT_FIRE),
            (KeyCode::KeyH, INPUT_SPECIAL),
        ],
        2 => &[
            (KeyCode::KeyJ, INPUT_LEFT),
            (KeyCode::KeyL, INPUT_RIGHT),
            (KeyCode::KeyI, INPUT_THRUST),
            (KeyCode::KeyN, INPUT_FIRE),
            (KeyCode::KeyM, INPUT_SPECIAL),
        ],
        _ => &[
            (KeyCode::Numpad4, INPUT_LEFT),
            (KeyCode::Numpad6, INPUT_RIGHT),
            (KeyCode::Numpad8, INPUT_THRUST),
            (KeyCode::Numpad0, INPUT_FIRE),
            (KeyCode::NumpadEnter, INPUT_SPECIAL),
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
        app.init_resource::<VirtualInput>()
            .init_resource::<SlotInputs>()
            .init_resource::<NetInputs>()
            // Refresh per-slot inputs once per FixedUpdate
            // BEFORE any gameplay system runs. We put it in
            // FixedUpdate (not Update) so the input snapshot
            // aligns with the physics tick that consumes it.
            .add_systems(
                FixedUpdate,
                gather_slot_inputs.in_set(SlotInputProducerSet),
            );
    }
}

/// SystemSet marker for the producer (`gather_slot_inputs`).
/// Gameplay systems that read `SlotInputs` add
/// `.after(SlotInputProducerSet)` so they always see fresh
/// inputs.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotInputProducerSet;

/// Per-FixedUpdate: build `SlotInputs` for the four match slots.
///
///   - If a `Session<Config>` is present: this is an online
///     match. The local-handle's slot reads kbd (slot-0 keymap
///     by convention, since every online peer uses arrow keys
///     regardless of which slot they own). Other slots pull
///     from `NetInputs` (latest GGRS-received inputs).
///   - Otherwise: local hotseat — each slot reads its own
///     keymap directly.
///
/// `just_pressed` / `just_released` for network slots are
/// computed by diffing `NetInputs.current` against
/// `NetInputs.previous`. For local slots we read the kbd's
/// edge directly.
pub fn gather_slot_inputs(
    keys: Res<ButtonInput<KeyCode>>,
    virt: Res<VirtualInput>,
    net: Res<NetInputs>,
    local_players: Option<Res<bevy_ggrs::LocalPlayers>>,
    session: Option<Res<bevy_ggrs::Session<crate::netplay::Config>>>,
    config: Res<crate::ship::MatchConfig>,
    mut slot_inputs: ResMut<SlotInputs>,
) {
    let online = session.is_some();
    let local_handle = local_players
        .as_ref()
        .and_then(|lp| lp.0.first().copied());

    let n = config.slot_count().min(4);
    for slot in 0..n {
        let (held, pressed_edge, released_edge) = if online {
            // Online: local handle reads kbd (slot 0 keymap);
            // every other slot reads from NetInputs.
            if Some(slot) == local_handle {
                let held = read_local_input_with_virtual(&keys, Some(&virt), 0);
                let pressed = read_local_just_pressed_with_virtual(&keys, Some(&virt), 0);
                let released = read_local_just_released_with_virtual(&keys, Some(&virt), 0);
                (held, pressed, released)
            } else {
                let cur = net.current[slot];
                let prev = net.previous[slot];
                let edge_press = cur.buttons & !prev.buttons;
                let edge_release = !cur.buttons & prev.buttons;
                (
                    cur,
                    PlayerInput { buttons: edge_press },
                    PlayerInput { buttons: edge_release },
                )
            }
        } else {
            // Local hotseat: each slot uses its own keymap.
            let held = read_local_input_with_virtual(&keys, Some(&virt), slot);
            let pressed = read_local_just_pressed_with_virtual(&keys, Some(&virt), slot);
            let released = read_local_just_released_with_virtual(&keys, Some(&virt), slot);
            (held, pressed, released)
        };
        slot_inputs.held[slot] = held;
        slot_inputs.just_pressed[slot] = pressed_edge;
        slot_inputs.just_released[slot] = released_edge;
    }
    // Clear any slots beyond the current match's count so a
    // leftover bit from a previous (longer) match doesn't
    // leak into the next one.
    for slot in n..4 {
        slot_inputs.held[slot] = PlayerInput::default();
        slot_inputs.just_pressed[slot] = PlayerInput::default();
        slot_inputs.just_released[slot] = PlayerInput::default();
    }
}

/// Edge-triggered just-released helper, mirror of
/// `read_local_just_pressed_with_virtual`. Lets
/// `gather_slot_inputs` compute the falling edge per slot.
pub fn read_local_just_released_with_virtual(
    keys: &ButtonInput<KeyCode>,
    virt: Option<&VirtualInput>,
    slot: usize,
) -> PlayerInput {
    let mut input = read_local_just_released(keys, slot);
    if slot == 0 {
        if let Some(v) = virt {
            input.buttons |= v.just_released.buttons;
        }
    }
    input
}
