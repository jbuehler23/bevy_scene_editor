use crate::EditorEntity;
use crate::brush::{
    Brush, BrushEditMode, BrushFaceData, BrushSelection, EditMode, SetBrush,
    TextureMaterialCache,
};
use crate::commands::{CommandGroup, CommandHistory, EditorCommand, SetComponentField};
use crate::custom_properties::{CustomProperties, PropertyValue, SetCustomProperties};
use crate::selection::{Selected, Selection};
use crate::texture_browser::ClearTextureFromFaces;
use std::any::TypeId;
use std::collections::HashSet;

use bevy::{
    ecs::{
        archetype::Archetype,
        component::{ComponentId, Components},
        reflect::{AppTypeRegistry, ReflectComponent},
    },
    feathers::{
        controls::{ButtonProps, button},
        theme::ThemedText,
    },
    input_focus::InputFocus,
    prelude::*,
    reflect::{DynamicEnum, DynamicStruct, DynamicTuple, DynamicVariant, ReflectRef},
    ui_widgets::observe,
};
use editor_feathers::{
    checkbox::{CheckboxCommitEvent, CheckboxProps, CheckboxState, checkbox},
    color_picker::{ColorPickerCommitEvent, ColorPickerProps, color_picker},
    combobox::{ComboBoxChangeEvent, ComboBoxSelectedIndex, combobox_with_selected},
    icons::{EditorFont, Icon, IconFont},
    list_view, numeric_input, text_input, tokens,
};
use editor_widgets::collapsible::{
    CollapsibleBody, CollapsibleHeader, CollapsibleSection, ToggleCollapsible,
};
use editor_widgets::numeric_input::{NumericInput, NumericValueChanged};
use editor_widgets::text_input::{EnteredText, TextInput};

const MAX_REFLECT_DEPTH: usize = 4;

/// Extract a human-readable module group name from a module path.
/// e.g., "bevy_pbr::material" → "Render", "bevy_transform" → "Transform"
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
            .insert(text_input::text_input("Name..."))
            .insert(TextInput::new(self.to_string()))
            .observe(
                move |text: On<EnteredText>,
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
            .add_observer(remove_component_displays)
            .add_observer(add_component_displays)
            .add_observer(on_add_component_button_click)
            .add_observer(on_checkbox_commit)
            .add_observer(on_custom_property_checkbox_commit)
            .add_observer(handle_clear_texture)
            .add_systems(
                Update,
                (
                    refresh_inspector_fields,
                    filter_component_picker,
                    update_brush_face_properties,
                ),
            );
    }
}

#[derive(Component)]
pub struct ComponentDisplay;

#[derive(Component)]
struct ComponentDisplayBody;

#[derive(Component)]
struct AddComponentButton;

/// Marker for the component picker panel
#[derive(Component)]
struct ComponentPicker;

/// Marker for the search input in the component picker
#[derive(Component)]
struct ComponentPickerSearch;

/// A selectable component entry in the picker list
#[derive(Component)]
struct ComponentPickerEntry {
    short_name: String,
}

/// Tracks which inspector field entity maps to which source entity + component + field path.
#[derive(Component)]
struct FieldBinding {
    source_entity: Entity,
    component_type_id: TypeId,
    field_path: String,
}

/// Container for brush face properties (texture, UV, etc). Populated dynamically.
#[derive(Component)]
struct BrushFacePropsContainer;

/// Binding for a brush face UV field.
#[derive(Component)]
#[allow(dead_code)]
struct BrushFaceFieldBinding {
    field: BrushFaceField,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BrushFaceField {
    UvOffsetX,
    UvOffsetY,
    UvScaleX,
    UvScaleY,
    UvRotation,
}

/// Tracks a custom property field binding (property name + source entity).
#[derive(Component)]
struct CustomPropertyBinding {
    source_entity: Entity,
    property_name: String,
}

/// Marker for the "Add Property" row container.
#[derive(Component)]
struct CustomPropertyAddRow;

/// Marker for the type selector ComboBox in the "Add Property" row.
#[derive(Component)]
struct CustomPropertyTypeSelector;

/// Marker for the property name text input in the "Add Property" row.
#[derive(Component)]
struct CustomPropertyNameInput;


fn add_component_displays(
    _: On<Add, Selected>,
    mut commands: Commands,
    components: &Components,
    type_registry: Res<AppTypeRegistry>,
    selection: Res<Selection>,
    entity_query: Query<(&Archetype, EntityRef), (With<Selected>, Without<EditorEntity>)>,
    inspector: Single<Entity, With<Inspector>>,
    names: Query<&Name>,
    icon_font: Res<IconFont>,
    editor_font: Res<EditorFont>,
) {
    // Show inspector for the primary selected entity
    let Some(primary) = selection.primary() else {
        return;
    };
    let Ok((archetype, entity_ref)) = entity_query.get(primary) else {
        return;
    };

    // First, clear existing displays
    // (the remove observer handles this when Selected is removed, but for multi-select
    //  we also need to rebuild when primary changes)

    let source_entity = entity_ref.entity();

    // Show multi-selection header when multiple entities are selected
    let sel_count = selection.entities.len();
    if sel_count > 1 {
        commands.spawn((
            ComponentDisplay,
            Node {
                padding: UiRect::axes(
                    Val::Px(tokens::SPACING_MD),
                    Val::Px(tokens::SPACING_SM),
                ),
                width: Val::Percent(100.0),
                ..Default::default()
            },
            BackgroundColor(tokens::SELECTED_BG),
            ChildOf(*inspector),
            children![(
                Text::new(format!("{sel_count} entities selected — edits apply to all")),
                TextFont {
                    font: editor_font.0.clone(),
                    font_size: tokens::FONT_SM,
                    ..Default::default()
                },
                TextColor(tokens::TEXT_PRIMARY),
            )],
        ));
    }

    let registry = type_registry.read();

    // (short_name, module_group, component_id)
    let mut comp_list: Vec<(String, String, ComponentId)> = archetype
        .iter_components()
        .filter_map(|component_id| {
            let info = components.get_info(component_id)?;
            let type_id = info.type_id();

            // Try TypeRegistry first for proper names
            if let Some(type_id) = type_id
                && let Some(registration) = registry.get(type_id)
            {
                let table = registration.type_info().type_path_table();
                let full_path = table.path();
                if full_path.starts_with("bevy_scene_editor") {
                    return None;
                }
                let short = table.short_path().to_string();
                let module_group = extract_module_group(table.module_path());
                return Some((short, module_group, component_id));
            }

            // Fallback: use Components name
            let name = components.get_name(component_id)?;
            if name.starts_with("bevy_scene_editor") {
                return None;
            }
            Some((name.shortname().to_string(), "Other".to_string(), component_id))
        })
        .collect();

    // Sort by (group, name)
    comp_list.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));

    // Group by module and spawn with group headers
    let mut current_group = String::new();
    let mut group_container = *inspector;

    for (name, module_group, component_id) in &comp_list {
        // Start a new group section if the module changed
        if *module_group != current_group {
            current_group = module_group.clone();
            let section = commands
                .spawn((
                    ComponentDisplay,
                    CollapsibleSection { collapsed: false },
                    Node {
                        flex_direction: FlexDirection::Column,
                        width: Val::Percent(100.0),
                        ..Default::default()
                    },
                    ChildOf(*inspector),
                ))
                .id();

            let header = commands
                .spawn((
                    CollapsibleHeader,
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(
                            Val::Px(tokens::SPACING_SM),
                            Val::Px(tokens::SPACING_XS),
                        ),
                        column_gap: Val::Px(tokens::SPACING_SM),
                        ..Default::default()
                    },
                    BackgroundColor(tokens::PANEL_BG),
                    ChildOf(section),
                ))
                .id();

            let section_for_toggle = section;
            commands.entity(header).observe(
                move |_: On<Pointer<Click>>, mut commands: Commands| {
                    commands.trigger(ToggleCollapsible {
                        entity: section_for_toggle,
                    });
                },
            );

            // Group name
            commands.spawn((
                Text::new(module_group.clone()),
                TextFont {
                    font: editor_font.0.clone(),
                    font_size: tokens::FONT_SM,
                    weight: FontWeight::SEMIBOLD,
                    ..Default::default()
                },
                TextColor(tokens::TEXT_SECONDARY),
                ChildOf(header),
            ));

            group_container = commands
                .spawn((
                    CollapsibleBody,
                    Node {
                        flex_direction: FlexDirection::Column,
                        width: Val::Percent(100.0),
                        ..Default::default()
                    },
                    ChildOf(section),
                ))
                .id();
        }

        let component_id = *component_id;
        let (display_entity, body_entity) =
            spawn_component_display(&mut commands, name, source_entity, component_id, &icon_font.0, &editor_font.0);
        commands
            .entity(display_entity)
            .insert(ChildOf(group_container));

        // Try Displayable first, then reflection, then fallback
        let type_id = components
            .get_info(component_id)
            .and_then(|info| info.type_id());

        if let Some(type_id) = type_id
            && let Some(registration) = registry.get(type_id)
            && let Some(reflect_component) = registration.data::<ReflectComponent>()
            && let Some(reflected) = reflect_component.reflect(entity_ref)
        {
            // Priority 1: Displayable trait override
            if let Some(reflect_displayable) = registration.data::<ReflectDisplayable>()
                && let Some(displayable) = reflect_displayable.get(reflected)
            {
                let mut body_commands = commands.entity(body_entity);
                displayable.display(&mut body_commands, source_entity);
                continue;
            }

            // Priority 2: MeshMaterial3d<StandardMaterial> — display material fields
            if type_id == TypeId::of::<MeshMaterial3d<StandardMaterial>>() {
                spawn_material_display_deferred(
                    &mut commands,
                    body_entity,
                    source_entity,
                );
                continue;
            }

            // Priority 3: CustomProperties — specialized property editor
            if type_id == TypeId::of::<CustomProperties>() {
                if let Some(cp) = reflected.downcast_ref::<CustomProperties>() {
                    spawn_custom_properties_display(
                        &mut commands,
                        body_entity,
                        source_entity,
                        cp,
                        &editor_font.0,
                        &icon_font.0,
                    );
                }
                continue;
            }

            // Priority 3b: Brush — show face/vertex info
            if type_id == TypeId::of::<crate::brush::Brush>() {
                if let Some(brush) = reflected.downcast_ref::<crate::brush::Brush>() {
                    spawn_brush_display(
                        &mut commands,
                        body_entity,
                        brush,
                    );
                }
                continue;
            }

            // Priority 3: Generic reflection display
            spawn_reflected_fields(
                &mut commands,
                body_entity,
                reflected,
                0,
                String::new(),
                source_entity,
                type_id,
                &names,
                &type_registry,
                &editor_font.0,
                &icon_font.0,
            );
            continue;
        }

        // Fallback: no reflection data
        commands.spawn((
            Text::new("(read-only)"),
            TextFont {
                font_size: tokens::FONT_SM,
                ..Default::default()
            },
            TextColor(tokens::TEXT_SECONDARY),
            ChildOf(body_entity),
        ));
    }

    commands.spawn((
        AddComponentButton,
        button(ButtonProps::default(), (), Spawn(Text::new("+"))),
        ChildOf(*inspector),
    ));

}

fn remove_component_displays(
    _: On<Remove, Selected>,
    mut commands: Commands,
    inspector: Single<(Entity, Option<&Children>), With<Inspector>>,
    displays: Query<
        Entity,
        Or<(
            With<ComponentDisplay>,
            With<AddComponentButton>,
            With<ComponentPicker>,
        )>,
    >,
) {
    let (_entity, children) = inspector.into_inner();

    let Some(children) = children else {
        return;
    };

    for child in displays.iter_many(children.collection()) {
        if let Ok(mut ec) = commands.get_entity(child) {
            ec.despawn();
        }
    }
}

// ---------------------------------------------------------------------------
// Add Component picker
// ---------------------------------------------------------------------------

/// Handle click on the "+" button to open the component picker.
fn on_add_component_button_click(
    event: On<editor_feathers::button::ButtonClickEvent>,
    add_buttons: Query<&ChildOf, With<AddComponentButton>>,
    existing_pickers: Query<Entity, With<ComponentPicker>>,
    mut commands: Commands,
    selection: Res<Selection>,
    type_registry: Res<AppTypeRegistry>,
    components: &Components,
    entity_query: Query<&Archetype, (With<Selected>, Without<EditorEntity>)>,
    inspector: Single<Entity, With<Inspector>>,
    mut input_focus: ResMut<InputFocus>,
) {
    // Check if this click is on an AddComponentButton
    if add_buttons.get(event.entity).is_err() {
        return;
    }

    // Toggle: if picker already open, close it
    for picker in &existing_pickers {
        commands.entity(picker).despawn();
        return;
    }

    let Some(primary) = selection.primary() else {
        return;
    };
    let Ok(archetype) = entity_query.get(primary) else {
        return;
    };

    // Collect existing component TypeIds on the entity
    let existing_types: HashSet<TypeId> = archetype
        .iter_components()
        .filter_map(|cid| components.get_info(cid).and_then(|info| info.type_id()))
        .collect();

    let registry = type_registry.read();

    // Collect all registered components that have ReflectComponent + ReflectDefault
    let mut available: Vec<(String, String, TypeId, ComponentId)> = Vec::new();
    for registration in registry.iter() {
        let type_id = registration.type_id();

        // Must have ReflectComponent and ReflectDefault
        if registration.data::<ReflectComponent>().is_none()
            || registration.data::<ReflectDefault>().is_none()
        {
            continue;
        }

        // Skip components already on the entity
        if existing_types.contains(&type_id) {
            continue;
        }

        // Skip editor-internal types
        let table = registration.type_info().type_path_table();
        let full_path = table.path();
        if full_path.starts_with("bevy_scene_editor") {
            continue;
        }

        // Get component ID
        let Some(component_id) = components.get_id(type_id) else {
            continue;
        };

        let short_name = table.short_path().to_string();
        let module = table
            .module_path()
            .unwrap_or("")
            .to_string();

        available.push((short_name, module, type_id, component_id));
    }

    available.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

    // Spawn the picker panel
    let picker = commands
        .spawn((
            ComponentPicker,
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Percent(100.0),
                max_height: Val::Px(300.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(tokens::BORDER_RADIUS_MD)),
                ..Default::default()
            },
            BackgroundColor(tokens::PANEL_BG),
            BorderColor::all(tokens::BORDER_SUBTLE),
            ChildOf(*inspector),
        ))
        .id();

    // Search input
    let search_entity = commands
        .spawn((
            ComponentPickerSearch,
            text_input::text_input("Search components..."),
            ChildOf(picker),
        ))
        .id();
    input_focus.set(search_entity);

    // Scrollable list
    let list = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..Default::default()
            },
            ChildOf(picker),
        ))
        .id();

    let source_entity = primary;
    for (short_name, _module, type_id, component_id) in available {
        let label = short_name.clone();
        commands
            .spawn((
                ComponentPickerEntry {
                    short_name: short_name.clone(),
                },
                Node {
                    padding: UiRect::axes(
                        Val::Px(tokens::SPACING_LG),
                        Val::Px(tokens::SPACING_SM),
                    ),
                    width: Val::Percent(100.0),
                    ..Default::default()
                },
                BackgroundColor(Color::NONE),
                ChildOf(list),
                observe(
                    move |_: On<Pointer<Click>>, mut commands: Commands| {
                        // Insert the component and close the picker
                        commands.queue(move |world: &mut World| {
                            let cmd = crate::commands::AddComponent {
                                entity: source_entity,
                                type_id,
                                component_id,
                            };
                            let cmd = Box::new(cmd);
                            cmd.execute(world);
                            let mut history =
                                world.resource_mut::<crate::commands::CommandHistory>();
                            history.undo_stack.push(cmd);
                            history.redo_stack.clear();

                            // Force refresh: toggle Selected to rebuild inspector
                            if let Ok(mut entity) = world.get_entity_mut(source_entity) {
                                entity.remove::<Selected>();
                            }
                            world.entity_mut(source_entity).insert(Selected);
                        });
                    },
                ),
                observe(
                    move |hover: On<Pointer<Over>>, mut bg: Query<&mut BackgroundColor>| {
                        if let Ok(mut bg) = bg.get_mut(hover.event_target()) {
                            bg.0 = tokens::HOVER_BG;
                        }
                    },
                ),
                observe(
                    move |out: On<Pointer<Out>>, mut bg: Query<&mut BackgroundColor>| {
                        if let Ok(mut bg) = bg.get_mut(out.event_target()) {
                            bg.0 = Color::NONE;
                        }
                    },
                ),
            ))
            .with_child((
                Text::new(label),
                TextFont {
                    font_size: tokens::FONT_MD,
                    ..Default::default()
                },
                ThemedText,
            ));
    }
}

/// Filter the component picker list based on search input.
fn filter_component_picker(
    search_query: Query<&TextInput, (With<ComponentPickerSearch>, Changed<TextInput>)>,
    entries: Query<(Entity, &ComponentPickerEntry)>,
    mut node_query: Query<&mut Node>,
) {
    let Ok(search) = search_query.single() else {
        return;
    };
    let filter = search.value.trim().to_lowercase();

    for (entity, entry) in &entries {
        if let Ok(mut node) = node_query.get_mut(entity) {
            node.display = if filter.is_empty()
                || entry.short_name.to_lowercase().contains(&filter)
            {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
}

fn spawn_component_display(
    commands: &mut Commands,
    name: &str,
    entity: Entity,
    component: ComponentId,
    icon_font: &Handle<Font>,
    editor_font: &Handle<Font>,
) -> (Entity, Entity) {
    let font = icon_font.clone();
    let body_font = editor_font.clone();

    let body_entity = commands
        .spawn((
            ComponentDisplayBody,
            CollapsibleBody,
            Node {
                padding: UiRect::new(
                    Val::Px(tokens::SPACING_MD),
                    Val::Px(tokens::SPACING_SM),
                    Val::Px(tokens::SPACING_XS),
                    Val::Px(tokens::SPACING_XS),
                ),
                flex_direction: FlexDirection::Column,
                width: Val::Percent(100.0),
                ..Default::default()
            },
        ))
        .id();

    let section_entity = commands
        .spawn((
            ComponentDisplay,
            CollapsibleSection { collapsed: false },
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Percent(100.0),
                ..Default::default()
            },
        ))
        .id();

    // Header
    let header = commands
        .spawn((
            CollapsibleHeader,
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                width: Val::Percent(100.0),
                padding: UiRect::axes(
                    Val::Px(tokens::SPACING_SM),
                    Val::Px(tokens::SPACING_XS),
                ),
                column_gap: Val::Px(tokens::SPACING_SM),
                ..Default::default()
            },
            BackgroundColor(tokens::PANEL_HEADER_BG),
            ChildOf(section_entity),
        ))
        .id();

    // Toggle area (chevron + title) — click to collapse/expand
    let toggle_area = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(tokens::SPACING_SM),
                flex_grow: 1.0,
                ..Default::default()
            },
            ChildOf(header),
        ))
        .id();

    // Chevron icon
    commands.spawn((
        Text::new(String::from(Icon::ChevronDown.unicode())),
        TextFont {
            font: font.clone(),
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(tokens::TEXT_SECONDARY),
        ChildOf(toggle_area),
    ));

    // Component name
    commands.spawn((
        Text::new(name.to_string()),
        TextFont {
            font: body_font,
            font_size: tokens::FONT_MD,
            weight: FontWeight::SEMIBOLD,
            ..Default::default()
        },
        TextColor(tokens::TEXT_DISPLAY_COLOR.into()),
        ChildOf(toggle_area),
    ));

    // Toggle on click (on toggle area, not on the X button)
    let section = section_entity;
    commands.entity(toggle_area).observe(
        move |_: On<Pointer<Click>>, mut commands: Commands| {
            commands.trigger(ToggleCollapsible { entity: section });
        },
    );

    // Remove component button (X icon)
    commands.spawn((
        Text::new(String::from(Icon::X.unicode())),
        TextFont {
            font,
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(tokens::TEXT_SECONDARY),
        ChildOf(header),
        observe(move |_: On<Pointer<Click>>, mut commands: Commands| {
            commands.entity(entity).remove_by_id(component);
        }),
    ));

    // Hover effect on header
    commands.entity(header).observe(
        |hover: On<Pointer<Over>>,
         mut bg: Query<&mut BackgroundColor, With<CollapsibleHeader>>| {
            if let Ok(mut bg) = bg.get_mut(hover.event_target()) {
                bg.0 = tokens::HOVER_BG;
            }
        },
    );
    commands.entity(header).observe(
        |out: On<Pointer<Out>>,
         mut bg: Query<&mut BackgroundColor, With<CollapsibleHeader>>| {
            if let Ok(mut bg) = bg.get_mut(out.event_target()) {
                bg.0 = tokens::PANEL_HEADER_BG;
            }
        },
    );

    // Attach body to section
    commands.entity(body_entity).insert(ChildOf(section_entity));

    (section_entity, body_entity)
}

fn spawn_reflected_fields(
    commands: &mut Commands,
    parent: Entity,
    reflected: &dyn Reflect,
    depth: usize,
    base_path: String,
    source_entity: Entity,
    component_type_id: TypeId,
    entity_names: &Query<&Name>,
    type_registry: &AppTypeRegistry,
    editor_font: &Handle<Font>,
    icon_font: &Handle<Font>,
) {
    match reflected.reflect_ref() {
        ReflectRef::Struct(s) => {
            for i in 0..s.field_len() {
                let Some(name) = s.name_at(i) else {
                    continue;
                };
                let Some(value) = s.field_at(i) else {
                    continue;
                };
                let child_path = if base_path.is_empty() {
                    name.to_string()
                } else {
                    format!("{base_path}.{name}")
                };
                spawn_field_row(
                    commands,
                    parent,
                    name,
                    value,
                    depth,
                    child_path,
                    source_entity,
                    component_type_id,
                    entity_names,
                    type_registry,
                    editor_font,
                    icon_font,
                );
            }
        }
        ReflectRef::TupleStruct(ts) => {
            for i in 0..ts.field_len() {
                let Some(value) = ts.field(i) else {
                    continue;
                };
                let child_path = if base_path.is_empty() {
                    format!(".{i}")
                } else {
                    format!("{base_path}.{i}")
                };
                spawn_field_row(
                    commands,
                    parent,
                    &format!("{i}"),
                    value,
                    depth,
                    child_path,
                    source_entity,
                    component_type_id,
                    entity_names,
                    type_registry,
                    editor_font,
                    icon_font,
                );
            }
        }
        ReflectRef::Enum(e) => {
            spawn_enum_field(
                commands,
                parent,
                e,
                depth,
                base_path,
                source_entity,
                component_type_id,
                entity_names,
                type_registry,
                editor_font,
                icon_font,
            );
        }
        ReflectRef::List(list) => {
            spawn_list_expansion(
                commands,
                parent,
                list.len(),
                |i| list.get(i),
                depth,
                &base_path,
                source_entity,
                component_type_id,
                entity_names,
            );
        }
        ReflectRef::Array(array) => {
            spawn_list_expansion(
                commands,
                parent,
                array.len(),
                |i| array.get(i),
                depth,
                &base_path,
                source_entity,
                component_type_id,
                entity_names,
            );
        }
        ReflectRef::Map(map) => {
            spawn_text_row(commands, parent, &format!("{{ {} entries }}", map.len()), depth);
            if !map.is_empty() {
                let lv = commands.spawn((list_view::list_view(), ChildOf(parent))).id();
                for (i, (key, val)) in map.iter().enumerate() {
                    let item_entity = commands.spawn((list_view::list_item(i), ChildOf(lv))).id();
                    let key_label = format_partial_reflect_value(key);
                    let child_path = if base_path.is_empty() {
                        format!("[{key_label}]")
                    } else {
                        format!("{base_path}[{key_label}]")
                    };
                    spawn_field_row(
                        commands,
                        item_entity,
                        &key_label,
                        val,
                        depth + 1,
                        child_path,
                        source_entity,
                        component_type_id,
                        entity_names,
                        type_registry,
                        editor_font,
                        icon_font,
                    );
                }
            }
        }
        ReflectRef::Set(set) => {
            spawn_text_row(commands, parent, &format!("{{ {} items }}", set.len()), depth);
            if !set.is_empty() {
                let lv = commands.spawn((list_view::list_view(), ChildOf(parent))).id();
                for (i, item) in set.iter().enumerate() {
                    let item_entity = commands.spawn((list_view::list_item(i), ChildOf(lv))).id();
                    spawn_text_row(commands, item_entity, &format_partial_reflect_value(item), depth + 1);
                }
            }
        }
        ReflectRef::Tuple(tuple) => {
            for i in 0..tuple.field_len() {
                let Some(value) = tuple.field(i) else {
                    continue;
                };
                let child_path = if base_path.is_empty() {
                    format!(".{i}")
                } else {
                    format!("{base_path}.{i}")
                };
                spawn_field_row(
                    commands,
                    parent,
                    &format!("{i}"),
                    value,
                    depth,
                    child_path,
                    source_entity,
                    component_type_id,
                    entity_names,
                    type_registry,
                    editor_font,
                    icon_font,
                );
            }
        }
        ReflectRef::Opaque(_) => {
            let label = reflected
                .get_represented_type_info()
                .map(|info| {
                    let path = info.type_path_table().short_path();
                    format!("<{path}>")
                })
                .unwrap_or_else(|| "(opaque)".to_string());
            spawn_text_row(commands, parent, &label, depth);
        }
    }
}

fn is_editable_primitive(value: &dyn PartialReflect) -> bool {
    value.try_downcast_ref::<f32>().is_some()
        || value.try_downcast_ref::<f64>().is_some()
        || value.try_downcast_ref::<i32>().is_some()
        || value.try_downcast_ref::<u32>().is_some()
        || value.try_downcast_ref::<usize>().is_some()
        || value.try_downcast_ref::<i8>().is_some()
        || value.try_downcast_ref::<i16>().is_some()
        || value.try_downcast_ref::<i64>().is_some()
        || value.try_downcast_ref::<u8>().is_some()
        || value.try_downcast_ref::<u16>().is_some()
        || value.try_downcast_ref::<u64>().is_some()
        || value.try_downcast_ref::<bool>().is_some()
        || value.try_downcast_ref::<String>().is_some()
}

fn spawn_field_row(
    commands: &mut Commands,
    parent: Entity,
    name: &str,
    value: &dyn PartialReflect,
    depth: usize,
    field_path: String,
    source_entity: Entity,
    component_type_id: TypeId,
    entity_names: &Query<&Name>,
    type_registry: &AppTypeRegistry,
    editor_font: &Handle<Font>,
    icon_font: &Handle<Font>,
) {
    // Entity reference -> clickable link (before any other check)
    if let Some(&entity_val) = value.try_downcast_ref::<Entity>() {
        let left_padding = depth as f32 * tokens::SPACING_MD;
        let row = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(tokens::SPACING_XS),
                    padding: UiRect::left(px(left_padding)),
                    ..Default::default()
                },
                ChildOf(parent),
            ))
            .id();
        commands.spawn((
            Text::new(format!("{name}:")),
            TextFont {
                font_size: tokens::FONT_SM,
                ..Default::default()
            },
            Node {
                min_width: px(20.0),
                flex_shrink: 0.0,
                ..Default::default()
            },
            TextColor(tokens::TYPE_ENTITY),
            ChildOf(row),
        ));
        let label = entity_names
            .get(entity_val)
            .map(|n| format!("{} ({entity_val})", n.as_str()))
            .unwrap_or_else(|_| format!("{entity_val}"));
        spawn_entity_link(commands, row, entity_val, &label);
        return;
    }

    // List/Array -> expand with ListView
    if let ReflectRef::List(list) = value.reflect_ref() {
        spawn_text_row(commands, parent, &format!("{name}: [{} items]", list.len()), depth);
        if !list.is_empty() {
            let lv = commands.spawn((list_view::list_view(), ChildOf(parent))).id();
            for i in 0..list.len() {
                if let Some(item) = list.get(i) {
                    let item_entity = commands.spawn((list_view::list_item(i), ChildOf(lv))).id();
                    let child_path = if field_path.is_empty() {
                        format!("[{i}]")
                    } else {
                        format!("{field_path}[{i}]")
                    };
                    spawn_list_item_value(
                        commands,
                        item_entity,
                        item,
                        depth + 1,
                        child_path,
                        source_entity,
                        component_type_id,
                        entity_names,
                    );
                }
            }
        }
        return;
    }
    if let ReflectRef::Array(array) = value.reflect_ref() {
        spawn_text_row(commands, parent, &format!("{name}: [{} items]", array.len()), depth);
        if !array.is_empty() {
            let lv = commands.spawn((list_view::list_view(), ChildOf(parent))).id();
            for i in 0..array.len() {
                if let Some(item) = array.get(i) {
                    let item_entity = commands.spawn((list_view::list_item(i), ChildOf(lv))).id();
                    let child_path = if field_path.is_empty() {
                        format!("[{i}]")
                    } else {
                        format!("{field_path}[{i}]")
                    };
                    spawn_list_item_value(
                        commands,
                        item_entity,
                        item,
                        depth + 1,
                        child_path,
                        source_entity,
                        component_type_id,
                        entity_names,
                    );
                }
            }
        }
        return;
    }
    if let ReflectRef::Map(map) = value.reflect_ref() {
        spawn_text_row(commands, parent, &format!("{name}: {{ {} entries }}", map.len()), depth);
        if !map.is_empty() {
            let lv = commands.spawn((list_view::list_view(), ChildOf(parent))).id();
            for (i, (key, val)) in map.iter().enumerate() {
                let item_entity = commands.spawn((list_view::list_item(i), ChildOf(lv))).id();
                let key_label = format_partial_reflect_value(key);
                let child_path = if field_path.is_empty() {
                    format!("[{key_label}]")
                } else {
                    format!("{field_path}[{key_label}]")
                };
                spawn_field_row(
                    commands,
                    item_entity,
                    &key_label,
                    val,
                    depth + 1,
                    child_path,
                    source_entity,
                    component_type_id,
                    entity_names,
                    type_registry,
                    editor_font,
                    icon_font,
                );
            }
        }
        return;
    }
    if let ReflectRef::Set(set) = value.reflect_ref() {
        spawn_text_row(commands, parent, &format!("{name}: {{ {} items }}", set.len()), depth);
        if !set.is_empty() {
            let lv = commands.spawn((list_view::list_view(), ChildOf(parent))).id();
            for (i, item) in set.iter().enumerate() {
                let item_entity = commands.spawn((list_view::list_item(i), ChildOf(lv))).id();
                spawn_text_row(commands, item_entity, &format_partial_reflect_value(item), depth + 1);
            }
        }
        return;
    }

    // Vec3 compact row with colored XYZ labels
    if let Some(vec3) = value.try_downcast_ref::<Vec3>() {
        spawn_vec3_row(
            commands,
            parent,
            name,
            vec3,
            field_path,
            source_entity,
            component_type_id,
            depth,
        );
        return;
    }

    // Vec2 compact row
    if let Some(vec2) = value.try_downcast_ref::<Vec2>() {
        spawn_vec2_row(
            commands,
            parent,
            name,
            vec2,
            field_path,
            source_entity,
            component_type_id,
            depth,
        );
        return;
    }

    // Color field with picker
    if let Some(color) = value.try_downcast_ref::<Color>() {
        spawn_color_field(
            commands,
            parent,
            name,
            *color,
            field_path,
            source_entity,
            component_type_id,
            depth,
        );
        return;
    }

    // Bool toggle
    if let Some(&bool_val) = value.try_downcast_ref::<bool>() {
        spawn_bool_toggle(
            commands,
            parent,
            name,
            bool_val,
            field_path,
            source_entity,
            component_type_id,
            depth,
            editor_font,
            icon_font,
        );
        return;
    }

    // Numeric fields → drag input
    if let Some(&v) = value.try_downcast_ref::<f32>() {
        spawn_numeric_field(
            commands,
            parent,
            name,
            v as f64,
            field_path,
            source_entity,
            component_type_id,
            depth,
        );
        return;
    }
    if let Some(&v) = value.try_downcast_ref::<f64>() {
        spawn_numeric_field(
            commands,
            parent,
            name,
            v,
            field_path,
            source_entity,
            component_type_id,
            depth,
        );
        return;
    }

    // Integer fields -> numeric input with drag-to-scrub
    if let Some(&v) = value.try_downcast_ref::<i32>() {
        spawn_numeric_field(commands, parent, name, v as f64, field_path, source_entity, component_type_id, depth);
        return;
    }
    if let Some(&v) = value.try_downcast_ref::<u32>() {
        spawn_numeric_field(commands, parent, name, v as f64, field_path, source_entity, component_type_id, depth);
        return;
    }
    if let Some(&v) = value.try_downcast_ref::<usize>() {
        spawn_numeric_field(commands, parent, name, v as f64, field_path, source_entity, component_type_id, depth);
        return;
    }
    if let Some(&v) = value.try_downcast_ref::<i8>() {
        spawn_numeric_field(commands, parent, name, v as f64, field_path, source_entity, component_type_id, depth);
        return;
    }
    if let Some(&v) = value.try_downcast_ref::<i16>() {
        spawn_numeric_field(commands, parent, name, v as f64, field_path, source_entity, component_type_id, depth);
        return;
    }
    if let Some(&v) = value.try_downcast_ref::<i64>() {
        spawn_numeric_field(commands, parent, name, v as f64, field_path, source_entity, component_type_id, depth);
        return;
    }
    if let Some(&v) = value.try_downcast_ref::<u8>() {
        spawn_numeric_field(commands, parent, name, v as f64, field_path, source_entity, component_type_id, depth);
        return;
    }
    if let Some(&v) = value.try_downcast_ref::<u16>() {
        spawn_numeric_field(commands, parent, name, v as f64, field_path, source_entity, component_type_id, depth);
        return;
    }
    if let Some(&v) = value.try_downcast_ref::<u64>() {
        spawn_numeric_field(commands, parent, name, v as f64, field_path, source_entity, component_type_id, depth);
        return;
    }

    // Enum fields -> ComboBox
    if let ReflectRef::Enum(e) = value.reflect_ref() {
        if value.try_as_reflect().is_some() {
            spawn_enum_field(
                commands,
                parent,
                e,
                depth,
                field_path,
                source_entity,
                component_type_id,
                entity_names,
                type_registry,
                editor_font,
                icon_font,
            );
            return;
        }
        // Fallback: just show variant name
        let text = format!("{name}: {}", e.variant_name());
        spawn_text_row(commands, parent, &text, depth);
        return;
    }

    let is_compound = matches!(
        value.reflect_ref(),
        ReflectRef::Struct(_) | ReflectRef::TupleStruct(_) | ReflectRef::Tuple(_)
    );

    // Check for opaque types that shouldn't be recursed into
    if is_compound && is_opaque_type(value) {
        let text = format!("{name}: {}", format_partial_reflect_value(value));
        spawn_text_row(commands, parent, &text, depth);
        return;
    }

    if depth >= MAX_REFLECT_DEPTH || !is_compound {
        if is_editable_primitive(value) {
            spawn_editable_field(
                commands,
                parent,
                name,
                &format_partial_reflect_value(value),
                field_path,
                source_entity,
                component_type_id,
                depth,
            );
        } else {
            let text = format!("{name}: {}", format_partial_reflect_value(value));
            spawn_text_row(commands, parent, &text, depth);
        }
    } else {
        // Sub-header + recurse
        spawn_text_row(commands, parent, name, depth);

        let container = commands
            .spawn((Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::left(px(tokens::SPACING_LG)),
                ..Default::default()
            },))
            .insert(ChildOf(parent))
            .id();

        match value.reflect_ref() {
            ReflectRef::Struct(s) => {
                for i in 0..s.field_len() {
                    let Some(field_name) = s.name_at(i) else {
                        continue;
                    };
                    let Some(field_value) = s.field_at(i) else {
                        continue;
                    };
                    let child_path = format!("{field_path}.{field_name}");
                    spawn_field_row(
                        commands,
                        container,
                        field_name,
                        field_value,
                        depth + 1,
                        child_path,
                        source_entity,
                        component_type_id,
                        entity_names,
                        type_registry,
                        editor_font,
                        icon_font,
                    );
                }
            }
            ReflectRef::TupleStruct(ts) => {
                for i in 0..ts.field_len() {
                    let Some(field_value) = ts.field(i) else {
                        continue;
                    };
                    let child_path = format!("{field_path}.{i}");
                    spawn_field_row(
                        commands,
                        container,
                        &format!("{i}"),
                        field_value,
                        depth + 1,
                        child_path,
                        source_entity,
                        component_type_id,
                        entity_names,
                        type_registry,
                        editor_font,
                        icon_font,
                    );
                }
            }
            ReflectRef::Tuple(tuple) => {
                for i in 0..tuple.field_len() {
                    let Some(field_value) = tuple.field(i) else {
                        continue;
                    };
                    let child_path = format!("{field_path}.{i}");
                    spawn_field_row(
                        commands,
                        container,
                        &format!("{i}"),
                        field_value,
                        depth + 1,
                        child_path,
                        source_entity,
                        component_type_id,
                        entity_names,
                        type_registry,
                        editor_font,
                        icon_font,
                    );
                }
            }
            _ => {}
        }
    }
}

// --- Axis colors for Vec3/Vec2 fields ---
const AXIS_X_COLOR: Color = Color::srgb(0.8, 0.3, 0.3);
const AXIS_Y_COLOR: Color = Color::srgb(0.3, 0.7, 0.3);
const AXIS_Z_COLOR: Color = Color::srgb(0.3, 0.5, 0.8);

fn spawn_vec3_row(
    commands: &mut Commands,
    parent: Entity,
    name: &str,
    vec3: &Vec3,
    field_path: String,
    source_entity: Entity,
    component_type_id: TypeId,
    depth: usize,
) {
    let left_padding = depth as f32 * tokens::SPACING_MD;
    let row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(tokens::SPACING_XS),
                padding: UiRect::left(px(left_padding)),
                width: Val::Percent(100.0),
                ..Default::default()
            },
            ChildOf(parent),
        ))
        .id();

    // Label
    commands.spawn((
        Text::new(format!("{name}:")),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        Node {
            min_width: px(20.0),
            flex_shrink: 0.0,
            ..Default::default()
        },
        ThemedText,
        ChildOf(row),
    ));

    spawn_axis_input(
        commands,
        row,
        "X",
        vec3.x as f64,
        AXIS_X_COLOR,
        format!("{field_path}.x"),
        source_entity,
        component_type_id,
    );
    spawn_axis_input(
        commands,
        row,
        "Y",
        vec3.y as f64,
        AXIS_Y_COLOR,
        format!("{field_path}.y"),
        source_entity,
        component_type_id,
    );
    spawn_axis_input(
        commands,
        row,
        "Z",
        vec3.z as f64,
        AXIS_Z_COLOR,
        format!("{field_path}.z"),
        source_entity,
        component_type_id,
    );
}

fn spawn_vec2_row(
    commands: &mut Commands,
    parent: Entity,
    name: &str,
    vec2: &Vec2,
    field_path: String,
    source_entity: Entity,
    component_type_id: TypeId,
    depth: usize,
) {
    let left_padding = depth as f32 * tokens::SPACING_MD;
    let row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(tokens::SPACING_XS),
                padding: UiRect::left(px(left_padding)),
                width: Val::Percent(100.0),
                ..Default::default()
            },
            ChildOf(parent),
        ))
        .id();

    commands.spawn((
        Text::new(format!("{name}:")),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        Node {
            min_width: px(20.0),
            flex_shrink: 0.0,
            ..Default::default()
        },
        ThemedText,
        ChildOf(row),
    ));

    spawn_axis_input(
        commands,
        row,
        "X",
        vec2.x as f64,
        AXIS_X_COLOR,
        format!("{field_path}.x"),
        source_entity,
        component_type_id,
    );
    spawn_axis_input(
        commands,
        row,
        "Y",
        vec2.y as f64,
        AXIS_Y_COLOR,
        format!("{field_path}.y"),
        source_entity,
        component_type_id,
    );
}

fn spawn_axis_input(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    value: f64,
    label_color: Color,
    field_path: String,
    source_entity: Entity,
    component_type_id: TypeId,
) {
    // Axis label
    commands.spawn((
        Text::new(label),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(label_color),
        Node {
            flex_shrink: 0.0,
            ..Default::default()
        },
        ChildOf(parent),
    ));

    // Numeric input
    let path = field_path.clone();
    let binding_path = field_path;
    commands
        .spawn((numeric_input::numeric_input(value), ChildOf(parent)))
        .insert(FieldBinding {
            source_entity,
            component_type_id,
            field_path: binding_path,
        })
        .observe(
            move |changed: On<NumericValueChanged>, mut commands: Commands| {
                let path = path.clone();
                let value_str = format!("{}", changed.value);
                commands.queue(move |world: &mut World| {
                    apply_field_value_with_undo(
                        world,
                        source_entity,
                        component_type_id,
                        &path,
                        &value_str,
                    );
                });
            },
        );
}

fn spawn_bool_toggle(
    commands: &mut Commands,
    parent: Entity,
    name: &str,
    value: bool,
    field_path: String,
    source_entity: Entity,
    component_type_id: TypeId,
    depth: usize,
    editor_font: &Handle<Font>,
    icon_font: &Handle<Font>,
) {
    let left_padding = depth as f32 * tokens::SPACING_MD;
    let row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(tokens::SPACING_XS),
                padding: UiRect::left(px(left_padding)),
                ..Default::default()
            },
            ChildOf(parent),
        ))
        .id();

    commands.spawn((
        Text::new(format!("{name}:")),
        TextFont {
            font: editor_font.clone(),
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        Node {
            min_width: px(20.0),
            flex_shrink: 0.0,
            ..Default::default()
        },
        TextColor(tokens::TYPE_BOOL),
        ChildOf(row),
    ));

    commands
        .spawn((
            checkbox(CheckboxProps::new("").checked(value), editor_font, icon_font),
            FieldBinding {
                source_entity,
                component_type_id,
                field_path,
            },
            ChildOf(row),
        ));
}

fn spawn_color_field(
    commands: &mut Commands,
    parent: Entity,
    name: &str,
    color: Color,
    field_path: String,
    source_entity: Entity,
    component_type_id: TypeId,
    depth: usize,
) {
    let left_padding = depth as f32 * tokens::SPACING_MD;
    let row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(tokens::SPACING_XS),
                padding: UiRect::left(px(left_padding)),
                ..Default::default()
            },
            ChildOf(parent),
        ))
        .id();

    commands.spawn((
        Text::new(format!("{name}:")),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        Node {
            min_width: px(20.0),
            flex_shrink: 0.0,
            ..Default::default()
        },
        ThemedText,
        ChildOf(row),
    ));

    let srgba = color.to_srgba();
    let rgba = [srgba.red, srgba.green, srgba.blue, srgba.alpha];

    let path = field_path.clone();
    commands
        .spawn((
            color_picker(ColorPickerProps::new().with_color(rgba)),
            FieldBinding {
                source_entity,
                component_type_id,
                field_path,
            },
            ChildOf(row),
        ))
        .observe(
            move |event: On<ColorPickerCommitEvent>, mut commands: Commands| {
                let color = event.color;
                let path = path.clone();
                commands.queue(move |world: &mut World| {
                    apply_color_with_undo(
                        world,
                        source_entity,
                        component_type_id,
                        &path,
                        color,
                    );
                });
            },
        );
}

/// Apply a color change with undo support (propagates to all selected entities).
fn apply_color_with_undo(
    world: &mut World,
    _entity: Entity,
    component_type_id: TypeId,
    field_path: &str,
    new_rgba: [f32; 4],
) {
    let registry = world.resource::<AppTypeRegistry>().clone();

    let selection = world.resource::<Selection>();
    let targets: Vec<Entity> = selection.entities.clone();

    let new_color = Color::srgba(new_rgba[0], new_rgba[1], new_rgba[2], new_rgba[3]);

    let reg = registry.read();
    let Some(registration) = reg.get(component_type_id) else {
        return;
    };
    let Some(reflect_component) = registration.data::<ReflectComponent>() else {
        return;
    };

    let mut sub_commands: Vec<Box<dyn EditorCommand>> = Vec::new();

    for &target in &targets {
        let Ok(entity_ref) = world.get_entity(target) else {
            continue;
        };
        let Some(reflected) = reflect_component.reflect(entity_ref) else {
            continue;
        };
        let Ok(field) = reflected.reflect_path(field_path) else {
            continue;
        };
        let old_value = field.to_dynamic();

        sub_commands.push(Box::new(SetComponentField {
            entity: target,
            component_type_id,
            field_path: field_path.to_string(),
            old_value,
            new_value: Box::new(new_color),
        }));
    }
    drop(reg);

    if sub_commands.is_empty() {
        return;
    }

    let cmd: Box<dyn EditorCommand> = if sub_commands.len() == 1 {
        sub_commands.pop().unwrap()
    } else {
        Box::new(CommandGroup {
            label: "Set color on multiple entities".to_string(),
            commands: sub_commands,
        })
    };
    cmd.execute(world);
    let mut history = world.resource_mut::<CommandHistory>();
    history.undo_stack.push(cmd);
    history.redo_stack.clear();
}

fn spawn_numeric_field(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    value: f64,
    field_path: String,
    source_entity: Entity,
    component_type_id: TypeId,
    depth: usize,
) {
    let left_padding = depth as f32 * tokens::SPACING_MD;
    let row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(tokens::SPACING_XS),
                padding: UiRect::left(px(left_padding)),
                ..Default::default()
            },
            ChildOf(parent),
        ))
        .id();

    commands.spawn((
        Text::new(format!("{label}:")),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        Node {
            min_width: px(20.0),
            flex_shrink: 0.0,
            ..Default::default()
        },
        TextColor(tokens::TYPE_NUMERIC),
        ChildOf(row),
    ));

    let path = field_path.clone();
    let binding_path = field_path;
    commands
        .spawn((numeric_input::numeric_input(value), ChildOf(row)))
        .insert(FieldBinding {
            source_entity,
            component_type_id,
            field_path: binding_path,
        })
        .observe(
            move |changed: On<NumericValueChanged>, mut commands: Commands| {
                let path = path.clone();
                let value_str = format!("{}", changed.value);
                commands.queue(move |world: &mut World| {
                    apply_field_value_with_undo(
                        world,
                        source_entity,
                        component_type_id,
                        &path,
                        &value_str,
                    );
                });
            },
        );
}

fn spawn_editable_field(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    current_value: &str,
    field_path: String,
    source_entity: Entity,
    component_type_id: TypeId,
    depth: usize,
) {
    let left_padding = depth as f32 * tokens::SPACING_MD;

    let row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(tokens::SPACING_XS),
                padding: UiRect::left(px(left_padding)),
                ..Default::default()
            },
            ChildOf(parent),
        ))
        .id();

    commands.spawn((
        Text::new(format!("{label}:")),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        Node {
            min_width: px(20.0),
            flex_shrink: 0.0,
            ..Default::default()
        },
        ThemedText,
        ChildOf(row),
    ));

    let input_entity = commands
        .spawn((text_input::text_input(""), ChildOf(row)))
        .insert(TextInput::new(current_value))
        .observe(
            move |text: On<EnteredText>, mut commands: Commands| {
                let path = field_path.clone();
                let value = text.value.clone();
                commands.queue(move |world: &mut World| {
                    apply_field_value_with_undo(
                        world,
                        source_entity,
                        component_type_id,
                        &path,
                        &value,
                    );
                });
            },
        )
        .id();

    commands
        .entity(input_entity)
        .entry::<Node>()
        .and_modify(|mut node| {
            node.width = Val::Auto;
            node.flex_grow = 1.0;
            node.flex_basis = Val::Px(0.0);
        });
}

/// Apply a field value change with undo support — snapshots old value, creates command.
/// Propagates the edit to all selected entities that have the same component.
fn apply_field_value_with_undo(
    world: &mut World,
    _entity: Entity,
    component_type_id: TypeId,
    field_path: &str,
    new_value_str: &str,
) {
    let registry = world.resource::<AppTypeRegistry>().clone();

    // Collect all selected entities
    let selection = world.resource::<Selection>();
    let targets: Vec<Entity> = selection.entities.clone();

    let mut sub_commands: Vec<Box<dyn EditorCommand>> = Vec::new();

    let reg = registry.read();
    let Some(reflect_component) = reg
        .get(component_type_id)
        .and_then(|r| r.data::<ReflectComponent>())
    else {
        return;
    };

    for &target in &targets {
        let Ok(entity_ref) = world.get_entity(target) else {
            continue;
        };
        let Some(reflected) = reflect_component.reflect(entity_ref) else {
            continue;
        };
        let Ok(field) = reflected.reflect_path(field_path) else {
            continue;
        };
        let old_value = field.to_dynamic();

        let mut new_val = old_value.to_dynamic();
        if !parse_into_reflect(&mut *new_val, new_value_str) {
            continue;
        }

        sub_commands.push(Box::new(SetComponentField {
            entity: target,
            component_type_id,
            field_path: field_path.to_string(),
            old_value,
            new_value: new_val,
        }));
    }
    drop(reg);

    if sub_commands.is_empty() {
        return;
    }

    let cmd: Box<dyn EditorCommand> = if sub_commands.len() == 1 {
        sub_commands.pop().unwrap()
    } else {
        Box::new(CommandGroup {
            label: "Set field on multiple entities".to_string(),
            commands: sub_commands,
        })
    };
    cmd.execute(world);
    let mut history = world.resource_mut::<CommandHistory>();
    history.undo_stack.push(cmd);
    history.redo_stack.clear();
}

/// Parse a string value into a reflected value, returning true on success.
fn parse_into_reflect(target: &mut dyn PartialReflect, value_str: &str) -> bool {
    if let Some(current) = target.try_downcast_mut::<f32>() {
        if let Ok(v) = value_str.parse::<f32>() {
            *current = v;
            return true;
        }
    } else if let Some(current) = target.try_downcast_mut::<f64>() {
        if let Ok(v) = value_str.parse::<f64>() {
            *current = v;
            return true;
        }
    } else if let Some(current) = target.try_downcast_mut::<i32>() {
        if let Ok(v) = value_str.parse::<i32>() {
            *current = v;
            return true;
        }
    } else if let Some(current) = target.try_downcast_mut::<u32>() {
        if let Ok(v) = value_str.parse::<u32>() {
            *current = v;
            return true;
        }
    } else if let Some(current) = target.try_downcast_mut::<usize>() {
        if let Ok(v) = value_str.parse::<usize>() {
            *current = v;
            return true;
        }
    } else if let Some(current) = target.try_downcast_mut::<i8>() {
        if let Ok(v) = value_str.parse::<i8>() {
            *current = v;
            return true;
        }
    } else if let Some(current) = target.try_downcast_mut::<i16>() {
        if let Ok(v) = value_str.parse::<i16>() {
            *current = v;
            return true;
        }
    } else if let Some(current) = target.try_downcast_mut::<i64>() {
        if let Ok(v) = value_str.parse::<i64>() {
            *current = v;
            return true;
        }
    } else if let Some(current) = target.try_downcast_mut::<u8>() {
        if let Ok(v) = value_str.parse::<u8>() {
            *current = v;
            return true;
        }
    } else if let Some(current) = target.try_downcast_mut::<u16>() {
        if let Ok(v) = value_str.parse::<u16>() {
            *current = v;
            return true;
        }
    } else if let Some(current) = target.try_downcast_mut::<u64>() {
        if let Ok(v) = value_str.parse::<u64>() {
            *current = v;
            return true;
        }
    } else if let Some(current) = target.try_downcast_mut::<bool>() {
        if let Ok(v) = value_str.parse::<bool>() {
            *current = v;
            return true;
        }
    } else if let Some(current) = target.try_downcast_mut::<String>() {
        *current = value_str.to_string();
        return true;
    }
    false
}

fn spawn_entity_link(commands: &mut Commands, parent: Entity, target: Entity, label: &str) {
    commands.spawn((
        Text::new(label),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(tokens::TEXT_ACCENT),
        ChildOf(parent),
        observe(
            move |_: On<Pointer<Click>>,
                  mut commands: Commands,
                  mut selection: ResMut<Selection>| {
                selection.select_single(&mut commands, target);
            },
        ),
        observe(
            move |hover: On<Pointer<Over>>, mut q: Query<&mut TextColor>| {
                if let Ok(mut c) = q.get_mut(hover.event_target()) {
                    c.0 = tokens::TEXT_ACCENT_HOVER;
                }
            },
        ),
        observe(
            move |out: On<Pointer<Out>>, mut q: Query<&mut TextColor>| {
                if let Ok(mut c) = q.get_mut(out.event_target()) {
                    c.0 = tokens::TEXT_ACCENT;
                }
            },
        ),
    ));
}

fn spawn_list_expansion<'a>(
    commands: &mut Commands,
    parent: Entity,
    len: usize,
    get_item: impl Fn(usize) -> Option<&'a dyn PartialReflect>,
    depth: usize,
    base_path: &str,
    source_entity: Entity,
    component_type_id: TypeId,
    entity_names: &Query<&Name>,
) {
    spawn_text_row(commands, parent, &format!("[{len} items]"), depth);
    if len == 0 {
        return;
    }
    let lv = commands
        .spawn((list_view::list_view(), ChildOf(parent)))
        .id();
    for i in 0..len {
        if let Some(item) = get_item(i) {
            let item_entity = commands
                .spawn((list_view::list_item(i), ChildOf(lv)))
                .id();
            let child_path = if base_path.is_empty() {
                format!("[{i}]")
            } else {
                format!("{base_path}[{i}]")
            };
            spawn_list_item_value(
                commands,
                item_entity,
                item,
                depth + 1,
                child_path,
                source_entity,
                component_type_id,
                entity_names,
            );
        }
    }
}

fn spawn_list_item_value(
    commands: &mut Commands,
    parent: Entity,
    value: &dyn PartialReflect,
    depth: usize,
    field_path: String,
    source_entity: Entity,
    component_type_id: TypeId,
    entity_names: &Query<&Name>,
) {
    // Entity -> clickable link
    if let Some(&entity_val) = value.try_downcast_ref::<Entity>() {
        let label = entity_names
            .get(entity_val)
            .map(|n| format!("{} ({entity_val})", n.as_str()))
            .unwrap_or_else(|_| format!("{entity_val}"));
        spawn_entity_link(commands, parent, entity_val, &label);
        return;
    }
    // Editable primitive -> inline text input
    if is_editable_primitive(value) {
        spawn_inline_editable(
            commands,
            parent,
            &format_partial_reflect_value(value),
            field_path,
            source_entity,
            component_type_id,
        );
        return;
    }
    // Compound -> recurse (list items don't have type_registry context, show as text)
    if let Some(reflected) = value.try_as_reflect() {
        // For list items we don't have type_registry context, so show text
        let text = format_reflect_value(reflected);
        spawn_text_row(commands, parent, &text, depth);
        return;
    }
    // Fallback -> plain text
    spawn_text_row(
        commands,
        parent,
        &format_partial_reflect_value(value),
        depth,
    );
}

fn spawn_inline_editable(
    commands: &mut Commands,
    parent: Entity,
    current_value: &str,
    field_path: String,
    source_entity: Entity,
    component_type_id: TypeId,
) {
    let input_entity = commands
        .spawn((text_input::text_input(""), ChildOf(parent)))
        .insert(TextInput::new(current_value))
        .observe(
            move |text: On<EnteredText>, mut commands: Commands| {
                let path = field_path.clone();
                let value = text.value.clone();
                commands.queue(move |world: &mut World| {
                    apply_field_value_with_undo(
                        world,
                        source_entity,
                        component_type_id,
                        &path,
                        &value,
                    );
                });
            },
        )
        .id();
    commands
        .entity(input_entity)
        .entry::<Node>()
        .and_modify(|mut node| {
            node.width = Val::Auto;
            node.flex_grow = 1.0;
            node.flex_basis = Val::Px(0.0);
        });
}

fn spawn_text_row(commands: &mut Commands, parent: Entity, text: &str, depth: usize) {
    let left_padding = depth as f32 * tokens::SPACING_MD;
    commands.spawn((
        Node {
            padding: UiRect::left(px(left_padding)),
            ..Default::default()
        },
        Text::new(text),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        ThemedText,
        ChildOf(parent),
    ));
}

fn format_reflect_value(value: &dyn Reflect) -> String {
    format_partial_reflect_value(value.as_partial_reflect())
}

fn format_partial_reflect_value(value: &dyn PartialReflect) -> String {
    if let Some(v) = value.try_downcast_ref::<f32>() {
        return format!("{v:.3}");
    }
    if let Some(v) = value.try_downcast_ref::<f64>() {
        return format!("{v:.3}");
    }
    if let Some(v) = value.try_downcast_ref::<bool>() {
        return format!("{v}");
    }
    if let Some(v) = value.try_downcast_ref::<String>() {
        return format!("\"{v}\"");
    }
    if let Some(v) = value.try_downcast_ref::<i32>() {
        return format!("{v}");
    }
    if let Some(v) = value.try_downcast_ref::<u32>() {
        return format!("{v}");
    }
    if let Some(v) = value.try_downcast_ref::<usize>() {
        return format!("{v}");
    }
    if let Some(v) = value.try_downcast_ref::<i8>() {
        return format!("{v}");
    }
    if let Some(v) = value.try_downcast_ref::<i16>() {
        return format!("{v}");
    }
    if let Some(v) = value.try_downcast_ref::<i64>() {
        return format!("{v}");
    }
    if let Some(v) = value.try_downcast_ref::<u8>() {
        return format!("{v}");
    }
    if let Some(v) = value.try_downcast_ref::<u16>() {
        return format!("{v}");
    }
    if let Some(v) = value.try_downcast_ref::<u64>() {
        return format!("{v}");
    }

    // Handle<T> and other opaque types -> clean format
    if is_opaque_type(value) {
        return "<opaque>".to_string();
    }

    // Fallback: show type name if available, otherwise Debug
    if let Some(info) = value.get_represented_type_info() {
        return format!("<{}>", info.type_path_table().short_path());
    }
    format!("{value:?}")
}

fn on_checkbox_commit(
    event: On<CheckboxCommitEvent>,
    bindings: Query<&FieldBinding>,
    mut commands: Commands,
) {
    let Ok(binding) = bindings.get(event.entity) else {
        return;
    };
    let source = binding.source_entity;
    let type_id = binding.component_type_id;
    let path = binding.field_path.clone();
    let val = format!("{}", event.checked);
    commands.queue(move |world: &mut World| {
        apply_field_value_with_undo(world, source, type_id, &path, &val);
    });
}

/// Refreshes inspector field values using reflection — handles all component types generically.
/// Uses exclusive world access to avoid query conflicts between EntityRef and &mut NumericInput.
fn refresh_inspector_fields(world: &mut World) {
    let selection = world.resource::<Selection>();
    let Some(primary) = selection.primary() else {
        return;
    };

    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = type_registry.read();

    // Collect numeric binding info and current input values
    let mut numeric_lookups: Vec<(Entity, TypeId, String, f64)> = Vec::new();
    let mut query = world.query::<(Entity, &FieldBinding, &NumericInput)>();
    for (entity, binding, input) in query.iter(world) {
        if binding.source_entity == primary {
            numeric_lookups.push((entity, binding.component_type_id, binding.field_path.clone(), input.value));
        }
    }

    // Collect checkbox binding info and current state
    let mut bool_lookups: Vec<(Entity, TypeId, String, bool)> = Vec::new();
    let mut checkbox_query = world.query::<(Entity, &FieldBinding, &CheckboxState)>();
    for (entity, binding, state) in checkbox_query.iter(world) {
        if binding.source_entity == primary {
            bool_lookups.push((entity, binding.component_type_id, binding.field_path.clone(), state.checked));
        }
    }

    if numeric_lookups.is_empty() && bool_lookups.is_empty() {
        return;
    }

    // Read reflected values and compute updates
    let mut numeric_updates: Vec<(Entity, f64)> = Vec::new();
    let mut bool_updates: Vec<(Entity, bool)> = Vec::new();
    let Ok(entity_ref) = world.get_entity(primary) else {
        return;
    };

    for (ui_entity, comp_type_id, field_path, current_val) in &numeric_lookups {
        let Some(registration) = registry.get(*comp_type_id) else {
            continue;
        };
        let Some(reflect_component) = registration.data::<ReflectComponent>() else {
            continue;
        };
        let Some(reflected) = reflect_component.reflect(entity_ref) else {
            continue;
        };
        let Ok(field) = reflected.reflect_path(field_path.as_str()) else {
            continue;
        };
        let value = reflect_field_to_f64(field);
        let Some(value) = value else {
            continue;
        };

        if (*current_val - value).abs() > f64::EPSILON {
            numeric_updates.push((*ui_entity, value));
        }
    }

    for (ui_entity, comp_type_id, field_path, current_checked) in &bool_lookups {
        let Some(registration) = registry.get(*comp_type_id) else {
            continue;
        };
        let Some(reflect_component) = registration.data::<ReflectComponent>() else {
            continue;
        };
        let Some(reflected) = reflect_component.reflect(entity_ref) else {
            continue;
        };
        let Ok(field) = reflected.reflect_path(field_path.as_str()) else {
            continue;
        };
        if let Some(&val) = field.try_downcast_ref::<bool>() {
            if val != *current_checked {
                bool_updates.push((*ui_entity, val));
            }
        }
    }

    drop(registry);

    // Apply numeric updates
    for (entity, value) in numeric_updates {
        if let Some(mut input) = world.get_mut::<NumericInput>(entity) {
            input.value = value;
        }
    }

    // Apply bool updates (sync_checkbox_icon handles the visual update)
    for (entity, value) in bool_updates {
        if let Some(mut state) = world.get_mut::<CheckboxState>(entity) {
            state.checked = value;
        }
    }
}

fn reflect_field_to_f64(field: &dyn PartialReflect) -> Option<f64> {
    if let Some(&v) = field.try_downcast_ref::<f32>() {
        Some(v as f64)
    } else if let Some(&v) = field.try_downcast_ref::<f64>() {
        Some(v)
    } else if let Some(&v) = field.try_downcast_ref::<i32>() {
        Some(v as f64)
    } else if let Some(&v) = field.try_downcast_ref::<u32>() {
        Some(v as f64)
    } else if let Some(&v) = field.try_downcast_ref::<usize>() {
        Some(v as f64)
    } else if let Some(&v) = field.try_downcast_ref::<i8>() {
        Some(v as f64)
    } else if let Some(&v) = field.try_downcast_ref::<i16>() {
        Some(v as f64)
    } else if let Some(&v) = field.try_downcast_ref::<i64>() {
        Some(v as f64)
    } else if let Some(&v) = field.try_downcast_ref::<u8>() {
        Some(v as f64)
    } else if let Some(&v) = field.try_downcast_ref::<u16>() {
        Some(v as f64)
    } else if let Some(&v) = field.try_downcast_ref::<u64>() {
        Some(v as f64)
    } else {
        None
    }
}

/// Check if a type is opaque and shouldn't be recursed into for inspection.
fn is_opaque_type(value: &dyn PartialReflect) -> bool {
    let Some(type_info) = value.get_represented_type_info() else {
        return false;
    };
    let type_path = type_info.type_path();
    type_path.starts_with("bevy_asset::handle::Handle")
        || type_path.starts_with("bevy_asset::id::AssetId")
        || type_path.contains("Cow<")
}

/// Spawn a ComboBox for enum fields, supporting unit-only enums with undo.
fn spawn_enum_field(
    commands: &mut Commands,
    parent: Entity,
    enum_ref: &dyn bevy::reflect::Enum,
    depth: usize,
    field_path: String,
    source_entity: Entity,
    component_type_id: TypeId,
    entity_names: &Query<&Name>,
    type_registry: &AppTypeRegistry,
    editor_font: &Handle<Font>,
    icon_font: &Handle<Font>,
) {
    let current_variant = enum_ref.variant_name().to_string();

    // Try to get variant names from type info
    let Some(type_info) = enum_ref.get_represented_type_info() else {
        spawn_text_row(commands, parent, &format!("variant: {current_variant}"), depth);
        return;
    };
    let bevy::reflect::TypeInfo::Enum(enum_info) = type_info else {
        spawn_text_row(commands, parent, &format!("variant: {current_variant}"), depth);
        return;
    };

    let variant_names: Vec<String> = enum_info
        .variant_names()
        .iter()
        .map(|n| n.to_string())
        .collect();

    if variant_names.is_empty() {
        spawn_text_row(commands, parent, &format!("variant: {current_variant}"), depth);
        return;
    }

    let selected_index = variant_names
        .iter()
        .position(|n| n == &current_variant)
        .unwrap_or(0);

    // Check if all variants are unit variants
    let all_unit = (0..enum_info.variant_len()).all(|i| {
        enum_info
            .variant_at(i)
            .map(|v| matches!(v, bevy::reflect::VariantInfo::Unit(_)))
            .unwrap_or(false)
    });

    let left_padding = depth as f32 * tokens::SPACING_MD;

    if all_unit {
        // Simple ComboBox for unit-only enums
        let row = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(tokens::SPACING_XS),
                    padding: UiRect::left(px(left_padding)),
                    width: Val::Percent(100.0),
                    ..Default::default()
                },
                ChildOf(parent),
            ))
            .id();

        let path = field_path.clone();
        commands
            .spawn((
                combobox_with_selected(variant_names, selected_index),
                FieldBinding {
                    source_entity,
                    component_type_id,
                    field_path,
                },
                ChildOf(row),
            ))
            .observe(
                move |event: On<ComboBoxChangeEvent>, mut commands: Commands| {
                    let variant_name = event.label.clone();
                    let path = path.clone();
                    commands.queue(move |world: &mut World| {
                        apply_enum_variant_with_undo(
                            world,
                            source_entity,
                            component_type_id,
                            &path,
                            &variant_name,
                        );
                    });
                },
            );
    } else {
        // Show ComboBox for variant selection + recurse into fields for data-carrying variants
        let container = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::left(px(left_padding)),
                    row_gap: px(tokens::SPACING_XS),
                    width: Val::Percent(100.0),
                    ..Default::default()
                },
                ChildOf(parent),
            ))
            .id();

        let path = field_path.clone();
        commands
            .spawn((
                combobox_with_selected(variant_names, selected_index),
                FieldBinding {
                    source_entity,
                    component_type_id,
                    field_path: field_path.clone(),
                },
                ChildOf(container),
            ))
            .observe(
                move |event: On<ComboBoxChangeEvent>, mut commands: Commands| {
                    let variant_name = event.label.clone();
                    let path = path.clone();
                    commands.queue(move |world: &mut World| {
                        apply_enum_variant_with_undo(
                            world,
                            source_entity,
                            component_type_id,
                            &path,
                            &variant_name,
                        );
                    });
                },
            );

        // Recurse into current variant's fields (if it has any)
        let variant_field_count = enum_ref.field_len();
        if variant_field_count > 0 {
            for i in 0..variant_field_count {
                let Some(field_value) = enum_ref.field_at(i) else {
                    continue;
                };
                let field_name = enum_ref
                    .name_at(i)
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| format!("{i}"));
                let child_path = if field_path.is_empty() {
                    field_name.clone()
                } else {
                    format!("{field_path}.{field_name}")
                };
                spawn_field_row(
                    commands,
                    container,
                    &field_name,
                    field_value,
                    depth + 1,
                    child_path,
                    source_entity,
                    component_type_id,
                    entity_names,
                    type_registry,
                    editor_font,
                    icon_font,
                );
            }
        }
    }
}

/// Apply an enum variant change with undo support.
fn apply_enum_variant_with_undo(
    world: &mut World,
    _entity: Entity,
    component_type_id: TypeId,
    field_path: &str,
    variant_name: &str,
) {
    let registry = world.resource::<AppTypeRegistry>().clone();

    let selection = world.resource::<Selection>();
    let targets: Vec<Entity> = selection.entities.clone();

    let reg = registry.read();
    let Some(registration) = reg.get(component_type_id) else {
        return;
    };
    let Some(reflect_component) = registration.data::<ReflectComponent>() else {
        return;
    };

    let mut sub_commands: Vec<Box<dyn EditorCommand>> = Vec::new();

    for &target in &targets {
        let Ok(entity_ref) = world.get_entity(target) else {
            continue;
        };
        let Some(reflected) = reflect_component.reflect(entity_ref) else {
            continue;
        };
        let old_value = if field_path.is_empty() {
            reflected.to_dynamic()
        } else {
            let Ok(field) = reflected.reflect_path(field_path) else {
                continue;
            };
            field.to_dynamic()
        };

        let Some(dynamic_variant) =
            build_dynamic_variant(old_value.as_ref(), variant_name, &reg)
        else {
            continue;
        };

        let new_value: Box<dyn PartialReflect> =
            Box::new(DynamicEnum::new(variant_name, dynamic_variant));

        sub_commands.push(Box::new(SetComponentField {
            entity: target,
            component_type_id,
            field_path: field_path.to_string(),
            old_value,
            new_value,
        }));
    }
    drop(reg);

    if sub_commands.is_empty() {
        return;
    }

    let cmd: Box<dyn EditorCommand> = if sub_commands.len() == 1 {
        sub_commands.pop().unwrap()
    } else {
        Box::new(CommandGroup {
            label: "Set enum on multiple entities".to_string(),
            commands: sub_commands,
        })
    };
    cmd.execute(world);
    let mut history = world.resource_mut::<CommandHistory>();
    history.undo_stack.push(cmd);
    history.redo_stack.clear();
}

// ---------------------------------------------------------------------------
// Material display: follow Handle<StandardMaterial> → display material fields
// ---------------------------------------------------------------------------

/// Marker for material field UI entities
#[derive(Component)]
struct MaterialFieldMarker;

/// Spawn material fields in a deferred command to access Assets<StandardMaterial>.
fn spawn_material_display_deferred(
    commands: &mut Commands,
    body_entity: Entity,
    source_entity: Entity,
) {
    commands.queue(move |world: &mut World| {
        spawn_material_fields(world, body_entity, source_entity);
    });
}

fn spawn_material_fields(world: &mut World, body_entity: Entity, source_entity: Entity) {
    // Look up the Handle<StandardMaterial> from MeshMaterial3d
    let handle = {
        let Some(mat) = world.get::<MeshMaterial3d<StandardMaterial>>(source_entity) else {
            return;
        };
        mat.0.clone()
    };

    let mat_data = {
        let materials = world.resource::<Assets<StandardMaterial>>();
        materials.get(&handle).map(|material| {
            (
                material.base_color,
                material.metallic,
                material.perceptual_roughness,
                material.reflectance,
                material.emissive,
                format!("{:?}", material.alpha_mode),
            )
        })
    };

    let Some((base_color, metallic, perceptual_roughness, reflectance, emissive, alpha_mode_str)) =
        mat_data
    else {
        world.spawn((
            Text::new("(material not loaded)"),
            TextFont {
                font_size: tokens::FONT_SM,
                ..Default::default()
            },
            TextColor(tokens::TEXT_SECONDARY),
            ChildOf(body_entity),
        ));
        return;
    };

    // base_color (Color picker)
    {
        let srgba = base_color.to_srgba();
        let row = world
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(tokens::SPACING_XS),
                    ..Default::default()
                },
                ChildOf(body_entity),
            ))
            .id();
        world.spawn((
            Text::new("base_color:"),
            TextFont {
                font_size: tokens::FONT_SM,
                ..Default::default()
            },
            TextColor(tokens::TEXT_SECONDARY),
            Node {
                min_width: Val::Px(20.0),
                flex_shrink: 0.0,
                ..Default::default()
            },
            ChildOf(row),
        ));
        let rgba = [srgba.red, srgba.green, srgba.blue, srgba.alpha];
        let picker = world
            .spawn((
                editor_feathers::color_picker::color_picker(
                    editor_feathers::color_picker::ColorPickerProps::new().with_color(rgba),
                ),
                MaterialFieldMarker,
                ChildOf(row),
            ))
            .id();
        world.entity_mut(picker).observe(
            move |event: On<editor_feathers::color_picker::ColorPickerCommitEvent>,
                  mut materials: ResMut<Assets<StandardMaterial>>,
                  mat_query: Query<&MeshMaterial3d<StandardMaterial>>| {
                let Ok(mat_comp) = mat_query.get(source_entity) else {
                    return;
                };
                if let Some(material) = materials.get_mut(&mat_comp.0) {
                    let c = event.color;
                    material.base_color = Color::srgba(c[0], c[1], c[2], c[3]);
                }
            },
        );
    }

    // metallic (f32 numeric input)
    spawn_material_numeric_field(world, body_entity, "metallic", metallic as f64, source_entity,
        |mat, val| { mat.metallic = val as f32; });

    // perceptual_roughness
    spawn_material_numeric_field(world, body_entity, "roughness", perceptual_roughness as f64, source_entity,
        |mat, val| { mat.perceptual_roughness = val as f32; });

    // reflectance
    spawn_material_numeric_field(world, body_entity, "reflectance", reflectance as f64, source_entity,
        |mat, val| { mat.reflectance = val as f32; });

    // emissive (show as text for now - it's LinearRgba which is complex)
    {
        let emissive_text = format!(
            "emissive: ({:.2}, {:.2}, {:.2})",
            emissive.red, emissive.green, emissive.blue
        );
        world.spawn((
            Text::new(emissive_text),
            TextFont {
                font_size: tokens::FONT_SM,
                ..Default::default()
            },
            TextColor(tokens::TEXT_SECONDARY),
            ChildOf(body_entity),
        ));
    }

    // alpha_mode (read-only for now)
    world.spawn((
        Text::new(format!("alpha_mode: {alpha_mode_str}")),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(tokens::TEXT_SECONDARY),
        ChildOf(body_entity),
    ));
}

fn spawn_material_numeric_field(
    world: &mut World,
    parent: Entity,
    label: &str,
    value: f64,
    source_entity: Entity,
    apply_fn: fn(&mut StandardMaterial, f64),
) {
    let row = world
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(tokens::SPACING_XS),
                ..Default::default()
            },
            ChildOf(parent),
        ))
        .id();

    world.spawn((
        Text::new(format!("{label}:")),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(tokens::TEXT_SECONDARY),
        Node {
            min_width: Val::Px(20.0),
            flex_shrink: 0.0,
            ..Default::default()
        },
        ChildOf(row),
    ));

    let input = world
        .spawn((numeric_input::numeric_input(value), ChildOf(row)))
        .id();

    world.entity_mut(input).observe(
        move |changed: On<NumericValueChanged>,
              mut materials: ResMut<Assets<StandardMaterial>>,
              mat_query: Query<&MeshMaterial3d<StandardMaterial>>| {
            let Ok(mat_comp) = mat_query.get(source_entity) else {
                return;
            };
            if let Some(material) = materials.get_mut(&mat_comp.0) {
                apply_fn(material, changed.value);
            }
        },
    );
}

/// Build the appropriate DynamicVariant for a given variant name,
/// using the enum's type info to determine if it's Unit, Tuple, or Struct.
/// Returns None if the variant has fields whose default values can't be constructed.
fn build_dynamic_variant(
    old_enum_value: &dyn PartialReflect,
    variant_name: &str,
    registry: &bevy::reflect::TypeRegistry,
) -> Option<DynamicVariant> {
    let type_info = old_enum_value.get_represented_type_info()?;
    let bevy::reflect::TypeInfo::Enum(enum_info) = type_info else {
        return Some(DynamicVariant::Unit);
    };
    let variant_info = enum_info.variant(variant_name)?;

    match variant_info {
        bevy::reflect::VariantInfo::Unit(_) => Some(DynamicVariant::Unit),
        bevy::reflect::VariantInfo::Tuple(tuple_info) => {
            let mut dynamic_tuple = DynamicTuple::default();
            for i in 0..tuple_info.field_len() {
                let field_info = tuple_info.field_at(i)?;
                let type_id = field_info.type_id();
                let default_val = registry
                    .get(type_id)
                    .and_then(|reg| reg.data::<ReflectDefault>())
                    .map(|rd| rd.default());
                let default = default_val?;
                dynamic_tuple.insert_boxed(default.into_partial_reflect());
            }
            Some(DynamicVariant::Tuple(dynamic_tuple))
        }
        bevy::reflect::VariantInfo::Struct(struct_info) => {
            let mut dynamic_struct = DynamicStruct::default();
            for i in 0..struct_info.field_len() {
                let field_info = struct_info.field_at(i)?;
                let field_name = field_info.name();
                let type_id = field_info.type_id();
                let default_val = registry
                    .get(type_id)
                    .and_then(|reg| reg.data::<ReflectDefault>())
                    .map(|rd| rd.default());
                let default = default_val?;
                dynamic_struct.insert_boxed(field_name, default.into_partial_reflect());
            }
            Some(DynamicVariant::Struct(dynamic_struct))
        }
    }
}

// ---------------------------------------------------------------------------
// Custom Properties display
// ---------------------------------------------------------------------------

fn spawn_brush_display(
    commands: &mut Commands,
    parent: Entity,
    brush: &crate::brush::Brush,
) {
    let (vertices, face_polygons) = crate::brush::compute_brush_geometry(&brush.faces);
    let face_count = brush.faces.len();
    let vertex_count = vertices.len();
    let edge_count = {
        let mut edges = std::collections::HashSet::new();
        for polygon in &face_polygons {
            for i in 0..polygon.len() {
                let a = polygon[i];
                let b = polygon[(i + 1) % polygon.len()];
                let edge = if a < b { (a, b) } else { (b, a) };
                edges.insert(edge);
            }
        }
        edges.len()
    };

    let info = format!("{face_count} faces, {vertex_count} vertices, {edge_count} edges");
    commands.spawn((
        Text::new(info),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(tokens::TEXT_SECONDARY),
        ChildOf(parent),
    ));

    // Face properties container — populated dynamically by update_brush_face_properties
    commands.spawn((
        BrushFacePropsContainer,
        EditorEntity,
        Node {
            flex_direction: FlexDirection::Column,
            width: Val::Percent(100.0),
            row_gap: px(tokens::SPACING_XS),
            ..Default::default()
        },
        ChildOf(parent),
    ));
}

// ---------------------------------------------------------------------------
// Brush face properties (UV editing, texture info)
// ---------------------------------------------------------------------------

/// Tracks the last state we rendered so we only rebuild on change.
#[derive(Default)]
struct BrushFacePropsState {
    entity: Option<Entity>,
    faces: Vec<usize>,
    /// Hash of face data to detect UV edits
    data_hash: u64,
}

fn hash_face_data(face: &BrushFaceData) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Hash the fields we care about for display
    face.texture_path.hash(&mut hasher);
    face.uv_offset.x.to_bits().hash(&mut hasher);
    face.uv_offset.y.to_bits().hash(&mut hasher);
    face.uv_scale.x.to_bits().hash(&mut hasher);
    face.uv_scale.y.to_bits().hash(&mut hasher);
    face.uv_rotation.to_bits().hash(&mut hasher);
    face.material_index.hash(&mut hasher);
    hasher.finish()
}

fn update_brush_face_properties(
    mut commands: Commands,
    edit_mode: Res<EditMode>,
    brush_selection: Res<BrushSelection>,
    brushes: Query<&Brush>,
    container_query: Query<(Entity, Option<&Children>), With<BrushFacePropsContainer>>,
    texture_cache: Res<TextureMaterialCache>,
    mut local_state: Local<BrushFacePropsState>,
) {
    let Ok((container_entity, container_children)) = container_query.single() else {
        return;
    };

    let show = *edit_mode == EditMode::BrushEdit(BrushEditMode::Face)
        && !brush_selection.faces.is_empty()
        && brush_selection.entity.is_some();

    if !show {
        // Clear if we had content
        if local_state.entity.is_some() {
            if let Some(children) = container_children {
                for child in children.iter() {
                    commands.entity(child).despawn();
                }
            }
            *local_state = BrushFacePropsState::default();
        }
        return;
    }

    let brush_entity = brush_selection.entity.unwrap();
    let Ok(brush) = brushes.get(brush_entity) else {
        return;
    };

    // Compute hash of selected face data
    let mut combined_hash = 0u64;
    for &fi in &brush_selection.faces {
        if fi < brush.faces.len() {
            combined_hash = combined_hash.wrapping_add(hash_face_data(&brush.faces[fi]));
        }
    }

    // Check if anything changed
    if local_state.entity == Some(brush_entity)
        && local_state.faces == brush_selection.faces
        && local_state.data_hash == combined_hash
    {
        return;
    }

    // Rebuild UI
    if let Some(children) = container_children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }

    local_state.entity = Some(brush_entity);
    local_state.faces = brush_selection.faces.clone();
    local_state.data_hash = combined_hash;

    // Use first selected face for display values
    let first_face_idx = brush_selection.faces[0];
    let face = &brush.faces[first_face_idx];
    let multi = brush_selection.faces.len() > 1;

    // Header
    let header_text = if multi {
        format!("{} faces selected", brush_selection.faces.len())
    } else {
        format!("Face {}", first_face_idx)
    };
    commands.spawn((
        Text::new(header_text),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(tokens::TEXT_PRIMARY),
        Node {
            margin: UiRect::vertical(Val::Px(tokens::SPACING_XS)),
            ..Default::default()
        },
        ChildOf(container_entity),
    ));

    // Texture info
    let texture_text = match &face.texture_path {
        Some(path) => path.clone(),
        None => "No Texture".to_string(),
    };
    let tex_row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(tokens::SPACING_XS),
                width: Val::Percent(100.0),
                ..Default::default()
            },
            ChildOf(container_entity),
        ))
        .id();

    // Texture thumbnail if available
    if let Some(ref path) = face.texture_path {
        if let Some(entry) = texture_cache.entries.get(path) {
            commands.spawn((
                ImageNode::new(entry.image.clone()),
                Node {
                    width: Val::Px(32.0),
                    height: Val::Px(32.0),
                    flex_shrink: 0.0,
                    ..Default::default()
                },
                ChildOf(tex_row),
            ));
        }
    }

    commands.spawn((
        Text::new(texture_text),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(tokens::TEXT_SECONDARY),
        Node {
            flex_grow: 1.0,
            ..Default::default()
        },
        ChildOf(tex_row),
    ));

    // Clear texture button (only if texture is set)
    if face.texture_path.is_some() {
        let btn = commands
            .spawn((
                Node {
                    padding: UiRect::axes(Val::Px(tokens::SPACING_SM), Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..Default::default()
                },
                BackgroundColor(tokens::INPUT_BG),
                ChildOf(tex_row),
            ))
            .id();
        commands.spawn((
            Text::new("Clear"),
            TextFont {
                font_size: tokens::FONT_SM,
                ..Default::default()
            },
            TextColor(tokens::TEXT_PRIMARY),
            ChildOf(btn),
        ));
        commands.entity(btn).observe(
            |_: On<Pointer<Click>>, mut commands: Commands| {
                commands.trigger(ClearTextureFromFaces);
            },
        );
        commands.entity(btn).observe(
            |hover: On<Pointer<Over>>, mut bg: Query<&mut BackgroundColor>| {
                if let Ok(mut bg) = bg.get_mut(hover.event_target()) {
                    bg.0 = tokens::HOVER_BG;
                }
            },
        );
        commands.entity(btn).observe(
            |out: On<Pointer<Out>>, mut bg: Query<&mut BackgroundColor>| {
                if let Ok(mut bg) = bg.get_mut(out.event_target()) {
                    bg.0 = tokens::INPUT_BG;
                }
            },
        );
    }

    // UV Offset
    spawn_brush_face_field_row(
        &mut commands,
        container_entity,
        "UV Offset",
        face.uv_offset.x as f64,
        face.uv_offset.y as f64,
        BrushFaceField::UvOffsetX,
        BrushFaceField::UvOffsetY,
        brush_entity,
    );

    // UV Scale
    spawn_brush_face_field_row(
        &mut commands,
        container_entity,
        "UV Scale",
        face.uv_scale.x as f64,
        face.uv_scale.y as f64,
        BrushFaceField::UvScaleX,
        BrushFaceField::UvScaleY,
        brush_entity,
    );

    // UV Rotation
    let rot_row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(tokens::SPACING_XS),
                width: Val::Percent(100.0),
                ..Default::default()
            },
            ChildOf(container_entity),
        ))
        .id();

    commands.spawn((
        Text::new("Rotation"),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(tokens::TEXT_SECONDARY),
        Node {
            min_width: px(60.0),
            flex_shrink: 0.0,
            ..Default::default()
        },
        ChildOf(rot_row),
    ));

    let rotation_degrees = face.uv_rotation.to_degrees() as f64;
    commands
        .spawn((
            numeric_input::numeric_input(rotation_degrees),
            BrushFaceFieldBinding {
                field: BrushFaceField::UvRotation,
            },
            ChildOf(rot_row),
        ))
        .observe(
            move |changed: On<NumericValueChanged>,
                  brush_selection: Res<BrushSelection>,
                  mut brushes: Query<&mut Brush>,
                  mut history: ResMut<CommandHistory>| {
                apply_brush_face_field(
                    BrushFaceField::UvRotation,
                    changed.value,
                    &brush_selection,
                    &mut brushes,
                    &mut history,
                );
            },
        );
}

fn spawn_brush_face_field_row(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    x_value: f64,
    y_value: f64,
    x_field: BrushFaceField,
    y_field: BrushFaceField,
    _brush_entity: Entity,
) {
    let row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(tokens::SPACING_XS),
                width: Val::Percent(100.0),
                ..Default::default()
            },
            ChildOf(parent),
        ))
        .id();

    commands.spawn((
        Text::new(label),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(tokens::TEXT_SECONDARY),
        Node {
            min_width: px(60.0),
            flex_shrink: 0.0,
            ..Default::default()
        },
        ChildOf(row),
    ));

    // X input
    commands
        .spawn((
            numeric_input::numeric_input(x_value),
            BrushFaceFieldBinding { field: x_field },
            ChildOf(row),
        ))
        .observe(
            move |changed: On<NumericValueChanged>,
                  brush_selection: Res<BrushSelection>,
                  mut brushes: Query<&mut Brush>,
                  mut history: ResMut<CommandHistory>| {
                apply_brush_face_field(
                    x_field,
                    changed.value,
                    &brush_selection,
                    &mut brushes,
                    &mut history,
                );
            },
        );

    // Y input
    commands
        .spawn((
            numeric_input::numeric_input(y_value),
            BrushFaceFieldBinding { field: y_field },
            ChildOf(row),
        ))
        .observe(
            move |changed: On<NumericValueChanged>,
                  brush_selection: Res<BrushSelection>,
                  mut brushes: Query<&mut Brush>,
                  mut history: ResMut<CommandHistory>| {
                apply_brush_face_field(
                    y_field,
                    changed.value,
                    &brush_selection,
                    &mut brushes,
                    &mut history,
                );
            },
        );
}

fn apply_brush_face_field(
    field: BrushFaceField,
    value: f64,
    brush_selection: &BrushSelection,
    brushes: &mut Query<&mut Brush>,
    history: &mut CommandHistory,
) {
    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(mut brush) = brushes.get_mut(brush_entity) else {
        return;
    };

    let old = brush.clone();
    for &face_idx in &brush_selection.faces {
        if face_idx >= brush.faces.len() {
            continue;
        }
        let face = &mut brush.faces[face_idx];
        match field {
            BrushFaceField::UvOffsetX => face.uv_offset.x = value as f32,
            BrushFaceField::UvOffsetY => face.uv_offset.y = value as f32,
            BrushFaceField::UvScaleX => face.uv_scale.x = value as f32,
            BrushFaceField::UvScaleY => face.uv_scale.y = value as f32,
            BrushFaceField::UvRotation => face.uv_rotation = (value as f32).to_radians(),
        }
    }

    let cmd = SetBrush {
        entity: brush_entity,
        old,
        new: brush.clone(),
        label: "Edit face UV".to_string(),
    };
    history.undo_stack.push(Box::new(cmd));
    history.redo_stack.clear();
}

fn handle_clear_texture(
    _event: On<ClearTextureFromFaces>,
    brush_selection: Res<BrushSelection>,
    edit_mode: Res<EditMode>,
    mut brushes: Query<&mut Brush>,
    mut history: ResMut<CommandHistory>,
) {
    if *edit_mode != EditMode::BrushEdit(BrushEditMode::Face) {
        return;
    }
    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    if brush_selection.faces.is_empty() {
        return;
    }
    let Ok(mut brush) = brushes.get_mut(brush_entity) else {
        return;
    };

    let old = brush.clone();
    for &face_idx in &brush_selection.faces {
        if face_idx < brush.faces.len() {
            brush.faces[face_idx].texture_path = None;
        }
    }

    let cmd = SetBrush {
        entity: brush_entity,
        old,
        new: brush.clone(),
        label: "Clear texture".to_string(),
    };
    history.undo_stack.push(Box::new(cmd));
    history.redo_stack.clear();
}

// ---------------------------------------------------------------------------

fn spawn_custom_properties_display(
    commands: &mut Commands,
    parent: Entity,
    source_entity: Entity,
    cp: &CustomProperties,
    editor_font: &Handle<Font>,
    icon_font: &Handle<Font>,
) {
    // Render each property row based on its variant type
    for (prop_name, prop_value) in &cp.properties {
        let row = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(tokens::SPACING_XS),
                    width: Val::Percent(100.0),
                    ..Default::default()
                },
                ChildOf(parent),
            ))
            .id();

        // Property name label
        commands.spawn((
            Text::new(format!("{}:", prop_name)),
            TextFont {
                font: editor_font.clone(),
                font_size: tokens::FONT_SM,
                ..Default::default()
            },
            Node {
                min_width: px(20.0),
                flex_shrink: 0.0,
                ..Default::default()
            },
            TextColor(tokens::TEXT_PRIMARY),
            ChildOf(row),
        ));

        let name = prop_name.clone();
        match prop_value {
            PropertyValue::Bool(val) => {
                let checked = *val;
                commands
                    .spawn((
                        checkbox(CheckboxProps::new("").checked(checked), editor_font, icon_font),
                        CustomPropertyBinding {
                            source_entity,
                            property_name: name,
                        },
                        ChildOf(row),
                    ));
            }
            PropertyValue::Int(val) => {
                let n = name.clone();
                commands
                    .spawn((numeric_input::numeric_input(*val as f64), ChildOf(row)))
                    .observe(
                        move |changed: On<NumericValueChanged>, mut commands: Commands| {
                            let n = n.clone();
                            let val = changed.value as i64;
                            commands.queue(move |world: &mut World| {
                                apply_custom_property_with_undo(
                                    world,
                                    source_entity,
                                    &n,
                                    PropertyValue::Int(val),
                                );
                            });
                        },
                    );
            }
            PropertyValue::Float(val) => {
                let n = name.clone();
                commands
                    .spawn((numeric_input::numeric_input(*val), ChildOf(row)))
                    .observe(
                        move |changed: On<NumericValueChanged>, mut commands: Commands| {
                            let n = n.clone();
                            let val = changed.value;
                            commands.queue(move |world: &mut World| {
                                apply_custom_property_with_undo(
                                    world,
                                    source_entity,
                                    &n,
                                    PropertyValue::Float(val),
                                );
                            });
                        },
                    );
            }
            PropertyValue::String(val) => {
                let n = name.clone();
                let input = commands
                    .spawn((text_input::text_input(""), ChildOf(row)))
                    .insert(TextInput::new(val.clone()))
                    .observe(
                        move |text: On<EnteredText>, mut commands: Commands| {
                            let n = n.clone();
                            let value = text.value.clone();
                            commands.queue(move |world: &mut World| {
                                apply_custom_property_with_undo(
                                    world,
                                    source_entity,
                                    &n,
                                    PropertyValue::String(value),
                                );
                            });
                        },
                    )
                    .id();
                commands
                    .entity(input)
                    .entry::<Node>()
                    .and_modify(|mut node| {
                        node.width = Val::Auto;
                        node.flex_grow = 1.0;
                        node.flex_basis = Val::Px(0.0);
                    });
            }
            PropertyValue::Vec2(val) => {
                let v = *val;
                let n_x = name.clone();
                let n_y = name.clone();
                spawn_custom_axis(commands, row, "X", v.x as f64, AXIS_X_COLOR, source_entity, n_x, |new_f, old| {
                    if let PropertyValue::Vec2(v) = old { v.x = new_f as f32; }
                });
                spawn_custom_axis(commands, row, "Y", v.y as f64, AXIS_Y_COLOR, source_entity, n_y, |new_f, old| {
                    if let PropertyValue::Vec2(v) = old { v.y = new_f as f32; }
                });
            }
            PropertyValue::Vec3(val) => {
                let v = *val;
                let n_x = name.clone();
                let n_y = name.clone();
                let n_z = name.clone();
                spawn_custom_axis(commands, row, "X", v.x as f64, AXIS_X_COLOR, source_entity, n_x, |new_f, old| {
                    if let PropertyValue::Vec3(v) = old { v.x = new_f as f32; }
                });
                spawn_custom_axis(commands, row, "Y", v.y as f64, AXIS_Y_COLOR, source_entity, n_y, |new_f, old| {
                    if let PropertyValue::Vec3(v) = old { v.y = new_f as f32; }
                });
                spawn_custom_axis(commands, row, "Z", v.z as f64, AXIS_Z_COLOR, source_entity, n_z, |new_f, old| {
                    if let PropertyValue::Vec3(v) = old { v.z = new_f as f32; }
                });
            }
            PropertyValue::Color(val) => {
                let srgba = val.to_srgba();
                let rgba = [srgba.red, srgba.green, srgba.blue, srgba.alpha];
                let n = name.clone();
                commands
                    .spawn((
                        color_picker(ColorPickerProps::new().with_color(rgba)),
                        ChildOf(row),
                    ))
                    .observe(
                        move |event: On<ColorPickerCommitEvent>, mut commands: Commands| {
                            let color = event.color;
                            let n = n.clone();
                            commands.queue(move |world: &mut World| {
                                let new_color = Color::srgba(color[0], color[1], color[2], color[3]);
                                apply_custom_property_with_undo(
                                    world,
                                    source_entity,
                                    &n,
                                    PropertyValue::Color(new_color),
                                );
                            });
                        },
                    );
            }
        }

        // Remove property button (X icon)
        let n = prop_name.clone();
        commands.spawn((
            Text::new(String::from(Icon::X.unicode())),
            TextFont {
                font: icon_font.clone(),
                font_size: tokens::FONT_SM,
                ..Default::default()
            },
            TextColor(tokens::TEXT_SECONDARY),
            ChildOf(row),
            observe(move |_: On<Pointer<Click>>, mut commands: Commands| {
                let n = n.clone();
                commands.queue(move |world: &mut World| {
                    remove_custom_property(world, source_entity, &n);
                });
            }),
        ));
    }

    // "Add Property" row
    spawn_add_property_row(commands, parent, source_entity, editor_font, icon_font);
}

fn spawn_custom_axis(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    value: f64,
    label_color: Color,
    source_entity: Entity,
    property_name: String,
    mutate: fn(f64, &mut PropertyValue),
) {
    commands.spawn((
        Text::new(label),
        TextFont {
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(label_color),
        Node {
            flex_shrink: 0.0,
            ..Default::default()
        },
        ChildOf(parent),
    ));

    let n = property_name;
    commands
        .spawn((numeric_input::numeric_input(value), ChildOf(parent)))
        .observe(
            move |changed: On<NumericValueChanged>, mut commands: Commands| {
                let n = n.clone();
                let new_f = changed.value;
                commands.queue(move |world: &mut World| {
                    // Read current value, mutate the axis, apply
                    let Some(cp) = world.get::<CustomProperties>(source_entity) else {
                        return;
                    };
                    let Some(current) = cp.properties.get(&n) else {
                        return;
                    };
                    let mut new_val = current.clone();
                    mutate(new_f, &mut new_val);
                    apply_custom_property_with_undo(world, source_entity, &n, new_val);
                });
            },
        );
}

fn spawn_add_property_row(
    commands: &mut Commands,
    parent: Entity,
    source_entity: Entity,
    _editor_font: &Handle<Font>,
    icon_font: &Handle<Font>,
) {
    let row = commands
        .spawn((
            CustomPropertyAddRow,
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(tokens::SPACING_XS),
                width: Val::Percent(100.0),
                padding: UiRect::top(Val::Px(tokens::SPACING_SM)),
                ..Default::default()
            },
            ChildOf(parent),
        ))
        .id();

    // Name input
    commands
        .spawn((
            CustomPropertyNameInput,
            text_input::text_input("name..."),
            ChildOf(row),
        ));

    // Type selector ComboBox
    let type_names: Vec<String> = PropertyValue::all_type_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    commands
        .spawn((
            CustomPropertyTypeSelector,
            combobox_with_selected(type_names, 2), // default to "Float"
            ChildOf(row),
        ));

    // Confirm button
    let font = icon_font.clone();
    commands.spawn((
        Text::new(String::from(Icon::Plus.unicode())),
        TextFont {
            font,
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(tokens::TEXT_ACCENT),
        ChildOf(row),
        observe(move |_: On<Pointer<Click>>, mut commands: Commands| {
            commands.queue(move |world: &mut World| {
                add_custom_property_from_ui(world, source_entity);
            });
        }),
    ));
}

/// Read the name input and type selector, then add a new property.
fn add_custom_property_from_ui(world: &mut World, source_entity: Entity) {
    // Read the name input value
    let name = {
        let mut query = world.query_filtered::<&TextInput, With<CustomPropertyNameInput>>();
        let Some(input) = query.iter(world).next() else {
            return;
        };
        let name = input.value.trim().to_string();
        if name.is_empty() {
            return;
        }
        name
    };

    // Read the type selector
    let type_name = {
        let mut query = world.query_filtered::<&ComboBoxSelectedIndex, With<CustomPropertyTypeSelector>>();
        let Some(index) = query.iter(world).next() else {
            return;
        };
        let all_types = PropertyValue::all_type_names();
        let idx = index.0.min(all_types.len().saturating_sub(1));
        all_types[idx].to_string()
    };

    let Some(default_value) = PropertyValue::default_for_type(&type_name) else {
        return;
    };

    let Some(cp) = world.get::<CustomProperties>(source_entity) else {
        return;
    };
    let old = cp.clone();
    let mut new = old.clone();
    new.properties.insert(name, default_value);

    let cmd = SetCustomProperties {
        entity: source_entity,
        old_properties: old,
        new_properties: new,
    };
    cmd.execute(world);

    let mut history = world.resource_mut::<CommandHistory>();
    history.undo_stack.push(Box::new(cmd));
    history.redo_stack.clear();

    // Rebuild inspector
    rebuild_inspector(world, source_entity);
}

/// Remove a property and push undo.
fn remove_custom_property(world: &mut World, source_entity: Entity, property_name: &str) {
    let Some(cp) = world.get::<CustomProperties>(source_entity) else {
        return;
    };
    let old = cp.clone();
    let mut new = old.clone();
    new.properties.remove(property_name);

    let cmd = SetCustomProperties {
        entity: source_entity,
        old_properties: old,
        new_properties: new,
    };
    cmd.execute(world);

    let mut history = world.resource_mut::<CommandHistory>();
    history.undo_stack.push(Box::new(cmd));
    history.redo_stack.clear();

    rebuild_inspector(world, source_entity);
}

/// Apply a custom property value change with undo.
fn apply_custom_property_with_undo(
    world: &mut World,
    source_entity: Entity,
    property_name: &str,
    new_value: PropertyValue,
) {
    let Some(cp) = world.get::<CustomProperties>(source_entity) else {
        return;
    };
    let old = cp.clone();
    let mut new = old.clone();
    new.properties.insert(property_name.to_string(), new_value);

    let cmd = SetCustomProperties {
        entity: source_entity,
        old_properties: old,
        new_properties: new,
    };
    cmd.execute(world);

    let mut history = world.resource_mut::<CommandHistory>();
    history.undo_stack.push(Box::new(cmd));
    history.redo_stack.clear();
}

/// Handle checkbox commit for custom property booleans.
fn on_custom_property_checkbox_commit(
    event: On<CheckboxCommitEvent>,
    bindings: Query<&CustomPropertyBinding>,
    mut commands: Commands,
) {
    let Ok(binding) = bindings.get(event.entity) else {
        return;
    };
    let source = binding.source_entity;
    let name = binding.property_name.clone();
    let checked = event.checked;
    commands.queue(move |world: &mut World| {
        apply_custom_property_with_undo(world, source, &name, PropertyValue::Bool(checked));
    });
}

/// Force inspector rebuild by toggling Selected.
fn rebuild_inspector(world: &mut World, source_entity: Entity) {
    if let Ok(mut ec) = world.get_entity_mut(source_entity) {
        ec.remove::<Selected>();
    }
    world.entity_mut(source_entity).insert(Selected);
}
