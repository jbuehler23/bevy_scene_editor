use bevy::prelude::*;

mod plugin;
mod state;
mod layout;
mod hierarchy;
mod inspector;
mod inspector_widgets;
mod viewport;
mod systems;

use plugin::EditorPlugin;

fn main() -> AppExit {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EditorPlugin)
        .add_systems(Startup, spawn_test_scene)
        .run()
}

fn spawn_test_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Ground plane
    commands.spawn((
        Name::new("Ground"),
        Mesh3d(meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(5.0)))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.5, 0.3),
            ..default()
        })),
    ));

    // Blue cube
    commands.spawn((
        Name::new("Blue Cube"),
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.3, 0.8),
            ..default()
        })),
        Transform::from_xyz(-1.5, 0.5, 0.0),
    ));

    // Red sphere
    commands.spawn((
        Name::new("Red Sphere"),
        Mesh3d(meshes.add(Sphere::new(0.5))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.2, 0.2),
            ..default()
        })),
        Transform::from_xyz(1.5, 0.5, 0.0),
    ));

    // Point light
    commands.spawn((
        Name::new("Point Light"),
        PointLight {
            shadows_enabled: true,
            intensity: 2_000_000.0,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    // Group with child cubes
    commands.spawn((
        Name::new("Group"),
        Transform::from_xyz(0.0, 0.0, -3.0),
        Visibility::default(),
        children![
            (
                Name::new("Child Cube A"),
                Mesh3d(meshes.add(Cuboid::new(0.5, 0.5, 0.5))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.9, 0.9, 0.2),
                    ..default()
                })),
                Transform::from_xyz(-1.0, 0.25, 0.0),
            ),
            (
                Name::new("Child Cube B"),
                Mesh3d(meshes.add(Cuboid::new(0.5, 0.5, 0.5))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.9, 0.5, 0.1),
                    ..default()
                })),
                Transform::from_xyz(1.0, 0.25, 0.0),
            ),
        ],
    ));
}
