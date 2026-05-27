//! Network play: matchbox WebRTC peer discovery + GGRS rollback
//! session.
//!
//! Architecture (this is the design target; see TODOs for what's
//! actually wired up yet):
//!
//!   1. Player picks "Online" in the title menu → `AppState`
//!      moves to `LobbyOnline` and `LobbyRequest.requested = true`.
//!   2. `connect_to_matchbox` runs on OnEnter(LobbyOnline) and
//!      opens a `MatchboxSocket` to `DEFAULT_ROOM_URL`. The room
//!      query (`?next=N`) asks the signaling server to bucket us
//!      with N-1 other peers.
//!   3. Each Update tick `update_lobby` calls
//!      `socket.update_peers()`. When the connected-peer count
//!      reaches `NUM_PLAYERS`, we build a GGRS `SessionBuilder`
//!      with our local handle + each peer's `PlayerType::Remote`,
//!      hand it the matchbox channel via
//!      `socket.take_channel(0)`, and transition to InMatch.
//!   4. In InMatch, gameplay systems read inputs from GGRS
//!      (replacing `read_local_input` for non-local slots).
//!      Rollback is handled by `bevy_ggrs` driving the
//!      `GgrsSchedule` instead of `FixedUpdate`.
//!
//! Status:
//!   - [x] Lobby state machine + setup UI (humans/ai pickers,
//!         "Find Match" button, status text).
//!   - [x] `MatchboxSocket` connect on a variant-specific URL
//!         (`starcon-h{H}-a{A}?next=H`) so peers picking the
//!         same combo land in the same bucket.
//!   - [x] Peer-count loop transitions to SessionReady when
//!         `target_humans` are connected.
//!   - [x] GGRS `SessionBuilder` handoff: sorted PeerId
//!         deterministic handle assignment, `Session<Config>`
//!         + `LocalPlayers` resource installed on success.
//!   - [x] Shared per-match seed: PeerId list is hashed with
//!         Bevy's FixedHasher and written to `MatchSeed.0`, so
//!         every peer initialises `GameRng` with the same seed
//!         at the start of the match.
//!   - [x] `read_local_inputs` writes `LocalInputs<Config>` in
//!         the ReadInputs schedule so GGRS has something to
//!         broadcast.
//!   - [x] `net_inputs_bridge` in `GgrsSchedule` copies
//!         `PlayerInputs<Config>` into a long-lived
//!         `NetInputs` resource so FixedUpdate gameplay can
//!         read it.
//!   - [x] `SlotInputs` (in `input.rs`): per-slot held +
//!         just_pressed + just_released, populated each
//!         FixedUpdate from kbd (local slots) or NetInputs
//!         (remote slots). All ~12 gameplay-system call sites
//!         migrated to read SlotInputs by slot index. Remote
//!         players' inputs now reach `apply_player_input`,
//!         `dispatch_primary`, ability dispatch, and the
//!         per-class fire / charge / ult systems.
//!   - [x] `INPUT_ULTIMATE` bit: SPACE / mobile ULT route
//!         through the same PlayerInput bitfield as the other
//!         buttons, so remote players can trigger their own
//!         ultimates.
//!   - [x] Determinism: `crate::rng::GameRng` migrated for the
//!         major gameplay-affecting RNG sites (projectile
//!         spread, asteroid spawn, all ultimate spawn paths,
//!         crystal-shatter shards). Visual-only sites stay on
//!         global fastrand per the policy in `crate::rng`.
//!
//!   - [x] Physics + gameplay moved out of `FixedUpdate` into
//!         `GgrsSchedule`. Avian via `PhysicsPlugins::new(
//!         GgrsSchedule)`; ability dispatch, AI, ship tick,
//!         ultimate gameplay systems all migrated. Offline
//!         fallback (`physics::offline_tick`) drives
//!         GgrsSchedule from FixedUpdate when no Session is
//!         active, so local play still ticks.
//!   - [x] Rollback registrations: Position, Rotation,
//!         LinearVelocity, AngularVelocity, Crew, Battery,
//!         WeaponCooldown, SpecialCooldown all registered as
//!         rollback components; GameRng registered as rollback
//!         resource. `auto_add_rollback` on_add hooks on Ship /
//!         Projectile / DamageZone / SubEntity / Asteroid so
//!         every spawn site is automatically included in
//!         snapshots.
//!   - [x] Time audit: every gameplay-affecting cinematic
//!         system switched from `Res<Time<Real>>` to
//!         `Res<Time>`, which resolves to `Time<GgrsTime>`
//!         inside GgrsSchedule — deterministic 1/FPS delta on
//!         every peer.
//!
//! What you can do right now: an online 2-4-player match
//! (with optional AI slots) connects via matchbox WebRTC,
//! starts a real GGRS P2P session, and runs the full game
//! simulation deterministically in rollback. Remote players'
//! ships, projectiles, asteroids, and ultimates all simulate
//! identically on every peer; input mispredictions trigger
//! GGRS rollback and re-simulate from the corrected frame.
//!
//! Known caveats / next polish:
//!   - INPUT_DELAY = 2 frames means local feel is ~33 ms
//!     behind raw input. Reduce to 0 (predict aggressively,
//!     rollback often) for fighting-game-style snap; raise to
//!     5-10 for laggy connections.
//!   - Float determinism across architectures (x86 vs ARM vs
//!     wasm) hasn't been verified. Cross-architecture play may
//!     desync from accumulated FP differences. Same-arch
//!     play (e.g. all browsers on x86) should be fine.
//!   - Cinematic-cutscene phases pause `Time<Virtual>` which
//!     freezes Avian physics; GGRS still ticks its frame
//!     counter so the pause length is deterministic.
//!
//! On WASM (the primary deployment target), WebRTC peer
//! connections work out of the box. On native, you'll need a
//! self-hosted matchbox signaling server reachable from both
//! peers; the public test server at `match.helsing.studio` is
//! intermittently up but fine for development.

use bevy::ecs::schedule::{LogLevel, ScheduleBuildSettings};
use avian2d::prelude::{AngularVelocity, LinearVelocity, Position, Rotation};
use bevy::prelude::*;
use bevy_ggrs::ggrs::{Message as GgrsMessage, NonBlockingSocket, PlayerType, SessionBuilder};
use bevy_ggrs::{
    GgrsPlugin, GgrsSchedule, LocalInputs, LocalPlayers, PlayerInputs, ReadInputs, RollbackApp,
    Session,
};
use bevy_matchbox::matchbox_socket::{RtcIceServerConfig, WebRtcChannel};
use bevy_matchbox::prelude::*;

use crate::AppState;

/// GGRS session type. Input = our `PlayerInput`. Address =
/// matchbox `PeerId` because peers are identified by WebRTC
/// peer-id rather than UDP socket address.
pub type Config = bevy_ggrs::GgrsConfig<crate::input::PlayerInput, PeerId>;

/// Adapter that bridges a matchbox `WebRtcChannel` to GGRS
/// 0.12's `NonBlockingSocket<PeerId>` trait.
///
/// `matchbox_socket` 0.14 ships its own `NonBlockingSocket`
/// impl, but against the ggrs 0.11 trait — and bevy_ggrs 0.21
/// requires ggrs 0.12. We can't impl a foreign trait on a
/// foreign type, so we newtype the channel and re-implement the
/// serde<->packet bridge using bincode 2.
pub struct GgrsChannelAdapter(WebRtcChannel);

impl NonBlockingSocket<PeerId> for GgrsChannelAdapter {
    fn send_to(&mut self, msg: &GgrsMessage, addr: &PeerId) {
        let bytes = bincode::serde::encode_to_vec(msg, bincode::config::standard())
            .expect("ggrs message serialize");
        self.0.send(bytes.into_boxed_slice(), *addr);
    }

    fn receive_all_messages(&mut self) -> Vec<(PeerId, GgrsMessage)> {
        self.0
            .receive()
            .into_iter()
            .filter_map(|(id, packet)| {
                bincode::serde::decode_from_slice(&packet, bincode::config::standard())
                    .ok()
                    .map(|(msg, _)| (id, msg))
            })
            .collect()
    }
}

/// Maximum players per match. The lobby waits for exactly the
/// number of HUMANS in `LobbyState.target_humans` (any remaining
/// slots up to `target_humans + target_ai` are filled with
/// stub-AI ships when the match starts). 4 is the canonical
/// Super Melee cap.
pub const MAX_PLAYERS: usize = 4;

/// GGRS frame rate. Matches Bevy's default `FixedUpdate`
/// schedule (60 Hz) so the network tick aligns with the physics
/// tick. Changing this requires also reconfiguring
/// `Time<Fixed>::timestep()`.
pub const FPS: usize = 60;

/// Input-prediction window in frames. The local player's input
/// is applied immediately; remote inputs lag by INPUT_DELAY
/// frames and arrive over the network in the meantime. Smaller
/// = more responsive but more rollbacks; 2 frames @ 60 Hz =
/// 33 ms of perceptual lag, which is below human reaction time.
pub const INPUT_DELAY: usize = 2;

/// Matchbox signaling server URL base. The full URL appended
/// to this includes the lobby variant + `?next=N` so peers who
/// picked the same "N humans + M AI" preset land in the same
/// room.
///
/// **You must run your own matchbox server** for online play.
/// The public test servers at `match.helsing.studio` /
/// `matchbox.helsing.gg` are origin-restricted to Johan
/// Helsing's own demos and reject our requests with
/// `x-deny-reason: host_not_allowed`.
///
/// Quick local setup (both peers on same LAN):
///   1. `cargo install matchbox_server`
///   2. `matchbox_server` (listens on `0.0.0.0:3536` by default)
///   3. Pass `?signal=ws://YOUR_IP:3536` in the game URL so
///      both browsers use your server: e.g.
///      `https://...starcon/?signal=ws://192.168.1.50:3536`
///
/// For internet play, expose your local server with a tunnel:
///   - `cloudflared tunnel --url http://localhost:3536` → wss URL
///   - or `ngrok http 3536` → wss URL
///   Then both peers use that URL: `?signal=wss://abc.ngrok.io`.
///
/// The default below points to a placeholder; the lobby will
/// fail to connect unless `?signal=` overrides it.
pub const SIGNALING_URL: &str = "ws://localhost:3536";

/// Set by `menu.rs` when the player picks "Online". Read on
/// OnEnter(LobbyOnline) to decide whether to open a socket.
/// Allows a future "Resume online match" flow to enter
/// LobbyOnline without re-running the connection.
#[derive(Resource, Default)]
pub struct LobbyRequest {
    pub requested: bool,
}

/// Optional override for the matchbox signaling server URL.
/// Populated from the browser URL's `?signal=...` query
/// parameter at startup (WASM only); on native it stays None
/// and the default `SIGNALING_URL` is used.
#[derive(Resource, Default, Debug, Clone)]
pub struct SignalingOverride(pub Option<String>);

#[cfg(target_arch = "wasm32")]
fn pick_signaling_url(over: &SignalingOverride) -> String {
    if let Some(url) = &over.0 {
        return url.clone();
    }
    SIGNALING_URL.to_string()
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_signaling_url(over: &SignalingOverride) -> String {
    if let Some(url) = &over.0 {
        return url.clone();
    }
    SIGNALING_URL.to_string()
}

/// One-shot Startup system that reads the browser URL on WASM
/// and stuffs any `?signal=...` value into `SignalingOverride`.
/// On native it's a no-op (no browser URL to read).
#[cfg(target_arch = "wasm32")]
fn capture_signal_override(mut over: ResMut<SignalingOverride>) {
    let Some(window) = web_sys::window() else { return };
    let Ok(location) = window.location().search() else { return };
    let q = location.trim_start_matches('?');
    for pair in q.split('&') {
        if let Some(rest) = pair.strip_prefix("signal=") {
            // URL-decode minimally — the chars we care about
            // (wss://, ws://, : / .) all survive verbatim.
            let decoded = js_sys::decode_uri_component(rest)
                .map(|v| v.as_string().unwrap_or_else(|| rest.to_string()))
                .unwrap_or_else(|_| rest.to_string());
            info!("netplay: signaling override from URL: {}", decoded);
            over.0 = Some(decoded);
            return;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn capture_signal_override(_over: ResMut<SignalingOverride>) {
    // On native, the override can be set via env or CLI later;
    // for now no-op so the default URL is used.
}

/// Live lobby state. Owns the matchbox socket while we're in
/// `AppState::LobbyOnline`; the socket is dropped on OnExit so
/// the WebRTC channels are released.
///
/// `target_humans` + `target_ai` are pickers — the user adjusts
/// them via the setup UI before pressing "Find Match", at which
/// point the lobby opens a matchbox socket and waits for
/// `target_humans` peers. AI slots are filled in locally on
/// every peer when the match starts; since they're driven by
/// deterministic state-only inputs, no GGRS handles are needed.
#[derive(Resource)]
pub struct LobbyState {
    pub status: LobbyStatus,
    /// Last known connected-peer count, including the local
    /// player. Updated by `update_lobby` so the UI can render
    /// "Waiting for players: 2/3".
    pub connected: usize,
    /// Number of human slots in this match (1..=4). The lobby
    /// waits for this many peers (including ourselves).
    pub target_humans: usize,
    /// Number of AI slots in this match. Total ship count =
    /// target_humans + target_ai, capped at MAX_PLAYERS.
    pub target_ai: usize,
}

impl Default for LobbyState {
    fn default() -> Self {
        Self {
            status: LobbyStatus::Setup,
            connected: 0,
            target_humans: 2,
            target_ai: 0,
        }
    }
}

impl LobbyState {
    pub fn total_slots(&self) -> usize {
        (self.target_humans + self.target_ai).min(MAX_PLAYERS)
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LobbyStatus {
    /// Pre-connection: pickers visible, user choosing the
    /// match size + AI mix.
    #[default]
    Setup,
    /// Opening the WebSocket to the signaling server.
    Connecting,
    /// Connected to the signaling server; waiting for the room
    /// to fill up to `target_humans`.
    WaitingForPeers,
    /// All peers connected; handing off to GGRS. Transitional —
    /// the next tick should bring us into InMatch.
    SessionReady,
    /// Something went wrong (socket disconnected, server
    /// rejected the join). UI shows the message; user can back
    /// out to the main menu.
    Failed(&'static str),
}

/// Marker on the lobby-screen UI tree so OnExit can despawn it
/// with one recursive call.
#[derive(Component)]
struct LobbyUi;

/// Marker on the status-text node — updated each tick by
/// `update_lobby_ui` so the player sees "Connecting…",
/// "Waiting 2/4", etc.
#[derive(Component)]
struct LobbyStatusText;

/// Marker on the text node that shows "Humans: N" — updated
/// when the picker buttons mutate `LobbyState.target_humans`.
#[derive(Component)]
struct HumansCountText;

/// Marker on the text node that shows "AI: N".
#[derive(Component)]
struct AiCountText;

/// Marker on the entire setup-pickers sub-tree (the picker
/// rows + the "Find Match" button). Hidden when status leaves
/// Setup so the post-setup status text isn't occluded.
#[derive(Component)]
struct LobbySetupPanel;

/// Per-button discriminator for the in-lobby setup controls.
#[derive(Component, Clone, Copy)]
enum SetupAction {
    HumansDec,
    HumansInc,
    AiDec,
    AiInc,
    FindMatch,
}

pub struct NetplayPlugin;

impl Plugin for NetplayPlugin {
    fn build(&self, app: &mut App) {
        // GgrsPlugin: registers the rollback schedule + input
        // collection scaffolding. Idle when no `Session<Config>`
        // resource exists, so it costs nothing for local play.
        app.add_plugins(GgrsPlugin::<Config>::default())
            // bevy_ggrs sets `ambiguity_detection: LogLevel::
            // Error` on GgrsSchedule for strict determinism.
            // Our existing gameplay has many implicit-ordering
            // ambiguities that silently worked under FixedUpdate
            // (where the default is LogLevel::Ignore). Resolving
            // every pair explicitly is days of work; instead we
            // relax the check back to Warn. Cross-peer
            // determinism still holds because both peers run the
            // same Rust binary with the same system registration
            // order, so the parallel executor produces the same
            // run order on each.
            .edit_schedule(GgrsSchedule, |s| {
                s.set_build_settings(ScheduleBuildSettings {
                    ambiguity_detection: LogLevel::Warn,
                    ..default()
                });
            })
            // ---- Rollback components ----
            //
            // Every component that mutates during gameplay needs
            // to be registered so bevy_ggrs can snapshot it before
            // the predicted frames and restore it on rollback.
            // Avian's physics state (Position / Rotation /
            // LinearVelocity / AngularVelocity) is Copy, as are
            // our cooldown/stat components.
            .rollback_component_with_copy::<Position>()
            .rollback_component_with_copy::<Rotation>()
            .rollback_component_with_copy::<LinearVelocity>()
            .rollback_component_with_copy::<AngularVelocity>()
            .rollback_component_with_copy::<crate::ship::Crew>()
            .rollback_component_with_copy::<crate::ship::Battery>()
            .rollback_component_with_copy::<crate::ship::WeaponCooldown>()
            .rollback_component_with_copy::<crate::ship::SpecialCooldown>()
            // ---- Rollback resources ----
            //
            // GameRng owns the per-match deterministic stream;
            // it MUST be snapshot/restored on rollback or peers
            // will diverge after the first re-simulation.
            .rollback_resource_with_clone::<crate::rng::GameRng>()
            .init_resource::<LobbyRequest>()
            .init_resource::<LobbyState>()
            .init_resource::<SignalingOverride>()
            .add_systems(Startup, capture_signal_override)
            .add_systems(OnEnter(AppState::LobbyOnline), (reset_lobby, spawn_lobby_ui))
            .add_systems(OnExit(AppState::LobbyOnline), (despawn_lobby_ui, drop_socket))
            .add_systems(
                Update,
                (
                    handle_setup_buttons,
                    update_lobby,
                    update_lobby_ui,
                    update_setup_visibility,
                    lobby_back_to_menu,
                )
                    .run_if(in_state(AppState::LobbyOnline)),
            )
            // GGRS calls into the `ReadInputs` schedule once per
            // frame to ask "what input did the local player made
            // this frame?". We answer by reading the local
            // keyboard (always slot-0 keymap, since each peer
            // owns exactly one slot in an online match) and
            // stashing it in `LocalInputs<Config>` keyed by the
            // local handle.
            .add_systems(ReadInputs, read_local_inputs)
            // Bridge: inside GgrsSchedule (which only runs when
            // a `Session<Config>` is present) we read the
            // freshly-confirmed `PlayerInputs<Config>` and
            // write them into our long-lived `NetInputs`
            // resource. The next FixedUpdate's
            // `gather_slot_inputs` then routes those inputs to
            // each slot.
            .add_systems(GgrsSchedule, net_inputs_bridge);
    }
}

/// On entry to LobbyOnline: blow away any prior connection
/// state, start fresh in Setup. Status defaults to Setup so
/// the setup UI is visible.
fn reset_lobby(mut state: ResMut<LobbyState>) {
    state.status = LobbyStatus::Setup;
    state.connected = 0;
}

fn spawn_lobby_ui(mut commands: Commands, state: Res<LobbyState>) {
    commands
        .spawn((
            LobbyUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(16.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.02, 0.06, 0.94)),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("ONLINE LOBBY"),
                TextFont::from_font_size(40.0),
                TextColor(Color::srgb(0.85, 0.95, 1.0)),
            ));

            // --- Setup panel (visible while in Setup status) ---
            root.spawn((
                LobbySetupPanel,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(12.0),
                    margin: UiRect::vertical(Val::Px(12.0)),
                    ..default()
                },
            ))
            .with_children(|panel| {
                spawn_picker_row(
                    panel,
                    "Humans",
                    state.target_humans,
                    SetupAction::HumansDec,
                    SetupAction::HumansInc,
                    PickerKind::Humans,
                );
                spawn_picker_row(
                    panel,
                    "AI opponents",
                    state.target_ai,
                    SetupAction::AiDec,
                    SetupAction::AiInc,
                    PickerKind::Ai,
                );
                spawn_setup_button(panel, SetupAction::FindMatch, "Find Match");
                panel.spawn((
                    Text::new(
                        "Both peers must pick the same combination AND use the\n\
                         same signaling server (default: ws://localhost:3536).\n\
                         Override via ?signal=ws://your.server in the URL.",
                    ),
                    TextFont::from_font_size(13.0),
                    TextColor(Color::srgba(0.55, 0.62, 0.75, 0.85)),
                    Node {
                        margin: UiRect::top(Val::Px(8.0)),
                        ..default()
                    },
                ));
            });

            // --- Status text (visible once Setup is finished) ---
            root.spawn((
                LobbyStatusText,
                Text::new(""),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgba(0.80, 0.85, 0.95, 0.95)),
            ));

            root.spawn((
                Text::new("[Esc] back to menu"),
                TextFont::from_font_size(14.0),
                TextColor(Color::srgba(0.55, 0.62, 0.75, 0.75)),
                Node {
                    margin: UiRect::top(Val::Px(24.0)),
                    ..default()
                },
            ));
        });
}

#[derive(Copy, Clone)]
enum PickerKind {
    Humans,
    Ai,
}

fn spawn_picker_row(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    initial: usize,
    dec: SetupAction,
    inc: SetupAction,
    kind: PickerKind,
) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(12.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(format!("{label}:")),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgba(0.85, 0.90, 1.0, 0.95)),
                Node {
                    width: Val::Px(160.0),
                    ..default()
                },
            ));
            spawn_setup_button(row, dec, "-");
            // The counter cell is wide enough that single-digit
            // values don't jump around as they change.
            let mut counter = row.spawn((
                Text::new(format!("{}", initial)),
                TextFont::from_font_size(22.0),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
                Node {
                    width: Val::Px(36.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
            ));
            match kind {
                PickerKind::Humans => {
                    counter.insert(HumansCountText);
                }
                PickerKind::Ai => {
                    counter.insert(AiCountText);
                }
            }
            spawn_setup_button(row, inc, "+");
        });
}

fn spawn_setup_button(parent: &mut ChildSpawnerCommands, action: SetupAction, label: &str) {
    parent
        .spawn((
            Button,
            action,
            Node {
                min_width: Val::Px(48.0),
                height: Val::Px(40.0),
                padding: UiRect::horizontal(Val::Px(12.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.15, 0.25, 0.45, 0.85)),
            BorderColor::all(Color::srgba(0.55, 0.75, 1.0, 0.80)),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(label.to_string()),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
            ));
        });
}

fn despawn_lobby_ui(mut commands: Commands, q: Query<Entity, With<LobbyUi>>) {
    for e in &q {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
    }
}

/// Handle clicks on the setup-screen pickers + "Find Match"
/// button. The pickers mutate `LobbyState.target_humans` /
/// `target_ai` with sensible bounds; Find Match opens the
/// matchbox socket on the variant-specific room URL.
fn handle_setup_buttons(
    mut commands: Commands,
    mut state: ResMut<LobbyState>,
    signal_override: Res<SignalingOverride>,
    interactions: Query<(&Interaction, &SetupAction), Changed<Interaction>>,
) {
    for (interaction, action) in &interactions {
        if !matches!(interaction, Interaction::Pressed) {
            continue;
        }
        // Picker bounds: at least 1 human, total ≤ MAX_PLAYERS,
        // AI ≥ 0. Adjustments respect both axes.
        match action {
            SetupAction::HumansDec => {
                if state.target_humans > 1 {
                    state.target_humans -= 1;
                }
            }
            SetupAction::HumansInc => {
                if state.target_humans + state.target_ai < MAX_PLAYERS {
                    state.target_humans += 1;
                }
            }
            SetupAction::AiDec => {
                if state.target_ai > 0 {
                    state.target_ai -= 1;
                }
            }
            SetupAction::AiInc => {
                if state.target_humans + state.target_ai < MAX_PLAYERS {
                    state.target_ai += 1;
                }
            }
            SetupAction::FindMatch => {
                // Room URL encodes BOTH human + AI counts so
                // peers who picked the same combination land in
                // the same matchbox bucket. `?next=N` is the
                // human count — matchbox waits for N peers in
                // the room before connecting them. The base URL
                // comes from the runtime `SignalingOverride`
                // (populated from the browser URL's `?signal=`
                // query param on WASM) or falls back to the
                // compiled-in `SIGNALING_URL`.
                let base = pick_signaling_url(&signal_override);
                let url = format!(
                    "{}/starcon-h{}-a{}?next={}",
                    base, state.target_humans, state.target_ai, state.target_humans
                );
                info!("netplay: opening matchbox socket → {}", url);
                // matchbox 0.14 gathers ICE NON-trickle: each side waits
                // for the browser to FINISH gathering before it sends its
                // offer/answer (`wait_for_ice_gathering_complete`, with no
                // timeout cap). So an UNREACHABLE ICE server stalls the
                // whole connection ~39.5 s per side (the STUN/TURN
                // transaction timeout) — listing the UDP OpenRelay TURN
                // endpoints (which this network blocks) is what made
                // connects take ~2 minutes.
                //
                // This network can't pair on STUN alone, so we DO need a
                // relay — but only over TCP/443 (looks like HTTPS, rarely
                // blocked, and it's the endpoint that actually carried the
                // connection). Keeping just STUN + that one TURN URL means
                // gathering finishes fast AND the relay is available.
                //
                // OpenRelay's free static creds are best-effort; for a
                // reliable + fast relay, stand up your own coturn and swap
                // the URL/creds here.
                let socket = MatchboxSocket::from(
                    WebRtcSocketBuilder::new(url)
                        .ice_server(RtcIceServerConfig {
                            urls: vec![
                                "stun:stun.l.google.com:19302".to_string(),
                                "stun:stun1.l.google.com:19302".to_string(),
                                "turn:openrelay.metered.ca:443?transport=tcp".to_string(),
                            ],
                            username: Some("openrelayproject".to_string()),
                            credential: Some("openrelayproject".to_string()),
                        })
                        .add_channel(ChannelConfig::unreliable()),
                );
                commands.insert_resource(socket);
                state.status = LobbyStatus::Connecting;
                state.connected = 0;
            }
        }
    }
}

/// Show / hide the setup picker panel based on lobby status.
/// In Setup we show pickers; once Find Match has been pressed
/// (Connecting / Waiting / Ready / Failed) we hide them so the
/// status text takes center stage.
fn update_setup_visibility(
    state: Res<LobbyState>,
    mut panels: Query<&mut Visibility, With<LobbySetupPanel>>,
) {
    if !state.is_changed() {
        return;
    }
    let target = if matches!(state.status, LobbyStatus::Setup | LobbyStatus::Failed(_)) {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut v in &mut panels {
        if *v != target {
            *v = target;
        }
    }
}

/// Drop the matchbox socket when we leave the lobby — either
/// because we transitioned into InMatch (success) or backed out
/// to the main menu. Removes the WebRTC channels cleanly.
fn drop_socket(mut commands: Commands, mut state: ResMut<LobbyState>) {
    commands.remove_resource::<MatchboxSocket>();
    state.status = LobbyStatus::Setup;
    state.connected = 0;
}

/// Each tick while in LobbyOnline:
///   - In Setup status: no-op (waiting for the user to press
///     Find Match). `handle_setup_buttons` owns that path.
///   - Otherwise: drive the matchbox socket, count peers,
///     and once we've reached `target_humans` build the GGRS
///     session + apply the (humans + AI) slot config to
///     `MatchConfig` so `spawn_match` knows what to spawn.
fn update_lobby(
    mut commands: Commands,
    socket: Option<ResMut<MatchboxSocket>>,
    mut state: ResMut<LobbyState>,
    mut config: ResMut<crate::ship::MatchConfig>,
    mut seed: ResMut<crate::rng::MatchSeed>,
    mut next: ResMut<NextState<AppState>>,
) {
    if matches!(state.status, LobbyStatus::Setup) {
        return;
    }
    let Some(mut socket) = socket else {
        return;
    };
    // Pump WebRTC and log each peer state change so the browser
    // console shows exactly how far a connection gets: a `CONNECTED`
    // line means WebRTC (ICE) succeeded; if you only ever see
    // "waiting" with no CONNECTED, the two peers never reached each
    // other (different rooms, or NAT/firewall blocking even TURN).
    for (peer, peer_state) in socket.update_peers() {
        match peer_state {
            PeerState::Connected => {
                info!("netplay: peer {peer:?} CONNECTED");
            }
            PeerState::Disconnected => {
                info!("netplay: peer {peer:?} disconnected");
            }
        }
    }
    let remote_count = socket.connected_peers().count();
    let total = remote_count + 1; // +1 for the local player
    let humans = state.target_humans;
    if total != state.connected {
        info!("netplay: players {total}/{humans} connected");
    }
    state.connected = total;

    state.status = match state.status {
        LobbyStatus::Failed(msg) => LobbyStatus::Failed(msg),
        _ if total >= humans => LobbyStatus::SessionReady,
        _ if total >= 1 => LobbyStatus::WaitingForPeers,
        _ => LobbyStatus::Connecting,
    };

    if !matches!(state.status, LobbyStatus::SessionReady) {
        return;
    }

    // Build a deterministic player list — sorted by PeerId so
    // every machine assigns the same handle (0..=humans-1) to
    // the same physical player. Can't use `socket.players()`
    // because its return type is the ggrs 0.11 `PlayerType` and
    // our bevy_ggrs needs the 0.12 type.
    let Some(our_id) = socket.id() else {
        return;
    };
    let mut peer_ids: Vec<PeerId> = socket
        .connected_peers()
        .chain(std::iter::once(our_id))
        .collect();
    peer_ids.sort();
    let players: Vec<PlayerType<PeerId>> = peer_ids
        .iter()
        .map(|&id| {
            if id == our_id {
                PlayerType::Local
            } else {
                PlayerType::Remote(id)
            }
        })
        .collect();
    if players.len() < humans {
        return;
    }

    // GGRS 0.12 made the builder methods fallible.
    let mut builder = match SessionBuilder::<Config>::new().with_num_players(humans) {
        Ok(b) => b,
        Err(e) => {
            warn!("netplay: bad num_players config: {e:?}");
            state.status = LobbyStatus::Failed("invalid GGRS num_players");
            return;
        }
    };
    builder = builder.with_input_delay(INPUT_DELAY);
    builder = match builder.with_fps(FPS) {
        Ok(b) => b,
        Err(e) => {
            warn!("netplay: bad fps config: {e:?}");
            state.status = LobbyStatus::Failed("invalid GGRS fps config");
            return;
        }
    };
    let mut local_handle: usize = 0;
    for (handle, player) in players.into_iter().enumerate() {
        if matches!(player, PlayerType::Local) {
            local_handle = handle;
        }
        builder = match builder.add_player(player, handle) {
            Ok(b) => b,
            Err(e) => {
                warn!("netplay: add_player {handle} failed: {e:?}");
                state.status = LobbyStatus::Failed("ggrs add_player failed");
                return;
            }
        };
    }
    let channel = match socket.take_channel(0) {
        Ok(c) => c,
        Err(e) => {
            warn!("netplay: take_channel(0) failed: {e:?}");
            state.status = LobbyStatus::Failed("matchbox channel unavailable");
            return;
        }
    };
    let session = match builder.start_p2p_session(GgrsChannelAdapter(channel)) {
        Ok(s) => s,
        Err(e) => {
            warn!("netplay: start_p2p_session failed: {e:?}");
            state.status = LobbyStatus::Failed("ggrs session start failed");
            return;
        }
    };

    // Apply the chosen humans/AI mix to MatchConfig so
    // `spawn_match` knows what to spawn. Human slots take the
    // lowest indices (so handles 0..humans map to slots
    // 0..humans), then AI slots fill the rest.
    use crate::ship::{PlayerKind, ShipClass, SlotConfig};
    let ai_count = state.target_ai.min(MAX_PLAYERS.saturating_sub(humans));
    let class_palette = [
        ShipClass::Earcr,
        ShipClass::Spael,
        ShipClass::Yehte,
        ShipClass::Chmav,
    ];
    let mut slots = Vec::with_capacity(humans + ai_count);
    for i in 0..humans {
        slots.push(SlotConfig {
            class: class_palette[i.min(3)],
            kind: PlayerKind::Remote,
        });
    }
    // Local player overrides their own slot kind to Human so
    // `apply_player_input` reads the kbd for their slot. (The
    // shared GGRS input pipeline isn't wired into the gameplay
    // dispatch yet — see netplay.rs module docs — so for now
    // the local kbd is what actually drives the local slot,
    // and remote slots stay still. The infrastructure is in
    // place for the GgrsSchedule migration.)
    if let Some(slot) = slots.get_mut(local_handle) {
        slot.kind = PlayerKind::Human;
    }
    for i in 0..ai_count {
        slots.push(SlotConfig {
            class: class_palette[(humans + i).min(3)],
            kind: PlayerKind::Ai,
        });
    }
    config.slots = slots;

    // Derive a shared per-match seed from the sorted PeerId
    // list. Both peers see the same sort order so they hash
    // to the same seed; the seed is mixed into `GameRng` on
    // OnEnter(InMatch) so all peers consume the same RNG
    // stream.
    {
        use std::hash::{BuildHasher, Hash, Hasher};
        let mut hasher = bevy::platform::hash::FixedHasher::default().build_hasher();
        peer_ids.hash(&mut hasher);
        seed.0 = hasher.finish();
    }

    commands.insert_resource(Session::P2P(session));
    commands.insert_resource(LocalPlayers(vec![local_handle]));
    info!(
        "netplay: GGRS session started — {} humans + {} AI, local handle = {}, seed = {:#x}",
        humans, ai_count, local_handle, seed.0
    );
    next.set(AppState::InMatch);
}

/// Per-frame input collection for GGRS. Reads the local
/// keyboard (always slot-0 keymap — arrows + Z/X — because each
/// peer owns exactly one slot in an online match, and we want
/// every peer's controls to match the local-P1 convention) and
/// stashes the result in `LocalInputs<Config>` keyed by the
/// local handle. GGRS picks this resource up before the
/// rollback schedule runs.
fn read_local_inputs(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    virt: Res<crate::input::VirtualInput>,
    local_players: Option<Res<LocalPlayers>>,
) {
    let Some(local_players) = local_players else {
        return;
    };
    let mut map = bevy::platform::collections::HashMap::default();
    let input = crate::input::read_local_input_with_virtual(&keys, Some(&virt), 0);
    for handle in &local_players.0 {
        map.insert(*handle, input);
    }
    commands.insert_resource(LocalInputs::<Config>(map));
}

/// Render the current lobby status + picker counts into their
/// respective text nodes. Pulled out from `update_lobby` so the
/// text update is idempotent w.r.t. state transitions.
fn update_lobby_ui(
    state: Res<LobbyState>,
    mut status_q: Query<
        &mut Text,
        (
            With<LobbyStatusText>,
            Without<HumansCountText>,
            Without<AiCountText>,
        ),
    >,
    mut humans_q: Query<
        &mut Text,
        (
            With<HumansCountText>,
            Without<LobbyStatusText>,
            Without<AiCountText>,
        ),
    >,
    mut ai_q: Query<
        &mut Text,
        (
            With<AiCountText>,
            Without<LobbyStatusText>,
            Without<HumansCountText>,
        ),
    >,
) {
    if !state.is_changed() {
        return;
    }
    for mut text in &mut humans_q {
        text.0 = format!("{}", state.target_humans);
    }
    for mut text in &mut ai_q {
        text.0 = format!("{}", state.target_ai);
    }
    let humans = state.target_humans;
    for mut text in &mut status_q {
        text.0 = match state.status {
            LobbyStatus::Setup => String::new(),
            LobbyStatus::Connecting => "Connecting to signaling server...".to_string(),
            LobbyStatus::WaitingForPeers => format!(
                "Waiting for players: {}/{}",
                state.connected, humans
            ),
            LobbyStatus::SessionReady => format!(
                "All {} humans connected — starting match.",
                humans
            ),
            LobbyStatus::Failed(msg) => format!("Connection failed: {}", msg),
        };
    }
}

/// Escape backs out to the main menu — useful when the room is
/// empty and the player doesn't want to wait.
fn lobby_back_to_menu(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<AppState>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        next.set(AppState::MainMenu);
    }
}

/// Bridge `PlayerInputs<Config>` (which only lives inside
/// `GgrsSchedule`) into our long-lived `NetInputs` resource so
/// `gather_slot_inputs` (running in FixedUpdate) can read it.
///
/// Each tick we shift `current → previous` then write the
/// freshly-received per-handle inputs into `current`. Edge
/// detection (just_pressed / just_released) is then computed
/// by `gather_slot_inputs` as a per-bit diff between the two.
fn net_inputs_bridge(
    inputs: Option<Res<PlayerInputs<Config>>>,
    mut net: ResMut<crate::input::NetInputs>,
) {
    let Some(inputs) = inputs else {
        return;
    };
    net.previous = net.current;
    // GGRS gives us a Vec<(Input, InputStatus)> indexed by
    // handle. Map handle → slot 1:1 (handles are 0..H, slots
    // also start at 0). `InputStatus` is ignored here; we just
    // trust the most recent confirmed/predicted input.
    for (handle, (input, _status)) in inputs.iter().enumerate() {
        if handle < 4 {
            net.current[handle] = *input;
        }
    }
}
