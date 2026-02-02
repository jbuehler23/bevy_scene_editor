mod headless;
mod feathers;

pub use headless::*;
pub use feathers::*;

use bevy::prelude::*;

pub struct SplitPanelPlugin;

impl Plugin for SplitPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            headless::split_panel_start_drag,
            headless::split_panel_stop_drag,
            headless::split_panel_drag_system,
        ));
    }
}
