use bevy::prelude::*;
use bevy_text_input::{feathers::text_input, *};

fn main() -> AppExit {
    App::new()
        .add_plugins((DefaultPlugins, TextInputPlugin))
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
        children![
            text_input(),
            (
                text_input(),
                TextInputPlaceholder::new("Enter text here... ")
            )
        ],
    ));
}
