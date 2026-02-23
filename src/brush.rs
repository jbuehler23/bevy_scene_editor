use std::collections::{HashMap, HashSet};

use avian3d::parry::math::Point as ParryPoint;
use avian3d::parry::transformation::convex_hull;
use bevy::{
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    input_focus::InputFocus,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    ui::UiGlobalTransform,
};

use crate::{
    commands::{CommandHistory, EditorCommand},
    gizmos::{point_to_segment_dist, window_to_viewport_cursor},
    selection::Selection,
    viewport::SceneViewport,
    EditorEntity,
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
// Geometry algorithms (pure functions)
// ---------------------------------------------------------------------------

const EPSILON: f32 = 1e-4;

/// Solve the intersection of three planes. Returns None if degenerate.
fn plane_triple_intersection(p1: &BrushPlane, p2: &BrushPlane, p3: &BrushPlane) -> Option<Vec3> {
    let n1 = p1.normal;
    let n2 = p2.normal;
    let n3 = p3.normal;

    let det = n1.dot(n2.cross(n3));
    if det.abs() < EPSILON {
        return None;
    }

    let point = (n2.cross(n3) * p1.distance + n3.cross(n1) * p2.distance + n1.cross(n2) * p3.distance) / det;
    Some(point)
}

/// Check if a point is inside (or on the boundary of) all half-planes.
fn point_inside_all_planes(point: Vec3, faces: &[BrushFaceData]) -> bool {
    for face in faces {
        if face.plane.normal.dot(point) > face.plane.distance + EPSILON {
            return false;
        }
    }
    true
}

/// Compute brush geometry from face planes.
/// Returns (unique vertices, per-face polygon vertex indices).
pub fn compute_brush_geometry(faces: &[BrushFaceData]) -> (Vec<Vec3>, Vec<Vec<usize>>) {
    let n = faces.len();
    let mut vertices: Vec<Vec3> = Vec::new();

    // Find all valid intersection points from triples of planes
    for i in 0..n {
        for j in (i + 1)..n {
            for k in (j + 1)..n {
                if let Some(point) = plane_triple_intersection(
                    &faces[i].plane,
                    &faces[j].plane,
                    &faces[k].plane,
                ) {
                    // Keep only if inside all planes
                    if point_inside_all_planes(point, faces) {
                        // Deduplicate
                        let already = vertices.iter().any(|v| (*v - point).length() < EPSILON);
                        if !already {
                            vertices.push(point);
                        }
                    }
                }
            }
        }
    }

    // For each face, collect vertices that lie on that face and sort by winding
    let mut face_polygons = Vec::with_capacity(n);
    for face in faces {
        let mut face_verts: Vec<usize> = Vec::new();
        for (vi, v) in vertices.iter().enumerate() {
            let d = face.plane.normal.dot(*v) - face.plane.distance;
            if d.abs() < EPSILON {
                face_verts.push(vi);
            }
        }

        // Sort by winding order around face normal
        if face_verts.len() >= 3 {
            sort_face_vertices_by_winding(&vertices, &mut face_verts, face.plane.normal);
        }

        face_polygons.push(face_verts);
    }

    (vertices, face_polygons)
}

/// Sort face vertex indices by winding order around the face normal.
fn sort_face_vertices_by_winding(vertices: &[Vec3], indices: &mut [usize], normal: Vec3) {
    if indices.len() < 3 {
        return;
    }

    // Compute centroid of face vertices
    let centroid: Vec3 = indices.iter().map(|&i| vertices[i]).sum::<Vec3>() / indices.len() as f32;

    // Build a local 2D coordinate system on the face plane
    let (u_axis, v_axis) = compute_face_tangent_axes(normal);

    indices.sort_by(|&a, &b| {
        let da = vertices[a] - centroid;
        let db = vertices[b] - centroid;
        let angle_a = da.dot(v_axis).atan2(da.dot(u_axis));
        let angle_b = db.dot(v_axis).atan2(db.dot(u_axis));
        angle_a.partial_cmp(&angle_b).unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Fan triangulation from vertex 0. Valid for convex polygons.
fn triangulate_face(indices: &[usize]) -> Vec<[u32; 3]> {
    let mut triangles = Vec::new();
    if indices.len() < 3 {
        return triangles;
    }
    for i in 1..(indices.len() - 1) {
        triangles.push([
            indices[0] as u32,
            indices[i] as u32,
            indices[i + 1] as u32,
        ]);
    }
    triangles
}

/// Compute tangent axes for a face from its normal (paraxial projection).
pub(crate) fn compute_face_tangent_axes(normal: Vec3) -> (Vec3, Vec3) {
    let abs_n = normal.abs();
    let up = if abs_n.y >= abs_n.x && abs_n.y >= abs_n.z {
        // Normal is mostly Y — use Z as reference
        Vec3::Z
    } else {
        Vec3::Y
    };
    let u = normal.cross(up).normalize_or_zero();
    let v = normal.cross(u).normalize_or_zero();
    (u, v)
}

// ---------------------------------------------------------------------------
// CSG subtraction
// ---------------------------------------------------------------------------

/// Transform brush face planes from local space to world space.
pub fn brush_planes_to_world(brush: &Brush, transform: &GlobalTransform) -> Vec<BrushFaceData> {
    let (_, rotation, translation) = transform.to_scale_rotation_translation();
    brush
        .faces
        .iter()
        .map(|face| {
            let world_normal = (rotation * face.plane.normal).normalize();
            let world_distance = face.plane.distance + world_normal.dot(translation);
            BrushFaceData {
                plane: BrushPlane {
                    normal: world_normal,
                    distance: world_distance,
                },
                material_index: face.material_index,
                texture_path: face.texture_path.clone(),
                uv_offset: face.uv_offset,
                uv_scale: face.uv_scale,
                uv_rotation: face.uv_rotation,
            }
        })
        .collect()
}

/// Check whether two convex volumes (defined by face planes) overlap.
pub fn brushes_intersect(a_faces: &[BrushFaceData], b_faces: &[BrushFaceData]) -> bool {
    let mut combined: Vec<BrushFaceData> = a_faces.to_vec();
    combined.extend_from_slice(b_faces);
    let (verts, _) = compute_brush_geometry(&combined);
    verts.len() >= 4
}

/// Subtract a cutter volume from a target brush. Both face sets must be in the same
/// coordinate space (typically world space). Returns the fragment face sets representing
/// the target minus the cutter.
pub fn subtract_brush(
    target_faces: &[BrushFaceData],
    cutter_faces: &[BrushFaceData],
) -> Vec<Vec<BrushFaceData>> {
    let mut result_fragments: Vec<Vec<BrushFaceData>> = Vec::new();
    let mut remaining: Vec<Vec<BrushFaceData>> = vec![target_faces.to_vec()];

    for cutter_face in cutter_faces {
        let n = cutter_face.plane.normal;
        let d = cutter_face.plane.distance;

        let mut next_remaining = Vec::new();

        for fragment in &remaining {
            // Outside half: keeps the part outside the cutter through this face
            let mut outside_faces = fragment.clone();
            outside_faces.push(BrushFaceData {
                plane: BrushPlane {
                    normal: -n,
                    distance: -d,
                },
                uv_scale: Vec2::ONE,
                ..default()
            });
            let (outside_verts, _) = compute_brush_geometry(&outside_faces);
            if outside_verts.len() >= 4 {
                result_fragments.push(outside_faces);
            }

            // Inside half: keeps the part inside the cutter through this face
            let mut inside_faces = fragment.clone();
            inside_faces.push(BrushFaceData {
                plane: BrushPlane {
                    normal: n,
                    distance: d,
                },
                uv_scale: Vec2::ONE,
                ..default()
            });
            let (inside_verts, _) = compute_brush_geometry(&inside_faces);
            if inside_verts.len() >= 4 {
                next_remaining.push(inside_faces);
            }
        }

        remaining = next_remaining;
    }

    // remaining = pieces fully inside the cutter → discard
    result_fragments
}

/// Remove faces that produce no vertices (degenerate) from a face set.
pub fn clean_degenerate_faces(faces: &[BrushFaceData]) -> Vec<BrushFaceData> {
    let (_, polys) = compute_brush_geometry(faces);
    faces
        .iter()
        .enumerate()
        .filter(|(i, _)| polys.get(*i).is_some_and(|p| p.len() >= 3))
        .map(|(_, f)| f.clone())
        .collect()
}

/// Compute UVs for vertices on a face using paraxial projection.
fn compute_face_uvs(
    vertices: &[Vec3],
    indices: &[usize],
    normal: Vec3,
    uv_offset: Vec2,
    uv_scale: Vec2,
    uv_rotation: f32,
) -> Vec<[f32; 2]> {
    let (u_axis, v_axis) = compute_face_tangent_axes(normal);
    let cos_r = uv_rotation.cos();
    let sin_r = uv_rotation.sin();

    indices
        .iter()
        .map(|&vi| {
            let pos = vertices[vi];
            let u = pos.dot(u_axis);
            let v = pos.dot(v_axis);
            // Apply rotation
            let ru = u * cos_r - v * sin_r;
            let rv = u * sin_r + v * cos_r;
            // Apply scale and offset
            let su = ru / uv_scale.x.max(0.001) + uv_offset.x;
            let sv = rv / uv_scale.y.max(0.001) + uv_offset.y;
            [su, sv]
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Convex hull infrastructure
// ---------------------------------------------------------------------------

fn vec3_to_point(v: Vec3) -> ParryPoint<f32> {
    ParryPoint::new(v.x, v.y, v.z)
}

fn point_to_vec3(p: &ParryPoint<f32>) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

pub struct HullFace {
    pub normal: Vec3,
    pub distance: f32,
    pub vertex_indices: Vec<usize>,
}

/// Merge the triangles from a convex hull into coplanar polygon faces.
pub fn merge_hull_triangles(vertices: &[Vec3], triangles: &[[u32; 3]]) -> Vec<HullFace> {
    // Compute normal + distance for each triangle, group coplanar ones.
    let mut face_groups: Vec<(Vec3, f32, HashSet<usize>)> = Vec::new();

    for tri in triangles {
        let a = vertices[tri[0] as usize];
        let b = vertices[tri[1] as usize];
        let c = vertices[tri[2] as usize];
        let normal = (b - a).cross(c - a).normalize_or_zero();
        if normal.length_squared() < 0.5 {
            continue; // degenerate triangle
        }
        let distance = normal.dot(a);

        // Find existing group with matching plane
        let mut found = false;
        for (gn, gd, gverts) in &mut face_groups {
            if gn.dot(normal) > 1.0 - EPSILON && (distance - *gd).abs() < EPSILON {
                gverts.insert(tri[0] as usize);
                gverts.insert(tri[1] as usize);
                gverts.insert(tri[2] as usize);
                found = true;
                break;
            }
        }
        if !found {
            let mut verts = HashSet::new();
            verts.insert(tri[0] as usize);
            verts.insert(tri[1] as usize);
            verts.insert(tri[2] as usize);
            face_groups.push((normal, distance, verts));
        }
    }

    face_groups
        .into_iter()
        .map(|(normal, distance, vert_set)| {
            let mut vertex_indices: Vec<usize> = vert_set.into_iter().collect();
            sort_face_vertices_by_winding(vertices, &mut vertex_indices, normal);
            HullFace {
                normal,
                distance,
                vertex_indices,
            }
        })
        .collect()
}

/// Rebuild a `Brush` from a new set of vertices using convex hull.
/// Attempts to match new faces to old faces for material/UV preservation.
fn rebuild_brush_from_vertices(
    old_brush: &Brush,
    _old_vertices: &[Vec3],
    old_face_polygons: &[Vec<usize>],
    new_vertices: &[Vec3],
) -> Option<Brush> {
    if new_vertices.len() < 4 {
        return None;
    }

    let points: Vec<ParryPoint<f32>> = new_vertices.iter().map(|v| vec3_to_point(*v)).collect();
    let (hull_verts, hull_tris) = convex_hull(&points);

    if hull_verts.len() < 4 || hull_tris.is_empty() {
        return None;
    }

    let hull_positions: Vec<Vec3> = hull_verts.iter().map(point_to_vec3).collect();
    let hull_faces = merge_hull_triangles(&hull_positions, &hull_tris);

    if hull_faces.len() < 4 {
        return None;
    }

    // Map hull vertex indices → input vertex indices (closest position match)
    let hull_to_input: Vec<usize> = hull_positions
        .iter()
        .map(|hp| {
            new_vertices
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    (**a - *hp)
                        .length_squared()
                        .partial_cmp(&(**b - *hp).length_squared())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0)
        })
        .collect();

    let mut faces = Vec::with_capacity(hull_faces.len());
    for hull_face in &hull_faces {
        // Remap vertex indices from hull-local to input-local
        let input_verts: HashSet<usize> = hull_face
            .vertex_indices
            .iter()
            .map(|&hi| hull_to_input[hi])
            .collect();

        // Match to best old face by vertex overlap + normal similarity
        let mut best_old = 0usize;
        let mut best_score = -1.0_f32;
        for (old_idx, old_polygon) in old_face_polygons.iter().enumerate() {
            let old_set: HashSet<usize> = old_polygon.iter().copied().collect();
            let overlap = input_verts.intersection(&old_set).count() as f32;
            let normal_sim = hull_face.normal.dot(old_brush.faces[old_idx].plane.normal);
            let score = overlap + normal_sim * 0.1;
            if score > best_score {
                best_score = score;
                best_old = old_idx;
            }
        }

        let old_face = &old_brush.faces[best_old];
        faces.push(BrushFaceData {
            plane: BrushPlane {
                normal: hull_face.normal,
                distance: hull_face.distance,
            },
            material_index: old_face.material_index,
            texture_path: old_face.texture_path.clone(),
            uv_offset: old_face.uv_offset,
            uv_scale: old_face.uv_scale,
            uv_rotation: old_face.uv_rotation,
        });
    }

    Some(Brush { faces })
}

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
            .add_systems(Startup, setup_default_materials)
            .add_systems(
                Update,
                (
                    handle_edit_mode_keys,
                    ensure_texture_materials,
                    set_texture_repeat_mode,
                    regenerate_brush_meshes,
                    brush_face_select,
                    brush_vertex_select,
                    brush_edge_select,
                    handle_face_drag,
                    handle_vertex_drag,
                    handle_edge_drag,
                    handle_brush_delete,
                    handle_clip_mode,
                    draw_brush_edit_gizmos,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

fn setup_default_materials(
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut palette: ResMut<BrushMaterialPalette>,
) {
    let defaults = [
        Color::srgb(0.7, 0.7, 0.7),  // default grey (matches Cube mesh)
        Color::srgb(0.5, 0.5, 0.5),  // gray
        Color::srgb(0.3, 0.3, 0.3),  // dark gray
        Color::srgb(0.7, 0.3, 0.2),  // brick red
        Color::srgb(0.3, 0.5, 0.7),  // steel blue
        Color::srgb(0.4, 0.6, 0.3),  // mossy green
        Color::srgb(0.6, 0.5, 0.3),  // sandy tan
        Color::srgb(0.5, 0.3, 0.5),  // purple
    ];
    for color in defaults {
        palette.materials.push(materials.add(StandardMaterial {
            base_color: color,
            ..default()
        }));
    }
}

// ---------------------------------------------------------------------------
// Texture material loading
// ---------------------------------------------------------------------------

fn ensure_texture_materials(
    brushes: Query<&Brush>,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<TextureMaterialCache>,
) {
    // Collect paths that need loading first to avoid borrow conflicts
    let mut paths_to_load: Vec<String> = Vec::new();
    for brush in &brushes {
        for face in &brush.faces {
            if let Some(ref path) = face.texture_path {
                if !cache.entries.contains_key(path) && !paths_to_load.contains(path) {
                    paths_to_load.push(path.clone());
                }
            }
        }
    }

    for path in paths_to_load {
        let image: Handle<Image> = asset_server.load(path.clone());
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(image.clone()),
            ..default()
        });
        cache.entries.insert(path, TextureCacheEntry { image, material });
    }
}

/// Set repeat wrapping mode on brush texture images once they finish loading.
fn set_texture_repeat_mode(
    cache: Res<TextureMaterialCache>,
    mut images: ResMut<Assets<Image>>,
    mut done: Local<HashSet<String>>,
) {
    for (path, entry) in &cache.entries {
        if done.contains(path) {
            continue;
        }
        if let Some(image) = images.get_mut(&entry.image) {
            image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: ImageAddressMode::Repeat,
                address_mode_v: ImageAddressMode::Repeat,
                ..ImageSamplerDescriptor::linear()
            });
            done.insert(path.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Mesh regeneration
// ---------------------------------------------------------------------------

fn regenerate_brush_meshes(
    mut commands: Commands,
    changed_brushes: Query<(Entity, &Brush), Changed<Brush>>,
    existing_caches: Query<&BrushMeshCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    palette: Res<BrushMaterialPalette>,
    texture_cache: Res<TextureMaterialCache>,
) {
    for (entity, brush) in &changed_brushes {
        // Despawn old face entities
        if let Ok(old_cache) = existing_caches.get(entity) {
            for &face_entity in &old_cache.face_entities {
                if face_entity == Entity::PLACEHOLDER {
                    continue;
                }
                if let Ok(mut ec) = commands.get_entity(face_entity) {
                    ec.despawn();
                }
            }
        }

        let (vertices, face_polygons) = compute_brush_geometry(&brush.faces);

        let mut face_entities = Vec::with_capacity(brush.faces.len());

        for (face_idx, face_data) in brush.faces.iter().enumerate() {
            let indices = &face_polygons[face_idx];
            if indices.len() < 3 {
                // Degenerate face, spawn nothing but track the slot
                face_entities.push(Entity::PLACEHOLDER);
                continue;
            }

            // Build per-face mesh with local vertex positions
            let positions: Vec<[f32; 3]> = indices.iter().map(|&vi| vertices[vi].to_array()).collect();
            let normals: Vec<[f32; 3]> = vec![face_data.plane.normal.to_array(); indices.len()];
            let uvs = compute_face_uvs(
                &vertices,
                indices,
                face_data.plane.normal,
                face_data.uv_offset,
                face_data.uv_scale,
                face_data.uv_rotation,
            );

            // Fan triangulate — local indices (0..positions.len())
            let local_tris = triangulate_face(&(0..indices.len()).collect::<Vec<_>>());
            let flat_indices: Vec<u32> = local_tris.iter().flat_map(|t| t.iter().copied()).collect();

            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
            mesh.insert_indices(Indices::U32(flat_indices));

            let mesh_handle = meshes.add(mesh);

            let material = match &face_data.texture_path {
                Some(path) => texture_cache
                    .entries
                    .get(path)
                    .map(|e| e.material.clone())
                    .unwrap_or_else(|| palette.materials[0].clone()),
                None => palette
                    .materials
                    .get(face_data.material_index)
                    .cloned()
                    .unwrap_or_else(|| palette.materials[0].clone()),
            };

            let face_entity = commands
                .spawn((
                    BrushFaceEntity {
                        brush_entity: entity,
                        face_index: face_idx,
                    },
                    Mesh3d(mesh_handle),
                    MeshMaterial3d(material),
                    Transform::default(),
                    ChildOf(entity),
                ))
                .id();

            face_entities.push(face_entity);
        }

        commands.entity(entity).insert(BrushMeshCache {
            vertices,
            face_polygons,
            face_entities,
        });
    }
}

// ---------------------------------------------------------------------------
// Edit mode toggle
// ---------------------------------------------------------------------------

fn handle_edit_mode_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    input_focus: Res<InputFocus>,
    selection: Res<Selection>,
    brushes: Query<(), With<Brush>>,
    mut edit_mode: ResMut<EditMode>,
    mut brush_selection: ResMut<BrushSelection>,
    modal: Res<crate::modal_transform::ModalTransformState>,
) {
    if input_focus.0.is_some() || modal.active.is_some() {
        return;
    }

    // Backtick (`) toggles in/out of brush edit mode
    if keyboard.just_pressed(KeyCode::Backquote) {
        match *edit_mode {
            EditMode::Object => {
                // Enter brush edit mode if a brush is selected
                if let Some(primary) = selection.primary() {
                    if brushes.contains(primary) {
                        *edit_mode = EditMode::BrushEdit(BrushEditMode::Face);
                        brush_selection.entity = Some(primary);
                        brush_selection.faces.clear();
                        brush_selection.vertices.clear();
                        brush_selection.edges.clear();
                    }
                }
            }
            EditMode::BrushEdit(_) => {
                *edit_mode = EditMode::Object;
                brush_selection.entity = None;
                brush_selection.faces.clear();
                brush_selection.vertices.clear();
                brush_selection.edges.clear();
            }
        }
        return;
    }

    // 1/2/3 switch sub-modes within brush edit mode
    if let EditMode::BrushEdit(_) = *edit_mode {
        if keyboard.just_pressed(KeyCode::Digit1) {
            *edit_mode = EditMode::BrushEdit(BrushEditMode::Vertex);
            brush_selection.faces.clear();
            brush_selection.vertices.clear();
            brush_selection.edges.clear();
        } else if keyboard.just_pressed(KeyCode::Digit2) {
            *edit_mode = EditMode::BrushEdit(BrushEditMode::Edge);
            brush_selection.faces.clear();
            brush_selection.vertices.clear();
            brush_selection.edges.clear();
        } else if keyboard.just_pressed(KeyCode::Digit3) {
            *edit_mode = EditMode::BrushEdit(BrushEditMode::Face);
            brush_selection.faces.clear();
            brush_selection.vertices.clear();
            brush_selection.edges.clear();
        } else if keyboard.just_pressed(KeyCode::Digit4) {
            *edit_mode = EditMode::BrushEdit(BrushEditMode::Clip);
            brush_selection.faces.clear();
            brush_selection.vertices.clear();
            brush_selection.edges.clear();
        }
    }

    // Exit brush edit mode if the brush entity gets deselected
    if let EditMode::BrushEdit(_) = *edit_mode {
        if let Some(brush_entity) = brush_selection.entity {
            if selection.primary() != Some(brush_entity) {
                *edit_mode = EditMode::Object;
                brush_selection.entity = None;
                brush_selection.faces.clear();
                brush_selection.vertices.clear();
                brush_selection.edges.clear();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Face selection (in face edit mode, click on face child entities)
// ---------------------------------------------------------------------------

fn brush_face_select(
    edit_mode: Res<EditMode>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    face_entities: Query<(Entity, &BrushFaceEntity, &GlobalTransform)>,
    mut brush_selection: ResMut<BrushSelection>,
    brush_caches: Query<&BrushMeshCache>,
) {
    let EditMode::BrushEdit(BrushEditMode::Face) = *edit_mode else {
        return;
    };
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };
    let Some(viewport_cursor) = window_to_viewport_cursor(cursor_pos, camera, &viewport_query) else {
        return;
    };

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };

    // Raycast: find face entity closest to click in screen space
    // We check centroids of face polygons projected to screen
    let Ok(cache) = brush_caches.get(brush_entity) else {
        return;
    };

    let mut best_face = None;
    let mut best_dist = 30.0_f32;

    for (_, face_ent, face_global) in &face_entities {
        if face_ent.brush_entity != brush_entity {
            continue;
        }
        let face_idx = face_ent.face_index;
        let polygon = &cache.face_polygons[face_idx];
        if polygon.is_empty() {
            continue;
        }

        // Compute face centroid in world space
        let brush_tf = face_global; // face children have identity local transform
        let centroid: Vec3 = polygon.iter().map(|&vi| cache.vertices[vi]).sum::<Vec3>()
            / polygon.len() as f32;
        let world_centroid = brush_tf.transform_point(centroid);

        if let Ok(screen_pos) = camera.world_to_viewport(cam_tf, world_centroid) {
            let dist = (screen_pos - viewport_cursor).length();
            if dist < best_dist {
                best_dist = dist;
                best_face = Some(face_idx);
            }
        }
    }

    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if let Some(face_idx) = best_face {
        if ctrl {
            if let Some(pos) = brush_selection.faces.iter().position(|&f| f == face_idx) {
                brush_selection.faces.remove(pos);
            } else {
                brush_selection.faces.push(face_idx);
            }
        } else {
            brush_selection.faces = vec![face_idx];
        }
    } else if !ctrl {
        brush_selection.faces.clear();
    }
}

// ---------------------------------------------------------------------------
// Vertex selection
// ---------------------------------------------------------------------------

fn brush_vertex_select(
    edit_mode: Res<EditMode>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    brush_transforms: Query<&GlobalTransform>,
    mut brush_selection: ResMut<BrushSelection>,
    brush_caches: Query<&BrushMeshCache>,
) {
    let EditMode::BrushEdit(BrushEditMode::Vertex) = *edit_mode else {
        return;
    };
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };
    let Some(viewport_cursor) = window_to_viewport_cursor(cursor_pos, camera, &viewport_query) else {
        return;
    };

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(cache) = brush_caches.get(brush_entity) else {
        return;
    };
    let Ok(brush_global) = brush_transforms.get(brush_entity) else {
        return;
    };

    let mut best_vert = None;
    let mut best_dist = 20.0_f32;

    for (vi, v) in cache.vertices.iter().enumerate() {
        let world_pos = brush_global.transform_point(*v);
        if let Ok(screen_pos) = camera.world_to_viewport(cam_tf, world_pos) {
            let dist = (screen_pos - viewport_cursor).length();
            if dist < best_dist {
                best_dist = dist;
                best_vert = Some(vi);
            }
        }
    }

    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if let Some(vi) = best_vert {
        if ctrl {
            if let Some(pos) = brush_selection.vertices.iter().position(|&v| v == vi) {
                brush_selection.vertices.remove(pos);
            } else {
                brush_selection.vertices.push(vi);
            }
        } else {
            brush_selection.vertices = vec![vi];
        }
    } else if !ctrl {
        brush_selection.vertices.clear();
    }
}

// ---------------------------------------------------------------------------
// Edge selection
// ---------------------------------------------------------------------------

fn brush_edge_select(
    edit_mode: Res<EditMode>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    brush_transforms: Query<&GlobalTransform>,
    mut brush_selection: ResMut<BrushSelection>,
    brush_caches: Query<&BrushMeshCache>,
) {
    let EditMode::BrushEdit(BrushEditMode::Edge) = *edit_mode else {
        return;
    };
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };
    let Some(viewport_cursor) = window_to_viewport_cursor(cursor_pos, camera, &viewport_query) else {
        return;
    };

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(cache) = brush_caches.get(brush_entity) else {
        return;
    };
    let Ok(brush_global) = brush_transforms.get(brush_entity) else {
        return;
    };

    // Collect unique edges from face polygons
    let mut unique_edges: Vec<(usize, usize)> = Vec::new();
    for polygon in &cache.face_polygons {
        if polygon.len() < 2 {
            continue;
        }
        for i in 0..polygon.len() {
            let a = polygon[i];
            let b = polygon[(i + 1) % polygon.len()];
            let edge = (a.min(b), a.max(b));
            if !unique_edges.contains(&edge) {
                unique_edges.push(edge);
            }
        }
    }

    // Find nearest edge in screen space
    let mut best_edge = None;
    let mut best_dist = 20.0_f32;

    for &(a, b) in &unique_edges {
        let wa = brush_global.transform_point(cache.vertices[a]);
        let wb = brush_global.transform_point(cache.vertices[b]);
        let Ok(sa) = camera.world_to_viewport(cam_tf, wa) else {
            continue;
        };
        let Ok(sb) = camera.world_to_viewport(cam_tf, wb) else {
            continue;
        };
        let dist = point_to_segment_dist(viewport_cursor, sa, sb);
        if dist < best_dist {
            best_dist = dist;
            best_edge = Some((a, b));
        }
    }

    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if let Some(edge) = best_edge {
        if ctrl {
            if let Some(pos) = brush_selection.edges.iter().position(|e| *e == edge) {
                brush_selection.edges.remove(pos);
            } else {
                brush_selection.edges.push(edge);
            }
        } else {
            brush_selection.edges = vec![edge];
        }
    } else if !ctrl {
        brush_selection.edges.clear();
    }
}

// ---------------------------------------------------------------------------
// Face drag (G key moves selected face along its normal)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct BrushDragState {
    pub active: bool,
    start_brush: Option<Brush>,
    start_cursor: Vec2,
    drag_face_normal: Vec3,
}

fn handle_face_drag(
    edit_mode: Res<EditMode>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    brush_selection: Res<BrushSelection>,
    mut brushes: Query<(&mut Brush, &GlobalTransform)>,
    mut drag_state: ResMut<BrushDragState>,
    input_focus: Res<InputFocus>,
    mut history: ResMut<CommandHistory>,
) {
    let EditMode::BrushEdit(BrushEditMode::Face) = *edit_mode else {
        drag_state.active = false;
        return;
    };

    if input_focus.0.is_some() {
        return;
    }

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok((mut brush, brush_global)) = brushes.get_mut(brush_entity) else {
        return;
    };

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };
    let Some(viewport_cursor) = window_to_viewport_cursor(cursor_pos, camera, &viewport_query) else {
        return;
    };

    // Start drag on G or E (extrude face = same as face move, plane system extends)
    if (keyboard.just_pressed(KeyCode::KeyG) || keyboard.just_pressed(KeyCode::KeyE))
        && !drag_state.active
        && !brush_selection.faces.is_empty()
    {
        drag_state.active = true;
        drag_state.start_brush = Some(brush.clone());
        drag_state.start_cursor = viewport_cursor;
        // Use the first selected face's normal
        let face_idx = brush_selection.faces[0];
        drag_state.drag_face_normal = brush.faces[face_idx].plane.normal;
        return;
    }

    if !drag_state.active {
        return;
    }

    // Cancel on Escape or right-click
    if keyboard.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right) {
        if let Some(ref start) = drag_state.start_brush {
            *brush = start.clone();
        }
        drag_state.active = false;
        return;
    }

    // Confirm on left-click or Enter
    if mouse.just_pressed(MouseButton::Left) || keyboard.just_pressed(KeyCode::Enter) {
        if let Some(ref start) = drag_state.start_brush {
            let cmd = SetBrush {
                entity: brush_entity,
                old: start.clone(),
                new: brush.clone(),
                label: "Move brush face".to_string(),
            };
            history.undo_stack.push(Box::new(cmd));
            history.redo_stack.clear();
        }
        drag_state.active = false;
        return;
    }

    // Continue drag — adjust face plane distance based on mouse movement
    let Some(ref start) = drag_state.start_brush else {
        drag_state.active = false;
        return;
    };

    // Project the face normal to screen space to determine drag direction
    let brush_pos = brush_global.translation();
    let Ok(origin_screen) = camera.world_to_viewport(cam_tf, brush_pos) else {
        return;
    };
    let Ok(normal_screen) = camera.world_to_viewport(cam_tf, brush_pos + drag_state.drag_face_normal) else {
        return;
    };
    let screen_dir = (normal_screen - origin_screen).normalize_or_zero();
    let mouse_delta = viewport_cursor - drag_state.start_cursor;
    let projected = mouse_delta.dot(screen_dir);

    // Scale by camera distance
    let cam_dist = (cam_tf.translation() - brush_pos).length();
    let drag_amount = projected * cam_dist * 0.003;

    // Apply to all selected faces
    for &face_idx in &brush_selection.faces {
        if face_idx < start.faces.len() && face_idx < brush.faces.len() {
            brush.faces[face_idx].plane.distance = start.faces[face_idx].plane.distance + drag_amount;
        }
    }
}

// ---------------------------------------------------------------------------
// Vertex drag (G key moves selected vertices)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VertexDragConstraint {
    #[default]
    Free,
    AxisX,
    AxisY,
    AxisZ,
}

#[derive(Resource, Default)]
pub struct VertexDragState {
    pub active: bool,
    pub constraint: VertexDragConstraint,
    start_brush: Option<Brush>,
    start_cursor: Vec2,
    start_vertex_positions: Vec<Vec3>,
    /// Full vertex list at drag start (for hull rebuild).
    start_all_vertices: Vec<Vec3>,
    /// Per-face polygon indices at drag start (for hull rebuild).
    start_face_polygons: Vec<Vec<usize>>,
}

fn handle_vertex_drag(
    edit_mode: Res<EditMode>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    brush_selection: Res<BrushSelection>,
    mut brushes: Query<&mut Brush>,
    brush_transforms: Query<&GlobalTransform>,
    brush_caches: Query<&BrushMeshCache>,
    mut drag_state: ResMut<VertexDragState>,
    input_focus: Res<InputFocus>,
    mut history: ResMut<CommandHistory>,
) {
    let EditMode::BrushEdit(BrushEditMode::Vertex) = *edit_mode else {
        drag_state.active = false;
        return;
    };

    if input_focus.0.is_some() {
        return;
    }

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(mut brush) = brushes.get_mut(brush_entity) else {
        return;
    };

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };
    let Some(viewport_cursor) = window_to_viewport_cursor(cursor_pos, camera, &viewport_query) else {
        return;
    };

    // Start drag on G or E (extrude = same as move, hull creates transition faces)
    if (keyboard.just_pressed(KeyCode::KeyG) || keyboard.just_pressed(KeyCode::KeyE))
        && !drag_state.active
        && !brush_selection.vertices.is_empty()
    {
        if let Ok(cache) = brush_caches.get(brush_entity) {
            drag_state.active = true;
            drag_state.constraint = VertexDragConstraint::Free;
            drag_state.start_brush = Some(brush.clone());
            drag_state.start_cursor = viewport_cursor;
            drag_state.start_vertex_positions = brush_selection
                .vertices
                .iter()
                .map(|&vi| cache.vertices.get(vi).copied().unwrap_or(Vec3::ZERO))
                .collect();
            drag_state.start_all_vertices = cache.vertices.clone();
            drag_state.start_face_polygons = cache.face_polygons.clone();
        }
        return;
    }

    if !drag_state.active {
        return;
    }

    // Axis constraint toggle: X/Y/Z keys (same key again = back to Free)
    if keyboard.just_pressed(KeyCode::KeyX) {
        drag_state.constraint = if drag_state.constraint == VertexDragConstraint::AxisX {
            VertexDragConstraint::Free
        } else {
            VertexDragConstraint::AxisX
        };
    } else if keyboard.just_pressed(KeyCode::KeyY) {
        drag_state.constraint = if drag_state.constraint == VertexDragConstraint::AxisY {
            VertexDragConstraint::Free
        } else {
            VertexDragConstraint::AxisY
        };
    } else if keyboard.just_pressed(KeyCode::KeyZ) {
        drag_state.constraint = if drag_state.constraint == VertexDragConstraint::AxisZ {
            VertexDragConstraint::Free
        } else {
            VertexDragConstraint::AxisZ
        };
    }

    // Cancel on Escape or right-click
    if keyboard.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right) {
        if let Some(ref start) = drag_state.start_brush {
            *brush = start.clone();
        }
        drag_state.active = false;
        drag_state.constraint = VertexDragConstraint::Free;
        return;
    }

    // Confirm on left-click or Enter
    if mouse.just_pressed(MouseButton::Left) || keyboard.just_pressed(KeyCode::Enter) {
        if let Some(ref start) = drag_state.start_brush {
            let cmd = SetBrush {
                entity: brush_entity,
                old: start.clone(),
                new: brush.clone(),
                label: "Move brush vertex".to_string(),
            };
            history.undo_stack.push(Box::new(cmd));
            history.redo_stack.clear();
        }
        drag_state.active = false;
        drag_state.constraint = VertexDragConstraint::Free;
        return;
    }

    // Continue drag — compute world-space offset from camera-relative mouse movement
    let Some(ref start) = drag_state.start_brush else {
        drag_state.active = false;
        return;
    };
    let Ok(brush_global) = brush_transforms.get(brush_entity) else {
        return;
    };

    let mouse_delta = viewport_cursor - drag_state.start_cursor;
    let Some(local_offset) = compute_brush_drag_offset(
        drag_state.constraint,
        mouse_delta,
        cam_tf,
        camera,
        brush_global,
    ) else {
        return;
    };

    // Rebuild from moved vertices using convex hull
    let mut new_verts = drag_state.start_all_vertices.clone();
    for (sel_idx, &vert_idx) in brush_selection.vertices.iter().enumerate() {
        if sel_idx < drag_state.start_vertex_positions.len() && vert_idx < new_verts.len() {
            new_verts[vert_idx] = drag_state.start_vertex_positions[sel_idx] + local_offset;
        }
    }

    if let Some(new_brush) = rebuild_brush_from_vertices(
        start,
        &drag_state.start_all_vertices,
        &drag_state.start_face_polygons,
        &new_verts,
    ) {
        *brush = new_brush;
    }
}

// ---------------------------------------------------------------------------
// Shared drag offset computation
// ---------------------------------------------------------------------------

/// Compute a local-space offset for brush vertex/edge drag based on mouse movement.
fn compute_brush_drag_offset(
    constraint: VertexDragConstraint,
    mouse_delta: Vec2,
    cam_tf: &GlobalTransform,
    camera: &Camera,
    brush_global: &GlobalTransform,
) -> Option<Vec3> {
    let brush_pos = brush_global.translation();
    let cam_dist = (cam_tf.translation() - brush_pos).length();
    let scale = cam_dist * 0.003;

    let offset = match constraint {
        VertexDragConstraint::Free => {
            let cam_right = cam_tf.right().as_vec3();
            let cam_up = cam_tf.up().as_vec3();
            let world_offset =
                cam_right * mouse_delta.x * scale + cam_up * (-mouse_delta.y) * scale;
            let (_, brush_rot, _) = brush_global.to_scale_rotation_translation();
            brush_rot.inverse() * world_offset
        }
        constraint => {
            let axis_dir = match constraint {
                VertexDragConstraint::AxisX => Vec3::X,
                VertexDragConstraint::AxisY => Vec3::Y,
                VertexDragConstraint::AxisZ => Vec3::Z,
                VertexDragConstraint::Free => unreachable!(),
            };
            let origin_screen = camera.world_to_viewport(cam_tf, brush_pos).ok()?;
            let (_, brush_rot, _) = brush_global.to_scale_rotation_translation();
            let world_axis = brush_rot * axis_dir;
            let axis_screen = camera
                .world_to_viewport(cam_tf, brush_pos + world_axis)
                .ok()?;
            let screen_axis = (axis_screen - origin_screen).normalize_or_zero();
            let projected = mouse_delta.dot(screen_axis);
            axis_dir * projected * scale
        }
    };
    Some(offset)
}

// ---------------------------------------------------------------------------
// Edge drag (G key moves selected edges)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct EdgeDragState {
    pub active: bool,
    pub constraint: VertexDragConstraint,
    start_brush: Option<Brush>,
    start_cursor: Vec2,
    /// Start positions for each selected edge's two endpoints (vertex indices + positions).
    start_edge_vertices: Vec<(usize, Vec3)>,
    /// Full vertex list at drag start (for hull rebuild).
    start_all_vertices: Vec<Vec3>,
    /// Per-face polygon indices at drag start (for hull rebuild).
    start_face_polygons: Vec<Vec<usize>>,
}

fn handle_edge_drag(
    edit_mode: Res<EditMode>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    brush_selection: Res<BrushSelection>,
    mut brushes: Query<&mut Brush>,
    brush_transforms: Query<&GlobalTransform>,
    brush_caches: Query<&BrushMeshCache>,
    mut drag_state: ResMut<EdgeDragState>,
    input_focus: Res<InputFocus>,
    mut history: ResMut<CommandHistory>,
) {
    let EditMode::BrushEdit(BrushEditMode::Edge) = *edit_mode else {
        drag_state.active = false;
        return;
    };

    if input_focus.0.is_some() {
        return;
    }

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(mut brush) = brushes.get_mut(brush_entity) else {
        return;
    };

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };
    let Some(viewport_cursor) = window_to_viewport_cursor(cursor_pos, camera, &viewport_query) else {
        return;
    };

    // Start drag on G or E (extrude = same as move, hull creates transition faces)
    if (keyboard.just_pressed(KeyCode::KeyG) || keyboard.just_pressed(KeyCode::KeyE))
        && !drag_state.active
        && !brush_selection.edges.is_empty()
    {
        if let Ok(cache) = brush_caches.get(brush_entity) {
            drag_state.active = true;
            drag_state.constraint = VertexDragConstraint::Free;
            drag_state.start_brush = Some(brush.clone());
            drag_state.start_cursor = viewport_cursor;
            drag_state.start_all_vertices = cache.vertices.clone();
            drag_state.start_face_polygons = cache.face_polygons.clone();

            // Collect unique vertex indices and their positions from all selected edges
            let mut seen = HashSet::new();
            let mut edge_verts = Vec::new();
            for &(a, b) in &brush_selection.edges {
                if seen.insert(a) {
                    let pos = cache.vertices.get(a).copied().unwrap_or(Vec3::ZERO);
                    edge_verts.push((a, pos));
                }
                if seen.insert(b) {
                    let pos = cache.vertices.get(b).copied().unwrap_or(Vec3::ZERO);
                    edge_verts.push((b, pos));
                }
            }
            drag_state.start_edge_vertices = edge_verts;
        }
        return;
    }

    if !drag_state.active {
        return;
    }

    // Axis constraint toggle
    if keyboard.just_pressed(KeyCode::KeyX) {
        drag_state.constraint = if drag_state.constraint == VertexDragConstraint::AxisX {
            VertexDragConstraint::Free
        } else {
            VertexDragConstraint::AxisX
        };
    } else if keyboard.just_pressed(KeyCode::KeyY) {
        drag_state.constraint = if drag_state.constraint == VertexDragConstraint::AxisY {
            VertexDragConstraint::Free
        } else {
            VertexDragConstraint::AxisY
        };
    } else if keyboard.just_pressed(KeyCode::KeyZ) {
        drag_state.constraint = if drag_state.constraint == VertexDragConstraint::AxisZ {
            VertexDragConstraint::Free
        } else {
            VertexDragConstraint::AxisZ
        };
    }

    // Cancel
    if keyboard.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right) {
        if let Some(ref start) = drag_state.start_brush {
            *brush = start.clone();
        }
        drag_state.active = false;
        drag_state.constraint = VertexDragConstraint::Free;
        return;
    }

    // Confirm
    if mouse.just_pressed(MouseButton::Left) || keyboard.just_pressed(KeyCode::Enter) {
        if let Some(ref start) = drag_state.start_brush {
            let cmd = SetBrush {
                entity: brush_entity,
                old: start.clone(),
                new: brush.clone(),
                label: "Move brush edge".to_string(),
            };
            history.undo_stack.push(Box::new(cmd));
            history.redo_stack.clear();
        }
        drag_state.active = false;
        drag_state.constraint = VertexDragConstraint::Free;
        return;
    }

    // Continue drag
    let Some(ref start) = drag_state.start_brush else {
        drag_state.active = false;
        return;
    };
    let Ok(brush_global) = brush_transforms.get(brush_entity) else {
        return;
    };

    let mouse_delta = viewport_cursor - drag_state.start_cursor;
    let Some(local_offset) = compute_brush_drag_offset(
        drag_state.constraint,
        mouse_delta,
        cam_tf,
        camera,
        brush_global,
    ) else {
        return;
    };

    // Rebuild from moved edge vertices using convex hull
    let mut new_verts = drag_state.start_all_vertices.clone();
    for &(vi, start_pos) in &drag_state.start_edge_vertices {
        if vi < new_verts.len() {
            new_verts[vi] = start_pos + local_offset;
        }
    }

    if let Some(new_brush) = rebuild_brush_from_vertices(
        start,
        &drag_state.start_all_vertices,
        &drag_state.start_face_polygons,
        &new_verts,
    ) {
        *brush = new_brush;
    }
}

// ---------------------------------------------------------------------------
// Delete selected vertices/edges/faces (Delete key)
// ---------------------------------------------------------------------------

fn handle_brush_delete(
    edit_mode: Res<EditMode>,
    keyboard: Res<ButtonInput<KeyCode>>,
    input_focus: Res<InputFocus>,
    mut brush_selection: ResMut<BrushSelection>,
    mut brushes: Query<&mut Brush>,
    brush_caches: Query<&BrushMeshCache>,
    mut history: ResMut<CommandHistory>,
    vertex_drag: Res<VertexDragState>,
    edge_drag: Res<EdgeDragState>,
    face_drag: Res<BrushDragState>,
) {
    let EditMode::BrushEdit(mode) = *edit_mode else {
        return;
    };
    if input_focus.0.is_some() {
        return;
    }
    if !keyboard.just_pressed(KeyCode::Delete) && !keyboard.just_pressed(KeyCode::Backspace) {
        return;
    }
    // Don't delete while dragging
    if vertex_drag.active || edge_drag.active || face_drag.active {
        return;
    }

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(mut brush) = brushes.get_mut(brush_entity) else {
        return;
    };

    match mode {
        BrushEditMode::Vertex => {
            if brush_selection.vertices.is_empty() {
                return;
            }
            let Ok(cache) = brush_caches.get(brush_entity) else {
                return;
            };
            let remove_set: HashSet<usize> = brush_selection.vertices.iter().copied().collect();
            let remaining: Vec<Vec3> = cache
                .vertices
                .iter()
                .enumerate()
                .filter(|(i, _)| !remove_set.contains(i))
                .map(|(_, v)| *v)
                .collect();
            if remaining.len() < 4 {
                return; // need at least a tetrahedron
            }
            let old = brush.clone();
            if let Some(new_brush) = rebuild_brush_from_vertices(
                &old,
                &cache.vertices,
                &cache.face_polygons,
                &remaining,
            ) {
                *brush = new_brush;
                let cmd = SetBrush {
                    entity: brush_entity,
                    old,
                    new: brush.clone(),
                    label: "Remove brush vertex".to_string(),
                };
                history.undo_stack.push(Box::new(cmd));
                history.redo_stack.clear();
                brush_selection.vertices.clear();
            }
        }
        BrushEditMode::Edge => {
            if brush_selection.edges.is_empty() {
                return;
            }
            let Ok(cache) = brush_caches.get(brush_entity) else {
                return;
            };
            let mut remove_set = HashSet::new();
            for &(a, b) in &brush_selection.edges {
                remove_set.insert(a);
                remove_set.insert(b);
            }
            let remaining: Vec<Vec3> = cache
                .vertices
                .iter()
                .enumerate()
                .filter(|(i, _)| !remove_set.contains(i))
                .map(|(_, v)| *v)
                .collect();
            if remaining.len() < 4 {
                return;
            }
            let old = brush.clone();
            if let Some(new_brush) = rebuild_brush_from_vertices(
                &old,
                &cache.vertices,
                &cache.face_polygons,
                &remaining,
            ) {
                *brush = new_brush;
                let cmd = SetBrush {
                    entity: brush_entity,
                    old,
                    new: brush.clone(),
                    label: "Remove brush edge".to_string(),
                };
                history.undo_stack.push(Box::new(cmd));
                history.redo_stack.clear();
                brush_selection.edges.clear();
            }
        }
        BrushEditMode::Face => {
            if brush_selection.faces.is_empty() {
                return;
            }
            let remaining = brush.faces.len() - brush_selection.faces.len();
            if remaining < 4 {
                return;
            }
            let old = brush.clone();
            let remove_set: HashSet<usize> = brush_selection.faces.iter().copied().collect();
            let new_faces: Vec<BrushFaceData> = brush
                .faces
                .iter()
                .enumerate()
                .filter(|(i, _)| !remove_set.contains(i))
                .map(|(_, f)| f.clone())
                .collect();
            brush.faces = new_faces;
            let cmd = SetBrush {
                entity: brush_entity,
                old,
                new: brush.clone(),
                label: "Remove brush face".to_string(),
            };
            history.undo_stack.push(Box::new(cmd));
            history.redo_stack.clear();
            brush_selection.faces.clear();
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Clip mode (4 key — add cutting plane)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct ClipState {
    pub points: Vec<Vec3>,
    pub preview_plane: Option<BrushPlane>,
}

fn handle_clip_mode(
    edit_mode: Res<EditMode>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    input_focus: Res<InputFocus>,
    windows: Query<&Window>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    brush_selection: Res<BrushSelection>,
    mut brushes: Query<&mut Brush>,
    brush_transforms: Query<&GlobalTransform>,
    brush_caches: Query<&BrushMeshCache>,
    mut clip_state: ResMut<ClipState>,
    mut history: ResMut<CommandHistory>,
    mut gizmos: Gizmos,
) {
    let EditMode::BrushEdit(BrushEditMode::Clip) = *edit_mode else {
        // Clear clip state when not in clip mode
        if !clip_state.points.is_empty() {
            clip_state.points.clear();
            clip_state.preview_plane = None;
        }
        return;
    };
    if input_focus.0.is_some() {
        return;
    }

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(brush_global) = brush_transforms.get(brush_entity) else {
        return;
    };

    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };

    // Escape clears clip points
    if keyboard.just_pressed(KeyCode::Escape) {
        clip_state.points.clear();
        clip_state.preview_plane = None;
        return;
    }

    // Left click: add point by raycasting to brush surface
    if mouse.just_pressed(MouseButton::Left) && clip_state.points.len() < 3 {
        let Some(cursor_pos) = window.cursor_position() else {
            return;
        };
        let Some(viewport_cursor) = window_to_viewport_cursor(cursor_pos, camera, &viewport_query) else {
            return;
        };

        // Cast ray from camera through cursor
        let Ok(ray) = camera.viewport_to_world(cam_tf, viewport_cursor) else {
            return;
        };

        let Ok(cache) = brush_caches.get(brush_entity) else {
            return;
        };

        // Find closest intersection with any brush face
        let (_, brush_rot, brush_trans) = brush_global.to_scale_rotation_translation();
        let mut best_t = f32::MAX;
        let mut best_point = None;

        for (face_idx, polygon) in cache.face_polygons.iter().enumerate() {
            if polygon.len() < 3 {
                continue;
            }
            let Ok(brush_ref) = brushes.get(brush_entity) else {
                return;
            };
            let face = &brush_ref.faces[face_idx];
            let world_normal = brush_rot * face.plane.normal;
            let face_centroid: Vec3 =
                polygon.iter().map(|&vi| cache.vertices[vi]).sum::<Vec3>() / polygon.len() as f32;
            let world_centroid = brush_global.transform_point(face_centroid);

            let denom = world_normal.dot(*ray.direction);
            if denom.abs() < EPSILON {
                continue;
            }
            let t = (world_centroid - ray.origin).dot(world_normal) / denom;
            if t > 0.0 && t < best_t {
                let hit = ray.origin + *ray.direction * t;
                // Verify hit is roughly on the brush (within face polygon bounds)
                let local_hit =
                    brush_rot.inverse() * (hit - brush_trans);
                if point_inside_all_planes(local_hit, &brush_ref.faces) {
                    best_t = t;
                    best_point = Some(local_hit);
                }
            }
        }

        if let Some(point) = best_point {
            clip_state.points.push(point);
        }
    }

    // Compute preview plane from collected points
    clip_state.preview_plane = match clip_state.points.len() {
        2 => {
            // Two points + camera forward for orientation
            let dir = clip_state.points[1] - clip_state.points[0];
            let (_, brush_rot, _) = brush_global.to_scale_rotation_translation();
            let local_cam_fwd = brush_rot.inverse() * cam_tf.forward().as_vec3();
            let normal = dir.cross(local_cam_fwd).normalize_or_zero();
            if normal.length_squared() > 0.5 {
                let distance = normal.dot(clip_state.points[0]);
                Some(BrushPlane { normal, distance })
            } else {
                None
            }
        }
        3 => {
            let a = clip_state.points[0];
            let b = clip_state.points[1];
            let c = clip_state.points[2];
            let normal = (b - a).cross(c - a).normalize_or_zero();
            if normal.length_squared() > 0.5 {
                let distance = normal.dot(a);
                Some(BrushPlane { normal, distance })
            } else {
                None
            }
        }
        _ => None,
    };

    // Enter: apply clip plane
    if keyboard.just_pressed(KeyCode::Enter) {
        if let Some(ref plane) = clip_state.preview_plane {
            let Ok(mut brush) = brushes.get_mut(brush_entity) else {
                return;
            };
            let old = brush.clone();
            brush.faces.push(BrushFaceData {
                plane: plane.clone(),
                material_index: 0,
                texture_path: None,
                uv_offset: Vec2::ZERO,
                uv_scale: Vec2::ONE,
                uv_rotation: 0.0,
            });
            let cmd = SetBrush {
                entity: brush_entity,
                old,
                new: brush.clone(),
                label: "Clip brush".to_string(),
            };
            history.undo_stack.push(Box::new(cmd));
            history.redo_stack.clear();
            clip_state.points.clear();
            clip_state.preview_plane = None;
        }
    }

    // Draw clip points and preview
    for (i, point) in clip_state.points.iter().enumerate() {
        let world_pos = brush_global.transform_point(*point);
        let color = Color::srgb(1.0, 0.3, 0.3);
        gizmos.sphere(Isometry3d::from_translation(world_pos), 0.06, color);
        // Draw connecting lines between points
        if i > 0 {
            let prev_world = brush_global.transform_point(clip_state.points[i - 1]);
            gizmos.line(prev_world, world_pos, color);
        }
    }

    // Draw preview plane as a translucent quad
    if let Some(ref plane) = clip_state.preview_plane {
        let (_, brush_rot, _) = brush_global.to_scale_rotation_translation();
        let world_normal = brush_rot * plane.normal;
        let center = brush_global.transform_point(plane.normal * plane.distance);
        let (u, v) = compute_face_tangent_axes(plane.normal);
        let world_u = brush_rot * u * 2.0;
        let world_v = brush_rot * v * 2.0;
        let preview_color = Color::srgba(1.0, 0.3, 0.3, 0.4);
        // Draw a diamond shape
        gizmos.line(center + world_u, center + world_v, preview_color);
        gizmos.line(center + world_v, center - world_u, preview_color);
        gizmos.line(center - world_u, center - world_v, preview_color);
        gizmos.line(center - world_v, center + world_u, preview_color);
        // Draw normal arrow
        gizmos.arrow(center, center + world_normal * 0.5, Color::srgb(1.0, 0.3, 0.3));
    }
}

// ---------------------------------------------------------------------------
// Draw brush edit gizmos
// ---------------------------------------------------------------------------

const EDIT_EDGE_COLOR: Color = Color::srgba(1.0, 0.8, 0.0, 1.0);
const EDIT_VERTEX_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 1.0);
const SELECTED_VERTEX_COLOR: Color = Color::srgba(0.0, 1.0, 0.5, 1.0);

fn draw_brush_edit_gizmos(
    edit_mode: Res<EditMode>,
    brush_selection: Res<BrushSelection>,
    brush_caches: Query<&BrushMeshCache>,
    brush_transforms: Query<&GlobalTransform>,
    brushes: Query<&Brush>,
    vertex_drag: Res<VertexDragState>,
    edge_drag: Res<EdgeDragState>,
    mut gizmos: Gizmos,
) {
    let EditMode::BrushEdit(mode) = *edit_mode else {
        return;
    };

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(cache) = brush_caches.get(brush_entity) else {
        return;
    };
    let Ok(brush_global) = brush_transforms.get(brush_entity) else {
        return;
    };

    // Collect unique edges and track selected state
    let mut drawn_edges: Vec<(usize, usize, bool)> = Vec::new();
    for polygon in &cache.face_polygons {
        if polygon.len() < 2 {
            continue;
        }
        for i in 0..polygon.len() {
            let a = polygon[i];
            let b = polygon[(i + 1) % polygon.len()];
            let edge = (a.min(b), a.max(b));
            if !drawn_edges.iter().any(|(ea, eb, _)| *ea == edge.0 && *eb == edge.1) {
                let selected = brush_selection.edges.contains(&edge);
                drawn_edges.push((edge.0, edge.1, selected));
            }
        }
    }

    // Draw all edges
    for &(a, b, selected) in &drawn_edges {
        let wa = brush_global.transform_point(cache.vertices[a]);
        let wb = brush_global.transform_point(cache.vertices[b]);
        let color = if selected {
            SELECTED_VERTEX_COLOR
        } else {
            EDIT_EDGE_COLOR
        };
        gizmos.line(wa, wb, color);
    }

    // Draw vertices as small spheres
    for (vi, v) in cache.vertices.iter().enumerate() {
        let world_pos = brush_global.transform_point(*v);
        let color = if brush_selection.vertices.contains(&vi) {
            SELECTED_VERTEX_COLOR
        } else {
            EDIT_VERTEX_COLOR
        };
        gizmos.sphere(Isometry3d::from_translation(world_pos), 0.04, color);
    }

    // Highlight selected faces
    if mode == BrushEditMode::Face {
        if let Ok(brush) = brushes.get(brush_entity) {
            for &face_idx in &brush_selection.faces {
                let polygon = &cache.face_polygons[face_idx];
                if polygon.len() < 3 {
                    continue;
                }
                // Draw face outline in bright color
                for i in 0..polygon.len() {
                    let a = brush_global.transform_point(cache.vertices[polygon[i]]);
                    let b = brush_global.transform_point(cache.vertices[polygon[(i + 1) % polygon.len()]]);
                    gizmos.line(a, b, SELECTED_VERTEX_COLOR);
                }
                // Draw the face normal from centroid
                let centroid: Vec3 = polygon.iter().map(|&vi| cache.vertices[vi]).sum::<Vec3>()
                    / polygon.len() as f32;
                let world_centroid = brush_global.transform_point(centroid);
                let normal = brush.faces[face_idx].plane.normal;
                let (_, brush_rot, _) = brush_global.to_scale_rotation_translation();
                let world_normal = brush_rot * normal;
                gizmos.arrow(world_centroid, world_centroid + world_normal * 0.5, Color::srgb(0.0, 1.0, 1.0));
            }
        }
    }

    // Draw drag constraint line (vertex or edge drag)
    let active_constraint = if vertex_drag.active {
        Some(vertex_drag.constraint)
    } else if edge_drag.active {
        Some(edge_drag.constraint)
    } else {
        None
    };
    if let Some(constraint) = active_constraint {
        if constraint != VertexDragConstraint::Free {
            let (axis_dir, color) = match constraint {
                VertexDragConstraint::AxisX => (Vec3::X, Color::srgb(1.0, 0.2, 0.2)),
                VertexDragConstraint::AxisY => (Vec3::Y, Color::srgb(0.2, 1.0, 0.2)),
                VertexDragConstraint::AxisZ => (Vec3::Z, Color::srgb(0.2, 0.4, 1.0)),
                VertexDragConstraint::Free => unreachable!(),
            };
            let (_, brush_rot, _) = brush_global.to_scale_rotation_translation();
            let world_axis = brush_rot * axis_dir;
            let center = brush_global.translation();
            gizmos.line(center - world_axis * 50.0, center + world_axis * 50.0, color);
        }
    }
}
