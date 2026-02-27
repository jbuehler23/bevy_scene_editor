use bevy::prelude::*;

use jackdaw_jsn::Brush;
use super::{
    BrushEditMode, BrushMeshCache, BrushSelection, EditMode,
};
use super::interaction::{EdgeDragState, VertexDragConstraint, VertexDragState};

const EDIT_EDGE_COLOR: Color = Color::srgba(1.0, 0.8, 0.0, 1.0);
const EDIT_VERTEX_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 1.0);
const SELECTED_VERTEX_COLOR: Color = Color::srgba(0.0, 1.0, 0.5, 1.0);

pub(super) fn draw_brush_edit_gizmos(
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
