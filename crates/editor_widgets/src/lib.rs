pub mod list_view;
pub mod split_panel;
pub mod text_input;
pub mod tree_view;

use bevy::app::{PluginGroup, PluginGroupBuilder};

pub struct EditorWidgetsPlugins;

impl PluginGroup for EditorWidgetsPlugins {
    fn build(self) -> bevy::app::PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(text_input::TextInputPlugin)
            .add(split_panel::SplitPanelPlugin)
            .add(tree_view::TreeViewPlugin)
            .add(list_view::ListViewPlugin)
    }
}
