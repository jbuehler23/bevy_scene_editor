use std::collections::BTreeMap;

use bevy::prelude::*;

pub struct CustomPropertiesPlugin;

impl Plugin for CustomPropertiesPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<CustomProperties>()
            .register_type::<PropertyValue>();
    }
}

// ---------------------------------------------------------------------------
// CustomProperties component
// ---------------------------------------------------------------------------

#[derive(Component, Reflect, Default, Clone, Debug)]
#[reflect(Component, Default)]
pub struct CustomProperties {
    pub properties: BTreeMap<String, PropertyValue>,
}

// ---------------------------------------------------------------------------
// PropertyValue enum
// ---------------------------------------------------------------------------

#[derive(Reflect, Clone, Debug, PartialEq)]
pub enum PropertyValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Vec2(Vec2),
    Vec3(Vec3),
    Color(Color),
}

impl PropertyValue {
    /// Human-readable type label for display in UI.
    pub fn type_label(&self) -> &'static str {
        match self {
            Self::Bool(_) => "Bool",
            Self::Int(_) => "Int",
            Self::Float(_) => "Float",
            Self::String(_) => "String",
            Self::Vec2(_) => "Vec2",
            Self::Vec3(_) => "Vec3",
            Self::Color(_) => "Color",
        }
    }

    /// Create a default value for a given type name.
    pub fn default_for_type(name: &str) -> Option<Self> {
        match name {
            "Bool" => Some(Self::Bool(false)),
            "Int" => Some(Self::Int(0)),
            "Float" => Some(Self::Float(0.0)),
            "String" => Some(Self::String(String::new())),
            "Vec2" => Some(Self::Vec2(Vec2::ZERO)),
            "Vec3" => Some(Self::Vec3(Vec3::ZERO)),
            "Color" => Some(Self::Color(Color::WHITE)),
            _ => None,
        }
    }

    /// All available type names for the UI picker.
    pub fn all_type_names() -> &'static [&'static str] {
        &["Bool", "Int", "Float", "String", "Vec2", "Vec3", "Color"]
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
