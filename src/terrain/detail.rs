//! Projecting an editor document's detail layers into what the renderer draws.
//! The store holds the density and the heights; `Terrain` holds the layers.

use bevy::prelude::*;
use jackdaw_scene_types::DetailLayer;
use jackdaw_terrain::GridRect;
use jackdaw_terrain::render::{DetailDirty, DetailSource, DetailTile, TerrainDetailSource};

use super::TerrainDataStore;

/// The layer the brush and the panel act on for one terrain: the selection,
/// clamped to what that terrain carries.
pub(crate) fn selected_detail_layer(
    terrain: &jackdaw_scene_types::Terrain,
    selected: usize,
) -> Option<usize> {
    (!terrain.detail.is_empty()).then(|| selected.min(terrain.detail.len() - 1))
}

/// Mark the ground a rect covers stale. A terrain with no layers carries no
/// [`DetailDirty`] and is left alone.
pub(crate) fn mark_detail_dirty(world: &mut World, entity: Entity, rect: GridRect) {
    if let Some(mut dirty) = world.get_mut::<DetailDirty>(entity) {
        dirty.touch(rect);
    }
}

/// Keep every terrain's detail source following its document and its layers.
/// The whole field is marked stale only for a change no per-cell mark covers.
pub fn sync_terrain_detail(
    mut commands: Commands,
    store: Res<TerrainDataStore>,
    terrains: Query<(
        Entity,
        Ref<jackdaw_scene_types::Terrain>,
        Option<&TerrainDetailSource>,
        Option<&DetailDirty>,
    )>,
) {
    let store_changed = store.is_changed();
    for (entity, terrain, held, marked) in &terrains {
        let carries_layers = !terrain.detail.is_empty();
        if !store_changed && !terrain.is_changed() && held.is_some() == carries_layers {
            continue;
        }

        let projected = store
            .get(&terrain.data_path)
            .and_then(|data| TerrainDetailSource::from_document(data, &terrain));
        let Some(projected) = projected else {
            if held.is_some() {
                commands
                    .entity(entity)
                    .remove::<(TerrainDetailSource, DetailDirty)>();
            }
            continue;
        };

        let reseeds = match held {
            Some(held) => reseeds_field(held.source(), projected.source()),
            None => true,
        };
        // An edit that moved neither the store nor a layer rewrites nothing.
        let layers_changed =
            held.is_none_or(|held| looks_of(held.source()) != looks_of(projected.source()));
        if !store_changed && !layers_changed && !reseeds {
            continue;
        }
        // Merged rather than replaced: an uncaught mark is still owed a reseed.
        let mut mark = marked.copied().unwrap_or_default();
        if reseeds {
            mark.touch(GridRect::whole(projected.source().grid.resolution));
        }
        commands.entity(entity).insert((projected, mark));
    }
}

/// Keep the seeded tiles out of the outliner and out of the saved scene.
/// A tile is grown from the terrain's document rather than saved with it.
pub fn hide_drawn_detail(add: On<Add, DetailTile>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert((crate::EditorHidden, crate::NonSerializable));
}

/// The layers a projection carries, in order.
fn looks_of(source: &DetailSource) -> Vec<&DetailLayer> {
    source.layers.iter().map(|entry| &entry.layer).collect()
}

/// Whether two projections of the same terrain stand every instance somewhere
/// else.
fn reseeds_field(before: &DetailSource, after: &DetailSource) -> bool {
    if before.heightmap.resolution != after.heightmap.resolution || before.grid != after.grid {
        return true;
    }
    if before.layers.len() != after.layers.len() {
        return true;
    }
    before
        .layers
        .iter()
        .zip(&after.layers)
        .any(|(before, after)| {
            before.max != after.max || places_differently(&before.layer, &after.layer)
        })
}

/// Whether two versions of one layer stand instances in different places, or
/// draw them with a different mesh. Every other field costs no reseed.
fn places_differently(before: &DetailLayer, after: &DetailLayer) -> bool {
    before.density_channel != after.density_channel
        || before.density_per_m2 != after.density_per_m2
        || before.height != after.height
        || before.align_to_normal != after.align_to_normal
        || before.mesh != after.mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use jackdaw_scene_types::{DetailMesh, Terrain};
    use jackdaw_terrain::region::{RegionSize, TerrainRegions};
    use jackdaw_terrain::{ChannelDescriptor, ChannelElement, RegionTerrainData};

    fn document() -> RegionTerrainData {
        let mut data = RegionTerrainData {
            channels: vec![ChannelDescriptor::new("grass", ChannelElement::U8)],
            regions: TerrainRegions::new(RegionSize::new(8).unwrap()),
            ..RegionTerrainData::default()
        };
        data.regions.set_channel_count(1);
        // A height of zero writes no cell, so the ground is painted instead.
        for z in 0..8 {
            for x in 0..8 {
                data.regions.set_channel(0, x, z, 128);
            }
        }
        data
    }

    fn terrain_with(layers: Vec<DetailLayer>) -> Terrain {
        Terrain {
            data_path: "t.jdterrain".to_string(),
            detail: layers,
            ..Terrain::default()
        }
    }

    fn terrain() -> Terrain {
        terrain_with(vec![DetailLayer::default()])
    }

    fn source_of(data: &RegionTerrainData) -> TerrainDetailSource {
        TerrainDetailSource::from_document(data, &terrain()).expect("the terrain carries a layer")
    }

    fn source_with(data: &RegionTerrainData, layer: DetailLayer) -> TerrainDetailSource {
        TerrainDetailSource::from_document(data, &terrain_with(vec![layer]))
            .expect("the terrain carries a layer")
    }

    #[test]
    fn a_painted_cell_alone_reseeds_nothing() {
        let before = document();
        let mut after = document();
        after.regions.set_channel(0, 3, 5, 255);
        assert!(!reseeds_field(
            source_of(&before).source(),
            source_of(&after).source()
        ));
    }

    #[test]
    fn changing_where_instances_stand_reseeds_the_field() {
        let data = document();
        let before = source_of(&data);
        let after = source_with(
            &data,
            DetailLayer {
                density_per_m2: 90.0,
                ..DetailLayer::default()
            },
        );
        assert!(reseeds_field(before.source(), after.source()));
    }

    #[test]
    fn only_the_placement_fields_reseed_the_field() {
        let data = document();
        let base = source_of(&data);
        let reseeded_by =
            |layer: DetailLayer| reseeds_field(base.source(), source_with(&data, layer).source());
        let default = DetailLayer::default;

        for (name, layer) in [
            (
                "density_channel",
                DetailLayer {
                    density_channel: "meadow".to_string(),
                    ..default()
                },
            ),
            (
                "height",
                DetailLayer {
                    height: [0.1, 2.0],
                    ..default()
                },
            ),
            (
                "density_per_m2",
                DetailLayer {
                    density_per_m2: 90.0,
                    ..default()
                },
            ),
            (
                "align_to_normal",
                DetailLayer {
                    align_to_normal: true,
                    ..default()
                },
            ),
            (
                "mesh",
                DetailLayer {
                    mesh: DetailMesh::Asset("models/fern.gltf".to_string()),
                    ..default()
                },
            ),
        ] {
            assert!(reseeded_by(layer), "{name} moves instances and must reseed");
        }

        for (name, layer) in [
            (
                "cull_distance",
                DetailLayer {
                    cull_distance: 150.0,
                    ..default()
                },
            ),
            (
                "width",
                DetailLayer {
                    width: [0.1, 0.5],
                    ..default()
                },
            ),
            (
                "color_base",
                DetailLayer {
                    color_base: [1.0, 0.0, 0.0],
                    ..default()
                },
            ),
            (
                "wind_strength",
                DetailLayer {
                    wind_strength: 0.9,
                    ..default()
                },
            ),
            (
                "wind_direction",
                DetailLayer {
                    wind_direction: [-1.0, 0.0],
                    ..default()
                },
            ),
            (
                "bend",
                DetailLayer {
                    bend: 0.9,
                    ..default()
                },
            ),
            (
                "push_strength",
                DetailLayer {
                    push_strength: 2.0,
                    ..default()
                },
            ),
            (
                "name",
                DetailLayer {
                    name: "meadow".to_string(),
                    ..default()
                },
            ),
        ] {
            assert!(
                !reseeded_by(layer),
                "{name} moves no instance and must not reseed"
            );
        }
    }

    #[test]
    fn adding_a_layer_reseeds_the_field() {
        let data = document();
        let before = source_of(&data);
        let after = TerrainDetailSource::from_document(
            &data,
            &terrain_with(vec![DetailLayer::default(), DetailLayer::default()]),
        )
        .expect("the terrain carries two layers");
        assert!(reseeds_field(before.source(), after.source()));
    }

    #[test]
    fn removing_a_middle_layer_reseeds_the_field() {
        let data = document();
        let named = |name: &str| DetailLayer {
            name: name.to_string(),
            ..DetailLayer::default()
        };
        let all = terrain_with(vec![named("a"), named("b"), named("c")]);
        let trimmed = terrain_with(vec![named("a"), named("c")]);
        let before =
            TerrainDetailSource::from_document(&data, &all).expect("the terrain carries layers");
        let after = TerrainDetailSource::from_document(&data, &trimmed)
            .expect("the terrain carries layers");

        // A tile names the layer it grew from by index, so the layers past the
        // gap draw the wrong instances until the field is seeded again.
        assert!(reseeds_field(before.source(), after.source()));
    }

    #[test]
    fn changing_only_the_look_reseeds_nothing() {
        let data = document();
        let before = source_of(&data);
        let after = source_with(
            &data,
            DetailLayer {
                color_tip: [1.0, 0.0, 0.0],
                width: [0.1, 0.5],
                ..DetailLayer::default()
            },
        );

        assert!(!reseeds_field(before.source(), after.source()));
        assert_ne!(looks_of(before.source()), looks_of(after.source()));
    }

    #[test]
    fn an_unchanged_projection_reseeds_nothing() {
        let data = document();
        assert!(!reseeds_field(
            source_of(&data).source(),
            source_of(&data).source()
        ));
    }
}
