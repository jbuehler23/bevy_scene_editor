use bevy::{
    ecs::reflect::AppTypeRegistry,
    prelude::*,
    scene::serde::{SceneDeserializer, SceneSerializer},
    tasks::IoTaskPool,
};
use serde::de::DeserializeSeed;

use crate::{
    commands::{collect_entity_ids, CommandHistory, DespawnEntity, EditorCommand},
    selection::{Selected, Selection},
    EditorEntity,
};

pub struct EntityTemplatesPlugin;

impl Plugin for EntityTemplatesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingTemplateSave>();
    }
}

/// Tracks which entity to save when the template save dialog is confirmed.
#[derive(Resource, Default)]
pub struct PendingTemplateSave {
    pub entity: Option<Entity>,
}

// ---------------------------------------------------------------------------
// Save entity template
// ---------------------------------------------------------------------------

pub fn save_entity_template(world: &mut World, name: &str) {
    let selection = world.resource::<Selection>();
    let Some(primary) = selection.primary() else {
        warn!("No entity selected to save as template");
        return;
    };

    if world.get::<EditorEntity>(primary).is_some() {
        warn!("Cannot save editor entity as template");
        return;
    }

    // Collect entity + descendants
    let mut entities = Vec::new();
    collect_entity_ids(world, primary, &mut entities);

    // Build DynamicScene
    let scene = DynamicSceneBuilder::from_world(world)
        .extract_entities(entities.into_iter())
        .build();

    // Serialize
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    let serializer = SceneSerializer::new(&scene, &registry);
    let json = match serde_json::to_string_pretty(&serializer) {
        Ok(json) => json,
        Err(err) => {
            warn!("Failed to serialize template: {err}");
            return;
        }
    };

    // Ensure templates directory exists and write
    let safe_name = sanitize_filename(name);
    let path = format!("assets/templates/{safe_name}.template.json");

    IoTaskPool::get()
        .spawn(async move {
            if let Err(err) = std::fs::create_dir_all("assets/templates") {
                warn!("Failed to create templates directory: {err}");
                return;
            }
            match std::fs::write(&path, &json) {
                Ok(()) => info!("Template saved to {path}"),
                Err(err) => warn!("Failed to write template file: {err}"),
            }
        })
        .detach();
}

// ---------------------------------------------------------------------------
// Instantiate entity template
// ---------------------------------------------------------------------------

pub fn instantiate_template(world: &mut World, path: &str, position: Vec3) {
    let json = match std::fs::read_to_string(path) {
        Ok(json) => json,
        Err(err) => {
            warn!("Failed to read template file '{path}': {err}");
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
            warn!("Failed to deserialize template: {err}");
            return;
        }
    };

    drop(registry);

    // Write scene to world
    let mut entity_map = Default::default();
    if let Err(err) = scene.write_to_world(world, &mut entity_map) {
        warn!("Failed to instantiate template: {err}");
        return;
    }

    // Find root entities (those without ChildOf pointing to another entity in the map)
    let mapped_entities: std::collections::HashSet<Entity> =
        entity_map.values().copied().collect();
    let mut roots = Vec::new();
    for &new_entity in entity_map.values() {
        let is_child_of_template = world
            .get::<ChildOf>(new_entity)
            .map(|c| mapped_entities.contains(&c.0))
            .unwrap_or(false);
        if !is_child_of_template {
            roots.push(new_entity);
            // Remove any stale ChildOf from the scene write
            world.entity_mut(new_entity).remove::<ChildOf>();
        }
    }

    // Offset root transforms to target position
    for &root in &roots {
        if let Some(mut transform) = world.get_mut::<Transform>(root) {
            transform.translation += position;
        }
    }

    // Build DespawnEntity snapshots for undo
    let mut despawn_cmds: Vec<DespawnEntity> = Vec::new();
    for &root in &roots {
        despawn_cmds.push(DespawnEntity::from_world(world, root));
    }

    // Select new root entities
    let mut selection = world.resource_mut::<Selection>();
    let old_selected = std::mem::take(&mut selection.entities);
    selection.entities = roots.clone();
    drop(selection);

    // Deselect old entities
    for &e in &old_selected {
        if let Ok(mut ec) = world.get_entity_mut(e) {
            ec.remove::<Selected>();
        }
    }

    // Select new roots
    for &root in &roots {
        world.entity_mut(root).insert(Selected);
    }

    // Push undo command
    if !despawn_cmds.is_empty() {
        let cmd = InstantiateEntities {
            snapshots: despawn_cmds,
        };
        let mut history = world.resource_mut::<CommandHistory>();
        history.undo_stack.push(Box::new(cmd));
        history.redo_stack.clear();
    }
}

// ---------------------------------------------------------------------------
// InstantiateEntities — undo command
// ---------------------------------------------------------------------------

pub struct InstantiateEntities {
    pub snapshots: Vec<DespawnEntity>,
}

impl EditorCommand for InstantiateEntities {
    fn execute(&self, world: &mut World) {
        // Redo: respawn from snapshots (DespawnEntity::undo respawns)
        for snapshot in &self.snapshots {
            snapshot.undo(world);
        }
    }

    fn undo(&self, world: &mut World) {
        // Undo: despawn the instantiated entities
        for snapshot in &self.snapshots {
            snapshot.execute(world);
        }
    }

    fn description(&self) -> &str {
        "Instantiate template"
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Sanitize a filename: allow alphanumeric, hyphens, underscores, spaces.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}
