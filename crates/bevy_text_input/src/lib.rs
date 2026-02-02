mod headless;
mod feathers;

pub use headless::*;
pub use feathers::*;

use bevy::prelude::*;

pub struct TextInputPlugin;

impl Plugin for TextInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<TextInputChanged>()
            .add_systems(Update, (
                headless::text_input_focus_system,
                headless::text_input_keyboard_system,
            ));
    }
}
