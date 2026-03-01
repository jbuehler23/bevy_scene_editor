use bevy::{
    asset::RenderAssetUsages,
    color::palettes::tailwind,
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use bevy_rerecast::{prelude::*, rerecast::DetailNavmesh};

use super::brp_client::{ObstacleGizmo, SceneVisualMesh};
use super::{NavmeshHandleRes, NavmeshState, NavmeshStatus};
use crate::EditorEntity;

/// Marker component for fill mesh entities spawned by the visualization system.
#[derive(Component)]
pub struct NavmeshFillMesh;

/// Marker component for the retained-mode gizmo wireframe entity.
#[derive(Component)]
pub struct NavmeshGizmoEntity;

/// Tracks the current navmesh asset ID to detect changes.
#[derive(Resource, Default)]
struct NavmeshVisuals {
    current_id: Option<AssetId<Navmesh>>,
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<NavmeshVisuals>();
    app.add_systems(
        Update,
        rebuild_navmesh_visuals.run_if(resource_exists::<NavmeshHandleRes>),
    );
    app.add_observer(on_navmesh_region_removed);
}

fn rebuild_navmesh_visuals(
    mut commands: Commands,
    navmesh_handle: Res<NavmeshHandleRes>,
    navmeshes: Res<Assets<Navmesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gizmo_assets: ResMut<Assets<GizmoAsset>>,
    mut visuals: ResMut<NavmeshVisuals>,
    mut asset_events: MessageReader<AssetEvent<Navmesh>>,
    existing_fills: Query<Entity, With<NavmeshFillMesh>>,
    existing_gizmo: Query<(Entity, &Gizmo), With<NavmeshGizmoEntity>>,
    mut state: ResMut<NavmeshState>,
) {
    let handle_id = navmesh_handle.id();

    // Check if we need to rebuild
    let handle_changed = visuals.current_id != Some(handle_id);
    let asset_modified = asset_events.read().any(|ev| match ev {
        AssetEvent::Added { id } | AssetEvent::Modified { id } => *id == handle_id,
        _ => false,
    });

    if !handle_changed && !asset_modified {
        return;
    }

    let Some(navmesh) = navmeshes.get(handle_id) else {
        return;
    };

    visuals.current_id = Some(handle_id);

    // --- Despawn old fill meshes ---
    for entity in &existing_fills {
        commands.entity(entity).despawn();
    }

    let detail = &navmesh.detail;
    let polygon = &navmesh.polygon;

    // --- Build fill meshes grouped by area type ---
    let mut area_vertices: std::collections::HashMap<u8, Vec<[f32; 3]>> =
        std::collections::HashMap::new();

    for (submesh_idx, submesh) in detail.meshes.iter().enumerate() {
        let area = if submesh_idx < polygon.areas.len() {
            *polygon.areas[submesh_idx]
        } else {
            0
        };

        let base_v = submesh.base_vertex_index as usize;
        let base_t = submesh.base_triangle_index as usize;
        let verts = &detail.vertices[base_v..base_v + submesh.vertex_count as usize];
        let tris = &detail.triangles[base_t..base_t + submesh.triangle_count as usize];

        let entry = area_vertices.entry(area).or_default();

        for tri in tris {
            for &idx in tri {
                let v = verts[idx as usize];
                entry.push([v.x, v.y, v.z]);
            }
        }
    }

    for (area, vertices) in &area_vertices {
        let color = area_color(*area);
        let vertex_count = vertices.len();

        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices.clone());
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            vec![[0.0, 1.0, 0.0]; vertex_count],
        );
        let indices: Vec<u32> = (0..vertex_count as u32).collect();
        mesh.insert_indices(Indices::U32(indices));

        let material = materials.add(StandardMaterial {
            base_color: color,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            depth_bias: -10.0,
            ..default()
        });

        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::default(),
            NavmeshFillMesh,
            NotShadowCaster,
            NotShadowReceiver,
            EditorEntity,
        ));
    }

    // --- Build retained-mode gizmo wireframe ---
    let wireframe_color: Color = tailwind::EMERALD_400.into();

    if let Ok((entity, gizmo)) = existing_gizmo.single() {
        // Update existing gizmo asset
        if let Some(asset) = gizmo_assets.get_mut(&gizmo.handle) {
            asset.clear();
            populate_wireframe(asset, detail, wireframe_color);
        }
        // Entity already exists with correct config, no need to respawn
        let _ = entity;
    } else {
        // Spawn new gizmo entity
        let mut gizmo_asset = GizmoAsset::default();
        populate_wireframe(&mut gizmo_asset, detail, wireframe_color);

        commands.spawn((
            Gizmo {
                handle: gizmo_assets.add(gizmo_asset),
                line_config: GizmoLineConfig {
                    width: 1.5,
                    perspective: true,
                    ..default()
                },
                depth_bias: -0.003,
            },
            NavmeshGizmoEntity,
            EditorEntity,
        ));
    }

    state.status = NavmeshStatus::Ready;
}

fn on_navmesh_region_removed(
    _trigger: On<Remove, jackdaw_jsn::NavmeshRegion>,
    mut commands: Commands,
    fills: Query<Entity, With<NavmeshFillMesh>>,
    gizmos: Query<Entity, With<NavmeshGizmoEntity>>,
    scene_visuals: Query<Entity, With<SceneVisualMesh>>,
    obstacle_gizmos: Query<Entity, With<ObstacleGizmo>>,
    mut state: ResMut<NavmeshState>,
) {
    for entity in fills.iter().chain(gizmos.iter()).chain(scene_visuals.iter()).chain(obstacle_gizmos.iter()) {
        commands.entity(entity).despawn();
    }
    // Reset the handle to default so rebuild_navmesh_visuals won't see a stale
    // asset id and recreate visuals.  Don't set visuals.current_id = None here —
    // that would make the rebuild system think the handle changed and re-spawn
    // everything before these deferred despawn commands apply.
    commands.queue(|world: &mut World| {
        world.resource_mut::<NavmeshHandleRes>().0 = Default::default();
        world.resource_mut::<NavmeshVisuals>().current_id = None;
    });
    state.status = NavmeshStatus::Idle;
}

fn populate_wireframe(gizmo: &mut GizmoAsset, detail: &DetailNavmesh, color: Color) {
    for submesh in &detail.meshes {
        let base_v = submesh.base_vertex_index as usize;
        let base_t = submesh.base_triangle_index as usize;
        let verts = &detail.vertices[base_v..base_v + submesh.vertex_count as usize];
        let tris = &detail.triangles[base_t..base_t + submesh.triangle_count as usize];

        for tri in tris {
            let a = verts[tri[0] as usize];
            let b = verts[tri[1] as usize];
            let c = verts[tri[2] as usize];
            gizmo.linestrip([a, b, c, a], color);
        }
    }
}

fn area_color(area: u8) -> Color {
    match area {
        0 => Color::srgba(0.0, 0.4, 0.8, 0.25),
        1 => Color::srgba(0.8, 0.4, 0.0, 0.25),
        2 => Color::srgba(0.8, 0.0, 0.4, 0.25),
        3 => Color::srgba(0.4, 0.0, 0.8, 0.25),
        _ => Color::srgba(0.5, 0.5, 0.5, 0.25),
    }
}
