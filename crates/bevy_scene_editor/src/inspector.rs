use bevy::prelude::*;
use bevy::reflect::PartialReflect;
use crate::state::{EditorEntity, EditorState, RebuildRequest};
use crate::layout::InspectorPanel;

#[derive(Component)]
pub struct InspectorRoot;

pub fn rebuild_inspector_system(world: &mut World) {
    let rebuild_inspector = {
        let rebuild = world.resource::<RebuildRequest>();
        rebuild.inspector
    };
    if !rebuild_inspector {
        return;
    }
    world.resource_mut::<RebuildRequest>().inspector = false;

    let selected = world.resource::<EditorState>().selected_entity;

    // Find the panel entity
    let mut panel_query = world.query_filtered::<Entity, With<InspectorPanel>>();
    let panel_entity = {
        let results: Vec<Entity> = panel_query.iter(world).collect();
        match results.into_iter().next() {
            Some(e) => e,
            None => return,
        }
    };

    // Collect component data before spawning UI
    struct ComponentData {
        type_name: String,
        short_name: String,
        fields: Vec<(String, String)>, // (field_name, field_value)
    }

    let mut components: Vec<ComponentData> = Vec::new();

    if let Some(selected_entity) = selected {
        if let Some(entity_ref) = world.get_entity(selected_entity).ok() {
            let registry = world.resource::<AppTypeRegistry>().clone();
            let registry = registry.read();

            for component_id in entity_ref.archetype().components() {
                let Some(component_info) = world.components().get_info(*component_id) else {
                    continue;
                };

                let type_name = format!("{}", component_info.name());

                if type_name.contains("EditorEntity")
                    || type_name.contains("bevy_ui")
                    || type_name.contains("Node")
                    || type_name.contains("Style")
                {
                    continue;
                }

                let type_id = match component_info.type_id() {
                    Some(id) => id,
                    None => continue,
                };

                let short_name = type_name
                    .rsplit("::")
                    .next()
                    .unwrap_or(&type_name)
                    .to_string();

                let mut fields = Vec::new();

                if let Some(reg) = registry.get(type_id) {
                    if let Some(reflect_component) = reg.data::<ReflectComponent>() {
                        if let Some(reflected) = reflect_component.reflect(entity_ref) {
                            collect_fields(reflected as &dyn PartialReflect, &mut fields, "", 0);
                        }
                    }
                }

                components.push(ComponentData {
                    type_name,
                    short_name,
                    fields,
                });
            }
        }
    }

    // Now spawn UI using commands
    let mut commands = world.commands();

    commands.entity(panel_entity).despawn_children();

    if selected.is_none() {
        let root = commands
            .spawn((
                EditorEntity,
                InspectorRoot,
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                children![
                    (
                        Text::new("No entity selected".to_string()),
                        TextFont { font_size: 13.0, ..default() },
                        TextColor(Color::srgba(0.5, 0.5, 0.5, 1.0)),
                    ),
                ],
            ))
            .id();
        commands.entity(panel_entity).add_child(root);
        return;
    }

    let selected_entity = selected.unwrap();

    let root = commands
        .spawn((
            EditorEntity,
            InspectorRoot,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                flex_grow: 1.0,
                ..default()
            },
        ))
        .id();

    let header = commands
        .spawn((
            EditorEntity,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor::from(Color::srgba(0.18, 0.18, 0.18, 1.0)),
            children![
                (
                    Text::new(format!("Entity {:?}", selected_entity)),
                    TextFont { font_size: 14.0, ..default() },
                    TextColor(Color::srgba(0.9, 0.9, 0.9, 1.0)),
                ),
            ],
        ))
        .id();
    commands.entity(root).add_child(header);

    for comp in &components {
        let section = commands
            .spawn((
                EditorEntity,
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(4.0)),
                    margin: UiRect::top(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor::from(Color::srgba(0.15, 0.15, 0.15, 1.0)),
            ))
            .id();

        let comp_header = commands
            .spawn((
                EditorEntity,
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                    ..default()
                },
                children![
                    (
                        Text::new(comp.short_name.clone()),
                        TextFont { font_size: 12.0, ..default() },
                        TextColor(Color::srgba(0.7, 0.85, 1.0, 1.0)),
                    ),
                ],
            ))
            .id();
        commands.entity(section).add_child(comp_header);

        for (field_name, field_value) in &comp.fields {
            let row = commands
                .spawn((
                    EditorEntity,
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(16.0), Val::Px(2.0)),
                        column_gap: Val::Px(8.0),
                        ..default()
                    },
                    children![
                        (
                            Text::new(format!("{}: {}", field_name, field_value)),
                            TextFont { font_size: 11.0, ..default() },
                            TextColor(Color::srgba(0.7, 0.7, 0.7, 1.0)),
                        ),
                    ],
                ))
                .id();
            commands.entity(section).add_child(row);
        }

        commands.entity(root).add_child(section);
    }

    commands.entity(panel_entity).add_child(root);
}

fn collect_fields(
    value: &dyn PartialReflect,
    fields: &mut Vec<(String, String)>,
    prefix: &str,
    depth: u32,
) {
    use bevy::reflect::ReflectRef;

    if depth > 3 {
        return;
    }

    match value.reflect_ref() {
        ReflectRef::Struct(s) => {
            for i in 0..s.field_len() {
                let name = s.name_at(i).unwrap_or("?");
                let field = s.field_at(i).unwrap();
                let full_name = if prefix.is_empty() {
                    name.to_string()
                } else {
                    format!("{}.{}", prefix, name)
                };
                // For simple types, show value directly
                match field.reflect_ref() {
                    ReflectRef::Struct(_) | ReflectRef::TupleStruct(_) | ReflectRef::List(_) | ReflectRef::Map(_) => {
                        collect_fields(field, fields, &full_name, depth + 1);
                    }
                    _ => {
                        fields.push((full_name, format!("{:?}", field)));
                    }
                }
            }
        }
        ReflectRef::TupleStruct(ts) => {
            for i in 0..ts.field_len() {
                let field = ts.field(i).unwrap();
                let full_name = if prefix.is_empty() {
                    format!("{}", i)
                } else {
                    format!("{}.{}", prefix, i)
                };
                fields.push((full_name, format!("{:?}", field)));
            }
        }
        _ => {
            let name = if prefix.is_empty() { "value".to_string() } else { prefix.to_string() };
            fields.push((name, format!("{:?}", value)));
        }
    }
}
