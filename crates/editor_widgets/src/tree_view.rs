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

/// Tracks whether a tree node's children have been lazily populated.
/// Set to `true` after first expansion spawns children; prevents re-population on re-expand.
#[derive(Component, Default)]
pub struct TreeChildrenPopulated(pub bool);

/// Classifies a scene entity by type for sorting and colored dot display.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntityCategory {
    Camera,
    Light,
    Mesh,
    Scene,
    #[default]
    Entity,
}

/// Marker for the colored category dot in a tree row.
#[derive(Component)]
pub struct TreeRowDot;

/// Marker for the visibility toggle icon in a tree row.
#[derive(Component)]
pub struct TreeRowVisibilityToggle;

/// Event fired when a visibility toggle is clicked
#[derive(EntityEvent)]
pub struct TreeRowVisibilityToggled {
    #[event_target]
    pub entity: Entity,
    /// The source (scene) entity to toggle visibility
    pub source_entity: Entity,
}

/// Marker on the text input during inline rename
#[derive(Component)]
pub struct TreeRowInlineRename;

// ---------------------------------------------------------------------------
// Index resource: source entity → tree row entity (O(1) lookups)
// ---------------------------------------------------------------------------

/// Maps source (scene) entities to their corresponding tree row UI entities.
/// Maintained automatically by systems that react to `TreeNode` additions/removals.
#[derive(Resource, Default)]
pub struct TreeIndex {
    /// source entity → tree row entity
    map: HashMap<Entity, Entity>,
}

impl TreeIndex {
    /// Get the tree row entity for a given source entity.
    pub fn get(&self, source: Entity) -> Option<Entity> {
        self.map.get(&source).copied()
    }

    /// Insert a mapping from source entity to tree row entity.
    pub fn insert(&mut self, source: Entity, tree_row: Entity) {
        self.map.insert(source, tree_row);
    }

    /// Remove the mapping for a source entity.
    pub fn remove(&mut self, source: Entity) {
        self.map.remove(&source);
    }

    /// Check if a source entity has a tree row.
    pub fn contains(&self, source: Entity) -> bool {
        self.map.contains_key(&source)
    }
}

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Focus tracking
// ---------------------------------------------------------------------------

/// Tracks which tree row has keyboard focus (rendered with a focus ring).
#[derive(Resource, Default)]
pub struct TreeFocused(pub Option<Entity>);

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

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

/// Event fired when an inline rename is committed
#[derive(EntityEvent)]
pub struct TreeRowRenamed {
    #[event_target]
    pub entity: Entity,
    /// The source (scene) entity
    pub source_entity: Entity,
    /// The new name entered by the user
    pub new_name: String,
}

/// Event fired to request starting an inline rename
#[derive(EntityEvent)]
pub struct TreeRowStartRename {
    #[event_target]
    pub entity: Entity,
    /// The source (scene) entity to rename
    pub source_entity: Entity,
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct TreeViewPlugin;

impl Plugin for TreeViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TreeIndex>()
            .init_resource::<TreeFocused>()
            .add_systems(PostUpdate, (maintain_tree_index,));
    }
}

/// Keep TreeIndex in sync with TreeNode additions and removals.
fn maintain_tree_index(
    mut index: ResMut<TreeIndex>,
    added: Query<(Entity, &TreeNode), Added<TreeNode>>,
    mut removed: RemovedComponents<TreeNode>,
) {
    for (tree_row, tree_node) in &added {
        index.insert(tree_node.0, tree_row);
    }

    for removed_entity in removed.read() {
        // Scan the map to find which source entity maps to this removed tree row.
        // This is O(n) but only runs on removal frames, not every frame.
        let source = index.map.iter()
            .find(|(_, tree_row)| **tree_row == removed_entity)
            .map(|(source, _)| *source);
        if let Some(source) = source {
            index.remove(source);
        }
    }
}
