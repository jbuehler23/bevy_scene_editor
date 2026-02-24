mod brush_display;
mod component_display;
mod component_picker;
mod custom_props_display;
mod material_display;
mod reflect_fields;

use crate::EditorEntity;
use crate::selection::Selected;
use std::any::TypeId;

use bevy::prelude::*;

const MAX_REFLECT_DEPTH: usize = 4;

/// Extract a human-readable module group name from a module path.
/// e.g., "bevy_pbr::material" -> "Render", "bevy_transform" -> "Transform"
fn extract_module_group(module_path: Option<&str>) -> String {
    let Some(path) = module_path else {
        return "Other".to_string();
    };
    // Get first path segment
    let first = path.split("::").next().unwrap_or(path);
    // Strip "bevy_" prefix and capitalize
    let name = first.strip_prefix("bevy_").unwrap_or(first);
    // Map common module names to cleaner labels
    match name {
        "pbr" | "core_pipeline" => "Render".to_string(),
        "render" => "Render".to_string(),
        "transform" => "Transform".to_string(),
        "ecs" => "ECS".to_string(),
        "hierarchy" => "Hierarchy".to_string(),
        "window" | "winit" => "Window".to_string(),
        "input" | "picking" => "Input".to_string(),
        "asset" => "Asset".to_string(),
        "scene" => "Scene".to_string(),
        "gltf" => "GLTF".to_string(),
        "ui" => "UI".to_string(),
        "text" => "Text".to_string(),
        "audio" => "Audio".to_string(),
        "animation" => "Animation".to_string(),
        "sprite" => "Sprite".to_string(),
        _ => {
            // Capitalize first letter
            let mut chars = name.chars();
            match chars.next() {
                None => "Other".to_string(),
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            }
        }
    }
}

#[reflect_trait]
pub trait Displayable {
    fn display(&self, entity: &mut EntityCommands, source: Entity);
}

impl Displayable for Name {
    fn display(&self, entity: &mut EntityCommands, source: Entity) {
        entity
            .insert(editor_feathers::text_input::text_input("Name..."))
            .insert(editor_widgets::text_input::TextInput::new(self.to_string()))
            .observe(
                move |text: On<editor_widgets::text_input::EnteredText>,
                      mut names: Query<&mut Name>|
                      -> Result<(), BevyError> {
                    let mut name = names.get_mut(source)?;

                    *name = Name::new(text.value.clone());

                    Ok(())
                },
            );
    }
}

#[derive(Component)]
#[require(EditorEntity)]
pub struct Inspector;

pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.register_type_data::<Name, ReflectDisplayable>()
            .add_observer(component_display::remove_component_displays)
            .add_observer(component_display::add_component_displays)
            .add_observer(component_picker::on_add_component_button_click)
            .add_observer(reflect_fields::on_checkbox_commit)
            .add_observer(custom_props_display::on_custom_property_checkbox_commit)
            .add_observer(brush_display::handle_clear_texture)
            .add_systems(
                Update,
                (
                    reflect_fields::refresh_inspector_fields,
                    component_picker::filter_component_picker,
                    brush_display::update_brush_face_properties,
                ),
            );
    }
}

#[derive(Component)]
pub struct ComponentDisplay;

#[derive(Component)]
pub(super) struct ComponentDisplayBody;

#[derive(Component)]
pub(super) struct AddComponentButton;

/// Marker for the component picker panel
#[derive(Component)]
pub(super) struct ComponentPicker;

/// Marker for the search input in the component picker
#[derive(Component)]
pub(super) struct ComponentPickerSearch;

/// A selectable component entry in the picker list
#[derive(Component)]
pub(super) struct ComponentPickerEntry {
    pub(super) short_name: String,
}

/// Tracks which inspector field entity maps to which source entity + component + field path.
#[derive(Component)]
pub(super) struct FieldBinding {
    pub(super) source_entity: Entity,
    pub(super) component_type_id: TypeId,
    pub(super) field_path: String,
}

/// Container for brush face properties (texture, UV, etc). Populated dynamically.
#[derive(Component)]
pub(super) struct BrushFacePropsContainer;

/// Binding for a brush face UV field.
#[derive(Component)]
#[allow(dead_code)]
pub(super) struct BrushFaceFieldBinding {
    pub(super) field: BrushFaceField,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BrushFaceField {
    UvOffsetX,
    UvOffsetY,
    UvScaleX,
    UvScaleY,
    UvRotation,
}

/// Tracks a custom property field binding (property name + source entity).
#[derive(Component)]
pub(super) struct CustomPropertyBinding {
    pub(super) source_entity: Entity,
    pub(super) property_name: String,
}

/// Marker for the "Add Property" row container.
#[derive(Component)]
pub(super) struct CustomPropertyAddRow;

/// Marker for the type selector ComboBox in the "Add Property" row.
#[derive(Component)]
pub(super) struct CustomPropertyTypeSelector;

/// Marker for the property name text input in the "Add Property" row.
#[derive(Component)]
pub(super) struct CustomPropertyNameInput;

// --- Axis colors for Vec3/Vec2 fields ---
pub(super) const AXIS_X_COLOR: Color = Color::srgb(0.8, 0.3, 0.3);
pub(super) const AXIS_Y_COLOR: Color = Color::srgb(0.3, 0.7, 0.3);
pub(super) const AXIS_Z_COLOR: Color = Color::srgb(0.3, 0.5, 0.8);

/// Force inspector rebuild by toggling Selected.
pub(super) fn rebuild_inspector(world: &mut World, source_entity: Entity) {
    if let Ok(mut ec) = world.get_entity_mut(source_entity) {
        ec.remove::<Selected>();
    }
    world.entity_mut(source_entity).insert(Selected);
}
