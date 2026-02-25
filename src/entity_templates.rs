use std::any::TypeId;
use std::collections::{HashMap, HashSet};

use bevy::{
    asset::AssetPath,
    ecs::reflect::AppTypeRegistry,
    prelude::*,
    reflect::serde::{TypedReflectDeserializer, TypedReflectSerializer},
    tasks::IoTaskPool,
};
use bevy_jsn::format::JsnEntity;
use serde::de::DeserializeSeed;

use crate::{
    commands::{collect_entity_ids, CommandHistory, DespawnEntity, EditorCommand},
    scene_io::should_skip_component,
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

    // Build entity → index map for parent references
    let index_map: HashMap<Entity, usize> = entities
        .iter()
        .enumerate()
        .map(|(i, &e)| (e, i))
        .collect();

    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    // Component types handled as explicit fields
    let skip_ids: HashSet<TypeId> = HashSet::from([
        TypeId::of::<Name>(),
        TypeId::of::<Transform>(),
        TypeId::of::<GlobalTransform>(),
        TypeId::of::<Visibility>(),
        TypeId::of::<InheritedVisibility>(),
        TypeId::of::<ViewVisibility>(),
        TypeId::of::<ChildOf>(),
        TypeId::of::<Children>(),
    ]);

    let jsn_entities: Vec<JsnEntity> = entities
        .iter()
        .map(|&entity| {
            let entity_ref = world.entity(entity);

            let name = entity_ref.get::<Name>().map(|n| n.to_string());
            let transform = entity_ref.get::<Transform>().map(|t| (*t).into());
            let visibility = entity_ref
                .get::<Visibility>()
                .map(|v| (*v).into())
                .unwrap_or_default();
            let parent = entity_ref
                .get::<ChildOf>()
                .and_then(|c| index_map.get(&c.parent()).copied());

            let mut components = HashMap::new();
            for registration in registry.iter() {
                if skip_ids.contains(&registration.type_id()) {
                    continue;
                }
                let type_path = registration.type_info().type_path_table().path();
                if should_skip_component(type_path) {
                    continue;
                }
                let Some(reflect_component) = registration.data::<ReflectComponent>() else {
                    continue;
                };
                let Some(component) = reflect_component.reflect(entity_ref) else {
                    continue;
                };
                let serializer = TypedReflectSerializer::new(component, &registry);
                if let Ok(value) = serde_json::to_value(&serializer) {
                    components.insert(type_path.to_string(), value);
                }
            }

            JsnEntity {
                name,
                transform,
                visibility,
                parent,
                components,
            }
        })
        .collect();

    let json = match serde_json::to_string_pretty(&jsn_entities) {
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

    let jsn_entities: Vec<JsnEntity> = match serde_json::from_str(&json) {
        Ok(v) => v,
        Err(err) => {
            warn!("Failed to parse template file: {err}");
            return;
        }
    };

    let registry = world.resource::<AppTypeRegistry>().clone();

    // First pass: spawn entities with core fields
    let mut spawned: Vec<Entity> = Vec::new();
    for jsn in &jsn_entities {
        let mut entity = world.spawn_empty();
        if let Some(name) = &jsn.name {
            entity.insert(Name::new(name.clone()));
        }
        if let Some(t) = &jsn.transform {
            entity.insert(Transform::from(t.clone()));
        }
        let vis: Visibility = jsn.visibility.clone().into();
        entity.insert(vis);
        spawned.push(entity.id());
    }

    // Second pass: set parents (ChildOf)
    for (i, jsn) in jsn_entities.iter().enumerate() {
        if let Some(parent_idx) = jsn.parent {
            if let Some(&parent_entity) = spawned.get(parent_idx) {
                world.entity_mut(spawned[i]).insert(ChildOf(parent_entity));
            }
        }
    }

    // Third pass: deserialize extensible components via reflection
    {
        let registry = registry.read();
        for (i, jsn) in jsn_entities.iter().enumerate() {
            for (type_path, value) in &jsn.components {
                let Some(registration) = registry.get_with_type_path(type_path) else {
                    warn!("Unknown type '{type_path}' — skipping");
                    continue;
                };
                let Some(reflect_component) = registration.data::<ReflectComponent>() else {
                    continue;
                };
                let deserializer = TypedReflectDeserializer::new(registration, &registry);
                let Ok(reflected) = deserializer.deserialize(value) else {
                    warn!("Failed to deserialize '{type_path}' — skipping");
                    continue;
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    reflect_component.insert(
                        &mut world.entity_mut(spawned[i]),
                        reflected.as_ref(),
                        &registry,
                    );
                }));
                if result.is_err() {
                    warn!("Panic while inserting component '{type_path}' — skipping");
                }
            }
        }
    }

    // Post-load: re-trigger GLTF loading for GltfSource entities
    let gltf_entities: Vec<(Entity, String, usize)> = spawned
        .iter()
        .filter_map(|&e| {
            world
                .get::<bevy_jsn::GltfSource>(e)
                .map(|gs| (e, gs.path.clone(), gs.scene_index))
        })
        .collect();
    for (entity, gltf_path, scene_index) in gltf_entities {
        let asset_server = world.resource::<AssetServer>();
        let asset_path: AssetPath<'static> = gltf_path.into();
        let scene = asset_server.load(GltfAssetLabel::Scene(scene_index).from_asset(asset_path));
        world.entity_mut(entity).insert(SceneRoot(scene));
    }

    // Find root entities (those without a parent in the template)
    let mut roots = Vec::new();
    for (i, jsn) in jsn_entities.iter().enumerate() {
        if jsn.parent.is_none() {
            roots.push(spawned[i]);
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
