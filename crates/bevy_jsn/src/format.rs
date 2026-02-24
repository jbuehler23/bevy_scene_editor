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
    /// The Bevy DynamicScene data, serialized as raw JSON.
    pub scene: serde_json::Value,
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
