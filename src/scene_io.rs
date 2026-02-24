use bevy::{
    ecs::reflect::AppTypeRegistry,
    prelude::*,
    scene::serde::{SceneDeserializer, SceneSerializer},
    tasks::IoTaskPool,
};
use bevy_jsn::format::{JsnAssets, JsnHeader, JsnMetadata, JsnScene};
use serde::de::DeserializeSeed;

use crate::EditorEntity;

pub struct SceneIoPlugin;

impl Plugin for SceneIoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SceneFilePath>()
            .add_systems(Update, handle_scene_io_keys);
    }
}

/// Stores the currently active scene file path and metadata.
#[derive(Resource, Default)]
pub struct SceneFilePath {
    pub path: Option<String>,
    pub metadata: JsnMetadata,
}

// ---------------------------------------------------------------------------
// Save
// ---------------------------------------------------------------------------

pub fn save_scene(world: &mut World) {
    let scene = build_scene_snapshot(world);

    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    // Serialize the DynamicScene to a JSON value
    let serializer = SceneSerializer::new(&scene, &registry);
    let scene_value = match serde_json::to_value(&serializer) {
        Ok(v) => v,
        Err(err) => {
            warn!("Failed to serialize scene: {err}");
            return;
        }
    };

    // Build asset manifest by scanning brush textures and GLTF sources
    let assets = build_asset_manifest(world);

    // Build metadata
    let now = chrono_now();
    let scene_path_res = world.resource::<SceneFilePath>();
    let mut metadata = scene_path_res.metadata.clone();
    metadata.modified = now.clone();
    if metadata.created.is_empty() {
        metadata.created = now;
    }
    if metadata.name.is_empty() {
        metadata.name = "Untitled".to_string();
    }

    let jsn = JsnScene {
        jsn: JsnHeader::default(),
        metadata: metadata.clone(),
        assets,
        editor: None,
        scene: scene_value,
    };

    let json = match serde_json::to_string_pretty(&jsn) {
        Ok(json) => json,
        Err(err) => {
            warn!("Failed to serialize JSN: {err}");
            return;
        }
    };

    let path = {
        let scene_path = world.resource::<SceneFilePath>();
        scene_path
            .path
            .clone()
            .unwrap_or_else(|| "scene.jsn".to_string())
    };

    // Save path and metadata back
    let mut scene_path = world.resource_mut::<SceneFilePath>();
    scene_path.path = Some(path.clone());
    scene_path.metadata = metadata;

    // Write to disk on the IO task pool
    let path_clone = path.clone();
    IoTaskPool::get()
        .spawn(async move {
            match std::fs::write(&path_clone, &json) {
                Ok(()) => info!("Scene saved to {path_clone}"),
                Err(err) => warn!("Failed to write scene file: {err}"),
            }
        })
        .detach();
}

// ---------------------------------------------------------------------------
// Load
// ---------------------------------------------------------------------------

pub fn load_scene(world: &mut World) {
    let path = {
        let scene_path = world.resource::<SceneFilePath>();
        scene_path
            .path
            .clone()
            .unwrap_or_else(|| "scene.jsn".to_string())
    };

    let json = match std::fs::read_to_string(&path) {
        Ok(json) => json,
        Err(err) => {
            warn!("Failed to read scene file '{path}': {err}");
            return;
        }
    };

    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    // Detect format: try JSN first, fall back to raw DynamicScene JSON
    if path.ends_with(".scene.json") {
        // Legacy format: raw DynamicScene JSON
        let scene_deserializer = SceneDeserializer {
            type_registry: &registry,
        };
        let mut json_de = serde_json::Deserializer::from_str(&json);
        let scene = match scene_deserializer.deserialize(&mut json_de) {
            Ok(scene) => scene,
            Err(err) => {
                warn!("Failed to deserialize legacy scene: {err}");
                return;
            }
        };

        clear_scene_entities(world);
        match scene.write_to_world(world, &mut Default::default()) {
            Ok(_) => info!("Scene loaded from {path} (legacy format)"),
            Err(err) => warn!("Failed to write scene to world: {err}"),
        }
    } else {
        // JSN format
        let jsn: JsnScene = match serde_json::from_str(&json) {
            Ok(jsn) => jsn,
            Err(err) => {
                warn!("Failed to parse JSN file: {err}");
                return;
            }
        };

        let scene_deserializer = SceneDeserializer {
            type_registry: &registry,
        };
        let scene = match scene_deserializer.deserialize(jsn.scene) {
            Ok(scene) => scene,
            Err(err) => {
                warn!("Failed to deserialize scene data: {err}");
                return;
            }
        };

        clear_scene_entities(world);
        match scene.write_to_world(world, &mut Default::default()) {
            Ok(_) => info!("Scene loaded from {path}"),
            Err(err) => warn!("Failed to write scene to world: {err}"),
        }

        // Restore metadata
        let mut scene_path = world.resource_mut::<SceneFilePath>();
        scene_path.metadata = jsn.metadata;
    }

    world.resource_mut::<SceneFilePath>().path = Some(path);
}

// ---------------------------------------------------------------------------
// New scene
// ---------------------------------------------------------------------------

fn new_scene(world: &mut World) {
    clear_scene_entities(world);
    let mut scene_path = world.resource_mut::<SceneFilePath>();
    scene_path.path = None;
    scene_path.metadata = JsnMetadata::default();
    info!("New scene created");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a DynamicScene from all non-editor entities.
fn build_scene_snapshot(world: &mut World) -> DynamicScene {
    let entities: Vec<Entity> = world
        .query_filtered::<Entity, Without<EditorEntity>>()
        .iter(world)
        .collect();

    DynamicSceneBuilder::from_world(world)
        .extract_entities(entities.into_iter())
        .build()
}

/// Remove all non-editor entities from the world.
fn clear_scene_entities(world: &mut World) {
    let entities: Vec<Entity> = world
        .query_filtered::<Entity, Without<EditorEntity>>()
        .iter(world)
        .collect();

    for entity in entities {
        if let Ok(entity_mut) = world.get_entity_mut(entity) {
            entity_mut.despawn();
        }
    }
}

/// Build an asset manifest by scanning entity components.
fn build_asset_manifest(world: &mut World) -> JsnAssets {
    let mut textures = Vec::new();
    let mut models = Vec::new();

    // Scan brush face textures
    let mut brush_query = world.query::<&bevy_jsn::Brush>();
    for brush in brush_query.iter(world) {
        for face in &brush.faces {
            if let Some(ref path) = face.texture_path {
                if !textures.contains(path) {
                    textures.push(path.clone());
                }
            }
        }
    }

    // Scan GLTF sources
    let mut gltf_query = world.query::<&bevy_jsn::GltfSource>();
    for source in gltf_query.iter(world) {
        if !models.contains(&source.path) {
            models.push(source.path.clone());
        }
    }

    textures.sort();
    models.sort();

    JsnAssets { textures, models }
}

/// ISO 8601 timestamp (simplified — no chrono dependency).
fn chrono_now() -> String {
    // Use std::time for a basic timestamp
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = since_epoch.as_secs();
    // Basic ISO 8601 approximation
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    // Days since 1970-01-01, approximate year/month/day
    let (year, month, day) = days_to_date(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn days_to_date(days: u64) -> (u64, u64, u64) {
    // Simplified calendar calculation
    let mut y = 1970;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let month_days = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md {
            m = i;
            break;
        }
        remaining -= md;
    }
    (y, m as u64 + 1, remaining + 1)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

// ---------------------------------------------------------------------------
// Keyboard shortcuts
// ---------------------------------------------------------------------------

fn handle_scene_io_keys(world: &mut World) {
    let keyboard = world.resource::<ButtonInput<KeyCode>>();
    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let s_pressed = keyboard.just_pressed(KeyCode::KeyS);
    let o_pressed = keyboard.just_pressed(KeyCode::KeyO);
    let n_pressed = keyboard.just_pressed(KeyCode::KeyN);

    if ctrl && s_pressed {
        save_scene(world);
    } else if ctrl && o_pressed {
        load_scene(world);
    } else if ctrl && shift && n_pressed {
        new_scene(world);
    }
}
