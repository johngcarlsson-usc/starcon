mod input;
mod netplay;
mod physics;
mod ship;

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
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Starcon".into(),
                resolution: (1280u32, 720u32).into(),
                ..default()
            }),
            ..default()
        }))
        .init_state::<AppState>()
        .add_plugins((
            ship::ShipPlugin,
            physics::PhysicsPlugin,
            input::InputPlugin,
            netplay::NetplayPlugin,
        ))
        .add_systems(Startup, setup_camera)
        .add_systems(OnEnter(AppState::Loading), ship::load_ship_catalog)
        .add_systems(
            Update,
            advance_to_menu.run_if(in_state(AppState::Loading)),
        )
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn advance_to_menu(
    catalog: Option<Res<ship::ShipCatalog>>,
    mut next: ResMut<NextState<AppState>>,
) {
    if catalog.is_some() {
        next.set(AppState::MainMenu);
    }
}
