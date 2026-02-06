use bevy::{input::common_conditions::input_just_pressed, prelude::*};
use bevy_scene_editor::{EditorPlugin, inspector::SelectedEntity};

#[derive(Component)]
pub struct Car;

fn main() -> AppExit {
    App::new()
        .add_plugins((DefaultPlugins, EditorPlugin))
        .add_systems(Startup, spawn_scene)
        .add_systems(
            Update,
            select_car.run_if(input_just_pressed(KeyCode::Space).and(run_once)),
        )
        .run()
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Car body
    let body_mesh = meshes.add(Cuboid::new(2.0, 0.6, 4.0));
    let body_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.8, 0.15, 0.15),
        ..default()
    });

    // Wheel mesh + material (shared)
    let wheel_mesh = meshes.add(Cylinder::new(0.4, 0.2));
    let wheel_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.15, 0.15),
        ..default()
    });

    commands.spawn((
        Name::new("Car"),
        Car,
        Mesh3d(body_mesh),
        MeshMaterial3d(body_material),
        Transform::from_xyz(0.0, 0.7, 0.0),
        children![
            (
                Name::new("Front Left Wheel"),
                Mesh3d(wheel_mesh.clone()),
                MeshMaterial3d(wheel_material.clone()),
                Transform::from_xyz(-1.1, -0.3, 1.2)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
            ),
            (
                Name::new("Front Right Wheel"),
                Mesh3d(wheel_mesh.clone()),
                MeshMaterial3d(wheel_material.clone()),
                Transform::from_xyz(1.1, -0.3, 1.2)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
            ),
            (
                Name::new("Back Left Wheel"),
                Mesh3d(wheel_mesh.clone()),
                MeshMaterial3d(wheel_material.clone()),
                Transform::from_xyz(-1.1, -0.3, -1.2)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
            ),
            (
                Name::new("Back Right Wheel"),
                Mesh3d(wheel_mesh),
                MeshMaterial3d(wheel_material),
                Transform::from_xyz(1.1, -0.3, -1.2)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
            ),
        ],
    ));

    // Directional light with shadows
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            shadows_enabled: true,
            illuminance: 10000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, 0.4, 0.0)),
    ));
}

fn select_car(mut commands: Commands, car: Single<Entity, With<Car>>) {
    println!("Selecting");
    commands.entity(*car).insert(SelectedEntity);
}
