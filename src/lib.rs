pub mod inspector;
pub mod layout;

use bevy::{
    feathers::{FeathersPlugins, dark_theme::create_dark_theme, theme::UiTheme},
    prelude::*,
};
use editor_feathers::EditorFeathersPlugin;

#[derive(Component, Default)]
struct EditorEntity;

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            FeathersPlugins,
            EditorFeathersPlugin,
            inspector::InspectorPlugin,
        ))
        .insert_resource(UiTheme(create_dark_theme()))
        .add_systems(Startup, spawn_layout);
    }
}

fn spawn_layout(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn(layout::editor_layout());
}
