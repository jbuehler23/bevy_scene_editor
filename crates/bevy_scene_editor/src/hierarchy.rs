use bevy::prelude::*;
use bevy_notify::prelude::*;
use bevy_tree_view::*;
use bevy_text_input::*;
use std::collections::HashMap;
use crate::state::{EditorEntity, EditorState, RebuildRequest, DragState};
use crate::layout::HierarchyPanel;

const INDENT_PX: f32 = 16.0;
const ROW_HEIGHT: f32 = 24.0;
const SELECTED_BG: Color = Color::srgba(0.2, 0.4, 0.7, 0.5);
const DROP_TARGET_BG: Color = Color::srgba(0.4, 0.6, 0.3, 0.5);

#[derive(Component)]
pub struct HierarchyContent;

/// Marker for the global monitor that watches for entity spawns/despawns/changes.
#[derive(Component)]
pub struct HierarchyGlobalMonitor;

/// Marker for the tree container entity that holds all tree rows.
#[derive(Component)]
pub struct HierarchyTreeContainer;

/// One-time setup: spawn a global bevy_notify monitor for hierarchy changes.
pub fn setup_hierarchy_monitor(mut commands: Commands) {
    commands.spawn((
        EditorEntity,
        HierarchyGlobalMonitor,
        // No Monitor component = react to ALL entities
        NotifyChanged::<Name>::default(),
        NotifyAdded::<Name>::default(),
        NotifyRemoved::<Name>::default(),
        NotifyChanged::<ChildOf>::default(),
        NotifyAdded::<ChildOf>::default(),
        NotifyRemoved::<ChildOf>::default(),
    ))
    .observe(on_name_changed)
    .observe(on_entity_added)
    .observe(on_entity_removed)
    .observe(on_parent_changed)
    .observe(on_parent_added)
    .observe(on_parent_removed);
}

/// Spawns a tree row with hierarchical structure and returns (row_entity, children_container_entity).
fn spawn_row(
    commands: &mut Commands,
    label: &str,
    has_children: bool,
    is_selected: bool,
    source_entity: Option<Entity>,
) -> (Entity, Entity) {
    let bg = if is_selected {
        BackgroundColor(SELECTED_BG)
    } else {
        BackgroundColor(Color::NONE)
    };
    let arrow = if has_children { "▼ " } else { "  " };

    // Spawn row container (vertical flex column)
    let row_entity = commands
        .spawn((
            EditorEntity,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            TreeNode {
                expanded: true,
                depth: 0,
                source_entity,
            },
        ))
        .id();

    // Spawn row content (the clickable part)
    let content_entity = commands
        .spawn((
            EditorEntity,
            TreeRowContent,
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(ROW_HEIGHT),
                align_items: AlignItems::Center,
                ..default()
            },
            bg,
            Interaction::default(),
        ))
        .id();

    // Spawn arrow toggle
    let toggle_entity = commands
        .spawn((
            EditorEntity,
            TreeNodeExpandToggle,
            Node {
                width: Val::Px(16.0),
                height: Val::Px(ROW_HEIGHT),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            Interaction::default(),
        ))
        .with_child((
            Text::new(arrow.to_string()),
            TextFont {
                font_size: 10.0,
                ..default()
            },
        ))
        .id();

    // Spawn label
    let label_entity = commands
        .spawn((
            EditorEntity,
            TreeRowLabel,
            Text::new(label.to_string()),
            TextFont {
                font_size: 13.0,
                ..default()
            },
        ))
        .id();

    commands
        .entity(content_entity)
        .add_children(&[toggle_entity, label_entity]);

    // Spawn children container (indented)
    let children_container = commands
        .spawn((
            EditorEntity,
            TreeRowChildren,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::left(Val::Px(INDENT_PX)),
                ..default()
            },
        ))
        .id();

    commands
        .entity(row_entity)
        .add_children(&[content_entity, children_container]);

    (row_entity, children_container)
}

/// Reactively update a tree row's label when the source entity's Name changes.
fn on_name_changed(
    trigger: On<Mutation<Name>>,
    names: Query<&Name>,
    rows: Query<(Entity, &TreeNode)>,
    children_query: Query<&Children>,
    mut labels: Query<&mut Text, With<TreeRowLabel>>,
    content_query: Query<(), With<TreeRowContent>>,
    editor_entities: Query<(), With<EditorEntity>>,
) {
    let mutated_entity = trigger.mutated;

    // Skip editor entities
    if editor_entities.contains(mutated_entity) {
        return;
    }

    let Ok(name) = names.get(mutated_entity) else { return };

    // Find the tree row that monitors this entity
    for (row_entity, node) in &rows {
        if node.source_entity == Some(mutated_entity) {
            // Row structure: row -> [content, children_container]
            // Content structure: content -> [toggle, label]
            let Ok(row_children) = children_query.get(row_entity) else { continue };
            for row_child in row_children.iter() {
                // Find the content entity (has TreeRowContent)
                if content_query.contains(row_child) {
                    let Ok(content_children) = children_query.get(row_child) else { continue };
                    for content_child in content_children.iter() {
                        if let Ok(mut text) = labels.get_mut(content_child) {
                            *text = Text::new(name.to_string());
                            return;
                        }
                    }
                }
            }
            break;
        }
    }
}

fn on_entity_added(
    trigger: On<Addition<Name>>,
    mut commands: Commands,
    tree_container: Query<Entity, With<HierarchyTreeContainer>>,
    names: Query<&Name>,
    child_of_query: Query<&ChildOf>,
    children_query: Query<&Children>,
    editor_entities: Query<(), With<EditorEntity>>,
    editor_state: Res<EditorState>,
    rows: Query<(Entity, &TreeNode)>,
    children_containers: Query<Entity, With<TreeRowChildren>>,
) {
    let added_entity = trigger.added;

    // Skip editor entities
    if editor_entities.contains(added_entity) {
        return;
    }

    let Ok(container) = tree_container.single() else {
        return;
    };

    let Ok(name) = names.get(added_entity) else {
        return;
    };

    let has_children = children_query.get(added_entity).is_ok_and(|c| !c.is_empty());
    let is_selected = editor_state.selected_entity == Some(added_entity);

    // Spawn the row with hierarchical structure
    let (row_entity, _children_container) =
        spawn_row(&mut commands, name.as_str(), has_children, is_selected, Some(added_entity));

    // Determine where to add the row
    if let Ok(child_of) = child_of_query.get(added_entity) {
        // Entity has a parent - try to add to parent's TreeRowChildren
        let parent = child_of.parent();
        for (parent_row, node) in &rows {
            if node.source_entity == Some(parent) {
                if let Ok(parent_row_children) = children_query.get(parent_row) {
                    for child in parent_row_children.iter() {
                        if children_containers.contains(child) {
                            commands.entity(child).add_child(row_entity);
                            return;
                        }
                    }
                }
            }
        }
    }

    // No parent or parent row not found - add to root container
    commands.entity(container).add_child(row_entity);
}

fn on_entity_removed(
    trigger: On<Removal<Name>>,
    mut commands: Commands,
    rows: Query<(Entity, &TreeNode)>,
    editor_entities: Query<(), With<EditorEntity>>,
) {
    let removed_entity = trigger.removed;

    // Skip editor entities
    if editor_entities.contains(removed_entity) {
        return;
    }

    // Find and despawn the row for this entity
    for (row_entity, node) in &rows {
        if node.source_entity == Some(removed_entity) {
            commands.entity(row_entity).despawn();
            break;
        }
    }
}

fn on_parent_changed(
    trigger: On<Mutation<ChildOf>>,
    mut commands: Commands,
    editor_entities: Query<(), With<EditorEntity>>,
    names: Query<&Name>,
    child_of_query: Query<&ChildOf>,
    rows: Query<(Entity, &TreeNode)>,
    children_query: Query<&Children>,
    children_containers: Query<Entity, With<TreeRowChildren>>,
) {
    let mutated_entity = trigger.mutated;

    // Skip editor entities
    if editor_entities.contains(mutated_entity) {
        return;
    }

    // Only react to entities with Names (scene entities)
    if names.get(mutated_entity).is_err() {
        return;
    }

    // Find the tree row for the mutated entity
    let Some((row_entity, _)) = rows.iter().find(|(_, n)| n.source_entity == Some(mutated_entity))
    else {
        return;
    };

    // Find the new parent's tree row children container
    if let Ok(child_of) = child_of_query.get(mutated_entity) {
        let new_parent = child_of.parent();
        for (parent_row, node) in &rows {
            if node.source_entity == Some(new_parent) {
                if let Ok(parent_row_children) = children_query.get(parent_row) {
                    for child in parent_row_children.iter() {
                        if children_containers.contains(child) {
                            commands.entity(row_entity).set_parent_in_place(child);
                            return;
                        }
                    }
                }
            }
        }
    }
}

fn on_parent_added(
    trigger: On<Addition<ChildOf>>,
    mut commands: Commands,
    editor_entities: Query<(), With<EditorEntity>>,
    names: Query<&Name>,
    child_of_query: Query<&ChildOf>,
    rows: Query<(Entity, &TreeNode)>,
    children_query: Query<&Children>,
    children_containers: Query<Entity, With<TreeRowChildren>>,
) {
    let added_entity = trigger.added;

    // Skip editor entities
    if editor_entities.contains(added_entity) {
        return;
    }

    // Only react to entities with Names (scene entities)
    if names.get(added_entity).is_err() {
        return;
    }

    // Find the tree row for this entity
    let Some((row_entity, _)) = rows.iter().find(|(_, n)| n.source_entity == Some(added_entity))
    else {
        return;
    };

    // Find the parent's tree row children container
    if let Ok(child_of) = child_of_query.get(added_entity) {
        let parent = child_of.parent();
        for (parent_row, node) in &rows {
            if node.source_entity == Some(parent) {
                if let Ok(parent_row_children) = children_query.get(parent_row) {
                    for child in parent_row_children.iter() {
                        if children_containers.contains(child) {
                            commands.entity(row_entity).set_parent_in_place(child);
                            return;
                        }
                    }
                }
            }
        }
    }
}

fn on_parent_removed(
    trigger: On<Removal<ChildOf>>,
    mut commands: Commands,
    editor_entities: Query<(), With<EditorEntity>>,
    names: Query<&Name>,
    rows: Query<(Entity, &TreeNode)>,
    tree_container: Query<Entity, With<HierarchyTreeContainer>>,
) {
    let removed_entity = trigger.removed;

    // Skip editor entities
    if editor_entities.contains(removed_entity) {
        return;
    }

    // Only react to entities with Names (scene entities)
    if names.get(removed_entity).is_err() {
        return;
    }

    let Ok(container) = tree_container.single() else {
        return;
    };

    // Find the tree row and move it to root
    for (row_entity, node) in &rows {
        if node.source_entity == Some(removed_entity) {
            commands.entity(row_entity).set_parent_in_place(container);
            return;
        }
    }
}

/// Initial build of hierarchy UI. Runs once at startup and on rebuild requests.
pub fn rebuild_hierarchy_system(
    mut commands: Commands,
    panel_query: Query<Entity, With<HierarchyPanel>>,
    entities: Query<(Entity, Option<&Name>, Option<&ChildOf>), Without<EditorEntity>>,
    editor_state: Res<EditorState>,
    mut rebuild: ResMut<RebuildRequest>,
    mut initialized: Local<bool>,
) {
    // Always build on first frame
    if !*initialized {
        *initialized = true;
        rebuild.hierarchy = true;
    }

    if !rebuild.hierarchy {
        return;
    }
    rebuild.hierarchy = false;

    let Ok(panel_entity) = panel_query.single() else {
        return;
    };

    commands.entity(panel_entity).despawn_children();

    let mut root_entities: Vec<(Entity, Option<String>)> = Vec::new();
    let mut children_map: HashMap<Entity, Vec<(Entity, Option<String>)>> = HashMap::new();

    for (entity, name, child_of) in &entities {
        let name_str = name.map(|n| n.to_string());
        match child_of {
            Some(c) => {
                children_map
                    .entry(c.parent())
                    .or_default()
                    .push((entity, name_str));
            }
            None => {
                root_entities.push((entity, name_str));
            }
        }
    }

    root_entities.sort_by(|a, b| a.1.cmp(&b.1));

    let filter_entity = commands
        .spawn((
            EditorEntity,
            HierarchyContent,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(4.0)),
                ..default()
            },
            children![text_input_field("Filter entities..."),],
        ))
        .id();

    let tree_entity = commands
        .spawn((
            EditorEntity,
            HierarchyContent,
            HierarchyTreeContainer,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                flex_grow: 1.0,
                ..default()
            },
        ))
        .id();

    fn spawn_tree_hierarchical(
        commands: &mut Commands,
        parent_children_container: Entity,
        entities: &[(Entity, Option<String>)],
        children_map: &HashMap<Entity, Vec<(Entity, Option<String>)>>,
        selected: Option<Entity>,
    ) {
        for (entity, name) in entities {
            let fallback = format!("Entity {:?}", entity);
            let label = name.as_deref().unwrap_or(&fallback);
            let has_children = children_map.contains_key(entity);
            let is_selected = selected == Some(*entity);

            let (row_entity, children_container) =
                spawn_row(commands, label, has_children, is_selected, Some(*entity));
            commands.entity(parent_children_container).add_child(row_entity);

            // Recursively spawn children into this row's TreeRowChildren
            if let Some(children) = children_map.get(entity) {
                let mut sorted = children.clone();
                sorted.sort_by(|a, b| a.1.cmp(&b.1));
                spawn_tree_hierarchical(commands, children_container, &sorted, children_map, selected);
            }
        }
    }

    spawn_tree_hierarchical(
        &mut commands,
        tree_entity,
        &root_entities,
        &children_map,
        editor_state.selected_entity,
    );

    commands
        .entity(panel_entity)
        .add_children(&[filter_entity, tree_entity]);
}

pub fn hierarchy_click_system(
    content_query: Query<(Entity, &Interaction, &ChildOf), (Changed<Interaction>, With<TreeRowContent>)>,
    rows: Query<&TreeNode>,
    mut editor_state: ResMut<EditorState>,
) {
    for (_content, interaction, content_parent) in &content_query {
        if *interaction == Interaction::Pressed {
            // Content's parent is the row entity which has TreeNode
            if let Ok(node) = rows.get(content_parent.parent()) {
                if let Some(source) = node.source_entity {
                    editor_state.selected_entity = Some(source);
                }
            }
        }
    }
}

pub fn filter_system(
    _changed: Query<(Entity, &TextInput), (Changed<TextInput>, With<TextInput>)>,
) {
    // TODO: integrate filtering to trigger rebuild
}

/// Check if dropping dragged onto target would create a cycle.
fn is_valid_drop(dragged: Entity, target: Entity, child_of_query: &Query<&ChildOf, Without<EditorEntity>>) -> bool {
    if dragged == target {
        return false;
    }

    // Walk up from target to see if we'd hit dragged (cycle detection)
    let mut current = target;
    while let Ok(child_of) = child_of_query.get(current) {
        let parent = child_of.parent();
        if parent == dragged {
            return false; // Would create cycle
        }
        current = parent;
    }
    true
}

/// Handles drag-and-drop for reparenting entities in the hierarchy.
pub fn hierarchy_drag_drop_system(
    mouse: Res<ButtonInput<MouseButton>>,
    mut drag_state: ResMut<DragState>,
    mut commands: Commands,
    content_query: Query<(Entity, &Interaction, &ChildOf), With<TreeRowContent>>,
    rows: Query<&TreeNode>,
    mut backgrounds: Query<&mut BackgroundColor>,
    child_of_query: Query<&ChildOf, Without<EditorEntity>>,
    mut prev_hovered: Local<Option<Entity>>,
) {
    // Start drag from pressed row
    if mouse.just_pressed(MouseButton::Left) {
        for (_content, interaction, content_parent) in &content_query {
            if *interaction == Interaction::Pressed {
                if let Ok(node) = rows.get(content_parent.parent()) {
                    drag_state.dragging = node.source_entity;
                    break;
                }
            }
        }
    }

    // Clear previous hover highlight
    if let Some(prev) = *prev_hovered {
        if let Ok(mut bg) = backgrounds.get_mut(prev) {
            *bg = BackgroundColor(Color::NONE);
        }
    }
    *prev_hovered = None;

    // Highlight valid drop targets while dragging
    if drag_state.dragging.is_some() {
        for (content_entity, interaction, content_parent) in &content_query {
            if *interaction == Interaction::Hovered {
                if let Ok(node) = rows.get(content_parent.parent()) {
                    if let (Some(dragged), Some(target)) = (drag_state.dragging, node.source_entity) {
                        if is_valid_drop(dragged, target, &child_of_query) {
                            if let Ok(mut bg) = backgrounds.get_mut(content_entity) {
                                *bg = BackgroundColor(DROP_TARGET_BG);
                            }
                            *prev_hovered = Some(content_entity);
                        }
                    }
                }
                break;
            }
        }
    }

    // Drop - just set_parent on scene entity, bevy_notify handles the rest
    if mouse.just_released(MouseButton::Left) {
        if let Some(dragged) = drag_state.dragging {
            for (_content, interaction, content_parent) in &content_query {
                if *interaction == Interaction::Hovered {
                    if let Ok(node) = rows.get(content_parent.parent()) {
                        if let Some(target) = node.source_entity {
                            if is_valid_drop(dragged, target, &child_of_query) {
                                commands.entity(dragged).set_parent_in_place(target);
                                // on_parent_changed observer handles tree row reparenting
                            }
                        }
                    }
                    break;
                }
            }
        }
        drag_state.dragging = None;
    }
}

/// Detects selection changes and triggers inspector rebuild + updates tree row highlighting.
pub fn selection_change_system(
    editor_state: Res<EditorState>,
    mut rebuild: ResMut<RebuildRequest>,
    rows: Query<(Entity, &TreeNode)>,
    children_query: Query<&Children>,
    content_query: Query<Entity, With<TreeRowContent>>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut prev_selected: Local<Option<Entity>>,
) {
    if !editor_state.is_changed() {
        return;
    }

    rebuild.inspector = true;

    // Helper to find content entity for a source entity
    let find_content = |source: Entity| -> Option<Entity> {
        for (row_entity, node) in &rows {
            if node.source_entity == Some(source) {
                if let Ok(row_children) = children_query.get(row_entity) {
                    for child in row_children.iter() {
                        if content_query.contains(child) {
                            return Some(child);
                        }
                    }
                }
            }
        }
        None
    };

    // Unhighlight previous selection
    if let Some(prev) = *prev_selected {
        if let Some(content_entity) = find_content(prev) {
            if let Ok(mut bg) = backgrounds.get_mut(content_entity) {
                *bg = BackgroundColor(Color::NONE);
            }
        }
    }

    // Highlight new selection
    if let Some(selected) = editor_state.selected_entity {
        if let Some(content_entity) = find_content(selected) {
            if let Ok(mut bg) = backgrounds.get_mut(content_entity) {
                *bg = BackgroundColor(SELECTED_BG);
            }
        }
    }

    *prev_selected = editor_state.selected_entity;
}
