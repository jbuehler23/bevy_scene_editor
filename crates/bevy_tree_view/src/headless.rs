use bevy::prelude::*;

#[derive(Component, Default)]
pub struct TreeView;

#[derive(Component)]
pub struct TreeNode {
    pub expanded: bool,
    pub depth: u32,
    pub source_entity: Option<Entity>,
}

impl Default for TreeNode {
    fn default() -> Self {
        Self {
            expanded: true,
            depth: 0,
            source_entity: None,
        }
    }
}

#[derive(Component)]
pub struct TreeNodeSelected;

#[derive(Component)]
pub struct TreeNodeExpandToggle;

/// Marker for the Text entity that displays the tree row label.
#[derive(Component)]
pub struct TreeRowLabel;

/// Marker for the container that holds a tree row's child rows.
#[derive(Component)]
pub struct TreeRowChildren;

/// Marker for the interactive row content (clickable part with arrow and label).
#[derive(Component)]
pub struct TreeRowContent;

#[derive(Message)]
pub struct TreeNodeActivated {
    pub node_entity: Entity,
    pub source_entity: Option<Entity>,
}

pub fn tree_node_toggle_system(
    query: Query<(Entity, &Interaction, &ChildOf), (Changed<Interaction>, With<TreeNodeExpandToggle>)>,
    child_of_query: Query<&ChildOf>,
    mut nodes: Query<&mut TreeNode>,
) {
    for (_, interaction, toggle_child_of) in &query {
        if *interaction == Interaction::Pressed {
            // Toggle is child of content, content is child of row
            // So we need to go up two levels: toggle -> content -> row
            let content_entity = toggle_child_of.parent();
            if let Ok(content_child_of) = child_of_query.get(content_entity) {
                let row_entity = content_child_of.parent();
                if let Ok(mut node) = nodes.get_mut(row_entity) {
                    node.expanded = !node.expanded;
                }
            }
        }
    }
}

pub fn tree_keyboard_nav_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    selected: Query<(Entity, &TreeNode), With<TreeNodeSelected>>,
    mut commands: Commands,
) {
    if !keyboard.any_just_pressed([KeyCode::ArrowUp, KeyCode::ArrowDown, KeyCode::ArrowLeft, KeyCode::ArrowRight]) {
        return;
    }

    let Ok((selected_entity, selected_node)) = selected.single() else {
        return;
    };

    if keyboard.just_pressed(KeyCode::ArrowLeft) {
        // Collapse if expanded, otherwise do nothing
        if selected_node.expanded {
            // We can't mutate here since we borrowed as immutable; use commands
            commands.entity(selected_entity).entry::<TreeNode>().and_modify(|mut n| n.expanded = false);
        }
    } else if keyboard.just_pressed(KeyCode::ArrowRight) {
        if !selected_node.expanded {
            commands.entity(selected_entity).entry::<TreeNode>().and_modify(|mut n| n.expanded = true);
        }
    }
}
