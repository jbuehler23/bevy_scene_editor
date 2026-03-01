use bevy::prelude::*;
use bevy_rerecast::prelude::*;

use super::{NavmeshHandleRes, NavmeshState, NavmeshStatus};

pub(super) fn plugin(app: &mut App) {
    app.add_observer(on_build_navmesh);
}

#[derive(Event)]
pub struct BuildNavmesh;

fn on_build_navmesh(
    _trigger: On<BuildNavmesh>,
    mut commands: Commands,
    regions: Query<&jackdaw_jsn::NavmeshRegion>,
    mut navmesh_generator: NavmeshGenerator,
    mut state: ResMut<NavmeshState>,
) {
    let Some(region) = regions.iter().next() else {
        warn!("No NavmeshRegion entity found");
        return;
    };

    let settings = region_to_settings_without_transform(region);
    let handle = navmesh_generator.generate(settings);
    commands.insert_resource(NavmeshHandleRes(handle));
    state.status = NavmeshStatus::Building;
}

/// Convert region settings without AABB (for BRP fetch — the remote app determines bounds).
pub(super) fn region_to_settings_without_transform(region: &jackdaw_jsn::NavmeshRegion) -> NavmeshSettings {
    NavmeshSettings {
        agent_radius: region.agent_radius,
        agent_height: region.agent_height,
        walkable_climb: region.walkable_climb,
        walkable_slope_angle: region.walkable_slope_degrees.to_radians(),
        cell_size_fraction: region.cell_size_fraction,
        cell_height_fraction: region.cell_height_fraction,
        min_region_size: region.min_region_size,
        merge_region_size: region.merge_region_size,
        max_simplification_error: region.max_simplification_error,
        max_vertices_per_polygon: region.max_vertices_per_polygon,
        edge_max_len_factor: region.edge_max_len_factor,
        detail_sample_dist: region.detail_sample_dist,
        detail_sample_max_error: region.detail_sample_max_error,
        tiling: region.tiling,
        tile_size: region.tile_size,
        aabb: None,
        ..default()
    }
}

