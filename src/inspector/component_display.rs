use crate::EditorEntity;
use crate::custom_properties::CustomProperties;
use crate::selection::{Selected, Selection};
use std::any::TypeId;

use bevy::{
    ecs::{
        archetype::Archetype,
        component::{ComponentId, Components},
        reflect::{AppTypeRegistry, ReflectComponent},
    },
    prelude::*,
};
use editor_feathers::{
    icons::{EditorFont, Icon, IconFont},
    tokens,
};
use editor_widgets::collapsible::{
    CollapsibleBody, CollapsibleHeader, CollapsibleSection, ToggleCollapsible,
};

use super::{
    AddComponentButton, ComponentDisplay, ComponentDisplayBody, ComponentPicker,
    ReflectDisplayable, Inspector, extract_module_group,
    custom_props_display, material_display, brush_display, reflect_fields,
};

pub(crate) fn add_component_displays(
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
                material_display::spawn_material_display_deferred(
                    &mut commands,
                    body_entity,
                    source_entity,
                );
                continue;
            }

            // Priority 3: CustomProperties — specialized property editor
            if type_id == TypeId::of::<CustomProperties>() {
                if let Some(cp) = reflected.downcast_ref::<CustomProperties>() {
                    custom_props_display::spawn_custom_properties_display(
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
                    brush_display::spawn_brush_display(
                        &mut commands,
                        body_entity,
                        brush,
                    );
                }
                continue;
            }

            // Priority 3: Generic reflection display
            reflect_fields::spawn_reflected_fields(
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
        bevy::feathers::controls::button(bevy::feathers::controls::ButtonProps::default(), (), Spawn(Text::new("+"))),
        ChildOf(*inspector),
    ));

}

pub(crate) fn remove_component_displays(
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

    // Toggle area (chevron + title) -- click to collapse/expand
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
        bevy::ui_widgets::observe(move |_: On<Pointer<Click>>, mut commands: Commands| {
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
