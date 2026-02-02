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

#[derive(Message)]
pub struct TreeNodeActivated {
    pub node_entity: Entity,
    pub source_entity: Option<Entity>,
}

pub fn tree_node_toggle_system(
    query: Query<(Entity, &Interaction, &ChildOf), (Changed<Interaction>, With<TreeNodeExpandToggle>)>,
    mut nodes: Query<&mut TreeNode>,
) {
    for (_, interaction, child_of) in &query {
        if *interaction == Interaction::Pressed {
            if let Ok(mut node) = nodes.get_mut(child_of.parent()) {
                node.expanded = !node.expanded;
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
