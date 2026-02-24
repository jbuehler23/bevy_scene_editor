use crate::commands::{CommandHistory, EditorCommand};
use crate::custom_properties::{CustomProperties, PropertyValue, SetCustomProperties};

use bevy::prelude::*;
use bevy::ui_widgets::observe;
use editor_feathers::{
    checkbox::{CheckboxCommitEvent, CheckboxProps, checkbox},
    color_picker::{ColorPickerCommitEvent, ColorPickerProps, color_picker},
    icons::Icon,
    numeric_input, text_input, tokens,
};
use editor_widgets::numeric_input::NumericValueChanged;
use editor_widgets::text_input::{EnteredText, TextInput};
use editor_feathers::combobox::{ComboBoxSelectedIndex, combobox_with_selected};

use super::{
    AXIS_X_COLOR, AXIS_Y_COLOR, AXIS_Z_COLOR,
    CustomPropertyAddRow, CustomPropertyBinding, CustomPropertyNameInput,
    CustomPropertyTypeSelector, rebuild_inspector,
};

pub(super) fn spawn_custom_properties_display(
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
pub(crate) fn on_custom_property_checkbox_commit(
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
