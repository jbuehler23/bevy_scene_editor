use bevy::prelude::*;
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy_tree_view::TreeViewPlugin;
use bevy_text_input::TextInputPlugin;
use bevy_split_panel::SplitPanelPlugin;

use crate::state::{EditorState, RebuildRequest};
use crate::layout;
use crate::hierarchy;
use crate::inspector;
use crate::viewport;
use crate::systems;

pub struct EditorPlugin;

/// Systems that handle user interaction.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EditorInput;

/// Systems that detect changes and set rebuild flags.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EditorDetect;

/// Systems that destroy and recreate UI content.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EditorRebuild;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            TreeViewPlugin,
            TextInputPlugin,
            SplitPanelPlugin,
            MeshPickingPlugin,
        ))
        .init_resource::<EditorState>()
        .init_resource::<RebuildRequest>()
        .configure_sets(Update, (
            EditorInput,
            EditorDetect,
            EditorRebuild,
        ).chain())
        .add_systems(Startup, (
            layout::spawn_editor_layout,
            viewport::spawn_editor_camera,
            hierarchy::setup_hierarchy_monitor,
        ))
        // Phase 1: Handle user input (clicks, drags, keyboard)
        .add_systems(Update, (
            hierarchy::hierarchy_click_system,
            hierarchy::filter_system,
            viewport::orbit_camera_system,
            viewport::picking_system,
            systems::selection_highlight_system,
            systems::sync_split_panel_sizes,
            bevy_split_panel::split_handle_hover_system,
        ).in_set(EditorInput))
        // Phase 2: Detect changes and set rebuild flags
        .add_systems(Update, (
            hierarchy::hierarchy_change_detection_system,
        ).in_set(EditorDetect))
        // Phase 3: Rebuild UI (despawn old, spawn new)
        // Chained sets ensure apply_deferred runs between phases.
        .add_systems(Update, (
            hierarchy::rebuild_hierarchy_system,
            inspector::rebuild_inspector_system,
        ).in_set(EditorRebuild));
    }
}
