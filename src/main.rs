mod hud;
mod input;
mod netplay;
mod physics;
mod ship;
mod timeflow;

use bevy::prelude::*;

#[derive(States, Clone, Copy, Eq, PartialEq, Hash, Debug, Default)]
enum AppState {
    #[default]
    Loading,
    MainMenu,
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
        .add_systems(
            Update,
            advance_to_match.run_if(in_state(AppState::Loading)),
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
