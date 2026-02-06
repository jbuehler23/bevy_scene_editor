use bevy::prelude::*;
use editor_feathers::tree_view::{tree_row, ROW_BG, ROW_SELECTED_BG};
use editor_widgets::{
    text_input::TextInput,
    tree_view::{
        TreeNode, TreeNodeExpanded, TreeRowChildren, TreeRowClicked, TreeRowContent,
        TreeRowDropped, TreeRowDroppedOnRoot, TreeRowLabel, TreeRowSelected,
    },
};

use crate::{inspector::SelectedEntity, layout::HierarchyFilter, EditorEntity};

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
        app.add_systems(PostStartup, rebuild_hierarchy)
            .add_systems(Update, apply_hierarchy_filter)
            .add_observer(on_tree_row_clicked)
            .add_observer(on_entity_added)
            .add_observer(on_entity_removed)
            .add_observer(on_name_changed)
            .add_observer(on_parent_changed)
            .add_observer(update_tree_expand_state)
            .add_observer(on_entity_selected)
            .add_observer(on_entity_deselected)
            .add_observer(on_tree_row_dropped)
            .add_observer(on_tree_row_dropped_on_root);
    }
}

/// Initial build of the hierarchy tree
fn rebuild_hierarchy(world: &mut World) {
    // First find the container
    let container = world
        .query_filtered::<Entity, With<HierarchyTreeContainer>>()
        .iter(world)
        .next();

    let Some(container) = container else {
        return;
    };

    // Get all root scene entities (non-editor, no parent)
    let roots: Vec<(Entity, Option<String>, bool)> = world
        .query_filtered::<(Entity, Option<&Name>, Option<&Children>), (Without<EditorEntity>, Without<ChildOf>)>()
        .iter(world)
        .map(|(e, n, c)| {
            (
                e,
                n.map(|n| n.as_str().to_string()),
                c.map(|c| !c.is_empty()).unwrap_or(false),
            )
        })
        .collect();

    // Spawn tree rows for roots
    for (entity, name, has_children) in roots {
        let label = name.unwrap_or_else(|| format!("Entity {:?}", entity));
        spawn_tree_row_recursive(world, entity, &label, has_children, container);
    }
}

fn spawn_tree_row_recursive(
    world: &mut World,
    source_entity: Entity,
    label: &str,
    has_children: bool,
    parent_container: Entity,
) {
    // Spawn the tree row
    let tree_row_entity = world
        .spawn((tree_row(label, has_children, false, source_entity), ChildOf(parent_container)))
        .id();

    // Get the TreeRowChildren container from this tree row
    let tree_row_children = world
        .query_filtered::<(Entity, &ChildOf), With<TreeRowChildren>>()
        .iter(world)
        .find(|(_, child_of)| child_of.0 == tree_row_entity)
        .map(|(e, _)| e);

    let Some(tree_row_children) = tree_row_children else {
        return;
    };

    // Get children of the source entity
    let source_children: Vec<(Entity, Option<String>, bool)> = {
        let children = world.get::<Children>(source_entity);
        if let Some(children) = children {
            children
                .iter()
                .filter_map(|child| {
                    // Skip editor entities
                    if world.get::<EditorEntity>(child).is_some() {
                        return None;
                    }

                    let name = world.get::<Name>(child).map(|n| n.as_str().to_string());
                    let grandchildren = world.get::<Children>(child);
                    let has_children = grandchildren.map(|c| !c.is_empty()).unwrap_or(false);
                    Some((child, name, has_children))
                })
                .collect()
        } else {
            Vec::new()
        }
    };

    // Recursively spawn children
    for (child_entity, name, child_has_children) in source_children {
        let child_label = name.unwrap_or_else(|| format!("Entity {:?}", child_entity));
        spawn_tree_row_recursive(world, child_entity, &child_label, child_has_children, tree_row_children);
    }
}

/// Handle tree row click -> select the source entity
fn on_tree_row_clicked(
    event: On<TreeRowClicked>,
    mut commands: Commands,
    selected: Query<Entity, With<SelectedEntity>>,
) {
    // Deselect previous
    for entity in &selected {
        commands.entity(entity).remove::<SelectedEntity>();
    }

    // Select new entity
    commands
        .entity(event.source_entity)
        .insert(SelectedEntity);
}

/// When a new entity is added (not an editor entity), spawn a tree row
fn on_entity_added(
    trigger: On<Add, Name>,
    mut commands: Commands,
    container: Option<Single<Entity, With<HierarchyTreeContainer>>>,
    entity_query: Query<(Option<&Name>, Option<&Children>, Option<&ChildOf>), Without<EditorEntity>>,
    tree_nodes: Query<(Entity, &TreeNode)>,
    tree_row_children: Query<(Entity, &ChildOf), With<TreeRowChildren>>,
) {
    let entity = trigger.event_target();
    let Some(container) = container else {
        return;
    };

    let Ok((name, children, parent)) = entity_query.get(entity) else {
        return; // It's an editor entity
    };

    let label = name
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| format!("Entity {:?}", entity));
    let has_children = children.map(|c| !c.is_empty()).unwrap_or(false);

    // Check if tree row already exists
    for (_, tree_node) in &tree_nodes {
        if tree_node.0 == entity {
            return; // Already exists
        }
    }

    // Find the parent container
    let parent_container = if let Some(&ChildOf(parent_entity)) = parent {
        // Find the tree row for the parent
        tree_nodes
            .iter()
            .find(|(_, node)| node.0 == parent_entity)
            .and_then(|(parent_tree_entity, _)| {
                // Find the TreeRowChildren that is a child of this tree row
                tree_row_children
                    .iter()
                    .find(|(_, child_of)| child_of.0 == parent_tree_entity)
                    .map(|(e, _)| e)
            })
            .unwrap_or(*container)
    } else {
        *container
    };

    commands.spawn((tree_row(&label, has_children, false, entity), ChildOf(parent_container)));
}

/// When an entity is removed, despawn its tree row
fn on_entity_removed(
    trigger: On<Remove, Name>,
    mut commands: Commands,
    tree_nodes: Query<(Entity, &TreeNode)>,
) {
    let entity = trigger.event_target();

    for (tree_entity, tree_node) in &tree_nodes {
        if tree_node.0 == entity {
            commands.entity(tree_entity).despawn();
            break;
        }
    }
}

/// When an entity's name changes, update the tree row label
fn on_name_changed(
    trigger: On<Add, Name>,
    name_query: Query<&Name>,
    tree_nodes: Query<(Entity, &TreeNode, &Children)>,
    mut label_query: Query<&mut Text, With<TreeRowLabel>>,
) {
    let entity = trigger.event_target();
    let Ok(name) = name_query.get(entity) else {
        return;
    };

    // Find the tree row for this entity and update its label
    for (_, tree_node, children) in &tree_nodes {
        if tree_node.0 == entity {
            // Find the label in children (need to traverse down)
            for child in children.iter() {
                if let Ok(mut text) = label_query.get_mut(child) {
                    text.0 = name.as_str().to_string();
                    return;
                }
            }
        }
    }
}

/// When parent changes, reparent the tree row
fn on_parent_changed(
    trigger: On<Insert, ChildOf>,
    mut commands: Commands,
    container: Option<Single<Entity, With<HierarchyTreeContainer>>>,
    parent_query: Query<&ChildOf, Without<EditorEntity>>,
    tree_nodes: Query<(Entity, &TreeNode)>,
    tree_row_children: Query<(Entity, &ChildOf), With<TreeRowChildren>>,
) {
    let entity = trigger.event_target();
    let Some(container) = container else {
        return;
    };

    // Skip tree row UI entities — only handle scene entities
    if tree_nodes.get(entity).is_ok() {
        return;
    }

    let Ok(&ChildOf(new_parent)) = parent_query.get(entity) else {
        return;
    };

    // Find the tree row for this entity
    let Some((tree_entity, _)) = tree_nodes.iter().find(|(_, node)| node.0 == entity) else {
        return;
    };

    // Find the TreeRowChildren container of the new parent
    let new_tree_parent = tree_nodes
        .iter()
        .find(|(_, node)| node.0 == new_parent)
        .and_then(|(parent_tree_entity, _)| {
            // Find the TreeRowChildren that is a child of this tree row
            tree_row_children
                .iter()
                .find(|(_, child_of)| child_of.0 == parent_tree_entity)
                .map(|(e, _)| e)
        })
        .unwrap_or(*container);

    commands.entity(tree_entity).insert(ChildOf(new_tree_parent));
}

/// Update expand/collapse state visually
fn update_tree_expand_state(
    _trigger: On<Add, TreeNodeExpanded>,
    changed: Query<(Entity, &TreeNodeExpanded, &Children), Changed<TreeNodeExpanded>>,
    children_container: Query<Entity, With<TreeRowChildren>>,
    mut visibility: Query<&mut Visibility>,
) {
    for (_, expanded, children) in &changed {
        // Find the TreeRowChildren container and toggle visibility
        for child in children.iter() {
            if children_container.contains(child)
                && let Ok(mut vis) = visibility.get_mut(child)
            {
                *vis = if expanded.0 {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
}

/// When SelectedEntity is added, highlight the corresponding tree row
fn on_entity_selected(
    _trigger: On<Add, SelectedEntity>,
    mut commands: Commands,
    selected: Query<Entity, With<SelectedEntity>>,
    tree_nodes: Query<(Entity, &TreeNode, &Children)>,
    tree_row_contents: Query<Entity, With<TreeRowContent>>,
    mut bg_query: Query<&mut BackgroundColor>,
) {
    let Ok(entity) = selected.single() else {
        return;
    };

    // Find the tree row for this entity
    let Some((_, _, children)) = tree_nodes.iter().find(|(_, node, _)| node.0 == entity) else {
        return;
    };

    // Find the TreeRowContent child
    for child in children.iter() {
        if tree_row_contents.contains(child) {
            commands.entity(child).insert(TreeRowSelected);
            if let Ok(mut bg) = bg_query.get_mut(child) {
                bg.0 = ROW_SELECTED_BG;
            }
            return;
        }
    }
}

/// When SelectedEntity is removed, unhighlight the corresponding tree row
fn on_entity_deselected(
    trigger: On<Remove, SelectedEntity>,
    mut commands: Commands,
    tree_nodes: Query<(Entity, &TreeNode, &Children)>,
    tree_row_contents: Query<Entity, With<TreeRowContent>>,
    mut bg_query: Query<&mut BackgroundColor>,
) {
    let entity = trigger.event_target();

    // Find the tree row for this entity
    let Some((_, _, children)) = tree_nodes.iter().find(|(_, node, _)| node.0 == entity) else {
        return;
    };

    // Find the TreeRowContent child
    for child in children.iter() {
        if tree_row_contents.contains(child) {
            commands.entity(child).remove::<TreeRowSelected>();
            if let Ok(mut bg) = bg_query.get_mut(child) {
                bg.0 = ROW_BG;
            }
            return;
        }
    }
}

/// Handle tree row dropped -> reparent the scene entity
fn on_tree_row_dropped(
    event: On<TreeRowDropped>,
    mut commands: Commands,
    parent_query: Query<&ChildOf>,
) {
    let dragged = event.dragged_source;
    let target = event.target_source;

    // Self-check
    if dragged == target {
        return;
    }

    // Cycle check: walk up from target, ensure dragged is not an ancestor
    let mut current = target;
    while let Ok(&ChildOf(parent)) = parent_query.get(current) {
        if parent == dragged {
            return; // Would create a cycle
        }
        current = parent;
    }

    // Reparent the scene entity
    commands.entity(dragged).insert(ChildOf(target));
}

/// Handle tree row dropped on root container -> deparent the scene entity
fn on_tree_row_dropped_on_root(
    event: On<TreeRowDroppedOnRoot>,
    mut commands: Commands,
    parent_query: Query<&ChildOf, Without<EditorEntity>>,
    tree_nodes: Query<(Entity, &TreeNode)>,
    container: Single<Entity, With<HierarchyTreeContainer>>,
) {
    let dragged = event.dragged_source;

    // Skip if already a root entity (no ChildOf)
    if parent_query.get(dragged).is_err() {
        return;
    }

    // Remove ChildOf from the scene entity to make it a root
    commands.entity(dragged).remove::<ChildOf>();

    // Move the tree row to the root container
    if let Some((tree_entity, _)) = tree_nodes.iter().find(|(_, node)| node.0 == dragged) {
        commands.entity(tree_entity).insert(ChildOf(*container));
    }
}

/// Filter hierarchy tree rows based on the filter text input
fn apply_hierarchy_filter(
    filter_input: Query<&TextInput, (With<HierarchyFilter>, Changed<TextInput>)>,
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
        // Show all tree rows
        for (tree_entity, _) in &tree_nodes {
            if let Ok(mut node) = display_query.get_mut(tree_entity) {
                node.display = Display::Flex;
            }
        }
        return;
    }

    // First pass: determine which source entities match the filter
    let mut visible_tree_entities: Vec<Entity> = Vec::new();

    for (tree_entity, tree_node) in &tree_nodes {
        let matches = names
            .get(tree_node.0)
            .map(|n| n.as_str().to_lowercase().contains(&filter))
            .unwrap_or(false);

        if matches {
            visible_tree_entities.push(tree_entity);

            // Walk up ancestors: tree row → ChildOf → (TreeRowChildren) → ChildOf → parent tree row
            let mut current = tree_entity;
            while let Ok(&ChildOf(parent)) = parent_query.get(current) {
                if tree_row_children_query.contains(parent) {
                    // parent is a TreeRowChildren container, go up one more to the tree row
                    if let Ok(&ChildOf(grandparent)) = parent_query.get(parent) {
                        if !visible_tree_entities.contains(&grandparent) {
                            visible_tree_entities.push(grandparent);
                        }
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
