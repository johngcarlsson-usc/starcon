//! Player input encoded as a fixed-size POD struct so GGRS can ship it
//! across the wire and roll it back deterministically.

use bevy::prelude::*;
use bytemuck::{Pod, Zeroable};

pub const INPUT_LEFT: u8 = 1 << 0;
pub const INPUT_RIGHT: u8 = 1 << 1;
pub const INPUT_THRUST: u8 = 1 << 2;
pub const INPUT_FIRE: u8 = 1 << 3;
pub const INPUT_SPECIAL: u8 = 1 << 4;
/// Ultimate cinematic trigger. On the keyboard this bit is NOT mapped
/// to a single key — it's *derived* in `gather_slot_inputs` when a
/// slot holds all five movement+weapon inputs at once (the
/// `INPUT_ULTIMATE_CHORD`). The on-screen gamepad keeps a dedicated
/// ULT button (`VirtualInput`) since mashing five touch buttons at
/// once is impractical. Either way the bit rides through `PlayerInput`
/// so it travels over the network and rolls back deterministically.
pub const INPUT_ULTIMATE: u8 = 1 << 5;
/// Set when this input is in "absolute aim" mode: the `aim` axis (not
/// the digital turn bits) drives steering — the ship turns to face the
/// stick's world direction and thrusts that way. See
/// `apply_player_input`.
pub const INPUT_ABSOLUTE: u8 = 1 << 6;
/// "Backward" — there's no reverse thrust in this game, so this key is
/// otherwise inert; it exists purely as the safe third key of the
/// ultimate chord (down-arrow for P1, S for P2, …).
pub const INPUT_BACKWARD: u8 = 1 << 7;

// ---- `PlayerInput.flags` bits (post-match / global state votes) ----

/// Player is asking for a rematch from the PostMatch summary screen.
/// Lives on the separate `flags` byte (not `buttons`) so it survives
/// the network round-trip independently of the gameplay-input space —
/// `request_rematch` consumes this on EITHER peer's slot to trigger
/// `AppState::Resetting` for both sides simultaneously.
pub const FLAG_REMATCH: u8 = 1 << 0;
/// Player is signalling "ready to start the next match" from the
/// netplay post-match ship-select lobby. While held, the lobby system
/// counts this slot as a yes-vote; the new match begins only when
/// EVERY active human slot has `FLAG_READY` set this tick. Local
/// toggle is on KeyR (same key as the legacy rematch — netplay shows
/// "Ready" semantics, hotseat keeps the instant rematch).
pub const FLAG_READY: u8 = 1 << 1;

/// Holding turn-left + turn-right + backward together fires the
/// ultimate. Deliberately NOT fire/special — pressing those would
/// trigger the ship's actual weapons (e.g. Arilou would warp away)
/// before the cinematic. The on-screen gamepad uses its own ULT button.
pub const INPUT_ULTIMATE_CHORD: u8 = INPUT_LEFT | INPUT_RIGHT | INPUT_BACKWARD;

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
    /// Analog turn axis, quantised to `[-100, 100]` (= −1.0..=1.0 via
    /// `turn_f32`). Sign matches the digital `dir` convention in
    /// `apply_player_input`: +1 = full turn-left, −1 = full turn-right.
    /// Keyboard players are bang-bang (±100 / 0); the touch analog
    /// stick fills in the in-between values for fine, slow turns. Rides
    /// inside `PlayerInput` so it crosses the network and rolls back
    /// like every other input bit.
    pub turn: i8,
    /// Absolute-aim stick vector, each axis quantised to `[-100, 100]`
    /// (= −1.0..=1.0 via `aim_vec`), world frame (+y = up/north). Only
    /// meaningful when `INPUT_ABSOLUTE` is set. The ship turns to face
    /// this direction and thrusts along it.
    pub aim_x: i8,
    pub aim_y: i8,
    /// Global-state votes that need to ride through the GGRS input
    /// channel but aren't gameplay buttons. See `FLAG_*` constants
    /// above (rematch, ready). Keeping these separate from `buttons`
    /// means edge-detection logic and the per-slot gameplay path
    /// don't see them.
    pub flags: u8,
    /// Lobby class vote — index into `ALL_CLASSES`. This is what
    /// the local peer wants to fly NEXT match; the lobby system
    /// reads `held[slot].class` for every slot when starting a new
    /// match. Outside the post-match lobby this just echoes the
    /// peer's currently-active class so the value is meaningful at
    /// any tick. Six 1-byte fields make `PlayerInput` `[u8; 6]` —
    /// still Pod-safe with no implicit padding under `repr(C)`.
    pub class: u8,
}

impl PlayerInput {
    pub fn pressed(&self, mask: u8) -> bool {
        self.buttons & mask != 0
    }
    /// Test a `FLAG_*` bit on the global-state-vote byte. Separate
    /// from `pressed` so callers can't accidentally test a flag
    /// against the gameplay-button space (or vice-versa).
    pub fn flag(&self, mask: u8) -> bool {
        self.flags & mask != 0
    }
    /// Analog turn as a float in `[-1.0, 1.0]`.
    pub fn turn_f32(&self) -> f32 {
        self.turn as f32 / 100.0
    }
    /// Absolute-aim stick vector as floats in `[-1.0, 1.0]` per axis.
    pub fn aim_vec(&self) -> Vec2 {
        Vec2::new(self.aim_x as f32 / 100.0, self.aim_y as f32 / 100.0)
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
    /// True iff ANY slot has the given `FLAG_*` bit set on its
    /// just-pressed edge this tick. Used for global-state votes
    /// (rematch) that don't care which peer pressed.
    pub fn any_flag_just_pressed(&self, mask: u8) -> bool {
        self.just_pressed.iter().any(|i| i.flag(mask))
    }
    /// True iff the given slot is HOLDING the `FLAG_*` bit on its
    /// current input. Used by the netplay lobby to detect "this peer
    /// is ready" — Ready is a level-triggered vote, not an edge.
    pub fn flag(&self, slot: usize, mask: u8) -> bool {
        slot < 4 && self.held[slot].flag(mask)
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
    /// Analog turn from the on-screen stick, in `[-1.0, 1.0]` (+1 =
    /// full left, −1 = full right). OR'd into player 1's input as the
    /// analog turn axis. `0.0` when the stick is centred or absent.
    /// In tilt+absolute mode this carries the TILT rotation instead.
    pub turn: f32,
    /// Absolute-aim mode active (tilt+stick scheme). When true the
    /// `aim` vector drives steering for player 1 instead of `turn`.
    pub absolute: bool,
    /// Stick vector for absolute-aim mode, world frame (+y up).
    pub aim: Vec2,
}

fn keymap(slot: usize) -> &'static [(KeyCode, u8)] {
    // Local hotseat keymaps. Online play uses network inputs for
    // slots that aren't the local player — these tables only
    // matter when more than one human is at the same keyboard.
    match slot {
        // P1 lives entirely on the RIGHT of the keyboard: arrow
        // cluster to steer, and the `/` (fire) / `.` (special) keys
        // just left of the arrows. Down-arrow is the inert "backward"
        // key used only for the ultimate chord.
        0 => &[
            (KeyCode::ArrowLeft, INPUT_LEFT),
            (KeyCode::ArrowRight, INPUT_RIGHT),
            (KeyCode::ArrowUp, INPUT_THRUST),
            (KeyCode::ArrowDown, INPUT_BACKWARD),
            (KeyCode::Slash, INPUT_FIRE),
            (KeyCode::Period, INPUT_SPECIAL),
        ],
        // P2 lives entirely on the LEFT: WAD to steer, Z + L-Shift
        // (both bottom-left) to fire, S as the inert "backward" key for
        // the ultimate chord. We avoid Left-Ctrl for fire: with W as
        // thrust, Ctrl+W would close the browser tab.
        1 => &[
            (KeyCode::KeyA, INPUT_LEFT),
            (KeyCode::KeyD, INPUT_RIGHT),
            (KeyCode::KeyW, INPUT_THRUST),
            (KeyCode::KeyS, INPUT_BACKWARD),
            (KeyCode::KeyZ, INPUT_FIRE),
            (KeyCode::ShiftLeft, INPUT_SPECIAL),
        ],
        2 => &[
            (KeyCode::KeyJ, INPUT_LEFT),
            (KeyCode::KeyL, INPUT_RIGHT),
            (KeyCode::KeyI, INPUT_THRUST),
            (KeyCode::KeyK, INPUT_BACKWARD),
            (KeyCode::KeyN, INPUT_FIRE),
            (KeyCode::KeyM, INPUT_SPECIAL),
        ],
        _ => &[
            (KeyCode::Numpad4, INPUT_LEFT),
            (KeyCode::Numpad6, INPUT_RIGHT),
            (KeyCode::Numpad8, INPUT_THRUST),
            (KeyCode::Numpad2, INPUT_BACKWARD),
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
    // Keyboard steering is bang-bang: a held turn key is full deflection
    // (sign matches the `dir` convention — LEFT = +1, RIGHT = −1).
    let turn = if buttons & INPUT_LEFT != 0 {
        100
    } else if buttons & INPUT_RIGHT != 0 {
        -100
    } else {
        0
    };
    // Rematch vote rides on `flags` (separate from the gameplay-button
    // space) so it survives the GGRS network round-trip and triggers
    // `AppState::Resetting` on BOTH peers simultaneously. R is global
    // — pressing it on any slot's keyboard counts.
    let flags = if keys.pressed(KeyCode::KeyR) { FLAG_REMATCH } else { 0 };
    PlayerInput { buttons, turn, aim_x: 0, aim_y: 0, flags, class: 0 }
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
            if v.absolute {
                // Tilt + absolute-aim scheme: the stick vector aims the
                // ship and `turn` carries the tilt rotation (used when
                // the stick is centred). `apply_player_input` reads the
                // INPUT_ABSOLUTE branch.
                input.buttons |= INPUT_ABSOLUTE;
                input.aim_x = (v.aim.x.clamp(-1.0, 1.0) * 100.0) as i8;
                input.aim_y = (v.aim.y.clamp(-1.0, 1.0) * 100.0) as i8;
                input.turn = (v.turn.clamp(-1.0, 1.0) * 100.0) as i8;
            } else if v.turn.abs() > f32::EPSILON {
                // Normal scheme: the deflected stick OWNS the turn axis
                // (overriding the keyboard's bang-bang value) for
                // proportional, fine-grained steering. The digital
                // LEFT/RIGHT bits the stick also sets keep bit-driven
                // systems (Supox strafe, etc.) working.
                input.turn = (v.turn.clamp(-1.0, 1.0) * 100.0) as i8;
            }
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
    let flags = if keys.just_pressed(KeyCode::KeyR) { FLAG_REMATCH } else { 0 };
    PlayerInput { buttons, turn: 0, aim_x: 0, aim_y: 0, flags, class: 0 }
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
    let flags = if keys.just_released(KeyCode::KeyR) { FLAG_REMATCH } else { 0 };
    PlayerInput { buttons, turn: 0, aim_x: 0, aim_y: 0, flags, class: 0 }
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
            // Gather slot inputs at the start of every gameplay
            // tick. Lives in `GgrsSchedule` so it gets re-run on
            // every rollback frame with the (predicted) inputs
            // for that frame.
            .add_systems(
                bevy_ggrs::GgrsSchedule,
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
        // Last tick's held state for this slot, captured before we
        // overwrite it — used to find the rising edge of the
        // ultimate chord below.
        let prev_held = slot_inputs.held[slot];
        let (mut held, mut pressed_edge, released_edge) = if online {
            // Online: EVERY slot — including the local handle — reads
            // from `NetInputs`, which `net_inputs_bridge` populates
            // from `PlayerInputs<Config>`. That's the canonical
            // per-tick input GGRS uses for the rollback simulation.
            //
            // Previously the local handle re-read `ButtonInput<KeyCode>`
            // direct from Bevy each tick. On rollback re-simulation
            // the keyboard state had moved on since the original
            // frame (the player has held / released keys in the
            // meantime), so the replay produced different inputs
            // than the live simulation — instant desync.
            //
            // Reading uniformly from `NetInputs` removes the
            // discrepancy: every tick's input is whatever GGRS
            // recorded for that tick, both during live play and
            // during replay.
            let _ = (&keys, &virt, local_handle);
            let cur = net.current[slot];
            let prev = net.previous[slot];
            let edge_press = cur.buttons & !prev.buttons;
            let edge_release = !cur.buttons & prev.buttons;
            // Flag bits are global votes (rematch) — compute their
            // edge the same way so `slot_inputs.just_pressed[slot].flag(...)`
            // works for remote peers.
            let flag_press = cur.flags & !prev.flags;
            let flag_release = !cur.flags & prev.flags;
            (
                cur,
                PlayerInput {
                    buttons: edge_press,
                    turn: 0,
                    aim_x: 0,
                    aim_y: 0,
                    flags: flag_press,
                    // `class` on edge inputs is meaningless (it's
                    // a level-triggered vote, not a press/release
                    // event). Keep at 0; lobby logic reads the
                    // held `class` instead.
                    class: 0,
                },
                PlayerInput {
                    buttons: edge_release,
                    turn: 0,
                    aim_x: 0,
                    aim_y: 0,
                    flags: flag_release,
                    class: 0,
                },
            )
        } else {
            // Local hotseat: each slot uses its own keymap.
            let held = read_local_input_with_virtual(&keys, Some(&virt), slot);
            let pressed = read_local_just_pressed_with_virtual(&keys, Some(&virt), slot);
            let released = read_local_just_released_with_virtual(&keys, Some(&virt), slot);
            (held, pressed, released)
        };

        // Derive the ultimate bit from the five-button chord. We do
        // this AFTER the keyboard/network read (and after the touch
        // OR for slot 0) so a remote player's chord — whose raw bits
        // arrive over the wire — resolves to the same ultimate on
        // every peer. The touch ULT button has already set
        // INPUT_ULTIMATE directly via VirtualInput, so this only
        // adds the keyboard path.
        let chord_now =
            held.buttons & INPUT_ULTIMATE_CHORD == INPUT_ULTIMATE_CHORD;
        let chord_prev =
            prev_held.buttons & INPUT_ULTIMATE_CHORD == INPUT_ULTIMATE_CHORD;
        if chord_now {
            held.buttons |= INPUT_ULTIMATE;
            if !chord_prev {
                pressed_edge.buttons |= INPUT_ULTIMATE;
            }
        }

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
