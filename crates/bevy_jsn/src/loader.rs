use bevy::{
    asset::{io::Reader, AssetLoader, LoadContext},
    ecs::{
        reflect::AppTypeRegistry,
        world::{FromWorld, World},
    },
    prelude::*,
    reflect::{
        serde::TypedReflectDeserializer,
        TypeRegistryArc,
    },
    scene::DynamicScene,
};
use serde::de::DeserializeSeed;

use crate::format::{JsnEntity, JsnScene};

/// Asset loader for `.jsn` files → `DynamicScene`.
#[derive(Debug, TypePath)]
pub struct JsnAssetLoader {
    type_registry: TypeRegistryArc,
}

impl FromWorld for JsnAssetLoader {
    fn from_world(world: &mut World) -> Self {
        let type_registry = world.resource::<AppTypeRegistry>();
        Self {
            type_registry: type_registry.0.clone(),
        }
    }
}

impl AssetLoader for JsnAssetLoader {
    type Asset = DynamicScene;
    type Settings = ();
    type Error = JsnLoadError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| JsnLoadError::Io(e.to_string()))?;

        let text = std::str::from_utf8(&bytes)
            .map_err(|e| JsnLoadError::Parse(e.to_string()))?;

        let jsn: JsnScene = serde_json::from_str(text)
            .map_err(|e| JsnLoadError::Parse(e.to_string()))?;

        // Build a DynamicScene by spawning into a temporary world
        let scene = build_dynamic_scene(&jsn.scene, &self.type_registry)
            .map_err(|e| JsnLoadError::Scene(e))?;

        Ok(scene)
    }

    fn extensions(&self) -> &[&str] {
        &["jsn"]
    }
}

/// Spawn JsnEntity list into a temp world, then extract a DynamicScene.
fn build_dynamic_scene(
    entities: &[JsnEntity],
    type_registry: &TypeRegistryArc,
) -> Result<DynamicScene, String> {
    let mut world = World::new();

    // First pass: spawn entities with core fields
    let mut spawned: Vec<Entity> = Vec::new();
    for jsn in entities {
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
    for (i, jsn) in entities.iter().enumerate() {
        if let Some(parent_idx) = jsn.parent {
            if let Some(&parent_entity) = spawned.get(parent_idx) {
                world.entity_mut(spawned[i]).insert(ChildOf(parent_entity));
            }
        }
    }

    // Third pass: deserialize extensible components via reflection
    let registry = type_registry.read();
    for (i, jsn) in entities.iter().enumerate() {
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
    drop(registry);

    // Extract all spawned entities into a DynamicScene
    let scene = DynamicSceneBuilder::from_world(&world)
        .extract_entities(spawned.into_iter())
        .build();

    Ok(scene)
}

#[derive(Debug, thiserror::Error)]
pub enum JsnLoadError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Scene deserialization error: {0}")]
    Scene(String),
}
