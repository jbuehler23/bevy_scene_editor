mod geometry;
mod csg;
mod hull;
mod mesh;
mod interaction;
mod gizmo_overlay;

use std::collections::HashMap;

use bevy::prelude::*;

use crate::commands::EditorCommand;

// ---------------------------------------------------------------------------
// Re-exports from bevy_jsn / bevy_jsn_geometry
// ---------------------------------------------------------------------------

pub use bevy_jsn::{Brush, BrushFaceData, BrushPlane};
pub use self::geometry::{compute_brush_geometry, compute_face_tangent_axes};
pub use self::csg::{brush_planes_to_world, brushes_intersect, subtract_brush, clean_degenerate_faces};
pub use self::hull::HullFace;
pub(crate) use self::hull::merge_hull_triangles;
pub(crate) use self::interaction::{
    BrushDragState, VertexDragState, VertexDragConstraint, EdgeDragState, ClipState,
};

// ---------------------------------------------------------------------------
// Editor-only data structures (not serialized / not in bevy_jsn)
// ---------------------------------------------------------------------------

/// Cached computed geometry (NOT serialized, rebuilt from Brush).
#[derive(Component)]
pub struct BrushMeshCache {
    pub vertices: Vec<Vec3>,
    /// Per-face: ordered vertex indices into `vertices`.
    pub face_polygons: Vec<Vec<usize>>,
    pub face_entities: Vec<Entity>,
}

/// Marker on child entities that render individual brush faces.
#[derive(Component)]
pub struct BrushFaceEntity {
    pub brush_entity: Entity,
    pub face_index: usize,
}

/// Edit mode: Object (default) or brush editing.
#[derive(Resource, Default, PartialEq, Eq, Clone, Copy, Debug, Reflect)]
pub enum EditMode {
    #[default]
    Object,
    BrushEdit(BrushEditMode),
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, Reflect)]
pub enum BrushEditMode {
    Face,
    Vertex,
    Edge,
    Clip,
}

/// Tracks selected sub-elements within brush edit mode.
#[derive(Resource, Default)]
pub struct BrushSelection {
    pub entity: Option<Entity>,
    pub faces: Vec<usize>,
    pub vertices: Vec<usize>,
    /// Selected edges as normalized (min, max) vertex index pairs.
    pub edges: Vec<(usize, usize)>,
}

/// Material palette for brush faces.
#[derive(Resource, Default)]
pub struct BrushMaterialPalette {
    pub materials: Vec<Handle<StandardMaterial>>,
}

/// Cached texture materials, keyed by asset-relative path.
#[derive(Resource, Default)]
pub struct TextureMaterialCache {
    pub entries: HashMap<String, TextureCacheEntry>,
}

pub struct TextureCacheEntry {
    pub image: Handle<Image>,
    pub material: Handle<StandardMaterial>,
}

// ---------------------------------------------------------------------------
// Undo command
// ---------------------------------------------------------------------------

pub struct SetBrush {
    pub entity: Entity,
    pub old: Brush,
    pub new: Brush,
    pub label: String,
}

impl EditorCommand for SetBrush {
    fn execute(&self, world: &mut World) {
        if let Some(mut brush) = world.get_mut::<Brush>(self.entity) {
            *brush = self.new.clone();
        }
    }

    fn undo(&self, world: &mut World) {
        if let Some(mut brush) = world.get_mut::<Brush>(self.entity) {
            *brush = self.old.clone();
        }
    }

    fn description(&self) -> &str {
        &self.label
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct BrushPlugin;

impl Plugin for BrushPlugin {
    fn build(&self, app: &mut App) {
        // Note: Brush, BrushFaceData, BrushPlane type registration is handled by JsnPlugin
        app.register_type::<EditMode>()
            .register_type::<BrushEditMode>()
            .init_resource::<EditMode>()
            .init_resource::<BrushSelection>()
            .init_resource::<BrushMaterialPalette>()
            .init_resource::<TextureMaterialCache>()
            .init_resource::<BrushDragState>()
            .init_resource::<VertexDragState>()
            .init_resource::<EdgeDragState>()
            .init_resource::<ClipState>()
            .add_systems(Startup, mesh::setup_default_materials)
            .add_systems(
                Update,
                (
                    interaction::handle_edit_mode_keys,
                    mesh::ensure_texture_materials,
                    mesh::set_texture_repeat_mode,
                    mesh::regenerate_brush_meshes,
                    interaction::brush_face_select,
                    interaction::brush_vertex_select,
                    interaction::brush_edge_select,
                    interaction::handle_face_drag,
                    interaction::handle_vertex_drag,
                    interaction::handle_edge_drag,
                    interaction::handle_brush_delete,
                    interaction::handle_clip_mode,
                    gizmo_overlay::draw_brush_edit_gizmos,
                )
                    .chain(),
            );
    }
}
