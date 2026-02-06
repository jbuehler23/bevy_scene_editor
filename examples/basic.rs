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

fn spawn_scene(mut commands: Commands) {
    commands.spawn((
        Name::new("Car"),
        Car,
        children![
            Name::new("Bonnet"),
            Name::new("Front Left Wheel"),
            Name::new("Front Right Wheel"),
            Name::new("Back Left Wheel"),
            Name::new("Back Right Wheel"),
        ],
    ));
}

fn select_car(mut commands: Commands, car: Single<Entity, With<Car>>) {
    println!("Selecting");
    commands.entity(*car).insert(SelectedEntity);
}
