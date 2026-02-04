use bevy::prelude::*;

#[derive(Component)]
pub struct TreeView;

#[derive(Component)]
#[relationship(relationship_target = TreeNodeSource)]
pub struct TreeNode(pub Entity);

#[derive(Component)]
#[relationship_target(relationship = TreeNode)]
pub struct TreeNodeSource(Entity);

#[derive(Component)]
pub struct TreeNodeExpandToggle;
