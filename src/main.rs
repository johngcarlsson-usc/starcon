mod ability;
mod ai;
mod collider;
mod hud;
mod input;
mod menu;
mod netplay;
mod physics;
mod rng;
mod settings_menu;
mod ship;
mod mobile_controls;
mod starfield;
mod timeflow;
mod ultimate;

use bevy::prelude::*;

#[derive(States, Clone, Copy, Eq, PartialEq, Hash, Debug, Default)]
pub enum AppState {
    #[default]
    Loading,
    /// Title screen + match-setup buttons (Local 2P / Local 4P /
    /// Online). Pick one to populate `MatchConfig` and transition
    /// into either `LobbyOnline` (matchmaking) or `InMatch` (local).
    MainMenu,
    /// Online lobby: connect to the matchbox signaling server,
    /// wait for peers, and transition to InMatch once the GGRS
    /// session is ready.
    LobbyOnline,
    /// Tears down the previous round's entities before re-entering
    /// InMatch. One-frame transient state — used by the rematch flow.
    Resetting,
    InMatch,
}

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Starcon".into(),
                        resolution: (1280u32, 720u32).into(),
                        // On WASM, fill the parent <body> so the canvas
                        // grows with the browser window. Ignored on native.
                        fit_canvas_to_parent: true,
                        // Stop the browser from highlighting the canvas
                        // or scrolling the page when arrow keys are
                        // pressed during play.
                        prevent_default_event_handling: true,
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    // Skip the per-asset `.meta` sidecar probe — we don't
                    // generate those, and on WASM every miss is a real
                    // network round-trip that doubles the asset-load
                    // count and floods devtools with 404 noise.
                    meta_check: bevy::asset::AssetMetaCheck::Never,
                    ..default()
                })
                .set(ImagePlugin::default_nearest()),
        )
        .init_state::<AppState>()
        .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.05)))
        .add_plugins((
            ship::ShipPlugin,
            ability::AbilityPlugin,
            collider::ColliderPlugin,
            physics::PhysicsPlugin,
            input::InputPlugin,
            netplay::NetplayPlugin,
            timeflow::TimeflowPlugin,
            hud::HudPlugin,
            starfield::StarfieldPlugin,
            ultimate::UltimatePlugin,
            mobile_controls::MobileControlsPlugin,
            menu::MenuPlugin,
            ai::AiPlugin,
            rng::RngPlugin,
            settings_menu::SettingsMenuPlugin,
        ))
        .init_resource::<ship::PreloadedAssets>()
        .add_systems(Startup, (setup_camera, ship::preload_all_assets))
        .add_systems(OnEnter(AppState::Loading), ship::load_ship_catalog)
        .add_systems(OnEnter(AppState::InMatch), ship::spawn_match)
        .add_systems(OnEnter(AppState::InMatch), starfield::reset_for_new_match)
        .add_systems(OnEnter(AppState::Resetting), ship::teardown_match)
        .add_systems(
            Update,
            (
                advance_loading_to_menu.run_if(in_state(AppState::Loading)),
                resume_from_reset.run_if(in_state(AppState::Resetting)),
                request_rematch.run_if(in_state(AppState::InMatch)),
            ),
        )
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Once the ship catalog has loaded, move from `Loading` to
/// `MainMenu`. The menu plugin owns the next transition (to
/// either `InMatch` for local play or `LobbyOnline` for online).
fn advance_loading_to_menu(
    catalog: Option<Res<ship::ShipCatalog>>,
    mut next: ResMut<NextState<AppState>>,
) {
    if catalog.is_some() {
        next.set(AppState::MainMenu);
    }
}

/// Once OnEnter(Resetting) has finished tearing down the old round,
/// we bounce straight back into InMatch on the next frame.
fn resume_from_reset(
    mut next: ResMut<NextState<AppState>>,
    mut phase: ResMut<hud::MatchPhase>,
    mut outcome: ResMut<hud::MatchOutcome>,
) {
    outcome.winner = None;
    *phase = hud::MatchPhase::Live;
    next.set(AppState::InMatch);
}

fn request_rematch(
    keys: Res<ButtonInput<KeyCode>>,
    phase: Res<hud::MatchPhase>,
    mut next: ResMut<NextState<AppState>>,
) {
    if *phase == hud::MatchPhase::PostMatch && keys.just_pressed(KeyCode::KeyR) {
        next.set(AppState::Resetting);
    }
}
