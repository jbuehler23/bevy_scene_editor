use bevy::{
    asset::{io::Reader, AssetLoader, LoadContext},
    ecs::{
        reflect::AppTypeRegistry,
        world::{FromWorld, World},
    },
    prelude::*,
    reflect::TypeRegistryArc,
    scene::serde::SceneDeserializer,
};
use serde::de::DeserializeSeed;

use crate::format::JsnScene;

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

        // Deserialize the inner scene value using Bevy's SceneDeserializer
        let registry = self.type_registry.read();
        let scene_deserializer = SceneDeserializer {
            type_registry: &registry,
        };

        let scene = scene_deserializer
            .deserialize(jsn.scene)
            .map_err(|e| JsnLoadError::Scene(e.to_string()))?;

        Ok(scene)
    }

    fn extensions(&self) -> &[&str] {
        &["jsn"]
    }
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
