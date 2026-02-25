//! Minimal example: load a `.jsn` scene exported from the editor.
//!
//! Place a scene file at `assets/examples/scenes/scene.jsn` (use the editor's
//! Ctrl+S to export one), then run:
//!
//! ```sh
//! cargo run --example jsn_load
//! ```

use bevy::prelude::*;
use bevy_jsn::JsnPlugin;

fn main() -> AppExit {
    App::new()
        .add_plugins((DefaultPlugins, JsnPlugin))
        .add_systems(Startup, setup)
        .run()
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Load a .jsn scene exported from the editor.
    // JsnPlugin registers the asset loader and auto-generates meshes for Brush components.
    commands.spawn(DynamicSceneRoot(
        asset_server.load("examples/scenes/scene.jsn"),
    ));

    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(5.0, 5.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Light
    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            illuminance: 10000.0,
            ..default()
        },
        Transform::from_xyz(10.0, 20.0, 10.0)
            .with_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, 0.4, 0.0)),
    ));
}
