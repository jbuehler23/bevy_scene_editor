use bevy::prelude::*;

pub struct ViewModesPlugin;

impl Plugin for ViewModesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewModeSettings>()
            .add_systems(Update, toggle_wireframe_key);
    }
}

#[derive(Resource, Default)]
pub struct ViewModeSettings {
    pub wireframe: bool,
}

fn toggle_wireframe_key(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<ViewModeSettings>,
) {
    // Ctrl+Shift+W toggles wireframe
    if keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
        && keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
        && keyboard.just_pressed(KeyCode::KeyW)
    {
        settings.wireframe = !settings.wireframe;
        if settings.wireframe {
            info!("Wireframe mode ON");
        } else {
            info!("Wireframe mode OFF");
        }
    }
}
