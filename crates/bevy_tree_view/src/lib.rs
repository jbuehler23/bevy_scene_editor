mod headless;
mod feathers;

pub use headless::*;
pub use feathers::*;

use bevy::prelude::*;

pub struct TreeViewPlugin;

impl Plugin for TreeViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<TreeNodeActivated>()
            .add_systems(Update, (
                headless::tree_node_toggle_system,
                headless::tree_keyboard_nav_system,
            ));
    }
}
