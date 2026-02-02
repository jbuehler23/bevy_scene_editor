use bevy::prelude::*;
use bevy_notify::prelude::*;
use bevy_text_input::{feathers::text_input, *};
use bevy_tree_view::*;
use crate::state::{EditorEntity, EditorState, RebuildRequest};
use crate::layout::HierarchyPanel;

#[derive(Component)]
pub struct HierarchyContent;

/// Marker for the entity that monitors hierarchy changes via bevy_notify.
/// When bevy_notify fires, the observer sets `dirty = true` on this component.
#[derive(Component)]
pub struct HierarchyMonitor {
    pub dirty: bool,
}

/// One-time setup: spawn a global bevy_notify monitor for Name and ChildOf changes.
pub fn setup_hierarchy_monitor(mut commands: Commands) {
    commands.spawn((
        EditorEntity,
        HierarchyMonitor { dirty: false },
        NotifyChanged::<Name>::default(),
        NotifyAdded::<Name>::default(),
        NotifyRemoved::<Name>::default(),
        NotifyChanged::<ChildOf>::default(),
        NotifyAdded::<ChildOf>::default(),
        NotifyRemoved::<ChildOf>::default(),
    ))
    .observe(|_trigger: On<Mutation<Name>>, mut monitor: Query<&mut HierarchyMonitor>| {
        for mut m in &mut monitor { m.dirty = true; }
    })
    .observe(|_trigger: On<Addition<Name>>, mut monitor: Query<&mut HierarchyMonitor>| {
        for mut m in &mut monitor { m.dirty = true; }
    })
    .observe(|_trigger: On<Removal<Name>>, mut monitor: Query<&mut HierarchyMonitor>| {
        for mut m in &mut monitor { m.dirty = true; }
    })
    .observe(|_trigger: On<Mutation<ChildOf>>, mut monitor: Query<&mut HierarchyMonitor>| {
        for mut m in &mut monitor { m.dirty = true; }
    })
    .observe(|_trigger: On<Addition<ChildOf>>, mut monitor: Query<&mut HierarchyMonitor>| {
        for mut m in &mut monitor { m.dirty = true; }
    })
    .observe(|_trigger: On<Removal<ChildOf>>, mut monitor: Query<&mut HierarchyMonitor>| {
        for mut m in &mut monitor { m.dirty = true; }
    });
}

/// Detects changes and sets the rebuild flag.
pub fn hierarchy_change_detection_system(
    editor_state: Res<EditorState>,
    mut rebuild: ResMut<RebuildRequest>,
    mut monitors: Query<&mut HierarchyMonitor>,
    mut initialized: Local<bool>,
) {
    if !*initialized {
        *initialized = true;
        rebuild.hierarchy = true;
        rebuild.inspector = true;
        return;
    }

    // Check if bevy_notify flagged a change
    for mut monitor in &mut monitors {
        if monitor.dirty {
            monitor.dirty = false;
            rebuild.hierarchy = true;
        }
    }

    if editor_state.is_changed() {
        rebuild.hierarchy = true;
        rebuild.inspector = true;
    }
}

/// Rebuilds hierarchy UI. Runs in the EditorRebuild set.
pub fn rebuild_hierarchy_system(
    mut commands: Commands,
    panel_query: Query<Entity, With<HierarchyPanel>>,
    entities: Query<(Entity, Option<&Name>, Option<&ChildOf>), Without<EditorEntity>>,
    editor_state: Res<EditorState>,
    mut rebuild: ResMut<RebuildRequest>,
) {
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
            children![(
                text_input(),
                TextInputPlaceholder::new("Filter entities...")
            )],
        ))
        .id();

    let tree_entity = commands
        .spawn((
            EditorEntity,
            HierarchyContent,
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
