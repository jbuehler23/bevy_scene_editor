use bevy::prelude::*;

/// Marker for the tree view container
#[derive(Component)]
pub struct TreeView;

/// Links a tree row UI entity to the source entity it represents
#[derive(Component)]
#[relationship(relationship_target = TreeNodeSource)]
pub struct TreeNode(pub Entity);

/// Inverse relationship: source entity -> tree row
#[derive(Component)]
#[relationship_target(relationship = TreeNode)]
pub struct TreeNodeSource(Entity);

/// Marker for expand/collapse toggle button
#[derive(Component)]
pub struct TreeNodeExpandToggle;

/// Tracks whether a tree node is expanded
#[derive(Component, Default)]
pub struct TreeNodeExpanded(pub bool);

/// The clickable content area of a tree row (contains toggle + label)
#[derive(Component)]
pub struct TreeRowContent;

/// Marker on TreeRowContent when its source entity is selected
#[derive(Component)]
pub struct TreeRowSelected;

/// Container for displaying the row label
#[derive(Component)]
#[require(Text)]
pub struct TreeRowLabel;

/// Container for child rows (indented)
#[derive(Component)]
pub struct TreeRowChildren;

/// Event fired when a tree row is clicked
#[derive(EntityEvent)]
pub struct TreeRowClicked {
    #[event_target]
    pub entity: Entity,
    /// The source entity this tree row represents
    pub source_entity: Entity,
}

/// Event fired when a tree row is dropped onto another tree row
#[derive(EntityEvent)]
pub struct TreeRowDropped {
    #[event_target]
    pub entity: Entity,
    /// The scene entity being moved
    pub dragged_source: Entity,
    /// The scene entity to become new parent
    pub target_source: Entity,
}

/// Event fired when a tree row is dropped onto the root container (deparent)
#[derive(EntityEvent)]
pub struct TreeRowDroppedOnRoot {
    #[event_target]
    pub entity: Entity,
    /// The scene entity being moved back to root
    pub dragged_source: Entity,
}

pub struct TreeViewPlugin;

impl Plugin for TreeViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(toggle_tree_node_expanded);
    }
}

fn toggle_tree_node_expanded(
    _click: On<Pointer<Click>>,
    mut commands: Commands,
    toggle_query: Query<&ChildOf, With<TreeNodeExpandToggle>>,
    tree_node_query: Query<(Entity, &TreeNodeExpanded)>,
) {
    let Ok(&ChildOf(parent)) = toggle_query.get(_click.event_target()) else {
        return;
    };

    // The parent of the toggle is TreeRowContent, and its parent is the tree row
    // Actually, let's find the tree node by walking up
    if let Ok((entity, expanded)) = tree_node_query.get(parent) {
        commands
            .entity(entity)
            .insert(TreeNodeExpanded(!expanded.0));
    }
}
