use bevy::{
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use crate::types::Brush;
use jackdaw_geometry::{compute_brush_geometry, compute_face_uvs, triangulate_face};

/// Simplified runtime mesh rebuild for consumers (no editor material palette,
/// no BrushFaceEntity, no texture cache — just a single mesh child per brush).
pub(crate) fn rebuild_brush_meshes(
    mut commands: Commands,
    new_brushes: Query<(Entity, &Brush), Added<Brush>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, brush) in &new_brushes {
        let (vertices, face_polygons) = compute_brush_geometry(&brush.faces);

        let mut all_positions: Vec<[f32; 3]> = Vec::new();
        let mut all_normals: Vec<[f32; 3]> = Vec::new();
        let mut all_uvs: Vec<[f32; 2]> = Vec::new();
        let mut all_indices: Vec<u32> = Vec::new();

        for (face_idx, face_data) in brush.faces.iter().enumerate() {
            let indices = &face_polygons[face_idx];
            if indices.len() < 3 {
                continue;
            }

            let base_vertex = all_positions.len() as u32;

            // Per-face vertices (duplicated for flat normals)
            for &vi in indices {
                all_positions.push(vertices[vi].to_array());
                all_normals.push(face_data.plane.normal.to_array());
            }

            let uvs = compute_face_uvs(
                &vertices,
                indices,
                face_data.plane.normal,
                face_data.uv_offset,
                face_data.uv_scale,
                face_data.uv_rotation,
            );
            all_uvs.extend_from_slice(&uvs);

            // Fan triangulate with local indices
            let local_indices: Vec<usize> = (0..indices.len()).collect();
            let tris = triangulate_face(&local_indices);
            for tri in &tris {
                all_indices.push(base_vertex + tri[0]);
                all_indices.push(base_vertex + tri[1]);
                all_indices.push(base_vertex + tri[2]);
            }
        }

        if all_positions.is_empty() {
            continue;
        }

        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, all_positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, all_normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, all_uvs);
        mesh.insert_indices(Indices::U32(all_indices));

        let mesh_handle = meshes.add(mesh);
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.7, 0.7, 0.7),
            ..default()
        });

        let child = commands
            .spawn((
                Mesh3d(mesh_handle),
                MeshMaterial3d(material),
                Transform::default(),
                ChildOf(entity),
            ))
            .id();
        let _ = child;
    }
}
