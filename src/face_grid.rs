use std::collections::HashSet;

use bevy::color::palettes::tailwind;
use bevy::prelude::*;

use crate::brush::{Brush, BrushMeshCache};
use crate::selection::Selected;
use crate::snapping::SnapSettings;
use crate::viewport_overlays::OverlaySettings;

/// Custom gizmo group for face grid / wireframe overlays, rendered with a depth
/// bias so lines sit in front of the brush geometry rather than z-fighting.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct FaceGridGizmoGroup;

pub struct FaceGridPlugin;

impl Plugin for FaceGridPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<FaceGridGizmoGroup>()
            .add_systems(Startup, configure_face_grid_gizmos)
            .add_systems(
                PostUpdate,
                (draw_brush_edges, draw_face_grids)
                    .after(bevy::transform::TransformSystems::Propagate)
                    .run_if(in_state(crate::AppState::Editor)),
            );
    }
}

fn configure_face_grid_gizmos(mut config_store: ResMut<GizmoConfigStore>) {
    let (config, _) = config_store.config_mut::<FaceGridGizmoGroup>();
    config.depth_bias = -0.0001;
}

/// Draw wireframe edges on all brushes (bright cyan on selected, subtle grey on unselected).
fn draw_brush_edges(
    mut gizmos: Gizmos<FaceGridGizmoGroup>,
    settings: Res<OverlaySettings>,
    brushes: Query<(&BrushMeshCache, &GlobalTransform, Has<Selected>)>,
) {
    if !settings.show_brush_wireframe {
        return;
    }

    for (cache, global_tf, is_selected) in &brushes {
        let color: Color = if is_selected {
            tailwind::CYAN_400.into()
        } else {
            Color::from(tailwind::GRAY_500).with_alpha(0.5)
        };

        let mut drawn_edges = HashSet::new();
        for polygon in &cache.face_polygons {
            for i in 0..polygon.len() {
                let a = polygon[i];
                let b = polygon[(i + 1) % polygon.len()];
                let edge = (a.min(b), a.max(b));
                if drawn_edges.insert(edge) {
                    let wa = global_tf.transform_point(cache.vertices[a]);
                    let wb = global_tf.transform_point(cache.vertices[b]);
                    gizmos.line(wa, wb, color);
                }
            }
        }
    }
}

/// Draw grid lines on each face of all brushes (brighter on selected).
fn draw_face_grids(
    mut gizmos: Gizmos<FaceGridGizmoGroup>,
    settings: Res<OverlaySettings>,
    snap: Res<SnapSettings>,
    brushes: Query<(&Brush, &BrushMeshCache, &GlobalTransform, Has<Selected>)>,
) {
    if !settings.show_face_grid {
        return;
    }

    let grid_size = snap.grid_size();

    for (brush, cache, global_tf, is_selected) in &brushes {
        let color = if is_selected {
            Color::from(tailwind::GRAY_600).with_alpha(0.5)
        } else {
            Color::from(tailwind::GRAY_600).with_alpha(0.25)
        };
        for (face_idx, face_data) in brush.faces.iter().enumerate() {
            let Some(polygon_indices) = cache.face_polygons.get(face_idx) else {
                continue;
            };
            if polygon_indices.len() < 3 {
                continue;
            }

            // Get world-space face vertices
            let world_verts: Vec<Vec3> = polygon_indices
                .iter()
                .map(|&i| global_tf.transform_point(cache.vertices[i]))
                .collect();

            // Compute world normal
            let world_normal = global_tf
                .compute_transform()
                .rotation
                .mul_vec3(face_data.plane.normal)
                .normalize();

            // Select 2D coordinate axes (TrenchBroom style):
            // Pick the two axes perpendicular to the dominant normal component
            let abs_n = world_normal.abs();
            let (axis_u, axis_v, plane_axis) = if abs_n.x >= abs_n.y && abs_n.x >= abs_n.z {
                // Dominant X: use Y, Z
                (1usize, 2usize, 0usize)
            } else if abs_n.y >= abs_n.x && abs_n.y >= abs_n.z {
                // Dominant Y: use X, Z
                (0, 2, 1)
            } else {
                // Dominant Z: use X, Y
                (0, 1, 2)
            };

            // Project face vertices to 2D using selected axes
            let polygon_2d: Vec<Vec2> = world_verts
                .iter()
                .map(|v| {
                    let arr = v.to_array();
                    Vec2::new(arr[axis_u], arr[axis_v])
                })
                .collect();

            // Find bounding rect of 2D polygon
            let mut min_2d = Vec2::splat(f32::MAX);
            let mut max_2d = Vec2::splat(f32::MIN);
            for &p in &polygon_2d {
                min_2d = min_2d.min(p);
                max_2d = max_2d.max(p);
            }

            // Snap bounds to grid
            let grid_min_u = (min_2d.x / grid_size).floor() * grid_size;
            let grid_max_u = (max_2d.x / grid_size).ceil() * grid_size;
            let grid_min_v = (min_2d.y / grid_size).floor() * grid_size;
            let grid_max_v = (max_2d.y / grid_size).ceil() * grid_size;

            // Plane equation for reconstructing 3rd axis:
            // normal . point = d (world-space)
            let plane_d = world_normal.dot(world_verts[0]);
            let normal_arr = world_normal.to_array();

            // Draw lines at constant U values (vertical lines in 2D)
            let mut u = grid_min_u;
            while u <= grid_max_u + grid_size * 0.01 {
                if let Some((p0_2d, p1_2d)) = clip_line_to_convex_polygon(&polygon_2d, true, u) {
                    let a = reconstruct_3d(p0_2d, axis_u, axis_v, plane_axis, plane_d, normal_arr);
                    let b = reconstruct_3d(p1_2d, axis_u, axis_v, plane_axis, plane_d, normal_arr);
                    if let (Some(a), Some(b)) = (a, b) {
                        let offset = world_normal * 0.002;
                        gizmos.line(a + offset, b + offset, color);
                    }
                }
                u += grid_size;
            }

            // Draw lines at constant V values (horizontal lines in 2D)
            let mut v = grid_min_v;
            while v <= grid_max_v + grid_size * 0.01 {
                if let Some((p0_2d, p1_2d)) = clip_line_to_convex_polygon(&polygon_2d, false, v) {
                    let a = reconstruct_3d(p0_2d, axis_u, axis_v, plane_axis, plane_d, normal_arr);
                    let b = reconstruct_3d(p1_2d, axis_u, axis_v, plane_axis, plane_d, normal_arr);
                    if let (Some(a), Some(b)) = (a, b) {
                        let offset = world_normal * 0.002;
                        gizmos.line(a + offset, b + offset, color);
                    }
                }
                v += grid_size;
            }
        }
    }
}

/// Reconstruct a 3D point from 2D coordinates + plane equation.
/// Returns None if the plane normal component along `plane_axis` is ~zero.
fn reconstruct_3d(
    point_2d: Vec2,
    axis_u: usize,
    axis_v: usize,
    plane_axis: usize,
    plane_d: f32,
    normal: [f32; 3],
) -> Option<Vec3> {
    if normal[plane_axis].abs() < 1e-6 {
        return None;
    }
    let mut arr = [0.0f32; 3];
    arr[axis_u] = point_2d.x;
    arr[axis_v] = point_2d.y;
    // Solve: normal[plane_axis] * arr[plane_axis] = plane_d - normal[axis_u]*arr[axis_u] - normal[axis_v]*arr[axis_v]
    arr[plane_axis] = (plane_d - normal[axis_u] * arr[axis_u] - normal[axis_v] * arr[axis_v])
        / normal[plane_axis];
    Some(Vec3::from_array(arr))
}

/// Clip a horizontal or vertical line to a convex polygon.
///
/// If `is_u_constant` is true, clips the line `u = val` (finds min/max v intersections).
/// If false, clips the line `v = val` (finds min/max u intersections).
///
/// Returns the two intersection endpoints, or None if the line doesn't cross the polygon.
fn clip_line_to_convex_polygon(
    polygon: &[Vec2],
    is_u_constant: bool,
    val: f32,
) -> Option<(Vec2, Vec2)> {
    let n = polygon.len();
    let mut intersections = Vec::new();

    for i in 0..n {
        let a = polygon[i];
        let b = polygon[(i + 1) % n];

        let (a_coord, b_coord, a_other, b_other) = if is_u_constant {
            (a.x, b.x, a.y, b.y)
        } else {
            (a.y, b.y, a.x, b.x)
        };

        // Check if edge crosses the line
        let min_c = a_coord.min(b_coord);
        let max_c = a_coord.max(b_coord);
        if val < min_c - 1e-6 || val > max_c + 1e-6 {
            continue;
        }

        let denom = b_coord - a_coord;
        let other = if denom.abs() < 1e-6 {
            // Edge is parallel to the line — use both endpoints
            intersections.push(a_other);
            b_other
        } else {
            let t = (val - a_coord) / denom;
            a_other + t * (b_other - a_other)
        };
        intersections.push(other);
    }

    if intersections.len() < 2 {
        return None;
    }

    let min_other = intersections.iter().copied().fold(f32::MAX, f32::min);
    let max_other = intersections.iter().copied().fold(f32::MIN, f32::max);

    if (max_other - min_other).abs() < 1e-6 {
        return None;
    }

    let (p0, p1) = if is_u_constant {
        (Vec2::new(val, min_other), Vec2::new(val, max_other))
    } else {
        (Vec2::new(min_other, val), Vec2::new(max_other, val))
    };

    Some((p0, p1))
}
