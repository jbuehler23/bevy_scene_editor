use crate::EditorEntity;
use std::any::TypeId;

use bevy::{
    ecs::{
        archetype::Archetype,
        component::{ComponentId, Components},
        lifecycle::HookContext,
        reflect::{AppTypeRegistry, ReflectComponent},
        world::DeferredWorld,
    },
    feathers::{
        controls::{ButtonProps, button},
        theme::ThemedText,
    },
    prelude::*,
    reflect::ReflectRef,
    ui_widgets::observe,
};
use editor_feathers::{list_view, text_input};
use editor_widgets::text_input::{EnteredText, TextInput};

const MAX_REFLECT_DEPTH: usize = 2;
const LINK_COLOR: Color = Color::srgba(0.4, 0.6, 1.0, 1.0);
const LINK_HOVER_COLOR: Color = Color::srgba(0.6, 0.8, 1.0, 1.0);
const MAX_LIST_ITEMS: usize = 20;

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
#[component(on_add)]
pub struct SelectedEntity;
impl SelectedEntity {
    pub fn on_add(mut world: DeferredWorld, ctx: HookContext) {
        let previous = world
            .try_query_filtered::<Entity, With<Self>>()
            .unwrap()
            .iter(&world)
            .filter(|entity| *entity != ctx.entity)
            .collect::<Vec<_>>();

        world.commands().queue(|world: &mut World| {
            for entity in previous {
                world.entity_mut(entity).remove::<Self>();
            }
        });
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
            .add_observer(add_component_displays);
    }
}

#[derive(Component)]
pub struct ComponentDisplay;

#[derive(Component)]
struct ComponentDisplayBody;

#[derive(Component)]
struct AddComponentButton;

fn add_component_displays(
    _: On<Add, SelectedEntity>,
    mut commands: Commands,
    components: &Components,
    type_registry: Res<AppTypeRegistry>,
    selected_entity: Single<(&Archetype, EntityRef), (With<SelectedEntity>, Without<EditorEntity>)>,
    inspector: Single<Entity, With<Inspector>>,
    names: Query<&Name>,
) {
    let (archetype, entity_ref) = selected_entity.into_inner();
    let source_entity = entity_ref.entity();
    let registry = type_registry.read();

    let mut comp_list = archetype
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
                return Some((short, component_id));
            }

            // Fallback: use Components name
            let name = components.get_name(component_id)?;
            if name.starts_with("bevy_scene_editor") {
                return None;
            }
            Some((name.shortname().to_string(), component_id))
        })
        .collect::<Vec<_>>();

    comp_list.sort_by_key(|(name, _)| name.to_lowercase());

    for (name, component_id) in comp_list {
        let (display_entity, body_entity) =
            spawn_component_display(&mut commands, &name, source_entity, component_id);
        commands.entity(display_entity).insert(ChildOf(*inspector));

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

            // Priority 2: Generic reflection display
            spawn_reflected_fields(
                &mut commands,
                body_entity,
                reflected,
                0,
                String::new(),
                source_entity,
                type_id,
                &names,
            );
            continue;
        }

        // Fallback: no reflection data
        commands.spawn((
            Text::new("(no reflection data)"),
            TextFont {
                font_size: 12.,
                ..Default::default()
            },
            ThemedText,
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
    _: On<Remove, SelectedEntity>,
    mut commands: Commands,
    inspector: Single<(Entity, Option<&Children>), With<Inspector>>,
    displays: Query<Entity, Or<(With<ComponentDisplay>, With<AddComponentButton>)>>,
) {
    let (_entity, children) = inspector.into_inner();

    let Some(children) = children else {
        return;
    };

    for child in displays.iter_many(children.collection()) {
        commands.entity(child).despawn();
    }
}

const INPUT_BORDER: Color = Color::srgba(0.3, 0.3, 0.3, 1.0);

fn spawn_component_display(
    commands: &mut Commands,
    name: &str,
    entity: Entity,
    component: ComponentId,
) -> (Entity, Entity) {
    let body_entity = commands
        .spawn((
            ComponentDisplayBody,
            Node {
                padding: px(4).all(),
                flex_direction: FlexDirection::Column,
                ..Default::default()
            },
        ))
        .id();

    let name_owned = name.to_string();

    let display_entity = commands
        .spawn((
            ComponentDisplay,
            Node {
                flex_direction: FlexDirection::Column,
                width: percent(100),
                border: px(2).all(),
                border_radius: BorderRadius::all(px(5)),
                ..Default::default()
            },
            BorderColor::all(INPUT_BORDER),
        ))
        .with_children(|parent| {
            // Header row
            parent
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        ..Default::default()
                    },
                    BackgroundColor(INPUT_BORDER),
                ))
                .with_children(|header| {
                    header.spawn((
                        Text::new(">"),
                        TextFont {
                            font_size: 13.,
                            ..Default::default()
                        },
                        ThemedText,
                    ));
                    header.spawn((
                        Text::new(name_owned),
                        TextFont {
                            font_size: 13.,
                            ..Default::default()
                        },
                        ThemedText,
                    ));
                    header.spawn((
                        Text::new("-"),
                        TextFont {
                            font_size: 13.,
                            ..Default::default()
                        },
                        ThemedText,
                        observe(move |_: On<Pointer<Click>>, mut commands: Commands| {
                            commands.entity(entity).remove_by_id(component);
                        }),
                    ));
                });
        })
        .id();

    // Attach body as child of display
    commands.entity(body_entity).insert(ChildOf(display_entity));

    (display_entity, body_entity)
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
                );
            }
        }
        ReflectRef::Enum(e) => {
            spawn_text_row(
                commands,
                parent,
                &format!("variant: {}", e.variant_name()),
                depth,
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
        _ => {
            spawn_text_row(commands, parent, &format_reflect_value(reflected), depth);
        }
    }
}

fn is_editable_primitive(value: &dyn PartialReflect) -> bool {
    value.try_downcast_ref::<f32>().is_some()
        || value.try_downcast_ref::<f64>().is_some()
        || value.try_downcast_ref::<i32>().is_some()
        || value.try_downcast_ref::<u32>().is_some()
        || value.try_downcast_ref::<usize>().is_some()
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
) {
    // Entity reference → clickable link (before any other check)
    if let Some(&entity_val) = value.try_downcast_ref::<Entity>() {
        let left_padding = depth as f32 * 8.0;
        let row = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(4),
                    padding: UiRect::left(px(left_padding)),
                    ..Default::default()
                },
                ChildOf(parent),
            ))
            .id();
        commands.spawn((
            Text::new(format!("{name}:")),
            TextFont {
                font_size: 12.,
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
        let label = entity_names
            .get(entity_val)
            .map(|n| format!("{} ({entity_val})", n.as_str()))
            .unwrap_or_else(|_| format!("{entity_val}"));
        spawn_entity_link(commands, row, entity_val, &label);
        return;
    }

    // List/Array → expand with ListView
    if let ReflectRef::List(list) = value.reflect_ref() {
        spawn_text_row(commands, parent, &format!("{name}: [{} items]", list.len()), depth);
        if !list.is_empty() {
            let lv = commands.spawn((list_view::list_view(), ChildOf(parent))).id();
            let display_count = list.len().min(MAX_LIST_ITEMS);
            for i in 0..display_count {
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
            if list.len() > MAX_LIST_ITEMS {
                spawn_text_row(
                    commands,
                    lv,
                    &format!("... and {} more", list.len() - MAX_LIST_ITEMS),
                    depth + 1,
                );
            }
        }
        return;
    }
    if let ReflectRef::Array(array) = value.reflect_ref() {
        spawn_text_row(commands, parent, &format!("{name}: [{} items]", array.len()), depth);
        if !array.is_empty() {
            let lv = commands.spawn((list_view::list_view(), ChildOf(parent))).id();
            let display_count = array.len().min(MAX_LIST_ITEMS);
            for i in 0..display_count {
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
            if array.len() > MAX_LIST_ITEMS {
                spawn_text_row(
                    commands,
                    lv,
                    &format!("... and {} more", array.len() - MAX_LIST_ITEMS),
                    depth + 1,
                );
            }
        }
        return;
    }

    let is_compound = matches!(
        value.reflect_ref(),
        ReflectRef::Struct(_) | ReflectRef::TupleStruct(_)
    );

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
                padding: UiRect::left(px(12.0)),
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
                    );
                }
            }
            _ => {}
        }
    }
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
    let left_padding = depth as f32 * 8.0;

    let row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(4),
                padding: UiRect::left(px(left_padding)),
                ..Default::default()
            },
            ChildOf(parent),
        ))
        .id();

    commands.spawn((
        Text::new(format!("{label}:")),
        TextFont {
            font_size: 12.,
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
                    apply_field_value(world, source_entity, component_type_id, &path, &value);
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

fn apply_field_value(
    world: &mut World,
    entity: Entity,
    component_type_id: TypeId,
    field_path: &str,
    new_value: &str,
) {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    let Some(registration) = registry.get(component_type_id) else {
        return;
    };
    let Some(reflect_component) = registration.data::<ReflectComponent>() else {
        return;
    };

    let Some(reflected) = reflect_component.reflect_mut(world.entity_mut(entity)) else {
        return;
    };

    let Ok(field) = reflected.into_inner().reflect_path_mut(field_path) else {
        return;
    };

    // Try parsing and applying for each supported type
    if let Some(current) = field.try_downcast_mut::<f32>() {
        if let Ok(v) = new_value.parse::<f32>() {
            *current = v;
        }
    } else if let Some(current) = field.try_downcast_mut::<f64>() {
        if let Ok(v) = new_value.parse::<f64>() {
            *current = v;
        }
    } else if let Some(current) = field.try_downcast_mut::<i32>() {
        if let Ok(v) = new_value.parse::<i32>() {
            *current = v;
        }
    } else if let Some(current) = field.try_downcast_mut::<u32>() {
        if let Ok(v) = new_value.parse::<u32>() {
            *current = v;
        }
    } else if let Some(current) = field.try_downcast_mut::<usize>() {
        if let Ok(v) = new_value.parse::<usize>() {
            *current = v;
        }
    } else if let Some(current) = field.try_downcast_mut::<bool>() {
        if let Ok(v) = new_value.parse::<bool>() {
            *current = v;
        }
    } else if let Some(current) = field.try_downcast_mut::<String>() {
        *current = new_value.to_string();
    }
}

fn spawn_entity_link(commands: &mut Commands, parent: Entity, target: Entity, label: &str) {
    commands.spawn((
        Text::new(label),
        TextFont {
            font_size: 12.,
            ..Default::default()
        },
        TextColor(LINK_COLOR),
        ChildOf(parent),
        observe(
            move |_: On<Pointer<Click>>,
                  mut commands: Commands,
                  selected: Query<Entity, With<SelectedEntity>>| {
                for entity in &selected {
                    commands.entity(entity).remove::<SelectedEntity>();
                }
                if let Ok(mut ec) = commands.get_entity(target) {
                    ec.insert(SelectedEntity);
                }
            },
        ),
        observe(
            move |hover: On<Pointer<Over>>, mut q: Query<&mut TextColor>| {
                if let Ok(mut c) = q.get_mut(hover.event_target()) {
                    c.0 = LINK_HOVER_COLOR;
                }
            },
        ),
        observe(
            move |out: On<Pointer<Out>>, mut q: Query<&mut TextColor>| {
                if let Ok(mut c) = q.get_mut(out.event_target()) {
                    c.0 = LINK_COLOR;
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
    let display_count = len.min(MAX_LIST_ITEMS);
    for i in 0..display_count {
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
    if len > MAX_LIST_ITEMS {
        spawn_text_row(
            commands,
            lv,
            &format!("... and {} more", len - MAX_LIST_ITEMS),
            depth + 1,
        );
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
    // Entity → clickable link
    if let Some(&entity_val) = value.try_downcast_ref::<Entity>() {
        let label = entity_names
            .get(entity_val)
            .map(|n| format!("{} ({entity_val})", n.as_str()))
            .unwrap_or_else(|_| format!("{entity_val}"));
        spawn_entity_link(commands, parent, entity_val, &label);
        return;
    }
    // Editable primitive → inline text input
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
    // Compound → recurse
    if let Some(reflected) = value.try_as_reflect() {
        spawn_reflected_fields(
            commands,
            parent,
            reflected,
            depth,
            field_path,
            source_entity,
            component_type_id,
            entity_names,
        );
        return;
    }
    // Fallback → plain text
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
                    apply_field_value(world, source_entity, component_type_id, &path, &value);
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
    let left_padding = depth as f32 * 8.0;
    commands.spawn((
        Node {
            padding: UiRect::left(px(left_padding)),
            ..Default::default()
        },
        Text::new(text),
        TextFont {
            font_size: 12.,
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

    // Fallback: use reflect debug
    format!("{value:?}")
}
