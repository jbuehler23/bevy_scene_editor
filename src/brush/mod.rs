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
// Re-exports (items used by other crate modules)
// ---------------------------------------------------------------------------

pub use self::geometry::compute_brush_geometry;
pub(crate) use self::geometry::compute_face_tangent_axes;
pub use self::csg::{brush_planes_to_world, brushes_intersect, subtract_brush, clean_degenerate_faces};
pub use self::hull::HullFace;
pub(crate) use self::hull::merge_hull_triangles;
pub(crate) use self::interaction::{
    BrushDragState, VertexDragState, VertexDragConstraint, EdgeDragState, ClipState,
};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Reflect, Default)]
pub struct BrushPlane {
    pub normal: Vec3,
    pub distance: f32,
}

#[derive(Clone, Debug, Reflect, Default)]
pub struct BrushFaceData {
    pub plane: BrushPlane,
    pub material_index: usize,
    /// Asset-relative texture path (e.g. "textures/brick.png"). Overrides material_index when set.
    pub texture_path: Option<String>,
    pub uv_offset: Vec2,
    pub uv_scale: Vec2,
    pub uv_rotation: f32,
}

/// Canonical brush data. Serialized. Geometry derived from this.
#[derive(Component, Reflect, Clone, Debug, Default)]
#[reflect(Component, Default)]
pub struct Brush {
    pub faces: Vec<BrushFaceData>,
}

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
// Brush constructors
// ---------------------------------------------------------------------------

impl Brush {
    /// Create a cuboid brush from 6 axis-aligned face planes.
    pub fn cuboid(half_x: f32, half_y: f32, half_z: f32) -> Self {
        Self {
            faces: vec![
                // +X
                BrushFaceData {
                    plane: BrushPlane {
                        normal: Vec3::X,
                        distance: half_x,
                    },
                    uv_scale: Vec2::ONE,
                    ..default()
                },
                // -X
                BrushFaceData {
                    plane: BrushPlane {
                        normal: Vec3::NEG_X,
                        distance: half_x,
                    },
                    uv_scale: Vec2::ONE,
                    ..default()
                },
                // +Y
                BrushFaceData {
                    plane: BrushPlane {
                        normal: Vec3::Y,
                        distance: half_y,
                    },
                    uv_scale: Vec2::ONE,
                    ..default()
                },
                // -Y
                BrushFaceData {
                    plane: BrushPlane {
                        normal: Vec3::NEG_Y,
                        distance: half_y,
                    },
                    uv_scale: Vec2::ONE,
                    ..default()
                },
                // +Z
                BrushFaceData {
                    plane: BrushPlane {
                        normal: Vec3::Z,
                        distance: half_z,
                    },
                    uv_scale: Vec2::ONE,
                    ..default()
                },
                // -Z
                BrushFaceData {
                    plane: BrushPlane {
                        normal: Vec3::NEG_Z,
                        distance: half_z,
                    },
                    uv_scale: Vec2::ONE,
                    ..default()
                },
            ],
        }
    }

    /// Create a sphere brush approximated as an icosahedron (20 triangular faces).
    pub fn sphere(radius: f32) -> Self {
        let phi = (1.0 + 5.0_f32.sqrt()) / 2.0;
        let raw = [
            Vec3::new(-1.0, phi, 0.0),
            Vec3::new(1.0, phi, 0.0),
            Vec3::new(-1.0, -phi, 0.0),
            Vec3::new(1.0, -phi, 0.0),
            Vec3::new(0.0, -1.0, phi),
            Vec3::new(0.0, 1.0, phi),
            Vec3::new(0.0, -1.0, -phi),
            Vec3::new(0.0, 1.0, -phi),
            Vec3::new(phi, 0.0, -1.0),
            Vec3::new(phi, 0.0, 1.0),
            Vec3::new(-phi, 0.0, -1.0),
            Vec3::new(-phi, 0.0, 1.0),
        ];
        let verts: Vec<Vec3> = raw.iter().map(|v| v.normalize() * radius).collect();

        // 20 triangular faces (standard icosahedron topology)
        let tris: [[usize; 3]; 20] = [
            [0, 11, 5],
            [0, 5, 1],
            [0, 1, 7],
            [0, 7, 10],
            [0, 10, 11],
            [1, 5, 9],
            [5, 11, 4],
            [11, 10, 2],
            [10, 7, 6],
            [7, 1, 8],
            [3, 9, 4],
            [3, 4, 2],
            [3, 2, 6],
            [3, 6, 8],
            [3, 8, 9],
            [4, 9, 5],
            [2, 4, 11],
            [6, 2, 10],
            [8, 6, 7],
            [9, 8, 1],
        ];

        let faces = tris
            .iter()
            .map(|&[a, b, c]| {
                let normal = (verts[b] - verts[a]).cross(verts[c] - verts[a]).normalize();
                let distance = normal.dot(verts[a]);
                // Ensure outward-facing
                let (normal, distance) = if distance < 0.0 {
                    (-normal, -distance)
                } else {
                    (normal, distance)
                };
                BrushFaceData {
                    plane: BrushPlane { normal, distance },
                    uv_scale: Vec2::ONE,
                    ..default()
                }
            })
            .collect();

        Self { faces }
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const EPSILON: f32 = 1e-4;

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct BrushPlugin;

impl Plugin for BrushPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Brush>()
            .register_type::<BrushFaceData>()
            .register_type::<BrushPlane>()
            .register_type::<EditMode>()
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
