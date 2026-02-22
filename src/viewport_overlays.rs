use std::f32::consts::FRAC_PI_2;

use avian3d::parry::math::Point as ParryPoint;
use avian3d::parry::transformation::convex_hull;
use bevy::prelude::*;

use crate::brush::{self, BrushMeshCache};
use crate::selection::Selected;

pub struct ViewportOverlaysPlugin;

impl Plugin for ViewportOverlaysPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverlaySettings>()
            .add_systems(
                PostUpdate,
                draw_selection_bounding_boxes
                    .after(bevy::camera::visibility::VisibilitySystems::CalculateBounds)
                    .after(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                (
                    draw_point_light_gizmo,
                    draw_spot_light_gizmo,
                    draw_dir_light_gizmo,
                    draw_camera_gizmo,
                ),
            )
            .add_systems(Update, draw_coordinate_indicator);
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoundingBoxMode {
    /// Simple axis-aligned bounding box (12 edges).
    #[default]
    Aabb,
    /// Full convex hull wireframe showing all geometry edges.
    ConvexHull,
}

#[derive(Resource)]
pub struct OverlaySettings {
    pub show_bounding_boxes: bool,
    pub show_coordinate_indicator: bool,
    pub bounding_box_mode: BoundingBoxMode,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            show_bounding_boxes: true,
            show_coordinate_indicator: true,
            bounding_box_mode: BoundingBoxMode::default(),
        }
    }
}

/// Draw bounding boxes around selected entities with geometry.
fn draw_selection_bounding_boxes(
    mut gizmos: Gizmos,
    settings: Res<OverlaySettings>,
    selected: Query<(Entity, &GlobalTransform, Option<&BrushMeshCache>), With<Selected>>,
    children_query: Query<&Children>,
    mesh_query: Query<(&Mesh3d, &GlobalTransform)>,
    meshes: Res<Assets<Mesh>>,
) {
    if !settings.show_bounding_boxes {
        return;
    }

    let color = Color::srgba(1.0, 1.0, 0.0, 0.8);

    for (entity, global_tf, maybe_brush_cache) in &selected {
        // Collect world-space vertices
        let world_verts = if let Some(cache) = maybe_brush_cache {
            if cache.vertices.is_empty() {
                continue;
            }
            match settings.bounding_box_mode {
                BoundingBoxMode::ConvexHull => {
                    // Convex hull mode for brushes: use face polygons directly
                    let verts: Vec<Vec3> = cache
                        .vertices
                        .iter()
                        .map(|v| global_tf.transform_point(*v))
                        .collect();
                    draw_hull_wireframe(&mut gizmos, &verts, &cache.face_polygons, color);
                    continue;
                }
                BoundingBoxMode::Aabb => {
                    cache
                        .vertices
                        .iter()
                        .map(|v| global_tf.transform_point(*v))
                        .collect::<Vec<Vec3>>()
                }
            }
        } else {
            let mut verts = Vec::new();
            collect_descendant_mesh_world_vertices(
                entity,
                &children_query,
                &mesh_query,
                &meshes,
                &mut verts,
            );
            if verts.is_empty() {
                continue;
            }
            verts
        };

        match settings.bounding_box_mode {
            BoundingBoxMode::Aabb => {
                let (min, max) = aabb_from_points(&world_verts);
                draw_aabb_wireframe(&mut gizmos, min, max, color);
            }
            BoundingBoxMode::ConvexHull => {
                let parry_points: Vec<ParryPoint<f32>> = world_verts
                    .iter()
                    .map(|v| ParryPoint::new(v.x, v.y, v.z))
                    .collect();
                let (hull_verts, hull_tris) = convex_hull(&parry_points);
                if hull_verts.is_empty() || hull_tris.is_empty() {
                    continue;
                }

                let hull_positions: Vec<Vec3> = hull_verts
                    .iter()
                    .map(|p| Vec3::new(p.x, p.y, p.z))
                    .collect();
                let hull_faces = brush::merge_hull_triangles(&hull_positions, &hull_tris);
                let face_polygons: Vec<Vec<usize>> = hull_faces
                    .into_iter()
                    .map(|f| f.vertex_indices)
                    .collect();
                draw_hull_wireframe(&mut gizmos, &hull_positions, &face_polygons, color);
            }
        }
    }
}

/// Compute axis-aligned bounding box from a set of points.
fn aabb_from_points(points: &[Vec3]) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for &p in points {
        min = min.min(p);
        max = max.max(p);
    }
    (min, max)
}

/// Draw 12 edges of an axis-aligned bounding box.
fn draw_aabb_wireframe(gizmos: &mut Gizmos, min: Vec3, max: Vec3, color: Color) {
    let corners = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ];
    // Bottom face
    gizmos.line(corners[0], corners[1], color);
    gizmos.line(corners[1], corners[2], color);
    gizmos.line(corners[2], corners[3], color);
    gizmos.line(corners[3], corners[0], color);
    // Top face
    gizmos.line(corners[4], corners[5], color);
    gizmos.line(corners[5], corners[6], color);
    gizmos.line(corners[6], corners[7], color);
    gizmos.line(corners[7], corners[4], color);
    // Vertical edges
    gizmos.line(corners[0], corners[4], color);
    gizmos.line(corners[1], corners[5], color);
    gizmos.line(corners[2], corners[6], color);
    gizmos.line(corners[3], corners[7], color);
}

/// Draw unique edges from face polygons as wireframe lines.
fn draw_hull_wireframe(
    gizmos: &mut Gizmos,
    world_verts: &[Vec3],
    face_polygons: &[Vec<usize>],
    color: Color,
) {
    let mut drawn: Vec<(usize, usize)> = Vec::new();
    for polygon in face_polygons {
        if polygon.len() < 2 {
            continue;
        }
        for i in 0..polygon.len() {
            let a = polygon[i];
            let b = polygon[(i + 1) % polygon.len()];
            let edge = (a.min(b), a.max(b));
            if !drawn.contains(&edge) {
                drawn.push(edge);
                gizmos.line(world_verts[edge.0], world_verts[edge.1], color);
            }
        }
    }
}

/// Recursively collect world-space vertex positions from Mesh3d components.
fn collect_descendant_mesh_world_vertices(
    entity: Entity,
    children_query: &Query<&Children>,
    mesh_query: &Query<(&Mesh3d, &GlobalTransform)>,
    meshes: &Assets<Mesh>,
    out: &mut Vec<Vec3>,
) {
    if let Ok((mesh3d, global_tf)) = mesh_query.get(entity) {
        if let Some(mesh) = meshes.get(&mesh3d.0) {
            if let Some(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|attr| attr.as_float3())
            {
                for pos in positions {
                    out.push(global_tf.transform_point(Vec3::from_array(*pos)));
                }
            }
        }
    }
    if let Ok(children) = children_query.get(entity) {
        for child in children.iter() {
            collect_descendant_mesh_world_vertices(child, children_query, mesh_query, meshes, out);
        }
    }
}

// ---------------------------------------------------------------------------
// Type-specific gizmo systems
// ---------------------------------------------------------------------------

/// Point light: 3 axis-aligned circles at range radius.
fn draw_point_light_gizmo(
    mut gizmos: Gizmos,
    settings: Res<OverlaySettings>,
    query: Query<(&PointLight, &GlobalTransform), With<Selected>>,
) {
    if !settings.show_bounding_boxes {
        return;
    }
    let color = Color::srgba(1.0, 1.0, 0.0, 0.8);
    for (light, tf) in &query {
        let pos = tf.translation();
        gizmos.circle(
            Isometry3d::new(pos, Quat::from_rotation_x(FRAC_PI_2)),
            light.range,
            color,
        );
        gizmos.circle(Isometry3d::new(pos, Quat::IDENTITY), light.range, color);
        gizmos.circle(
            Isometry3d::new(pos, Quat::from_rotation_y(FRAC_PI_2)),
            light.range,
            color,
        );
    }
}

/// Spot light: cone from outer_angle + range.
fn draw_spot_light_gizmo(
    mut gizmos: Gizmos,
    settings: Res<OverlaySettings>,
    query: Query<(&SpotLight, &GlobalTransform), With<Selected>>,
) {
    if !settings.show_bounding_boxes {
        return;
    }
    let color = Color::srgba(1.0, 1.0, 0.0, 0.8);
    for (light, tf) in &query {
        let pos = tf.translation();
        let fwd = tf.forward().as_vec3();
        let right = tf.right().as_vec3();
        let up = tf.up().as_vec3();
        let r = light.range * light.outer_angle.tan();
        let tip = pos + fwd * light.range;
        // Circle at cone end
        gizmos.circle(
            Isometry3d::new(tip, tf.compute_transform().rotation),
            r,
            color,
        );
        // 4 lines from origin to circle edges
        gizmos.line(pos, tip + right * r, color);
        gizmos.line(pos, tip - right * r, color);
        gizmos.line(pos, tip + up * r, color);
        gizmos.line(pos, tip - up * r, color);
    }
}

/// Directional light: arrow along forward direction.
fn draw_dir_light_gizmo(
    mut gizmos: Gizmos,
    settings: Res<OverlaySettings>,
    query: Query<&GlobalTransform, (With<DirectionalLight>, With<Selected>)>,
) {
    if !settings.show_bounding_boxes {
        return;
    }
    let color = Color::srgba(1.0, 1.0, 0.0, 0.8);
    for tf in &query {
        let pos = tf.translation();
        let dir = tf.forward().as_vec3();
        gizmos.arrow(pos, pos + dir * 2.0, color);
    }
}

/// Camera: frustum wireframe from Projection.
fn draw_camera_gizmo(
    mut gizmos: Gizmos,
    settings: Res<OverlaySettings>,
    query: Query<(&Projection, &GlobalTransform), With<Selected>>,
) {
    if !settings.show_bounding_boxes {
        return;
    }
    let color = Color::srgba(1.0, 1.0, 0.0, 0.8);
    for (projection, tf) in &query {
        let Projection::Perspective(proj) = projection else {
            continue;
        };
        let depth = 2.0;
        let half_v = depth * (proj.fov / 2.0).tan();
        let half_h = half_v * proj.aspect_ratio;
        let fwd = tf.forward().as_vec3();
        let right = tf.right().as_vec3();
        let up = tf.up().as_vec3();
        let origin = tf.translation();
        let far_center = origin + fwd * depth;
        let corners = [
            far_center + right * half_h + up * half_v,
            far_center - right * half_h + up * half_v,
            far_center - right * half_h - up * half_v,
            far_center + right * half_h - up * half_v,
        ];
        // 4 lines from origin to far corners
        for &c in &corners {
            gizmos.line(origin, c, color);
        }
        // Far rectangle
        for i in 0..4 {
            gizmos.line(corners[i], corners[(i + 1) % 4], color);
        }
    }
}

// ---------------------------------------------------------------------------
// Coordinate indicator
// ---------------------------------------------------------------------------

/// Draw a small coordinate indicator showing camera orientation.
fn draw_coordinate_indicator(
    mut gizmos: Gizmos,
    settings: Res<OverlaySettings>,
    camera_query: Query<&GlobalTransform, (With<Camera3d>, With<crate::EditorEntity>)>,
) {
    if !settings.show_coordinate_indicator {
        return;
    }

    let Ok(cam_tf) = camera_query.single() else {
        return;
    };

    let cam_pos = cam_tf.translation();
    let cam_forward = cam_tf.forward().as_vec3();

    // Place the indicator in front of the camera, offset to bottom-left
    let indicator_pos = cam_pos
        + cam_forward * 2.0
        + cam_tf.right().as_vec3() * -0.8
        + cam_tf.up().as_vec3() * -0.5;
    let size = 0.1;

    gizmos.line(indicator_pos, indicator_pos + Vec3::X * size, Color::srgb(1.0, 0.2, 0.2));
    gizmos.line(indicator_pos, indicator_pos + Vec3::Y * size, Color::srgb(0.2, 1.0, 0.2));
    gizmos.line(indicator_pos, indicator_pos + Vec3::Z * size, Color::srgb(0.2, 0.4, 1.0));
}
