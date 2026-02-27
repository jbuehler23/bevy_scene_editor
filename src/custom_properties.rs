use bevy::prelude::*;

// Re-export types from jackdaw_jsn
pub use jackdaw_jsn::{CustomProperties, PropertyValue};

pub struct CustomPropertiesPlugin;

impl Plugin for CustomPropertiesPlugin {
    fn build(&self, _app: &mut App) {
        // Note: Type registration is handled by JsnPlugin
    }
}

// ---------------------------------------------------------------------------
// SetCustomProperties — undo command
// ---------------------------------------------------------------------------

/// Undo command that stores old/new snapshots of the entire CustomProperties component.
pub struct SetCustomProperties {
    pub entity: Entity,
    pub old_properties: CustomProperties,
    pub new_properties: CustomProperties,
}

impl crate::commands::EditorCommand for SetCustomProperties {
    fn execute(&self, world: &mut World) {
        if let Some(mut cp) = world.get_mut::<CustomProperties>(self.entity) {
            *cp = self.new_properties.clone();
        }
    }

    fn undo(&self, world: &mut World) {
        if let Some(mut cp) = world.get_mut::<CustomProperties>(self.entity) {
            *cp = self.old_properties.clone();
        }
    }

    fn description(&self) -> &str {
        "Set custom properties"
    }
}
