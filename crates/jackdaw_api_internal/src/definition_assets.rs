//! Definition asset types: reflected assets saved one value per file.
//!
//! A definition type claims a directory under the project's assets and a file
//! suffix, for example an item definition at `content/items` with
//! `.item.bsn`. The editor scans each registered directory, keeps the loaded
//! values by name, and shows a file of that suffix in the inspector.

use bevy::prelude::*;

/// A reflected asset type the project stores one value per file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefinitionAssetType {
    /// Short identifier used by the operators and reported by the remote,
    /// for example `item`.
    pub kind: String,
    /// Name shown on menus and tiles, for example `Item`.
    pub label: String,
    /// Reflect type path of the asset type.
    pub type_path: String,
    /// Directory holding the files, relative to the project's assets
    /// directory.
    pub directory: String,
    /// File suffix, including the leading dot. The name is the stem before it.
    pub suffix: String,
    /// Whether the definition scanner loads this directory. A kind whose
    /// files are loaded elsewhere registers with [`Self::loaded_elsewhere`]
    /// and publishes its own entries into the registry.
    pub scanned: bool,
}

impl DefinitionAssetType {
    pub fn new(
        kind: impl Into<String>,
        label: impl Into<String>,
        type_path: impl Into<String>,
        directory: impl Into<String>,
        suffix: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            label: label.into(),
            type_path: type_path.into(),
            directory: directory.into(),
            suffix: suffix.into(),
            scanned: true,
        }
    }

    /// Leave loading to whoever already owns this directory.
    pub fn loaded_elsewhere(mut self) -> Self {
        self.scanned = false;
        self
    }

    /// The name a file path denotes, or `None` when the path is not a file of
    /// this kind.
    pub fn name_of_file(&self, path: &std::path::Path) -> Option<String> {
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(&self.suffix))
            .filter(|stem| !stem.is_empty())
            .map(str::to_owned)
    }

    /// The file name a definition of this name saves to.
    pub fn file_name(&self, name: &str) -> String {
        format!("{name}{}", self.suffix)
    }
}

/// Every definition asset type registered with the editor.
#[derive(Resource, Default)]
pub struct DefinitionAssetTypes {
    types: Vec<DefinitionAssetType>,
}

impl DefinitionAssetTypes {
    /// Register a type, replacing an earlier registration of the same kind.
    pub fn register(&mut self, definition: DefinitionAssetType) {
        self.types.retain(|known| known.kind != definition.kind);
        self.types.push(definition);
    }

    pub fn unregister(&mut self, kind: &str) {
        self.types.retain(|known| known.kind != kind);
    }

    pub fn by_kind(&self, kind: &str) -> Option<&DefinitionAssetType> {
        self.types.iter().find(|known| known.kind == kind)
    }

    pub fn by_type_path(&self, type_path: &str) -> Option<&DefinitionAssetType> {
        self.types.iter().find(|known| known.type_path == type_path)
    }

    /// The type whose suffix this file name ends with, longest suffix first so
    /// `.item.bsn` wins over a plain `.bsn`.
    pub fn for_file(&self, path: &std::path::Path) -> Option<&DefinitionAssetType> {
        let name = path.file_name()?.to_str()?;
        self.types
            .iter()
            .filter(|known| name.ends_with(&known.suffix) && name.len() > known.suffix.len())
            .max_by_key(|known| known.suffix.len())
    }

    pub fn iter(&self) -> impl Iterator<Item = &DefinitionAssetType> {
        self.types.iter()
    }
}

/// Marks an entity as tracking a definition asset type registered by an
/// extension. An observer unregisters the kind when the marker despawns.
#[derive(Component, Clone, Debug)]
pub struct RegisteredDefinitionAsset {
    pub(crate) kind: String,
}

pub(crate) fn cleanup_definition_asset_on_remove(
    trigger: On<Remove, RegisteredDefinitionAsset>,
    registrations: Query<&RegisteredDefinitionAsset>,
    mut types: ResMut<DefinitionAssetTypes>,
) {
    if let Ok(registration) = registrations.get(trigger.event_target()) {
        types.unregister(&registration.kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn a_file_belongs_to_the_type_with_the_longest_matching_suffix() {
        let mut types = DefinitionAssetTypes::default();
        types.register(DefinitionAssetType::new(
            "scene", "Scene", "Scene", "scenes", ".bsn",
        ));
        types.register(DefinitionAssetType::new(
            "item",
            "Item",
            "content::ItemDef",
            "content/items",
            ".item.bsn",
        ));

        let matched = types
            .for_file(Path::new("/p/assets/content/items/torch.item.bsn"))
            .expect("a registered type claims the file");
        assert_eq!(matched.kind, "item");
        assert_eq!(
            matched
                .name_of_file(Path::new("/p/assets/content/items/torch.item.bsn"))
                .as_deref(),
            Some("torch")
        );
    }

    #[test]
    fn a_bare_suffix_is_not_a_definition_file() {
        let mut types = DefinitionAssetTypes::default();
        types.register(DefinitionAssetType::new(
            "item",
            "Item",
            "content::ItemDef",
            "content/items",
            ".item.bsn",
        ));
        assert!(types.for_file(Path::new("/p/assets/.item.bsn")).is_none());
        assert!(types.for_file(Path::new("/p/assets/level.bsn")).is_none());
    }

    #[test]
    fn registering_a_kind_again_replaces_the_earlier_registration() {
        let mut types = DefinitionAssetTypes::default();
        types.register(DefinitionAssetType::new(
            "item",
            "Item",
            "content::ItemDef",
            "items",
            ".item.bsn",
        ));
        types.register(DefinitionAssetType::new(
            "item",
            "Item",
            "content::ItemDef",
            "content/items",
            ".item.bsn",
        ));
        assert_eq!(types.iter().count(), 1);
        assert_eq!(types.by_kind("item").unwrap().directory, "content/items");
    }
}
