use bevy::{
    ecs::reflect::AppTypeRegistry,
    prelude::*,
    scene::serde::{SceneDeserializer, SceneSerializer},
    tasks::IoTaskPool,
};
use serde::de::DeserializeSeed;

use crate::EditorEntity;

pub struct SceneIoPlugin;

impl Plugin for SceneIoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SceneFilePath>()
            .add_systems(Update, handle_scene_io_keys);
    }
}

/// Stores the currently active scene file path.
#[derive(Resource, Default)]
pub struct SceneFilePath {
    pub path: Option<String>,
}

// ---------------------------------------------------------------------------
// Save
// ---------------------------------------------------------------------------

pub fn save_scene(world: &mut World) {
    let scene = build_scene_snapshot(world);

    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    let serializer = SceneSerializer::new(&scene, &registry);
    let json = match serde_json::to_string_pretty(&serializer) {
        Ok(json) => json,
        Err(err) => {
            warn!("Failed to serialize scene: {err}");
            return;
        }
    };

    let path = {
        let scene_path = world.resource::<SceneFilePath>();
        scene_path
            .path
            .clone()
            .unwrap_or_else(|| "scene.scene.json".to_string())
    };

    // Save the path back
    world.resource_mut::<SceneFilePath>().path = Some(path.clone());

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
            .unwrap_or_else(|| "scene.scene.json".to_string())
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

    let scene_deserializer = SceneDeserializer {
        type_registry: &registry,
    };

    let mut json_de = serde_json::Deserializer::from_str(&json);
    let scene = match scene_deserializer.deserialize(&mut json_de) {
        Ok(scene) => scene,
        Err(err) => {
            warn!("Failed to deserialize scene: {err}");
            return;
        }
    };

    // Clear existing non-editor entities
    clear_scene_entities(world);

    // Write the loaded scene to the world
    match scene.write_to_world(world, &mut Default::default()) {
        Ok(_) => info!("Scene loaded from {path}"),
        Err(err) => warn!("Failed to write scene to world: {err}"),
    }

    world.resource_mut::<SceneFilePath>().path = Some(path);
}

// ---------------------------------------------------------------------------
// New scene
// ---------------------------------------------------------------------------

fn new_scene(world: &mut World) {
    clear_scene_entities(world);
    world.resource_mut::<SceneFilePath>().path = None;
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
