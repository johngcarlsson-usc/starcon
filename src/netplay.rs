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

use bevy::prelude::*;
use bevy_matchbox::matchbox_socket::RtcIceServerConfig;
use bevy_matchbox::prelude::*;

use crate::AppState;

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
/// Default matchbox signaling server. Points at the deployed Replit
/// Reserved-VM matchbox so the plain GitHub Pages URL works online with
/// no query string. Override with `?signal=ws(s)://host` for local dev
/// (`ws://localhost:3536`) or a different server.
pub const SIGNALING_URL: &str = "wss://basic-boilerplate.replit.app";

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

/// Optional override for the WebRTC TURN relay, from the browser URL's
/// `?turn=`, `?turn_user=`, `?turn_cred=` query params. Lets you point
/// at your own (or a signed-up metered.ca) TURN server WITHOUT a client
/// rebuild — paste creds into the URL and reload. When `turn_url` is
/// unset we fall back to the best-effort public OpenRelay endpoints.
#[derive(Resource, Default, Debug, Clone)]
pub struct IceOverride {
    pub turn_url: Option<String>,
    pub turn_user: Option<String>,
    pub turn_cred: Option<String>,
    /// STUN override from `?stun=`. A value of `none` disables STUN
    /// entirely (relay-only) — useful when STUN servers your network
    /// can't reach would otherwise stall matchbox's non-trickle ICE
    /// gathering. Unset → default Google STUN.
    pub stun_url: Option<String>,
}

/// Build the ICE server list for the matchbox socket.
///
/// Defaults (tuned for this game's players, who sit behind symmetric
/// NAT and thus always relay): NO STUN, and a single TCP TURN relay.
/// matchbox 0.14 gathers ICE NON-trickle — each side blocks until the
/// browser finishes gathering before sending its offer/answer, with no
/// timeout cap — so any listed server the network can't reach (or a
/// lossy UDP path that makes the TURN Allocate retransmit) stalls the
/// connect ~40s per side. We measured exactly that: STUN and UDP/TLS
/// relays stalled ~90s; the TCP relay connects in a couple of seconds.
///
/// Overrides (URL query params): `?stun=` adds a STUN server (or
/// `none`); `?turn=`/`?turn_user=`/`?turn_cred=` swap the relay (e.g.
/// for a self-hosted coturn).
fn build_ice_config(over: &IceOverride) -> RtcIceServerConfig {
    let mut urls = Vec::new();
    // STUN is opt-in only — see the non-trickle stall note above.
    match over.stun_url.as_deref() {
        Some("none") | Some("") | None => {}
        Some(s) => urls.push(s.to_string()),
    }
    if let Some(turn) = &over.turn_url {
        urls.push(turn.clone());
        RtcIceServerConfig {
            urls,
            username: over.turn_user.clone(),
            credential: over.turn_cred.clone(),
        }
    } else {
        // metered.ca TCP relay — the transport proven to connect fast
        // on a lossy-UDP / symmetric-NAT network. Free-tier creds; you
        // can regenerate them in the metered dashboard if abused.
        urls.push("turn:global.relay.metered.ca:80?transport=tcp".to_string());
        RtcIceServerConfig {
            urls,
            username: Some("a6c88029d884fc61f4607daa".to_string()),
            credential: Some("4cJdlCmAI4qGgnwo".to_string()),
        }
    }
}

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
fn capture_signal_override(
    mut over: ResMut<SignalingOverride>,
    mut ice: ResMut<IceOverride>,
) {
    let Some(window) = web_sys::window() else { return };
    let Ok(location) = window.location().search() else { return };
    let q = location.trim_start_matches('?');
    let decode = |rest: &str| -> String {
        js_sys::decode_uri_component(rest)
            .map(|v| v.as_string().unwrap_or_else(|| rest.to_string()))
            .unwrap_or_else(|_| rest.to_string())
    };
    for pair in q.split('&') {
        if let Some(rest) = pair.strip_prefix("signal=") {
            let decoded = decode(rest);
            info!("netplay: signaling override from URL: {}", decoded);
            over.0 = Some(decoded);
        } else if let Some(rest) = pair.strip_prefix("turn=") {
            let decoded = decode(rest);
            info!("netplay: TURN override from URL: {}", decoded);
            ice.turn_url = Some(decoded);
        } else if let Some(rest) = pair.strip_prefix("turn_user=") {
            ice.turn_user = Some(decode(rest));
        } else if let Some(rest) = pair.strip_prefix("turn_cred=") {
            ice.turn_cred = Some(decode(rest));
        } else if let Some(rest) = pair.strip_prefix("stun=") {
            let decoded = decode(rest);
            info!("netplay: STUN override from URL: {}", decoded);
            ice.stun_url = Some(decoded);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn capture_signal_override(_over: ResMut<SignalingOverride>, _ice: ResMut<IceOverride>) {
    // On native, the override can be set via env or CLI later;
    // for now no-op so the defaults are used.
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
        // GGRS rollback is gone — see `NETCODE_REFACTOR.md`. Gameplay
        // now ticks on plain `FixedUpdate`, and the
        // `netcode::NetSocket` host/guest path syncs state instead of
        // re-simulating from a shared deterministic seed. The matchbox
        // socket setup + lobby UI stay; everything that's downstream
        // of "are we connected to a peer?" is unchanged.
        app.init_resource::<LobbyRequest>()
            .init_resource::<LobbyState>()
            .init_resource::<SignalingOverride>()
            .init_resource::<IceOverride>()
            .add_systems(Startup, capture_signal_override)
            .add_systems(OnEnter(AppState::LobbyOnline), (reset_lobby, spawn_lobby_ui))
            .add_systems(OnExit(AppState::LobbyOnline), despawn_lobby_ui)
            // The matchbox socket has to outlive `LobbyOnline` —
            // `take_channel(0)` hands us a `WebRtcChannel` whose
            // underlying WebRTC peer connection is owned by the
            // `MatchboxSocket` runtime. Dropping the socket tears
            // down that runtime and the channel goes Disconnected,
            // which panics the next `channel.send()` call. So we
            // hold onto the socket through `InMatch` and only drop
            // it on the way back to `MainMenu`.
            .add_systems(OnEnter(AppState::MainMenu), drop_socket)
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
            ;
        // `read_local_inputs` + `net_inputs_bridge` (the old GGRS
        // input pipeline) are now dead code under the new netcode.
        // Guest input forwarding goes through `netcode::NetMessage::
        // Input` instead; host reads its own keyboard via the
        // existing `gather_slot_inputs` local-handle path.
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
    ice_override: Res<IceOverride>,
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
                let ice = build_ice_config(&ice_override);
                info!("netplay: ICE servers: {:?}", ice.urls);
                // Two channels:
                //   0: unreliable — GGRS rollback inputs (current
                //      path) + the future authoritative-host
                //      snapshot stream. High volume, latency
                //      sensitive, drop-tolerant.
                //   1: reliable, ordered — lobby votes,
                //      ship-select state changes, anything that
                //      "must arrive exactly once." Low volume.
                // The host/guest split (see NETCODE_REFACTOR.md)
                // moves snapshot + input traffic to channel 0
                // while leaving channel 1 for lobby state, so the
                // two protocols don't head-of-line each other.
                let socket = MatchboxSocket::from(
                    WebRtcSocketBuilder::new(url)
                        .ice_server(ice)
                        .add_channel(ChannelConfig::unreliable())
                        .add_channel(ChannelConfig::reliable()),
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

/// Drop the matchbox socket + all derived netcode resources when
/// we land back on the main menu. We deliberately do NOT do this on
/// `OnExit(LobbyOnline)` — the `NetSocket.channel` we extract from
/// the socket via `take_channel(0)` keeps its WebRTC peer connection
/// alive via the `MatchboxSocket` runtime, so dropping the socket
/// while a match is running disconnects the channel and panics the
/// next `channel.send()`. By the time we're back at `MainMenu` we're
/// genuinely done with that peer connection, so the cleanup is safe.
fn drop_socket(mut commands: Commands, mut state: ResMut<LobbyState>) {
    commands.remove_resource::<crate::netcode::NetSocket>();
    commands.remove_resource::<crate::netcode::LocalHandle>();
    // NetRole and NetIdAllocator are `init_resource`'d at startup and
    // read by always-on run conditions (e.g. `role_is_authoritative`),
    // so they have to exist for the whole app lifetime. Reset them
    // back to the Solo default instead of removing them.
    commands.insert_resource(crate::netcode::NetRole::Solo);
    commands.insert_resource(crate::netcode::NetIdAllocator::default());
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

    // Elect host / guest from the sorted PeerId list. Lower id = Host.
    // Same answer on both peers because they both see the same sort.
    let role = crate::netcode::elect_role(our_id, &peer_ids);
    commands.insert_resource(role);
    info!("netplay: role elected = {:?}", role);
    // Reset the NetId allocator so id 1 is the first ship of the new
    // match. Lives on the host; guests don't allocate but resetting
    // is harmless and keeps the resource consistent.
    commands.insert_resource(crate::netcode::NetIdAllocator::default());

    if peer_ids.len() < humans {
        return;
    }

    // Local-handle = position of `our_id` in the sorted PeerId list.
    // Host is index 0; guest is 1+.
    let local_handle: usize = peer_ids
        .iter()
        .position(|&p| p == our_id)
        .unwrap_or(0);

    // Channel 0 used to go to GGRS; now it's the host/guest
    // `NetMessage` path. Cache the remote peer list alongside the
    // channel because `WebRtcChannel` doesn't expose connected
    // peers directly.
    let remote_peers: Vec<PeerId> = peer_ids
        .iter()
        .copied()
        .filter(|id| *id != our_id)
        .collect();
    let net_channel = match socket.take_channel(0) {
        Ok(c) => c,
        Err(e) => {
            warn!("netplay: take_channel(0) failed: {e:?}");
            state.status = LobbyStatus::Failed("matchbox channel unavailable");
            return;
        }
    };
    commands.insert_resource(crate::netcode::NetSocket {
        channel: Some(net_channel),
        heartbeat_s: 0.0,
        peers: remote_peers,
    });
    // Channel 1 (the reliable one) goes unused for the moment — the
    // lobby-vote re-routing in step 5 of NETCODE_REFACTOR.md will
    // claim it once we move votes off the unreliable snapshot path.
    let _ = socket.take_channel(1);
    commands.insert_resource(crate::netcode::LocalHandle(local_handle));

    // Apply the chosen humans/AI mix to MatchConfig so
    // `spawn_match` knows what to spawn. Human slots take the
    // lowest indices (so peer 0..humans map to slots 0..humans),
    // then AI slots fill the rest.
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

    // Per-match seed still derives from the sorted PeerId list —
    // host + guest both compute the same `GameRng` start so any
    // single-machine RNG draws (planet quadrant, asteroid spawns)
    // come out identical on both sides without explicit sync.
    {
        use std::hash::{BuildHasher, Hash, Hasher};
        let mut hasher = bevy::platform::hash::FixedHasher::default().build_hasher();
        peer_ids.hash(&mut hasher);
        seed.0 = hasher.finish();
    }

    info!(
        "netplay: session up — role = {:?}, {} humans + {} AI, local handle = {}, seed = {:#x}",
        role, humans, ai_count, local_handle, seed.0
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
// `read_local_inputs` + `net_inputs_bridge` removed — the GGRS
// input pipeline they fed is gone. Guest input forwarding lives
// in `netcode::NetMessage::Input` now; host reads its own keyboard
// via the existing `gather_slot_inputs` local-handle path.

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

// `net_inputs_bridge` removed with the GGRS pipeline. Guest input
// arrives via `netcode::drain_messages` and gets written into
// `NetInputs.current[guest_slot]` from there.
