use bevy::prelude::*;
use bevy_notify::prelude::*;
use bevy_tree_view::*;
use bevy_text_input::*;
use crate::state::{EditorEntity, EditorState, RebuildRequest};
use crate::layout::HierarchyPanel;

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

/// Reactively update a tree row's label when the source entity's Name changes.
fn on_name_changed(
    trigger: On<Mutation<Name>>,
    names: Query<&Name>,
    rows: Query<(Entity, &TreeNode, &Children)>,
    mut labels: Query<&mut Text, With<TreeRowLabel>>,
    editor_entities: Query<(), With<EditorEntity>>,
) {
    let mutated_entity = trigger.mutated;

    // Skip editor entities
    if editor_entities.contains(mutated_entity) {
        return;
    }

    let Ok(name) = names.get(mutated_entity) else { return };

    // Find the tree row that monitors this entity
    for (_row_entity, node, children) in &rows {
        if node.source_entity == Some(mutated_entity) {
            // Update the label text
            for child in children.iter() {
                if let Ok(mut text) = labels.get_mut(child) {
                    *text = Text::new(name.to_string());
                    break;
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

    // Determine depth based on parent hierarchy
    let depth = calculate_depth(added_entity, &child_of_query);
    let has_children = children_query.get(added_entity).is_ok_and(|c| !c.is_empty());
    let is_selected = editor_state.selected_entity == Some(added_entity);

    let row_entity = commands
        .spawn((
            EditorEntity,
            tree_row(name.as_str(), depth, true, has_children, is_selected, Some(added_entity)),
        ))
        .id();

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
    mut rebuild: ResMut<RebuildRequest>,
    editor_entities: Query<(), With<EditorEntity>>,
    names: Query<&Name>,
) {
    let mutated_entity = trigger.mutated;

    // Skip editor entities
    if editor_entities.contains(mutated_entity) {
        return;
    }

    // Only react to entities with Names (scene entities)
    // Skip unnamed entities (internal Bevy/UI entities)
    if names.get(mutated_entity).is_err() {
        return;
    }

    // Reparenting is complex - fall back to full rebuild
    rebuild.hierarchy = true;
}

fn on_parent_added(
    trigger: On<Addition<ChildOf>>,
    mut rebuild: ResMut<RebuildRequest>,
    editor_entities: Query<(), With<EditorEntity>>,
    names: Query<&Name>,
) {
    let added_entity = trigger.added;

    // Skip editor entities
    if editor_entities.contains(added_entity) {
        return;
    }

    // Only react to entities with Names (scene entities)
    // Skip unnamed entities (internal Bevy/UI entities)
    if names.get(added_entity).is_err() {
        return;
    }

    // Parent added - needs rebuild for proper tree structure
    rebuild.hierarchy = true;
}

fn on_parent_removed(
    trigger: On<Removal<ChildOf>>,
    mut rebuild: ResMut<RebuildRequest>,
    editor_entities: Query<(), With<EditorEntity>>,
    names: Query<&Name>,
) {
    let removed_entity = trigger.removed;

    // Skip editor entities
    if editor_entities.contains(removed_entity) {
        return;
    }

    // Only react to entities with Names (scene entities)
    // Skip unnamed entities (internal Bevy/UI entities)
    if names.get(removed_entity).is_err() {
        return;
    }

    // Parent removed - needs rebuild for proper tree structure
    rebuild.hierarchy = true;
}

fn calculate_depth(entity: Entity, child_of_query: &Query<&ChildOf>) -> u32 {
    let mut depth = 0;
    let mut current = entity;
    while let Ok(child_of) = child_of_query.get(current) {
        depth += 1;
        current = child_of.parent();
    }
    depth
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
    let mut children_map: std::collections::HashMap<Entity, Vec<(Entity, Option<String>)>> =
        std::collections::HashMap::new();

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
            children![
                text_input_field("Filter entities..."),
            ],
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

    fn spawn_tree(
        commands: &mut Commands,
        parent: Entity,
        entities: &[(Entity, Option<String>)],
        children_map: &std::collections::HashMap<Entity, Vec<(Entity, Option<String>)>>,
        depth: u32,
        selected: Option<Entity>,
    ) {
        for (entity, name) in entities {
            let fallback = format!("Entity {:?}", entity);
            let label = name.as_deref().unwrap_or(&fallback);
            let has_children = children_map.contains_key(entity);
            let is_selected = selected == Some(*entity);

            let row_entity = commands
                .spawn((
                    EditorEntity,
                    tree_row(label, depth, true, has_children, is_selected, Some(*entity)),
                ))
                .id();

            commands.entity(parent).add_child(row_entity);

            if has_children {
                if let Some(children) = children_map.get(entity) {
                    let mut sorted = children.clone();
                    sorted.sort_by(|a, b| a.1.cmp(&b.1));
                    spawn_tree(commands, parent, &sorted, children_map, depth + 1, selected);
                }
            }
        }
    }

    spawn_tree(
        &mut commands,
        tree_entity,
        &root_entities,
        &children_map,
        0,
        editor_state.selected_entity,
    );

    commands.entity(panel_entity).add_children(&[filter_entity, tree_entity]);
}

pub fn hierarchy_click_system(
    query: Query<(Entity, &Interaction, &TreeNode), (Changed<Interaction>, With<TreeNode>)>,
    mut editor_state: ResMut<EditorState>,
) {
    for (_, interaction, node) in &query {
        if *interaction == Interaction::Pressed {
            if let Some(source) = node.source_entity {
                editor_state.selected_entity = Some(source);
            }
        }
    }
}

pub fn filter_system(
    _changed: Query<(Entity, &TextInput), (Changed<TextInput>, With<TextInput>)>,
) {
    // TODO: integrate filtering to trigger rebuild
}

const SELECTED_BG: Color = Color::srgba(0.2, 0.4, 0.7, 0.5);

/// Detects selection changes and triggers inspector rebuild + updates tree row highlighting.
pub fn selection_change_system(
    editor_state: Res<EditorState>,
    mut rebuild: ResMut<RebuildRequest>,
    rows: Query<(Entity, &TreeNode)>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut prev_selected: Local<Option<Entity>>,
) {
    if !editor_state.is_changed() {
        return;
    }

    rebuild.inspector = true;

    // Unhighlight previous selection
    if let Some(prev) = *prev_selected {
        for (row_entity, node) in &rows {
            if node.source_entity == Some(prev) {
                if let Ok(mut bg) = backgrounds.get_mut(row_entity) {
                    *bg = BackgroundColor(Color::NONE);
                }
                break;
            }
        }
    }

    // Highlight new selection
    if let Some(selected) = editor_state.selected_entity {
        for (row_entity, node) in &rows {
            if node.source_entity == Some(selected) {
                if let Ok(mut bg) = backgrounds.get_mut(row_entity) {
                    *bg = BackgroundColor(SELECTED_BG);
                }
                break;
            }
        }
    }

    *prev_selected = editor_state.selected_entity;
}
