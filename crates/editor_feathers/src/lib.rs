pub mod split_panel;
pub mod text_input;

use bevy::{app::Plugin, input_focus::InputDispatchPlugin};

pub struct EditorFeathersPlugin;

impl Plugin for EditorFeathersPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        if !app.is_plugin_added::<InputDispatchPlugin>() {
            app.add_plugins(InputDispatchPlugin);
        }
        app.add_plugins((
            editor_widgets::EditorWidgetsPlugins,
            split_panel::SplitPanelPlugin,
        ));
    }
}
