use bevy::prelude::*;
use editor_feathers::{EditorFeathersPlugin, text_input::text_input};

fn main() -> AppExit {
    App::new()
        .add_plugins((DefaultPlugins, EditorFeathersPlugin))
        .add_systems(Startup, spawn_text_input)
        .run()
}

fn spawn_text_input(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(20),
            ..Default::default()
        },
        children![text_input(""), text_input("Enter text here...")],
    ));
}
