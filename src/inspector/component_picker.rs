use crate::EditorEntity;
use crate::commands::EditorCommand;
use crate::selection::{Selected, Selection};
use std::any::TypeId;
use std::collections::HashSet;

use bevy::{
    ecs::{
        archetype::Archetype,
        component::{ComponentId, Components},
        reflect::{AppTypeRegistry, ReflectComponent},
    },
    feathers::theme::ThemedText,
    input_focus::InputFocus,
    prelude::*,
    ui_widgets::observe,
};
use jackdaw_feathers::tokens;
use jackdaw_widgets::text_input::TextInput;

use super::{
    AddComponentButton, ComponentPicker, ComponentPickerEntry, ComponentPickerSearch, Inspector,
};

/// Handle click on the "+" button to open the component picker.
pub(crate) fn on_add_component_button_click(
    event: On<jackdaw_feathers::button::ButtonClickEvent>,
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
    if let Some(picker) = existing_pickers.iter().next() {
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
        if full_path.starts_with("jackdaw") {
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
            jackdaw_feathers::text_input::text_input("Search components..."),
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
pub(crate) fn filter_component_picker(
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
