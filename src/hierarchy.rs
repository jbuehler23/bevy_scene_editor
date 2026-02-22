use std::any::TypeId;
use std::collections::HashSet;

use bevy::{input_focus::InputFocus, prelude::*, ui::ui_transform::UiGlobalTransform};
use bevy_notify::prelude::{Mutation, NotifyChanged};
use editor_feathers::{
    context_menu::spawn_context_menu,
    icons::IconFont,
    text_input, tokens,
    tree_view::{tree_row, TreeRowStyle, ROW_BG},
};
use editor_widgets::context_menu::{ContextMenuAction, ContextMenuCloseSet, ContextMenuState};
use editor_widgets::text_input::{EnteredText, TextInput, TextInputPlacholder};
use editor_widgets::tree_view::{
    EntityCategory, TreeChildrenPopulated, TreeFocused, TreeIndex, TreeNode, TreeNodeExpanded,
    TreeRowChildren, TreeRowClicked, TreeRowContent,
    TreeRowDropped, TreeRowDroppedOnRoot, TreeRowInlineRename, TreeRowLabel, TreeRowRenamed,
    TreeRowSelected, TreeRowStartRename, TreeRowVisibilityToggled,
};

use crate::{
    commands::{CommandHistory, EditorCommand, ReparentEntity, SetComponentField},
    entity_ops,
    layout::HierarchyFilter,
    selection::{Selected, Selection},
    EditorEntity, EditorHidden,
};
use editor_feathers::dialog::{DialogActionEvent, DialogChildrenSlot};

/// Stores the default name for the template save dialog.
#[derive(Resource, Default)]
struct PendingTemplateDefaultName(String);

/// Marker for the template name text input inside the dialog.
#[derive(Component)]
struct TemplateNameInput;

/// Marker for the hierarchy panel
#[derive(Component)]
#[require(EditorEntity)]
pub struct HierarchyPanel;

/// Marker for the container that holds tree rows
#[derive(Component)]
#[require(EditorEntity)]
pub struct HierarchyTreeContainer;

pub struct HierarchyPlugin;

impl Plugin for HierarchyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ContextMenuState>()
            .init_resource::<PendingTemplateDefaultName>()
            .add_systems(Startup, setup_name_watcher)
            .add_systems(PostStartup, rebuild_hierarchy)
            .add_systems(
                Update,
                (
                    apply_hierarchy_filter,
                    cancel_inline_rename,
                    handle_hierarchy_right_click.after(ContextMenuCloseSet),
                    populate_template_dialog,
                    editor_feathers::tree_view::tree_keyboard_navigation,
                ),
            )
            .add_observer(on_root_entity_added)
            .add_observer(on_entity_reparented)
            .add_observer(on_tree_node_expanded)
            .add_observer(on_tree_row_clicked)
            .add_observer(on_entity_removed)
            .add_observer(on_name_changed)
            .add_observer(on_entity_selected)
            .add_observer(on_entity_deselected)
            .add_observer(on_tree_row_dropped)
            .add_observer(on_tree_row_dropped_on_root)
            .add_observer(on_tree_row_start_rename)
            .add_observer(on_tree_row_renamed)
            .add_observer(on_context_menu_action)
            .add_observer(on_visibility_toggled)
            .add_observer(on_template_dialog_action);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Classify a scene entity by its primary component for tree display.
fn classify_entity(world: &World, entity: Entity) -> EntityCategory {
    if world.get::<Camera>(entity).is_some() {
        return EntityCategory::Camera;
    }
    if world.get::<PointLight>(entity).is_some()
        || world.get::<DirectionalLight>(entity).is_some()
        || world.get::<SpotLight>(entity).is_some()
    {
        return EntityCategory::Light;
    }
    if world.get::<Mesh3d>(entity).is_some() {
        return EntityCategory::Mesh;
    }
    if world.get::<SceneRoot>(entity).is_some() {
        return EntityCategory::Scene;
    }
    EntityCategory::Entity
}

/// Check if an entity has any non-editor children.
fn has_visible_children(world: &World, entity: Entity) -> bool {
    let Some(children) = world.get::<Children>(entity) else {
        return false;
    };
    children.iter().any(|child| {
        world.get::<EditorEntity>(child).is_none()
            && world.get::<EditorHidden>(child).is_none()
    })
}

/// Spawn a single (non-recursive) tree row for a source entity.
/// Updates TreeIndex immediately.
fn spawn_single_tree_row(
    world: &mut World,
    source: Entity,
    parent_container: Entity,
) -> Entity {
    let label = world
        .get::<Name>(source)
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| format!("Entity {source}"));
    let has_children = has_visible_children(world, source);
    let category = classify_entity(world, source);
    let icon_font = world.resource::<IconFont>().0.clone();
    let style = TreeRowStyle { icon_font };

    let tree_row_entity = world
        .spawn((
            tree_row(&label, has_children, false, source, category, &style),
            ChildOf(parent_container),
        ))
        .id();

    world
        .resource_mut::<TreeIndex>()
        .insert(source, tree_row_entity);
    tree_row_entity
}

// ---------------------------------------------------------------------------
// Initial build
// ---------------------------------------------------------------------------

/// Populate the hierarchy tree with root-level entities only (non-recursive).
/// Children are spawned lazily when parents are expanded.
fn rebuild_hierarchy(world: &mut World) {
    let container = world
        .query_filtered::<Entity, With<HierarchyTreeContainer>>()
        .iter(world)
        .next();

    let Some(container) = container else {
        return;
    };

    // Collect all root scene entities (Transform, no ChildOf, no editor markers)
    let roots: Vec<Entity> = world
        .query_filtered::<Entity, (
            With<Transform>,
            Without<EditorEntity>,
            Without<EditorHidden>,
            Without<ChildOf>,
        )>()
        .iter(world)
        .collect();

    // Sort by (category, name) for consistent ordering
    let mut root_data: Vec<(Entity, EntityCategory, String)> = roots
        .into_iter()
        .filter(|&e| !world.resource::<TreeIndex>().contains(e))
        .map(|e| {
            let category = classify_entity(world, e);
            let name = world
                .get::<Name>(e)
                .map(|n| n.as_str().to_string())
                .unwrap_or_else(|| format!("Entity {e}"));
            (e, category, name)
        })
        .collect();

    root_data.sort_by(|(_, cat_a, name_a), (_, cat_b, name_b)| {
        cat_a.cmp(cat_b).then_with(|| name_a.cmp(name_b))
    });

    for (entity, _category, _name) in root_data {
        spawn_single_tree_row(world, entity, container);
    }
}

// ---------------------------------------------------------------------------
// Observers: entity lifecycle
// ---------------------------------------------------------------------------

/// When a new entity gets Transform and has no parent, create a root tree row.
fn on_root_entity_added(
    trigger: On<Add, Transform>,
    mut commands: Commands,
    tree_index: Res<TreeIndex>,
    container: Option<Single<Entity, With<HierarchyTreeContainer>>>,
    editor_check: Query<(), Or<(With<EditorEntity>, With<EditorHidden>)>>,
    child_of_check: Query<(), With<ChildOf>>,
) {
    let entity = trigger.event_target();
    let Some(container) = container else {
        return;
    };

    if editor_check.contains(entity)
        || child_of_check.contains(entity)
        || tree_index.contains(entity)
    {
        return;
    }

    let container = *container;
    commands.queue(move |world: &mut World| {
        if world.resource::<TreeIndex>().contains(entity) {
            return;
        }
        // Re-check: ChildOf may have been added between observer and command flush
        if world.get::<ChildOf>(entity).is_some() {
            return;
        }
        if world.get::<EditorEntity>(entity).is_some()
            || world.get::<EditorHidden>(entity).is_some()
        {
            return;
        }
        spawn_single_tree_row(world, entity, container);
    });
}

/// When an entity's Name is added/changed, update its tree row label.
/// Also creates a tree row if the entity is a root without one.
fn on_name_changed(
    trigger: On<Add, Name>,
    mut commands: Commands,
    name_query: Query<&Name>,
    tree_index: Res<TreeIndex>,
    tree_nodes: Query<&Children, With<TreeNode>>,
    content_query: Query<&Children, With<TreeRowContent>>,
    mut label_query: Query<&mut Text, With<TreeRowLabel>>,
    container: Option<Single<Entity, With<HierarchyTreeContainer>>>,
    editor_check: Query<(), Or<(With<EditorEntity>, With<EditorHidden>)>>,
    child_of_check: Query<(), With<ChildOf>>,
) {
    let entity = trigger.event_target();
    let Ok(name) = name_query.get(entity) else {
        return;
    };

    if let Some(tree_entity) = tree_index.get(entity) {
        // Update existing label: TreeNode → Children → TreeRowContent → Children → TreeRowLabel
        let Ok(children) = tree_nodes.get(tree_entity) else {
            return;
        };
        for child in children.iter() {
            if let Ok(content_children) = content_query.get(child) {
                for grandchild in content_children.iter() {
                    if let Ok(mut text) = label_query.get_mut(grandchild) {
                        text.0 = name.as_str().to_string();
                        return;
                    }
                }
            }
        }
    } else {
        // Entity has no tree row — create one if it's a visible root
        let Some(container) = container else {
            return;
        };
        if editor_check.contains(entity) || child_of_check.contains(entity) {
            return;
        }

        let container = *container;
        commands.queue(move |world: &mut World| {
            if world.resource::<TreeIndex>().contains(entity) {
                return;
            }
            // Re-check: ChildOf may have been added between observer and command flush
            if world.get::<ChildOf>(entity).is_some() {
                return;
            }
            if world.get::<EditorEntity>(entity).is_some()
                || world.get::<EditorHidden>(entity).is_some()
            {
                return;
            }
            spawn_single_tree_row(world, entity, container);
        });
    }
}

/// Spawn a watcher entity that notifies us when Name is mutated in-place.
fn setup_name_watcher(mut commands: Commands) {
    commands
        .spawn((EditorEntity, NotifyChanged::<Name>::default()))
        .observe(on_name_mutated);
}

/// When an entity's Name is mutated in-place (e.g. via inspector), update the tree row label.
fn on_name_mutated(
    trigger: On<Mutation<Name>>,
    name_query: Query<&Name>,
    tree_index: Res<TreeIndex>,
    tree_nodes: Query<&Children, With<TreeNode>>,
    content_query: Query<&Children, With<TreeRowContent>>,
    mut label_query: Query<&mut Text, With<TreeRowLabel>>,
) {
    let entity = trigger.mutated;
    let Ok(name) = name_query.get(entity) else {
        return;
    };
    let Some(tree_entity) = tree_index.get(entity) else {
        return;
    };
    let Ok(children) = tree_nodes.get(tree_entity) else {
        return;
    };
    for child in children.iter() {
        let Ok(content_children) = content_query.get(child) else {
            continue;
        };
        for grandchild in content_children.iter() {
            if let Ok(mut text) = label_query.get_mut(grandchild) {
                text.0 = name.as_str().to_string();
                return;
            }
        }
    }
}

/// When an entity gets a parent (ChildOf added or changed), reparent or create its tree row.
fn on_entity_reparented(
    trigger: On<Add, ChildOf>,
    mut commands: Commands,
    tree_index: Res<TreeIndex>,
    editor_check: Query<(), Or<(With<EditorEntity>, With<EditorHidden>)>>,
    tree_node_check: Query<(), With<TreeNode>>,
    child_of_query: Query<&ChildOf>,
    children_query: Query<&Children>,
    tree_row_children: Query<Entity, With<TreeRowChildren>>,
    populated_query: Query<&TreeChildrenPopulated>,
) {
    let entity = trigger.event_target();

    // Skip editor/hidden entities and tree row UI entities
    if editor_check.contains(entity) || tree_node_check.contains(entity) {
        return;
    }

    let Ok(&ChildOf(new_parent)) = child_of_query.get(entity) else {
        return;
    };

    // Find the new parent's TreeRowChildren container via TreeIndex + child walk
    let parent_container = tree_index.get(new_parent).and_then(|parent_tree| {
        children_query.get(parent_tree).ok().and_then(|children| {
            children
                .iter()
                .find(|c| tree_row_children.contains(*c))
        })
    });

    // If tree row already exists for this entity → reparent it
    if let Some(tree_entity) = tree_index.get(entity) {
        if let Some(container) = parent_container {
            commands.entity(tree_entity).insert(ChildOf(container));
        } else {
            // Parent has no tree row yet — remove this incorrectly-rooted tree row.
            // Lazy loading will re-create it when the parent is expanded.
            let source = entity;
            commands.queue(move |world: &mut World| {
                world.resource_mut::<TreeIndex>().remove(source);
                if let Ok(ec) = world.get_entity_mut(tree_entity) {
                    ec.despawn();
                }
            });
        }
        return;
    }

    // No tree row exists — only spawn if the parent's children are already populated
    let Some(parent_container) = parent_container else {
        return;
    };
    let parent_tree = tree_index.get(new_parent).unwrap(); // safe: we found parent_container from it
    let populated = populated_query
        .get(parent_tree)
        .map(|p| p.0)
        .unwrap_or(false);
    if !populated {
        return; // Lazy loading will handle it when parent is expanded
    }

    let container = parent_container;
    commands.queue(move |world: &mut World| {
        if world.resource::<TreeIndex>().contains(entity) {
            return;
        }
        spawn_single_tree_row(world, entity, container);
    });
}

/// When an entity's Name is removed, despawn its tree row.
fn on_entity_removed(
    trigger: On<Despawn, Name>,
    mut commands: Commands,
    tree_index: Res<TreeIndex>,
) {
    let entity = trigger.event_target();

    if let Some(tree_entity) = tree_index.get(entity) {
        if let Ok(mut ec) = commands.get_entity(tree_entity) {
            ec.despawn();
        }
    }
}

// ---------------------------------------------------------------------------
// Observer: lazy child population
// ---------------------------------------------------------------------------

/// When a tree node is expanded for the first time, spawn tree rows for its children.
fn on_tree_node_expanded(
    trigger: On<Mutation<TreeNodeExpanded>>,
    mut commands: Commands,
    tree_query: Query<(&TreeNodeExpanded, &TreeChildrenPopulated, &TreeNode, &Children)>,
    tree_row_children_marker: Query<Entity, With<TreeRowChildren>>,
) {
    let entity = trigger.event_target();
    let Ok((expanded, populated, tree_node, children)) = tree_query.get(entity) else {
        return;
    };

    // Only populate on first expansion
    if !expanded.0 || populated.0 {
        return;
    }

    let source = tree_node.0;
    let Some(container) = children
        .iter()
        .find(|c| tree_row_children_marker.contains(*c))
    else {
        return;
    };
    let tree_row_entity = entity;

    commands.queue(move |world: &mut World| {
        // Double-check populated flag (guard against duplicate events)
        if let Some(pop) = world.get::<TreeChildrenPopulated>(tree_row_entity) {
            if pop.0 {
                return;
            }
        }

        // Mark as populated
        if let Some(mut pop) = world.get_mut::<TreeChildrenPopulated>(tree_row_entity) {
            pop.0 = true;
        }

        // Collect visible children with classification
        let source_children: Vec<Entity> = world
            .get::<Children>(source)
            .map(|c| c.iter().collect())
            .unwrap_or_default();

        let mut child_data: Vec<(Entity, String, EntityCategory)> = Vec::new();
        for child in source_children {
            if world.get::<EditorEntity>(child).is_some()
                || world.get::<EditorHidden>(child).is_some()
            {
                continue;
            }
            // Skip children that already have tree rows
            if world.resource::<TreeIndex>().contains(child) {
                continue;
            }
            let name = world
                .get::<Name>(child)
                .map(|n| n.as_str().to_string())
                .unwrap_or_else(|| format!("Entity {child}"));
            let category = classify_entity(world, child);
            child_data.push((child, name, category));
        }

        // Sort by (category, name)
        child_data.sort_by(|(_, name_a, cat_a), (_, name_b, cat_b)| {
            cat_a.cmp(cat_b).then_with(|| name_a.cmp(name_b))
        });

        // Spawn tree rows
        for (child_entity, _name, _category) in child_data {
            spawn_single_tree_row(world, child_entity, container);
        }
    });
}

// ---------------------------------------------------------------------------
// Observer: selection
// ---------------------------------------------------------------------------

/// Handle tree row click → select the source entity.
/// Plain click on selected entity → deselect. Ctrl+Click → toggle.
fn on_tree_row_clicked(
    event: On<TreeRowClicked>,
    mut commands: Commands,
    mut selection: ResMut<Selection>,
    mut focused: ResMut<TreeFocused>,
    keyboard: Res<ButtonInput<KeyCode>>,
    parent_query: Query<&ChildOf>,
    tree_nodes: Query<Entity, With<TreeNode>>,
) {
    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);

    if ctrl {
        selection.toggle(&mut commands, event.source_entity);
    } else if selection.is_selected(event.source_entity) {
        selection.clear(&mut commands);
    } else {
        selection.select_single(&mut commands, event.source_entity);
    }

    // Set keyboard focus to the tree row containing this content
    let content_entity = event.entity;
    if let Ok(&ChildOf(tree_row)) = parent_query.get(content_entity) {
        if tree_nodes.contains(tree_row) {
            focused.0 = Some(tree_row);
        }
    }
}

/// When Selected is added, highlight the corresponding tree row.
fn on_entity_selected(
    trigger: On<Add, Selected>,
    mut commands: Commands,
    tree_index: Res<TreeIndex>,
    tree_nodes: Query<&Children, With<TreeNode>>,
    tree_row_contents: Query<Entity, With<TreeRowContent>>,
    mut bg_query: Query<&mut BackgroundColor>,
    mut border_query: Query<&mut BorderColor>,
) {
    let entity = trigger.event_target();

    let Some(tree_entity) = tree_index.get(entity) else {
        return;
    };
    let Ok(children) = tree_nodes.get(tree_entity) else {
        return;
    };

    for child in children.iter() {
        if tree_row_contents.contains(child) {
            if let Ok(mut ec) = commands.get_entity(child) {
                ec.insert(TreeRowSelected);
            }
            if let Ok(mut bg) = bg_query.get_mut(child) {
                bg.0 = tokens::SELECTED_BG;
            }
            if let Ok(mut border) = border_query.get_mut(child) {
                *border = BorderColor::all(tokens::SELECTED_BORDER);
            }
            return;
        }
    }
}

/// When Selected is removed, unhighlight the corresponding tree row.
fn on_entity_deselected(
    trigger: On<Remove, Selected>,
    mut commands: Commands,
    tree_index: Res<TreeIndex>,
    tree_nodes: Query<&Children, With<TreeNode>>,
    tree_row_contents: Query<Entity, With<TreeRowContent>>,
    mut bg_query: Query<&mut BackgroundColor>,
    mut border_query: Query<&mut BorderColor>,
) {
    let entity = trigger.event_target();

    let Some(tree_entity) = tree_index.get(entity) else {
        return;
    };
    let Ok(children) = tree_nodes.get(tree_entity) else {
        return;
    };

    for child in children.iter() {
        if tree_row_contents.contains(child) {
            if let Ok(mut ec) = commands.get_entity(child) {
                ec.remove::<TreeRowSelected>();
            }
            if let Ok(mut bg) = bg_query.get_mut(child) {
                bg.0 = ROW_BG;
            }
            if let Ok(mut border) = border_query.get_mut(child) {
                *border = BorderColor::all(Color::NONE);
            }
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Observers: drag-and-drop reparenting
// ---------------------------------------------------------------------------

/// Handle tree row dropped → reparent the scene entity with undo support.
fn on_tree_row_dropped(
    event: On<TreeRowDropped>,
    mut commands: Commands,
    parent_query: Query<&ChildOf>,
) {
    let dragged = event.dragged_source;
    let target = event.target_source;

    if dragged == target {
        return;
    }

    // Cycle check: walk up from target, ensure dragged is not an ancestor
    let mut current = target;
    while let Ok(&ChildOf(parent)) = parent_query.get(current) {
        if parent == dragged {
            return;
        }
        current = parent;
    }

    let old_parent = parent_query.get(dragged).ok().map(|c| c.0);

    let cmd = ReparentEntity {
        entity: dragged,
        old_parent,
        new_parent: Some(target),
    };

    commands.queue(move |world: &mut World| {
        cmd.execute(world);
        world
            .resource_mut::<CommandHistory>()
            .undo_stack
            .push(Box::new(cmd));
        world.resource_mut::<CommandHistory>().redo_stack.clear();
    });
}

/// Handle tree row dropped on root container → deparent the scene entity.
fn on_tree_row_dropped_on_root(
    event: On<TreeRowDroppedOnRoot>,
    mut commands: Commands,
    parent_query: Query<&ChildOf, Without<EditorEntity>>,
    tree_index: Res<TreeIndex>,
    container: Single<Entity, With<HierarchyTreeContainer>>,
) {
    let dragged = event.dragged_source;

    let old_parent = match parent_query.get(dragged) {
        Ok(child_of) => Some(child_of.0),
        Err(_) => return,
    };

    let container_entity = *container;

    let cmd = ReparentEntity {
        entity: dragged,
        old_parent,
        new_parent: None,
    };

    commands.queue(move |world: &mut World| {
        cmd.execute(world);
        world
            .resource_mut::<CommandHistory>()
            .undo_stack
            .push(Box::new(cmd));
        world.resource_mut::<CommandHistory>().redo_stack.clear();
    });

    // Move the tree row to the root container
    if let Some(tree_entity) = tree_index.get(dragged) {
        commands
            .entity(tree_entity)
            .insert(ChildOf(container_entity));
    }
}

// ---------------------------------------------------------------------------
// Context menu
// ---------------------------------------------------------------------------

/// Detect right-click on tree rows and open a context menu.
fn handle_hierarchy_right_click(
    mouse: Res<ButtonInput<MouseButton>>,
    mut commands: Commands,
    mut state: ResMut<ContextMenuState>,
    windows: Query<&Window>,
    selection: Res<Selection>,
    tree_row_contents: Query<(Entity, &ChildOf), With<TreeRowContent>>,
    tree_nodes: Query<&TreeNode>,
    computed_nodes: Query<(&ComputedNode, &UiGlobalTransform), With<TreeRowContent>>,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };

    // Close any existing context menu
    if let Some(menu) = state.menu_entity.take() {
        if let Ok(mut ec) = commands.get_entity(menu) {
            ec.despawn();
        }
    }

    // Find which tree row content the cursor is over by hit testing
    let mut target_source = None;
    for (content_entity, child_of) in &tree_row_contents {
        let Ok((computed, global_transform)) = computed_nodes.get(content_entity) else {
            continue;
        };
        let size = computed.size();
        let (_, _, translation) = global_transform.to_scale_angle_translation();
        let pos = translation;
        let half = size / 2.0;
        let rect = Rect::from_center_half_size(pos, half);
        if rect.contains(cursor_pos) {
            if let Ok(tree_node) = tree_nodes.get(child_of.0) {
                target_source = Some(tree_node.0);
                break;
            }
        }
    }

    let Some(target) = target_source else {
        return;
    };

    // If the right-clicked entity isn't selected, select it
    if !selection.is_selected(target) {
        commands.queue(move |world: &mut World| {
            let old_entities: Vec<Entity> = world.resource::<Selection>().entities.clone();
            let mut selection = world.resource_mut::<Selection>();
            selection.entities.clear();
            selection.entities.push(target);

            for &e in &old_entities {
                if e != target {
                    if let Ok(mut ec) = world.get_entity_mut(e) {
                        ec.remove::<Selected>();
                    }
                }
            }
            world.entity_mut(target).insert(Selected);
        });
    }

    let menu_items = &[
        ("hierarchy.rename", "Rename              F2"),
        ("hierarchy.duplicate", "Duplicate        Ctrl+D"),
        ("hierarchy.delete", "Delete             Del"),
        ("---", ""),
        ("hierarchy.save_template", "Save as Template..."),
        ("---", ""),
        ("hierarchy.add_cube", "Add Child Cube"),
        ("hierarchy.add_sphere", "Add Child Sphere"),
        ("hierarchy.add_light", "Add Child Light"),
        ("hierarchy.add_empty", "Add Child Empty"),
    ];

    // Filter out separators for spawn_context_menu (it doesn't handle them)
    let items: Vec<(&str, &str)> = menu_items
        .iter()
        .filter(|(action, _)| *action != "---")
        .copied()
        .collect();

    let menu = spawn_context_menu(&mut commands, cursor_pos, Some(target), &items);
    state.menu_entity = Some(menu);
    state.target_entity = Some(target);
}

/// Handle context menu actions for hierarchy operations.
fn on_context_menu_action(
    event: On<ContextMenuAction>,
    mut commands: Commands,
) {
    let target_entity = event.target_entity;

    match event.action.as_str() {
        "hierarchy.rename" => {
            if let Some(target) = target_entity {
                commands.trigger(TreeRowStartRename {
                    entity: Entity::PLACEHOLDER,
                    source_entity: target,
                });
            }
        }
        "hierarchy.duplicate" => {
            commands.queue(|world: &mut World| {
                entity_ops::duplicate_selected(world);
            });
        }
        "hierarchy.delete" => {
            commands.queue(|world: &mut World| {
                entity_ops::delete_selected(world);
            });
        }
        "hierarchy.add_cube" => {
            if let Some(parent) = target_entity {
                commands.queue(move |world: &mut World| {
                    entity_ops::create_entity_in_world(world, entity_ops::EntityTemplate::Cube);
                    // Reparent the newly created entity under the target
                    let selection = world.resource::<Selection>();
                    if let Some(new_entity) = selection.primary() {
                        world.entity_mut(new_entity).insert(ChildOf(parent));
                    }
                });
            }
        }
        "hierarchy.add_sphere" => {
            if let Some(parent) = target_entity {
                commands.queue(move |world: &mut World| {
                    entity_ops::create_entity_in_world(world, entity_ops::EntityTemplate::Sphere);
                    let selection = world.resource::<Selection>();
                    if let Some(new_entity) = selection.primary() {
                        world.entity_mut(new_entity).insert(ChildOf(parent));
                    }
                });
            }
        }
        "hierarchy.add_light" => {
            if let Some(parent) = target_entity {
                commands.queue(move |world: &mut World| {
                    entity_ops::create_entity_in_world(world, entity_ops::EntityTemplate::PointLight);
                    let selection = world.resource::<Selection>();
                    if let Some(new_entity) = selection.primary() {
                        world.entity_mut(new_entity).insert(ChildOf(parent));
                    }
                });
            }
        }
        "hierarchy.add_empty" => {
            if let Some(parent) = target_entity {
                commands.queue(move |world: &mut World| {
                    entity_ops::create_entity_in_world(world, entity_ops::EntityTemplate::Empty);
                    let selection = world.resource::<Selection>();
                    if let Some(new_entity) = selection.primary() {
                        world.entity_mut(new_entity).insert(ChildOf(parent));
                    }
                });
            }
        }
        "hierarchy.save_template" => {
            if let Some(target) = target_entity {
                // Store the target entity and open a dialog for template name
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<crate::entity_templates::PendingTemplateSave>().entity = Some(target);
                    // Get the entity name as default template name
                    let default_name = world
                        .get::<Name>(target)
                        .map(|n| n.as_str().to_string())
                        .unwrap_or_else(|| "template".to_string());
                    world.resource_mut::<PendingTemplateDefaultName>().0 = default_name;
                });
                commands.trigger(editor_feathers::dialog::OpenDialogEvent::new(
                    "Save as Template",
                    "Save",
                ));
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Visibility toggle
// ---------------------------------------------------------------------------

/// Toggle entity visibility when the eye icon is clicked.
fn on_visibility_toggled(
    event: On<TreeRowVisibilityToggled>,
    mut commands: Commands,
    visibility_query: Query<&Visibility>,
) {
    let source = event.source_entity;

    let current = visibility_query
        .get(source)
        .copied()
        .unwrap_or(Visibility::Inherited);

    let new_visibility = match current {
        Visibility::Hidden => Visibility::Inherited,
        _ => Visibility::Hidden,
    };

    // Apply with undo
    let old_value: Box<dyn bevy::reflect::PartialReflect> = Box::new(current);
    let new_value: Box<dyn bevy::reflect::PartialReflect> = Box::new(new_visibility);

    let cmd = SetComponentField {
        entity: source,
        component_type_id: TypeId::of::<Visibility>(),
        field_path: String::new(),
        old_value,
        new_value,
    };

    commands.queue(move |world: &mut World| {
        let cmd = Box::new(cmd);
        cmd.execute(world);
        let mut history = world.resource_mut::<CommandHistory>();
        history.undo_stack.push(cmd);
        history.redo_stack.clear();
    });
}

// ---------------------------------------------------------------------------
// Inline rename
// ---------------------------------------------------------------------------

/// Cancel inline rename on Escape key.
fn cancel_inline_rename(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    rename_query: Query<(Entity, &Children), With<TreeRowInlineRename>>,
    tree_nodes: Query<&TreeNode>,
    parent_query: Query<&ChildOf>,
    names: Query<&Name>,
    mut input_focus: ResMut<InputFocus>,
) {
    if !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }
    for (label_entity, children) in &rename_query {
        input_focus.clear();

        // Walk up to find the TreeNode and get the source entity for original name
        let mut current = label_entity;
        let mut source_entity = None;
        for _ in 0..4 {
            let Ok(&ChildOf(parent)) = parent_query.get(current) else {
                break;
            };
            if let Ok(tree_node) = tree_nodes.get(parent) {
                source_entity = Some(tree_node.0);
                break;
            }
            current = parent;
        }

        let original_name = source_entity
            .and_then(|e| names.get(e).ok())
            .map(|n| n.as_str().to_string())
            .unwrap_or_default();

        // Despawn children added by text_input() (TextInputDisplay etc.)
        for child in children.iter() {
            commands.entity(child).despawn();
        }

        // Restore label
        commands.entity(label_entity).remove::<(
            TreeRowInlineRename,
            TextInput,
            TextInputPlacholder,
            BackgroundColor,
            BorderColor,
        )>();
        commands
            .entity(label_entity)
            .insert(Text::new(original_name));
    }
}

/// Start inline rename: replace label text with a text input.
fn on_tree_row_start_rename(
    event: On<TreeRowStartRename>,
    mut commands: Commands,
    tree_index: Res<TreeIndex>,
    tree_nodes: Query<&Children, With<TreeNode>>,
    content_query: Query<&Children, With<TreeRowContent>>,
    label_query: Query<Entity, With<TreeRowLabel>>,
    names: Query<&Name>,
    rename_check: Query<(), With<TreeRowInlineRename>>,
    mut input_focus: ResMut<InputFocus>,
) {
    let source = event.source_entity;

    // Don't start a rename if one is already active
    if !rename_check.is_empty() {
        return;
    }

    let Some(tree_entity) = tree_index.get(source) else {
        return;
    };
    let Ok(children) = tree_nodes.get(tree_entity) else {
        return;
    };

    // Find the TreeRowLabel entity
    let mut label_entity = None;
    for child in children.iter() {
        if let Ok(content_children) = content_query.get(child) {
            for grandchild in content_children.iter() {
                if label_query.contains(grandchild) {
                    label_entity = Some(grandchild);
                    break;
                }
            }
        }
    }
    let Some(label_entity) = label_entity else {
        return;
    };

    let current_name = names
        .get(source)
        .map(|n| n.as_str().to_string())
        .unwrap_or_default();

    // Remove the static Text so it doesn't render alongside the text input
    commands.entity(label_entity).remove::<Text>();
    // Replace the label with a text input
    commands.entity(label_entity).insert((
        TreeRowInlineRename,
        text_input::text_input(""),
    ));
    // Set the value separately — text_input() already includes TextInput::default()
    commands
        .entity(label_entity)
        .insert(TextInput::new(current_name));

    // Focus the input
    input_focus.set(label_entity);

    // Add Enter/Escape observers
    let source_entity = source;
    commands.entity(label_entity).observe(
        move |text: On<EnteredText>, mut commands: Commands| {
            commands.trigger(TreeRowRenamed {
                entity: text.entity,
                source_entity,
                new_name: text.value.clone(),
            });
        },
    );
}

/// Commit inline rename: update Name with undo, restore label.
fn on_tree_row_renamed(
    event: On<TreeRowRenamed>,
    mut commands: Commands,
    names: Query<&Name>,
    children_query: Query<&Children>,
    mut input_focus: ResMut<InputFocus>,
) {
    let source = event.source_entity;
    let new_name = event.new_name.clone();
    let label_entity = event.entity;

    // Clear focus
    input_focus.clear();

    // Despawn children added by text_input() (TextInputDisplay etc.)
    if let Ok(children) = children_query.get(label_entity) {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }

    // Restore label: remove text input components, restore TreeRowLabel text
    commands.entity(label_entity).remove::<(
        TreeRowInlineRename,
        TextInput,
        TextInputPlacholder,
        BackgroundColor,
        BorderColor,
    )>();
    commands.entity(label_entity).insert(Text::new(new_name.clone()));

    // Apply name change with undo
    let old_name = names
        .get(source)
        .map(|n| n.as_str().to_string())
        .unwrap_or_default();

    if old_name == new_name {
        return;
    }

    commands.queue(move |world: &mut World| {
        let cmd = SetComponentField {
            entity: source,
            component_type_id: TypeId::of::<Name>(),
            field_path: String::new(),
            old_value: Box::new(Name::new(old_name)),
            new_value: Box::new(Name::new(new_name)),
        };
        let cmd = Box::new(cmd);
        cmd.execute(world);
        let mut history = world.resource_mut::<CommandHistory>();
        history.undo_stack.push(cmd);
        history.redo_stack.clear();
    });
}

// ---------------------------------------------------------------------------
// Template save dialog
// ---------------------------------------------------------------------------

/// When the template dialog opens, populate its children slot with a name input.
fn populate_template_dialog(
    mut commands: Commands,
    pending: Res<crate::entity_templates::PendingTemplateSave>,
    default_name: Res<PendingTemplateDefaultName>,
    slots: Query<(Entity, &Children), (With<DialogChildrenSlot>, Changed<Children>)>,
    existing_inputs: Query<(), With<TemplateNameInput>>,
    mut input_focus: ResMut<InputFocus>,
) {
    // Only act when there's a pending template save
    if pending.entity.is_none() {
        return;
    }
    // Don't re-populate if we already have an input
    if !existing_inputs.is_empty() {
        return;
    }
    for (slot_entity, children) in &slots {
        if children.is_empty() {
            let input_entity = commands
                .spawn((
                    TemplateNameInput,
                    text_input::text_input("Template name..."),
                    TextInput::new(default_name.0.clone()),
                    ChildOf(slot_entity),
                ))
                .id();
            input_focus.set(input_entity);
        }
    }
}

/// When the dialog's action button is clicked, save the template.
fn on_template_dialog_action(
    _event: On<DialogActionEvent>,
    mut commands: Commands,
    pending: Res<crate::entity_templates::PendingTemplateSave>,
    name_inputs: Query<&TextInput, With<TemplateNameInput>>,
) {
    let Some(_entity) = pending.entity else {
        return;
    };

    let name = name_inputs
        .iter()
        .next()
        .map(|input| input.value.trim().to_string())
        .unwrap_or_default();

    if name.is_empty() {
        return;
    }

    commands.queue(move |world: &mut World| {
        crate::entity_templates::save_entity_template(world, &name);
        world.resource_mut::<crate::entity_templates::PendingTemplateSave>().entity = None;
    });
}

// ---------------------------------------------------------------------------
// Filter system
// ---------------------------------------------------------------------------

/// Filter hierarchy tree rows based on the filter text input.
fn apply_hierarchy_filter(
    filter_input: Query<&editor_widgets::text_input::TextInput, (With<HierarchyFilter>, Changed<editor_widgets::text_input::TextInput>)>,
    tree_nodes: Query<(Entity, &TreeNode)>,
    names: Query<&Name>,
    parent_query: Query<&ChildOf>,
    tree_row_children_query: Query<(), With<TreeRowChildren>>,
    mut display_query: Query<&mut Node>,
) {
    let Ok(text_input) = filter_input.single() else {
        return;
    };

    let filter = text_input.value.trim().to_lowercase();

    if filter.is_empty() {
        for (tree_entity, _) in &tree_nodes {
            if let Ok(mut node) = display_query.get_mut(tree_entity) {
                node.display = Display::Flex;
            }
        }
        return;
    }

    // First pass: determine which source entities match the filter
    let mut visible_tree_entities: HashSet<Entity> = HashSet::new();

    for (tree_entity, tree_node) in &tree_nodes {
        let label = names
            .get(tree_node.0)
            .map(|n| n.as_str().to_lowercase())
            .unwrap_or_else(|_| format!("entity {}", tree_node.0).to_lowercase());
        let matches = label.contains(&filter);

        if matches {
            visible_tree_entities.insert(tree_entity);

            // Walk up ancestors: tree row → ChildOf → TreeRowChildren → ChildOf → parent tree row
            let mut current = tree_entity;
            while let Ok(&ChildOf(parent)) = parent_query.get(current) {
                if tree_row_children_query.contains(parent) {
                    if let Ok(&ChildOf(grandparent)) = parent_query.get(parent) {
                        visible_tree_entities.insert(grandparent);
                        current = grandparent;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    }

    // Second pass: set display on all tree rows
    for (tree_entity, _) in &tree_nodes {
        if let Ok(mut node) = display_query.get_mut(tree_entity) {
            node.display = if visible_tree_entities.contains(&tree_entity) {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
}
