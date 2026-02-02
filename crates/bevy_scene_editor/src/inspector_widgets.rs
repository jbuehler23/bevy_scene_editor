use bevy::prelude::*;
use bevy::reflect::{ReflectRef, PartialReflect};
use crate::state::EditorEntity;

const LABEL_COLOR: Color = Color::srgba(0.6, 0.6, 0.6, 1.0);
const VALUE_COLOR: Color = Color::srgba(0.9, 0.9, 0.9, 1.0);
const INDENT_PX: f32 = 12.0;

pub fn spawn_reflect_widget(
    commands: &mut Commands,
    parent: Entity,
    value: &dyn PartialReflect,
    depth: u32,
) {
    match value.reflect_ref() {
        ReflectRef::Struct(s) => {
            for i in 0..s.field_len() {
                let field_name = s.name_at(i).unwrap_or("?");
                let field_value = s.field_at(i).unwrap();
                spawn_field_row(commands, parent, field_name, field_value, depth);
            }
        }
        ReflectRef::TupleStruct(ts) => {
            for i in 0..ts.field_len() {
                let field_value = ts.field(i).unwrap();
                spawn_field_row(commands, parent, &format!("{i}"), field_value, depth);
            }
        }
        ReflectRef::Tuple(t) => {
            for i in 0..t.field_len() {
                let field_value = t.field(i).unwrap();
                spawn_field_row(commands, parent, &format!("{i}"), field_value, depth);
            }
        }
        ReflectRef::Enum(e) => {
            let variant = e.variant_name();
            let row = commands
                .spawn((
                    EditorEntity,
                    field_row_node(depth),
                    children![
                        (
                            Text::new(format!("Variant: {variant}")),
                            TextFont { font_size: 11.0, ..default() },
                            TextColor(VALUE_COLOR),
                        ),
                    ],
                ))
                .id();
            commands.entity(parent).add_child(row);

            // Show enum fields
            for i in 0..e.field_len() {
                let name = e.name_at(i).map(|s| s.to_string()).unwrap_or_else(|| format!("{i}"));
                let field = e.field_at(i).unwrap();
                spawn_field_row(commands, parent, &name, field, depth + 1);
            }
        }
        ReflectRef::List(list) => {
            for i in 0..list.len().min(20) {
                let item = list.get(i).unwrap();
                spawn_field_row(commands, parent, &format!("[{i}]"), item, depth);
            }
            if list.len() > 20 {
                let row = commands
                    .spawn((
                        EditorEntity,
                        field_row_node(depth),
                        children![
                            (
                                Text::new(format!("... and {} more", list.len() - 20)),
                                TextFont { font_size: 11.0, ..default() },
                                TextColor(LABEL_COLOR),
                            ),
                        ],
                    ))
                    .id();
                commands.entity(parent).add_child(row);
            }
        }
        ReflectRef::Map(map) => {
            let row = commands
                .spawn((
                    EditorEntity,
                    field_row_node(depth),
                    children![
                        (
                            Text::new(format!("Map ({} entries)", map.len())),
                            TextFont { font_size: 11.0, ..default() },
                            TextColor(LABEL_COLOR),
                        ),
                    ],
                ))
                .id();
            commands.entity(parent).add_child(row);
        }
        _ => {
            // Opaque/value type - show debug representation
            let text = format!("{value:?}");
            let row = commands
                .spawn((
                    EditorEntity,
                    field_row_node(depth),
                    children![
                        (
                            Text::new(text),
                            TextFont { font_size: 11.0, ..default() },
                            TextColor(VALUE_COLOR),
                        ),
                    ],
                ))
                .id();
            commands.entity(parent).add_child(row);
        }
    }
}

fn spawn_field_row(
    commands: &mut Commands,
    parent: Entity,
    name: &str,
    value: &dyn PartialReflect,
    depth: u32,
) {
    // Check if it's a simple value we can display inline
    match value.reflect_ref() {
        ReflectRef::Struct(_) | ReflectRef::TupleStruct(_) | ReflectRef::Enum(_)
        | ReflectRef::List(_) | ReflectRef::Map(_) => {
            // Complex type: show name as header, recurse
            let header = commands
                .spawn((
                    EditorEntity,
                    field_row_node(depth),
                    children![
                        (
                            Text::new(format!("▸ {name}")),
                            TextFont { font_size: 11.0, ..default() },
                            TextColor(LABEL_COLOR),
                        ),
                    ],
                ))
                .id();
            commands.entity(parent).add_child(header);
            spawn_reflect_widget(commands, parent, value, depth + 1);
        }
        _ => {
            // Simple value: show name: value inline
            let display = format!("{value:?}");
            let row = commands
                .spawn((
                    EditorEntity,
                    field_row_node(depth),
                    children![
                        (
                            Text::new(format!("{name}: ")),
                            TextFont { font_size: 11.0, ..default() },
                            TextColor(LABEL_COLOR),
                        ),
                        (
                            Text::new(display),
                            TextFont { font_size: 11.0, ..default() },
                            TextColor(VALUE_COLOR),
                        ),
                    ],
                ))
                .id();
            commands.entity(parent).add_child(row);
        }
    }
}

fn field_row_node(depth: u32) -> Node {
    Node {
        width: Val::Percent(100.0),
        height: Val::Px(20.0),
        padding: UiRect::left(Val::Px(8.0 + depth as f32 * INDENT_PX)),
        align_items: AlignItems::Center,
        flex_direction: FlexDirection::Row,
        ..default()
    }
}
