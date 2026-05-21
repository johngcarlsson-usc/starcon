mod hud;
mod input;
mod netplay;
mod physics;
mod ship;
mod timeflow;

use bevy::prelude::*;

#[derive(States, Clone, Copy, Eq, PartialEq, Hash, Debug, Default)]
pub enum AppState {
    #[default]
    Loading,
    MainMenu,
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
                .set(ImagePlugin::default_nearest()),
        )
        .init_state::<AppState>()
        .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.05)))
        .add_plugins((
            ship::ShipPlugin,
            physics::PhysicsPlugin,
            input::InputPlugin,
            netplay::NetplayPlugin,
            timeflow::TimeflowPlugin,
            hud::HudPlugin,
        ))
        .add_systems(Startup, setup_camera)
        .add_systems(OnEnter(AppState::Loading), ship::load_ship_catalog)
        .add_systems(OnEnter(AppState::InMatch), ship::spawn_match)
        .add_systems(OnEnter(AppState::Resetting), ship::teardown_match)
        .add_systems(
            Update,
            (
                advance_to_match.run_if(in_state(AppState::Loading)),
                resume_from_reset.run_if(in_state(AppState::Resetting)),
                request_rematch.run_if(in_state(AppState::InMatch)),
            ),
        )
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

// M1: skip the main menu and drop straight into a one-ship test scene
// so we can see physics + input + sprite rotation working.
fn advance_to_match(
    catalog: Option<Res<ship::ShipCatalog>>,
    mut next: ResMut<NextState<AppState>>,
) {
    if catalog.is_some() {
        next.set(AppState::InMatch);
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
