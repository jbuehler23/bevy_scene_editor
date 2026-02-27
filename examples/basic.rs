use bevy::prelude::*;
use jackdaw::EditorPlugin;

fn main() -> AppExit {
    App::new()
        .add_plugins((DefaultPlugins, EditorPlugin))
        .add_systems(Startup, spawn_scene)
        .run()
}

fn spawn_scene(mut commands: Commands) {
    // Directional light with shadows, positioned away from origin
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            shadows_enabled: true,
            illuminance: 10000.0,
            ..default()
        },
        Transform::from_xyz(10.0, 20.0, 10.0)
            .with_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, 0.4, 0.0)),
    ));
}
