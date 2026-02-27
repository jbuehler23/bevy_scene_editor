use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Top-level `.jsn` file structure.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JsnScene {
    /// Format header with version info.
    pub jsn: JsnHeader,
    /// Scene metadata (name, author, timestamps).
    pub metadata: JsnMetadata,
    /// Asset manifest — lists referenced asset paths.
    pub assets: JsnAssets,
    /// Reserved for future editor state (camera bookmarks, snap settings, etc.).
    pub editor: Option<JsnEditorState>,
    /// Per-entity scene data with reflection-based components.
    pub scene: Vec<JsnEntity>,
}

// ---------------------------------------------------------------------------
// Per-entity scene types
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JsnTransform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl From<Transform> for JsnTransform {
    fn from(t: Transform) -> Self {
        Self {
            translation: t.translation,
            rotation: t.rotation,
            scale: t.scale,
        }
    }
}

impl From<JsnTransform> for Transform {
    fn from(t: JsnTransform) -> Self {
        Self {
            translation: t.translation,
            rotation: t.rotation,
            scale: t.scale,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JsnVisibility {
    #[default]
    Inherited,
    Visible,
    Hidden,
}

impl JsnVisibility {
    pub fn is_default(&self) -> bool {
        *self == Self::Inherited
    }
}

impl From<Visibility> for JsnVisibility {
    fn from(v: Visibility) -> Self {
        match v {
            Visibility::Inherited => Self::Inherited,
            Visibility::Visible => Self::Visible,
            Visibility::Hidden => Self::Hidden,
        }
    }
}

impl From<JsnVisibility> for Visibility {
    fn from(v: JsnVisibility) -> Self {
        match v {
            JsnVisibility::Inherited => Self::Inherited,
            JsnVisibility::Visible => Self::Visible,
            JsnVisibility::Hidden => Self::Hidden,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JsnEntity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<JsnTransform>,
    #[serde(default, skip_serializing_if = "JsnVisibility::is_default")]
    pub visibility: JsnVisibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub components: HashMap<String, serde_json::Value>,
}

/// Format version and tool info.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JsnHeader {
    /// Semantic version triple `[major, minor, patch]`.
    pub format_version: [u32; 3],
    /// Version of the editor that wrote this file.
    pub editor_version: String,
    /// Bevy version used.
    pub bevy_version: String,
}

impl Default for JsnHeader {
    fn default() -> Self {
        Self {
            format_version: [1, 0, 0],
            editor_version: env!("CARGO_PKG_VERSION").to_string(),
            bevy_version: "0.18".to_string(),
        }
    }
}

/// Human-readable scene metadata.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct JsnMetadata {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub modified: String,
}

/// Asset manifest — lists files referenced by the scene.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct JsnAssets {
    #[serde(default)]
    pub textures: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

/// Reserved for editor-specific state. Currently unused.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct JsnEditorState {}
