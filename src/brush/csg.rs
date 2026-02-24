use bevy::prelude::*;

use super::{BrushFaceData, BrushPlane};
use super::geometry::compute_brush_geometry;

/// Transform brush face planes from local space to world space.
pub fn brush_planes_to_world(brush: &super::Brush, transform: &GlobalTransform) -> Vec<BrushFaceData> {
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
