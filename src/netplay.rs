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
//!   - [x] Lobby state machine + UI text (this file + `menu.rs`).
//!   - [x] `MatchboxSocket` connect on OnEnter(LobbyOnline).
//!   - [x] Peer-count loop that transitions to InMatch once full.
//!   - [ ] GGRS `SessionBuilder` handoff. **TODO** — wire
//!         bevy_ggrs `Session<Config>` + register Rollback
//!         components + move physics tick into `GgrsSchedule`.
//!   - [ ] Determinism audit: switch the global `fastrand` calls
//!         to a per-frame seeded RNG; replace `Time<Real>` reads
//!         in gameplay systems with `Time<Physics>` /
//!         `Time<Fixed>`; sort `Query` iteration where order
//!         affects RNG consumption.
//!   - [ ] Per-slot input routing: `read_local_input` returns
//!         GGRS inputs for non-local slots once the session is
//!         running.
//!
//! On WASM (the primary deployment target), WebRTC peer
//! connections work out of the box. On native, you'll need a
//! self-hosted matchbox signaling server reachable from both
//! peers; the public test server at `match.helsing.studio` is
//! intermittently up but fine for development.

use bevy::prelude::*;
use bevy_matchbox::prelude::*;

use crate::AppState;

/// Maximum players per match. The lobby waits for exactly
/// `NUM_PLAYERS` peers (including ourselves) before starting.
/// 4 is the canonical Super Melee cap; smaller numbers are
/// supported by setting fewer slots in `MatchConfig.classes`.
pub const NUM_PLAYERS: usize = 4;

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

/// Matchbox signaling server URL used to find peers. The
/// `?next=N` query asks the server to bucket us with N peers
/// total before connecting us up.
pub const DEFAULT_ROOM_URL: &str = "wss://match.helsing.studio/starcon?next=4";

/// Set by `menu.rs` when the player picks "Online". Read on
/// OnEnter(LobbyOnline) to decide whether to open a socket.
/// Allows a future "Resume online match" flow to enter
/// LobbyOnline without re-running the connection.
#[derive(Resource, Default)]
pub struct LobbyRequest {
    pub requested: bool,
}

/// Live lobby connection state. Owns the matchbox socket while
/// we're in `AppState::LobbyOnline`; the socket is dropped on
/// OnExit so the WebRTC channels are released.
#[derive(Resource, Default)]
pub struct LobbyState {
    pub status: LobbyStatus,
    /// Last known connected-peer count, including the local
    /// player. Updated by `update_lobby` so the UI can render
    /// "Waiting for players: 2/4".
    pub connected: usize,
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LobbyStatus {
    #[default]
    Idle,
    /// Opening the WebSocket to the signaling server.
    Connecting,
    /// Connected to the signaling server; waiting for the room
    /// to fill up to NUM_PLAYERS.
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

pub struct NetplayPlugin;

impl Plugin for NetplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LobbyRequest>()
            .init_resource::<LobbyState>()
            .add_systems(OnEnter(AppState::LobbyOnline), (spawn_lobby_ui, connect_to_matchbox))
            .add_systems(OnExit(AppState::LobbyOnline), (despawn_lobby_ui, drop_socket))
            .add_systems(
                Update,
                (update_lobby, update_lobby_ui, lobby_back_to_menu)
                    .run_if(in_state(AppState::LobbyOnline)),
            );
    }
}

fn spawn_lobby_ui(mut commands: Commands) {
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
            root.spawn((
                LobbyStatusText,
                Text::new("Connecting..."),
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

fn despawn_lobby_ui(mut commands: Commands, q: Query<Entity, With<LobbyUi>>) {
    for e in &q {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
    }
}

/// On entry to LobbyOnline: open a matchbox WebRTC socket to the
/// signaling server. The socket is stored as a resource so
/// `update_lobby` can poll its peer list each tick.
///
/// `MatchboxSocket::from(WebRtcSocketBuilder)` defers the actual
/// connection — the socket is "Connecting" until the first
/// `update_peers()` call returns a peer event. No async/await,
/// no spawned task; Bevy's Update tick drives the polling.
fn connect_to_matchbox(
    mut commands: Commands,
    mut state: ResMut<LobbyState>,
    request: Res<LobbyRequest>,
) {
    if !request.requested {
        state.status = LobbyStatus::Idle;
        return;
    }
    info!("netplay: opening matchbox socket → {}", DEFAULT_ROOM_URL);
    // `new_unreliable` is the right channel for GGRS — rollback
    // netcode already handles its own retransmits, so we want
    // raw UDP-over-WebRTC without the ordered/reliable wrapper.
    let socket = MatchboxSocket::new_unreliable(DEFAULT_ROOM_URL);
    commands.insert_resource(socket);
    state.status = LobbyStatus::Connecting;
    state.connected = 0;
}

/// Drop the matchbox socket when we leave the lobby — either
/// because we transitioned into InMatch (success) or backed out
/// to the main menu. Removes the WebRTC channels cleanly.
fn drop_socket(mut commands: Commands, mut state: ResMut<LobbyState>) {
    commands.remove_resource::<MatchboxSocket>();
    state.status = LobbyStatus::Idle;
    state.connected = 0;
}

/// Each tick while in LobbyOnline:
///   - Drive the matchbox socket forward by calling
///     `update_peers()`. Returns the list of new/lost peer
///     events but we mostly care about the connected-peer count.
///   - Update `LobbyState.connected` and `status`.
///   - When the peer count reaches `NUM_PLAYERS`, mark
///     SessionReady and transition to InMatch.
///
/// TODO: when SessionReady fires, hand the socket off to a GGRS
/// `SessionBuilder` and stash the resulting `Session<Config>` as
/// a resource. The InMatch systems then read inputs from that
/// session instead of the keyboard for remote slots.
fn update_lobby(
    socket: Option<ResMut<MatchboxSocket>>,
    mut state: ResMut<LobbyState>,
    mut next: ResMut<NextState<AppState>>,
) {
    let Some(mut socket) = socket else {
        return;
    };
    // Pump WebRTC: returns the list of peer state changes since
    // the last call, but we just count connected peers afterward.
    let _events = socket.update_peers();
    // Connected peers + ourselves. matchbox treats the local
    // player implicitly — we count remote players via the
    // connected_peers() iterator.
    let remote_count = socket.connected_peers().count();
    let total = remote_count + 1; // +1 for the local player
    state.connected = total;

    state.status = match state.status {
        LobbyStatus::Failed(msg) => LobbyStatus::Failed(msg),
        _ if total >= NUM_PLAYERS => LobbyStatus::SessionReady,
        _ if total >= 1 => LobbyStatus::WaitingForPeers,
        _ => LobbyStatus::Connecting,
    };

    if matches!(state.status, LobbyStatus::SessionReady) {
        // TODO: build GGRS session here:
        //   let mut sess = SessionBuilder::<GgrsConfig>::new()
        //       .with_num_players(NUM_PLAYERS)
        //       .with_input_delay(INPUT_DELAY)
        //       .with_fps(FPS)?;
        //   for (i, peer) in socket.players().iter().enumerate() {
        //       sess = sess.add_player(peer.clone(), i)?;
        //   }
        //   let channel = socket.take_channel(0)?;
        //   let session = sess.start_p2p_session(channel)?;
        //   commands.insert_resource(Session::P2P(session));
        // For now we just drop into InMatch with local kbd
        // driving every slot — useful for end-to-end testing of
        // the lobby UI before the GGRS plumbing lands.
        info!("netplay: {}/{} peers, entering InMatch", total, NUM_PLAYERS);
        next.set(AppState::InMatch);
    }
}

/// Render the current lobby status into the status-text node.
/// Pulled out from `update_lobby` so the text update is
/// idempotent w.r.t. state transitions (i.e. the text always
/// shows the freshest status, even on the same-frame transition
/// from WaitingForPeers to SessionReady).
fn update_lobby_ui(
    state: Res<LobbyState>,
    mut q: Query<&mut Text, With<LobbyStatusText>>,
) {
    if !state.is_changed() {
        return;
    }
    for mut text in &mut q {
        text.0 = match state.status {
            LobbyStatus::Idle => "Idle.".to_string(),
            LobbyStatus::Connecting => "Connecting to signaling server...".to_string(),
            LobbyStatus::WaitingForPeers => format!(
                "Waiting for players: {}/{}",
                state.connected, NUM_PLAYERS
            ),
            LobbyStatus::SessionReady => format!(
                "All {} players connected — starting match.",
                NUM_PLAYERS
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
