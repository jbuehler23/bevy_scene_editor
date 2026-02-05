use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct EditorState {
    pub selected_entity: Option<Entity>,
}

/// Marker component for all editor UI entities, excluded from inspection.
#[derive(Component)]
pub struct EditorEntity;

/// Flag resource to request a hierarchy rebuild next frame.
#[derive(Resource, Default)]
pub struct RebuildRequest {
    pub hierarchy: bool,
    pub inspector: bool,
}

/// Tracks drag-and-drop state for hierarchy reparenting.
#[derive(Resource, Default)]
pub struct DragState {
    /// The scene entity currently being dragged, if any.
    pub dragging: Option<Entity>,
}
