pub mod feathers;
pub mod headless;

use bevy::prelude::*;
pub use headless::*;

pub struct TextInputPlugin;

impl Plugin for TextInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(headless::text_input_focus)
            .add_systems(Update, headless::text_input_keyboard_system);
    }
}
