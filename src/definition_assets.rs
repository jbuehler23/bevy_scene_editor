//! Definition assets: one reflected value per file, anywhere under the
//! project's assets.
//!
//! A file says which type it holds, so the editor finds a kind's files by
//! reading them rather than by where they sit. A type the editor has compiled
//! in loads through [`jackdaw_bsn::load_bsn_assets`] and saves through
//! [`jackdaw_bsn::serialize_assets_to_bsn`]; a type the open project reported
//! in its schema is known no other way, so its files load into
//! [`DefinitionValues`] as the patch they hold and save back through the same
//! emitter. Either way a file holds only what the value changes from its
//! default.
//!
//! Opening a definition puts it in the inspector: the open definition rides on
//! an editor entity carrying [`DefinitionAssetEdit`], the field rows read the
//! value it names instead of a component, and their edits come back here as
//! [`SetDefinitionField`] undo entries.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};

use bevy::asset::{ReflectAsset, UntypedHandle};
use bevy::prelude::*;
use bevy::reflect::{GetPath, ReflectRef, prelude::ReflectDefault};
use jackdaw_api::prelude::{AssetKind, AssetKinds};
use jackdaw_api_internal::operator::report_to_caller;
use jackdaw_bsn::{BsnPatch, BsnPatches, BsnStructData, BsnStructFields, CatalogAssetRef};
use jackdaw_commands::CommandHistory;

use crate::EditorEntity;
use crate::asset_files::{AssetFileKind, asset_file_text, read_asset_kind};
use crate::commands::EditorCommand;
use crate::prelude::*;
use crate::project::ProjectRoot;

/// Where a definition's value lives: in its type's asset store, or, for a type
/// the editor knows only as the project's schema, in [`DefinitionValues`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DefinitionValue {
    Asset(UntypedHandle),
    Schema { kind: String, name: String },
}

impl DefinitionValue {
    /// The handle behind a definition whose type the editor has compiled in.
    pub fn handle(&self) -> Option<&UntypedHandle> {
        match self {
            Self::Asset(handle) => Some(handle),
            Self::Schema { .. } => None,
        }
    }

    fn schema_key(&self) -> Option<(&str, &str)> {
        match self {
            Self::Asset(_) => None,
            Self::Schema { kind, name } => Some((kind, name)),
        }
    }
}

/// Every value whose type the editor knows only as schema, held as the BSN
/// patch its file spells, by the kind and name of the definition holding it.
#[derive(Resource, Default)]
pub struct DefinitionValues {
    values: std::collections::HashMap<(String, String), BsnStructData>,
}

impl DefinitionValues {
    pub fn get(&self, kind: &str, name: &str) -> Option<&BsnStructData> {
        self.values.get(&(kind.to_string(), name.to_string()))
    }

    pub fn insert(&mut self, kind: &str, name: &str, data: BsnStructData) {
        self.values
            .insert((kind.to_string(), name.to_string()), data);
    }

    pub fn remove(&mut self, kind: &str, name: &str) {
        self.values.remove(&(kind.to_string(), name.to_string()));
    }
}

/// A definition loaded from a file, by the name its file stem gives it.
pub struct DefinitionEntry {
    pub kind: String,
    pub name: String,
    pub value: DefinitionValue,
    pub path: PathBuf,
}

/// Every loaded definition, across all registered types.
#[derive(Resource, Default)]
pub struct DefinitionRegistry {
    pub entries: Vec<DefinitionEntry>,
}

impl DefinitionRegistry {
    pub fn get(&self, kind: &str, name: &str) -> Option<&DefinitionEntry> {
        self.entries
            .iter()
            .find(|entry| entry.kind == kind && entry.name == name)
    }

    pub fn names_of(&self, kind: &str) -> Vec<String> {
        let mut names: Vec<String> = self
            .entries
            .iter()
            .filter(|entry| entry.kind == kind)
            .map(|entry| entry.name.clone())
            .collect();
        names.sort();
        names
    }

    /// Record a loaded definition, replacing any entry of the same kind and
    /// name.
    pub fn insert(&mut self, entry: DefinitionEntry) {
        self.entries
            .retain(|known| known.kind != entry.kind || known.name != entry.name);
        self.entries.push(entry);
    }

    pub fn remove(&mut self, kind: &str, name: &str) {
        self.entries
            .retain(|known| known.kind != kind || known.name != name);
    }

    /// The definition loaded from a file, whichever kind claims it.
    pub fn by_path(&self, path: &Path) -> Option<&DefinitionEntry> {
        self.entries.iter().find(|entry| entry.path == path)
    }
}

/// The first `<kind>_N` name with neither an entry nor a file of its own in
/// the folder the new file lands in.
fn next_free_name(world: &World, kind: &str, dir: &Path) -> String {
    let registry = world.resource::<DefinitionRegistry>();
    let mut index = 1u32;
    loop {
        let candidate = format!("{kind}_{index}");
        let taken = registry.get(kind, &candidate).is_some()
            || definition_file_path(dir, &candidate).exists();
        if !taken {
            return candidate;
        }
        index += 1;
    }
}

/// The definition open in the inspector, on its own editor entity.
#[derive(Component)]
#[require(EditorEntity)]
pub struct DefinitionAssetEdit {
    pub kind: String,
    pub name: String,
    pub type_path: String,
    pub value: DefinitionValue,
    pub path: PathBuf,
    /// Whether the definition has been edited since it was loaded or saved.
    pub dirty: bool,
}

/// The entity carrying the open definition, if one is open.
#[derive(Resource, Default)]
pub struct OpenDefinition(pub Option<Entity>);

/// Strip what cannot appear in a file stem, so a definition name always maps
/// to exactly one file.
pub fn sanitize_definition_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "definition".to_string()
    } else {
        cleaned
    }
}

/// Whether a path names a file to write rather than the folder to write in.
fn names_a_file(path: &Path) -> bool {
    !path.is_dir()
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("bsn"))
}

/// The file a definition of this name lands in, in a folder of the user's
/// choosing.
pub fn definition_file_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{}.bsn", sanitize_definition_name(name)))
}

/// The name a file gives the definition it holds: everything before the first
/// dot of its file name, so `torch.item.bsn` holds `torch`.
pub fn definition_name_of(path: &Path) -> String {
    let file = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .trim_start_matches('.');
    file.split('.').next().unwrap_or(file).to_string()
}

/// The folder a new definition lands in: the one the browser is showing when
/// it is under the project's assets, and the assets directory otherwise.
pub fn new_definition_dir(world: &World) -> Option<PathBuf> {
    let assets = world.get_resource::<ProjectRoot>()?.assets_dir();
    let showing = world
        .get_resource::<crate::asset_browser::AssetBrowserState>()
        .map(|state| state.current_directory.clone());
    match showing {
        Some(dir) if dir.starts_with(&assets) && dir.is_dir() => Some(dir),
        _ => Some(assets),
    }
}

/// Load one file into wherever its type's values live. A file that cannot be
/// read or parsed, or that holds a value of another type, is reported and
/// skipped.
pub fn load_definition_file(
    world: &mut World,
    definition: &AssetKind,
    path: &Path,
) -> Option<DefinitionValue> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            warn!("Failed to read {}: {err}", path.display());
            return None;
        }
    };
    if definition.schema_backed() {
        return load_schema_definition(world, definition, path, &text);
    }
    let type_id = registered_type_id(world, &definition.type_path)?;
    let entries = match jackdaw_bsn::load_bsn_assets(world, &text) {
        Ok(entries) => entries,
        Err(err) => {
            warn!("Failed to parse {}: {err}", path.display());
            return None;
        }
    };
    let entry = entries.into_iter().next()?;
    if entry.handle.type_id() != type_id {
        warn!(
            "{} does not hold a {}",
            path.display(),
            definition.type_path
        );
        return None;
    }
    Some(DefinitionValue::Asset(entry.handle))
}

/// Load a file whose type the editor knows only as schema: the patch it holds
/// goes into [`DefinitionValues`] as it stands.
fn load_schema_definition(
    world: &mut World,
    definition: &AssetKind,
    path: &Path,
    text: &str,
) -> Option<DefinitionValue> {
    let name = definition_name_of(path);
    let ast = match jackdaw_bsn::parse_bsn_text(text) {
        Ok(ast) => ast,
        Err(err) => {
            warn!("Failed to parse {}: {err}", path.display());
            return None;
        }
    };
    let data = ast
        .roots
        .iter()
        .find_map(|&root| patch_of_root(&ast, root))
        .unwrap_or_else(|| empty_patch(&definition.type_path));
    if data.type_path != definition.type_path {
        warn!(
            "{} holds a {} where a {} was expected",
            path.display(),
            data.type_path,
            definition.type_path
        );
        return None;
    }
    world
        .get_resource_or_init::<DefinitionValues>()
        .insert(&definition.kind, &name, data);
    Some(DefinitionValue::Schema {
        kind: definition.kind.clone(),
        name,
    })
}

/// The struct patch a document root carries, with a bare type reading as a
/// value that authors nothing.
fn patch_of_root(ast: &jackdaw_bsn::SceneBsnAst, root: Entity) -> Option<BsnStructData> {
    let patches = ast.get_patches(root)?;
    patches
        .0
        .iter()
        .find_map(|&patch| match ast.get_patch(patch) {
            Some(BsnPatch::Struct(data)) => Some(data.clone()),
            Some(BsnPatch::Type(type_path)) => Some(empty_patch(type_path)),
            _ => None,
        })
}

fn empty_patch(type_path: &str) -> BsnStructData {
    BsnStructData {
        type_path: type_path.to_string(),
        fields: BsnStructFields::default(),
    }
}

/// The name the root of an existing file carries, so a save writes the file
/// back under the name it already spells.
fn root_name_of(text: &str) -> Option<String> {
    let ast = jackdaw_bsn::parse_bsn_text(text).ok()?;
    ast.roots.iter().find_map(|&root| {
        ast.get_patches(root)?
            .0
            .iter()
            .find_map(|&patch| match ast.get_patch(patch) {
                Some(BsnPatch::Name(name)) => Some(name.clone()),
                _ => None,
            })
    })
}

/// Write one definition to the file it was opened from or created in, under
/// the root name that file already carries. An identical rewrite is skipped so
/// the asset watcher does not reload behind an unchanged save.
pub fn write_definition_file(
    world: &World,
    name: &str,
    value: &DefinitionValue,
    path: &Path,
) -> std::io::Result<PathBuf> {
    let existing = std::fs::read_to_string(path).ok();
    let name = existing
        .as_deref()
        .and_then(root_name_of)
        .unwrap_or_else(|| name.to_string());
    let text = definition_text(world, &name, value).unwrap_or_default();
    if text.trim().is_empty() {
        return Err(std::io::Error::other(format!(
            "nothing to write for '{name}'"
        )));
    }
    if existing.is_some_and(|existing| existing == text) {
        return Ok(path.to_path_buf());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::scene_io::save::write_atomic(path, text.as_bytes())?;
    Ok(path.to_path_buf())
}

/// The text one definition saves as: the stamp and the header naming its
/// type, then an asset catalog entry for a compiled type or the patch it holds
/// for a schema-backed one.
fn definition_text(world: &World, name: &str, value: &DefinitionValue) -> Option<String> {
    let body = match value {
        DefinitionValue::Asset(handle) => jackdaw_bsn::serialize_assets_to_bsn(
            world,
            &[CatalogAssetRef {
                name: sanitize_definition_name(name),
                type_id: handle.type_id(),
                asset_id: handle.id(),
            }],
        ),
        DefinitionValue::Schema { kind, name: stored } => {
            let data = world
                .get_resource::<DefinitionValues>()?
                .get(kind, stored)?;
            emit_definition_patch(&sanitize_definition_name(name), data.clone())
        }
    };
    if body.trim().is_empty() {
        return None;
    }
    Some(asset_file_text(&definition_type_path(world, value)?, &body))
}

/// The type a definition's value holds, as its files name it.
fn definition_type_path(world: &World, value: &DefinitionValue) -> Option<String> {
    match value {
        DefinitionValue::Asset(handle) => {
            let registry = world.resource::<AppTypeRegistry>().read();
            Some(
                registry
                    .get(handle.type_id())?
                    .type_info()
                    .type_path()
                    .to_string(),
            )
        }
        DefinitionValue::Schema { kind, name } => Some(
            world
                .get_resource::<DefinitionValues>()?
                .get(kind, name)?
                .type_path
                .clone(),
        ),
    }
}

/// Emit one named patch as a document of its own.
fn emit_definition_patch(name: &str, data: BsnStructData) -> String {
    let mut ast = jackdaw_bsn::SceneBsnAst::default();
    let name_patch = ast.world.spawn(BsnPatch::Name(name.to_string())).id();
    let type_patch = ast.world.spawn(BsnPatch::Struct(data)).id();
    let root = ast
        .world
        .spawn(BsnPatches(vec![name_patch, type_patch]))
        .id();
    ast.add_to_roots(root);
    jackdaw_bsn::emit_scene(&ast)
}

/// Put a fresh default value where this kind's values live.
pub fn default_definition_value(
    world: &mut World,
    definition: &AssetKind,
    name: &str,
) -> Option<DefinitionValue> {
    if definition.schema_backed() {
        world.get_resource_or_init::<DefinitionValues>().insert(
            &definition.kind,
            name,
            empty_patch(&definition.type_path),
        );
        return Some(DefinitionValue::Schema {
            kind: definition.kind.clone(),
            name: name.to_string(),
        });
    }
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let registration = registry.get_with_type_path(&definition.type_path)?;
    let reflect_asset = registration.data::<ReflectAsset>()?;
    let value = registration.data::<ReflectDefault>()?.default();
    Some(DefinitionValue::Asset(
        reflect_asset.add(world, value.as_partial_reflect()),
    ))
}

fn registered_type_id(world: &World, type_path: &str) -> Option<std::any::TypeId> {
    let registry = world.resource::<AppTypeRegistry>().read();
    Some(registry.get_with_type_path(type_path)?.type_id())
}

fn registered_types(world: &World) -> Vec<AssetKind> {
    world
        .get_resource::<AssetKinds>()
        .map(|types| types.iter().cloned().collect())
        .unwrap_or_default()
}

/// What a rescan of the project's asset files found.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct DefinitionRescan {
    /// `(kind, name)` of files that appeared since the last scan.
    pub added: Vec<(String, String)>,
    /// `(kind, name)` of entries whose file has gone.
    pub removed: Vec<(String, String)>,
}

/// Every definition file the project holds, as `(kind, name, path)`, read from
/// what each file says it is. A name a file of the same kind already took is
/// reported and left out, so one name means one file.
fn scan_definition_files(
    world: &World,
    cache: &mut crate::asset_files::AssetKindCache,
) -> Vec<(String, String, PathBuf)> {
    let Some(assets) = world
        .get_resource::<ProjectRoot>()
        .map(ProjectRoot::assets_dir)
    else {
        return Vec::new();
    };
    let Some(kinds) = world.get_resource::<AssetKinds>() else {
        return Vec::new();
    };
    let mut found: Vec<(String, String, PathBuf)> = Vec::new();
    for path in crate::asset_files::walk_document_files(&assets) {
        let AssetFileKind::Asset { type_path } = cache.check(&path, kinds) else {
            continue;
        };
        let Some(definition) = kinds.by_type_path(&type_path).filter(|kind| kind.scanned()) else {
            continue;
        };
        let name = definition_name_of(&path);
        if found
            .iter()
            .any(|(kind, known, _)| kind == &definition.kind && known == &name)
        {
            warn!(
                "two {} files are called '{name}'; {} stays unopened",
                definition.kind,
                path.display()
            );
            continue;
        }
        found.push((definition.kind.clone(), name, path));
    }
    found
}

/// Scan the project's assets: files that appeared are loaded, entries whose
/// file has gone are dropped.
///
/// A file changed in place is not reloaded, and a definition open in the
/// inspector keeps its value when its file disappears: the loaded value is
/// what the editor is editing, and its Save writes the file again.
pub fn rescan_definitions(world: &mut World) -> DefinitionRescan {
    let mut scan = DefinitionRescan::default();
    world.get_resource_or_init::<crate::asset_files::AssetKindCache>();
    let found = world.resource_scope(
        |world, mut cache: Mut<crate::asset_files::AssetKindCache>| {
            scan_definition_files(world, &mut cache)
        },
    );
    for definition in registered_types(world) {
        if !definition.scanned() {
            continue;
        }
        let files: Vec<(String, PathBuf)> = found
            .iter()
            .filter(|(kind, _, _)| kind == &definition.kind)
            .map(|(_, name, path)| (name.clone(), path.clone()))
            .collect();
        let known = world
            .resource::<DefinitionRegistry>()
            .names_of(&definition.kind);

        for gone in known
            .iter()
            .filter(|name| !files.iter().any(|(found, _)| found == *name))
        {
            world
                .resource_mut::<DefinitionRegistry>()
                .remove(&definition.kind, gone);
            scan.removed.push((definition.kind.clone(), gone.clone()));
        }

        for (name, path) in files {
            if known.contains(&name) {
                continue;
            }
            let Some(value) = load_definition_file(world, &definition, &path) else {
                continue;
            };
            world
                .resource_mut::<DefinitionRegistry>()
                .insert(DefinitionEntry {
                    kind: definition.kind.clone(),
                    name: name.clone(),
                    value,
                    path,
                });
            scan.added.push((definition.kind.clone(), name));
        }
    }
    scan
}

// -- Reading and writing a definition's fields ------------------------------

/// The asset a definition-editing entity stands for, reflected out of its
/// store. `None` when the entity is not editing a definition of `type_path`.
pub fn definition_value<'w>(
    world: &'w World,
    entity: Entity,
    type_path: &str,
    registry: &bevy::reflect::TypeRegistry,
) -> Option<&'w dyn Reflect> {
    let edit = world.get::<DefinitionAssetEdit>(entity)?;
    if edit.type_path != type_path {
        return None;
    }
    let reflect_asset = registry
        .get_with_type_path(type_path)?
        .data::<ReflectAsset>()?;
    reflect_asset.get(world, edit.value.handle()?.id())
}

/// The schema the project reported for a definition kind's type, when the
/// editor knows the type no other way.
pub fn definition_schema(world: &World, kind: &str) -> Option<jackdaw_schema::TypeSchema> {
    let definition = definition_of_kind(world, kind)?;
    world
        .get_resource::<crate::project_types::ProjectTypes>()?
        .asset(&definition.type_path)
        .cloned()
}

/// The whole of a schema-backed definition as JSON: what its file authors,
/// over what its type defaults to.
pub fn schema_definition_json(world: &World, kind: &str, name: &str) -> Option<serde_json::Value> {
    let schema = definition_schema(world, kind)?;
    let data = world.get_resource::<DefinitionValues>()?.get(kind, name)?;
    let types = world.get_resource::<crate::project_types::ProjectTypes>()?;
    Some(crate::schema_values::value_json(
        world, types, &schema, data,
    ))
}

/// Whether this entity is editing a definition of `type_path`.
fn edits_definition(world: &World, entity: Entity, type_path: &str) -> bool {
    world
        .get::<DefinitionAssetEdit>(entity)
        .is_some_and(|edit| edit.type_path == type_path)
}

/// The open definition's entity, while the inspector is showing it and it is
/// editing `type_path`. Selecting a scene entity again hands the same type's
/// edits back to the scene.
fn open_edit_of(world: &World, type_path: &str) -> Option<Entity> {
    let entity = world.get_resource::<OpenDefinition>()?.0?;
    let shown = world
        .get_resource::<crate::selection::Selection>()
        .is_some_and(|selection| selection.primary() == Some(entity));
    (shown && edits_definition(world, entity, type_path)).then_some(entity)
}

/// The baseline a drag started from, so the undo entry a drag commits restores
/// what the field held before the first tick rather than after the last one.
#[derive(Resource, Default)]
struct DefinitionEditSession {
    field: Option<(String, String)>,
    baseline: Option<serde_json::Value>,
}

fn field_as_json(
    world: &World,
    entity: Entity,
    type_path: &str,
    field_path: &str,
) -> Option<serde_json::Value> {
    let edit = world.get::<DefinitionAssetEdit>(entity)?;
    if edit.type_path != type_path {
        return None;
    }
    definition_field_json(world, &edit.value, type_path, field_path)
}

/// One field of a definition, whichever way its value is held.
fn definition_field_json(
    world: &World,
    value: &DefinitionValue,
    type_path: &str,
    field_path: &str,
) -> Option<serde_json::Value> {
    match value {
        DefinitionValue::Asset(handle) => asset_field_json(world, handle, type_path, field_path),
        DefinitionValue::Schema { kind, name } => {
            let whole = schema_definition_json(world, kind, name)?;
            let steps = crate::schema_values::parse_path(field_path);
            crate::schema_values::json_at(&whole, &steps).cloned()
        }
    }
}

/// One field of the asset behind `handle`. A field naming an asset reports the
/// path it names, which is the only spelling that sets it again.
fn asset_field_json(
    world: &World,
    handle: &UntypedHandle,
    type_path: &str,
    field_path: &str,
) -> Option<serde_json::Value> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let reflect_asset = registry
        .get_with_type_path(type_path)?
        .data::<ReflectAsset>()?;
    let value = reflect_asset.get(world, handle.id())?;
    let field = if field_path.is_empty() {
        value.as_partial_reflect()
    } else {
        value.reflect_path(field_path).ok()?
    };
    let server = world.get_resource::<AssetServer>();
    if let Some(path) = crate::typed_values::asset_path_json(&registry, server, field) {
        return Some(path);
    }
    crate::inspector::reflect_fields::reflect_to_json(field, &registry)
}

/// Write one field of a definition, and mark whatever card is editing it as
/// having unsaved changes.
fn write_field(
    world: &mut World,
    value: &DefinitionValue,
    type_path: &str,
    field_path: &str,
    json: &serde_json::Value,
) -> bool {
    let written = match value {
        DefinitionValue::Asset(handle) => {
            write_asset_field(world, handle, type_path, field_path, json)
        }
        DefinitionValue::Schema { kind, name } => {
            write_schema_field(world, kind, name, field_path, json)
        }
    };
    if written {
        mark_dirty(world, value);
    }
    written
}

fn write_asset_field(
    world: &mut World,
    handle: &UntypedHandle,
    type_path: &str,
    field_path: &str,
    json: &serde_json::Value,
) -> bool {
    let spelled = json
        .as_str()
        .and_then(|text| text_value_for_asset_field(world, handle, type_path, field_path, text));
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let Some(reflect_asset) = registry
        .get_with_type_path(type_path)
        .and_then(|registration| registration.data::<ReflectAsset>())
    else {
        return false;
    };
    let Some(value) = reflect_asset.get_mut(world, handle.id()) else {
        return false;
    };
    let field = if field_path.is_empty() {
        Some(value.as_partial_reflect_mut())
    } else {
        value.reflect_path_mut(field_path).ok()
    };
    let Some(field) = field else {
        return false;
    };
    match spelled {
        Some(spelled) => field.try_apply(spelled.as_ref()).is_ok(),
        None => {
            crate::commands::apply_json_to_reflect(field, json, &registry);
            true
        }
    }
}

/// The value a plain string stands for in one field of an asset: an asset path
/// or a colour, which no JSON spelling reaches.
fn text_value_for_asset_field(
    world: &World,
    handle: &UntypedHandle,
    type_path: &str,
    field_path: &str,
    text: &str,
) -> Option<Box<dyn bevy::reflect::PartialReflect>> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let reflect_asset = registry
        .get_with_type_path(type_path)?
        .data::<ReflectAsset>()?;
    let value = reflect_asset.get(world, handle.id())?;
    let field = if field_path.is_empty() {
        value.as_partial_reflect()
    } else {
        value.reflect_path(field_path).ok()?
    };
    let type_id = field.get_represented_type_info()?.type_id();
    if crate::typed_values::takes_asset_path(&registry, type_id) {
        report_missing_asset(text);
    }
    let server = world.get_resource::<AssetServer>();
    crate::typed_values::text_value_for_field(&registry, server, type_id, text)
}

/// Say when a path names nothing under the project's assets, without refusing
/// it: a file that arrives later loads on its own.
fn report_missing_asset(path: &str) {
    let file = path.split('#').next().unwrap_or(path);
    if file.is_empty() || file.starts_with('@') {
        return;
    }
    let Some(assets) = crate::project::open_project_assets_dir() else {
        return;
    };
    if !assets.join(file).exists() {
        warn!("{file} is not under this project's assets yet");
    }
}

/// Write one field of a value the editor knows only as schema. A field set
/// back to what its type defaults to stops being authored at all, so the file
/// keeps holding only what the definition changes.
fn write_schema_field(
    world: &mut World,
    kind: &str,
    name: &str,
    field_path: &str,
    json: &serde_json::Value,
) -> bool {
    let Some(schema) = definition_schema(world, kind) else {
        warn!("this project reported no schema for its {kind} definitions");
        return false;
    };
    let Some(mut data) = world
        .get_resource::<DefinitionValues>()
        .and_then(|values| values.get(kind, name))
        .cloned()
    else {
        return false;
    };
    if world
        .get_resource::<crate::project_types::ProjectTypes>()
        .is_none()
    {
        return false;
    }
    let written = world.resource_scope(|world, types: Mut<crate::project_types::ProjectTypes>| {
        let mut whole = crate::schema_values::value_json(world, &types, &schema, &data);
        let steps = crate::schema_values::parse_path(field_path);
        if steps.is_empty() {
            whole = json.clone();
        } else if !crate::schema_values::json_set(&mut whole, &steps, json.clone()) {
            return false;
        }
        let touched: Vec<String> = match steps.first() {
            None => schema
                .fields
                .iter()
                .map(|field| field.name.clone())
                .collect(),
            Some(crate::schema_values::Step::Field(name)) => vec![name.clone()],
            Some(crate::schema_values::Step::Index(_)) => return false,
        };
        for field_name in touched {
            let Some(field) = schema.fields.iter().find(|field| field.name == field_name) else {
                return false;
            };
            let Some(new) = whole.get(&field_name) else {
                continue;
            };
            if crate::schema_values::default_field_json(&schema, &field_name).as_ref() == Some(new)
            {
                crate::schema_values::set_authored(&mut data, &schema, &field_name, None);
                continue;
            }
            let Some(value) =
                crate::schema_values::bsn_for_json(world, &types, &field.type_path, new)
            else {
                return false;
            };
            crate::schema_values::set_authored(&mut data, &schema, &field_name, Some(value));
        }
        true
    });
    if written {
        world
            .get_resource_or_init::<DefinitionValues>()
            .insert(kind, name, data);
    }
    written
}

/// The entity editing this value, if one is open.
fn edit_of_value(world: &mut World, value: &DefinitionValue) -> Option<Entity> {
    let mut edits = world.query::<(Entity, &DefinitionAssetEdit)>();
    edits
        .iter(world)
        .find(|(_, edit)| edit.value == *value)
        .map(|(entity, _)| entity)
}

fn mark_dirty(world: &mut World, value: &DefinitionValue) {
    let Some(entity) = edit_of_value(world, value) else {
        return;
    };
    if let Some(mut edit) = world.get_mut::<DefinitionAssetEdit>(entity) {
        edit.dirty = true;
    }
}

/// Set one field of a definition, as one undo entry.
///
/// Keyed by the value rather than by the entity the definition was open on, so
/// undo still reaches it after the card has been closed and reopened.
pub struct SetDefinitionField {
    pub value: DefinitionValue,
    pub type_path: String,
    pub field_path: String,
    pub old_json: serde_json::Value,
    pub new_json: serde_json::Value,
    /// Whether applying this edit changes which rows the inspector shows, as
    /// an enum variant or a list length does.
    pub rebuilds_rows: bool,
}

impl SetDefinitionField {
    /// Write the value, reporting whether the definition took it.
    fn apply(&self, world: &mut World, json: &serde_json::Value) -> bool {
        if !write_field(world, &self.value, &self.type_path, &self.field_path, json) {
            return false;
        }
        let open = edit_of_value(world, &self.value);
        if self.rebuilds_rows
            && let Some(open) = open
            && let Some(mut pending) =
                world.get_resource_mut::<crate::inspector::PendingInspectorRebuild>()
        {
            pending.0 = Some(open);
        }
        true
    }
}

impl EditorCommand for SetDefinitionField {
    fn execute(&mut self, world: &mut World) {
        let json = self.new_json.clone();
        self.apply(world, &json);
    }

    fn undo(&mut self, world: &mut World) {
        let json = self.old_json.clone();
        self.apply(world, &json);
    }

    fn description(&self) -> &str {
        "Set definition field"
    }
}

/// Commit a field edit to the open definition, pushing one undo entry.
///
/// Returns whether the edit landed on a definition; a caller whose edit is
/// refused here writes to the selection as usual. A value the definition will
/// not take mints no history, so undo does not walk back over a no-op.
pub(crate) fn commit_definition_field(
    world: &mut World,
    type_path: &str,
    field_path: &str,
    new_json: &serde_json::Value,
) -> bool {
    let Some(entity) = open_edit_of(world, type_path) else {
        return false;
    };
    let Some(value) = world
        .get::<DefinitionAssetEdit>(entity)
        .map(|edit| edit.value.clone())
    else {
        return false;
    };
    let Some(current) = field_as_json(world, entity, type_path, field_path) else {
        return false;
    };
    let old_json = take_baseline(world, type_path, field_path).unwrap_or(current);
    let rebuilds_rows = field_rebuilds_rows(world, entity, type_path, field_path);
    let command = SetDefinitionField {
        value,
        type_path: type_path.to_string(),
        field_path: field_path.to_string(),
        old_json,
        new_json: new_json.clone(),
        rebuilds_rows,
    };
    if !command.apply(world, new_json) {
        return false;
    }
    world
        .resource_mut::<CommandHistory>()
        .push_executed(Box::new(command));
    true
}

/// Write a field of the open definition without an undo entry, for the ticks
/// of a drag.
pub(crate) fn preview_definition_field(
    world: &mut World,
    type_path: &str,
    field_path: &str,
    new_json: &serde_json::Value,
) -> bool {
    let Some(entity) = open_edit_of(world, type_path) else {
        return false;
    };
    let Some(value) = world
        .get::<DefinitionAssetEdit>(entity)
        .map(|edit| edit.value.clone())
    else {
        return false;
    };
    remember_baseline(world, entity, type_path, field_path);
    write_field(world, &value, type_path, field_path, new_json)
}

/// Record what the field held before a drag's first tick.
fn remember_baseline(world: &mut World, entity: Entity, type_path: &str, field_path: &str) {
    let field = (type_path.to_string(), field_path.to_string());
    if world
        .get_resource::<DefinitionEditSession>()
        .is_some_and(|session| session.field.as_ref() == Some(&field))
    {
        return;
    }
    let baseline = field_as_json(world, entity, type_path, field_path);
    let mut session = world.get_resource_or_init::<DefinitionEditSession>();
    session.field = Some(field);
    session.baseline = baseline;
}

/// Take the baseline a drag on this field left, if there is one.
fn take_baseline(
    world: &mut World,
    type_path: &str,
    field_path: &str,
) -> Option<serde_json::Value> {
    let mut session = world.get_resource_or_init::<DefinitionEditSession>();
    let matches = session
        .field
        .as_ref()
        .is_some_and(|(known_type, known_field)| {
            known_type == type_path && known_field == field_path
        });
    session.field = None;
    let baseline = session.baseline.take();
    matches.then_some(baseline).flatten()
}

/// Whether the field holds a value whose shape decides the rows shown for it.
fn field_rebuilds_rows(world: &World, entity: Entity, type_path: &str, field_path: &str) -> bool {
    if let Some(kind) = world
        .get::<DefinitionAssetEdit>(entity)
        .filter(|edit| edit.value.schema_key().is_some())
        .map(|edit| edit.kind.clone())
    {
        return schema_field_rebuilds_rows(world, &kind, field_path);
    }
    let registry = world.resource::<AppTypeRegistry>().read();
    let Some(value) = definition_value(world, entity, type_path, &registry) else {
        return false;
    };
    let field = if field_path.is_empty() {
        Some(value.as_partial_reflect())
    } else {
        value.reflect_path(field_path).ok()
    };
    field.is_some_and(|field| {
        matches!(
            field.reflect_ref(),
            ReflectRef::Enum(_) | ReflectRef::List(_) | ReflectRef::Array(_)
        )
    })
}

/// The elements of a list field on a schema-backed definition, as the JSON a
/// field edit takes. `None` when the entity is editing something else.
pub(crate) fn schema_list_items(
    world: &World,
    entity: Entity,
    type_path: &str,
    field_path: &str,
) -> Option<Vec<serde_json::Value>> {
    let edit = world.get::<DefinitionAssetEdit>(entity)?;
    if edit.type_path != type_path {
        return None;
    }
    edit.value.schema_key()?;
    let held = definition_field_json(world, &edit.value, type_path, field_path)?;
    held.as_array().cloned()
}

/// A fresh element for a list field on a schema-backed definition, from what
/// its item type defaults to.
pub(crate) fn schema_default_list_item(
    world: &World,
    entity: Entity,
    type_path: &str,
    field_path: &str,
) -> Option<serde_json::Value> {
    let edit = world.get::<DefinitionAssetEdit>(entity)?;
    if edit.type_path != type_path {
        return None;
    }
    edit.value.schema_key()?;
    let types = world.get_resource::<crate::project_types::ProjectTypes>()?;
    let steps = crate::schema_values::parse_path(field_path);
    let field_type = crate::schema_values::field_type_path(types, type_path, &steps)?;
    let item_type = crate::schema_values::list_item_type_path(&field_type)?;
    if let Some(schema) = types.type_schema(item_type) {
        return crate::schema_values::type_default_json(schema);
    }
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let default = registry
        .get_with_type_path(item_type)?
        .data::<ReflectDefault>()?
        .default();
    crate::inspector::reflect_fields::reflect_to_json(default.as_partial_reflect(), &registry)
}

/// Whether a schema-backed field decides which rows are shown for it.
fn schema_field_rebuilds_rows(world: &World, kind: &str, field_path: &str) -> bool {
    let Some(definition) = definition_of_kind(world, kind) else {
        return false;
    };
    let Some(types) = world.get_resource::<crate::project_types::ProjectTypes>() else {
        return false;
    };
    let steps = crate::schema_values::parse_path(field_path);
    if steps.is_empty() {
        return true;
    }
    crate::schema_values::field_type_path(types, &definition.type_path, &steps)
        .is_some_and(|type_path| crate::schema_values::shapes_its_own_rows(types, &type_path))
}

// -- Opening, saving and creating -------------------------------------------

/// The kind whose type a file says it holds, read from the file itself.
pub fn kind_of_file(world: &World, path: &Path) -> Option<AssetKind> {
    let kinds = world.get_resource::<AssetKinds>()?;
    let AssetFileKind::Asset { type_path } = read_asset_kind(path, kinds) else {
        return None;
    };
    kinds.by_type_path(&type_path).cloned()
}

/// Load a definition file, or reuse the loaded one, and put it in the
/// inspector. A kind loaded elsewhere is only ever shown through the value its
/// owner published.
pub fn open_definition_file(world: &mut World, path: &Path) -> bool {
    let Some(definition) = kind_of_file(world, path) else {
        return false;
    };
    let name = definition_name_of(path);

    let known = world
        .resource::<DefinitionRegistry>()
        .get(&definition.kind, &name)
        .map(|entry| entry.value.clone());
    let value = match known {
        Some(value) => value,
        None => {
            if !definition.scanned() {
                warn!("No {} named '{name}' is loaded", definition.kind);
                return false;
            }
            let Some(value) = load_definition_file(world, &definition, path) else {
                return false;
            };
            world
                .resource_mut::<DefinitionRegistry>()
                .insert(DefinitionEntry {
                    kind: definition.kind.clone(),
                    name: name.clone(),
                    value: value.clone(),
                    path: path.to_path_buf(),
                });
            value
        }
    };

    show_definition(world, &definition, &name, value, path.to_path_buf());
    true
}

fn show_definition(
    world: &mut World,
    definition: &AssetKind,
    name: &str,
    value: DefinitionValue,
    path: PathBuf,
) {
    close_open_definition(world);
    let entity = world
        .spawn((
            Name::new(format!("{} ({})", name, definition.label)),
            DefinitionAssetEdit {
                kind: definition.kind.clone(),
                name: name.to_string(),
                type_path: definition.type_path.clone(),
                value,
                path,
                dirty: false,
            },
        ))
        .id();
    world.resource_mut::<OpenDefinition>().0 = Some(entity);
    crate::selection::select_only(world, entity);
}

/// Drop the editing entity for whatever definition was open. The definition
/// itself stays loaded and registered.
pub fn close_open_definition(world: &mut World) {
    let Some(entity) = world.resource_mut::<OpenDefinition>().0.take() else {
        return;
    };
    if let Ok(entity_mut) = world.get_entity_mut(entity) {
        entity_mut.despawn();
    }
    if world
        .get_resource::<crate::selection::Selection>()
        .is_some_and(|selection| selection.entities.contains(&entity))
    {
        crate::selection::clear_selection_in_world(world);
    }
}

/// Write the open definition back to its file, reporting the name it saved
/// under.
fn save_open_definition(world: &mut World, entity: Entity) -> Option<String> {
    let (kind, name, value, path) = world.get::<DefinitionAssetEdit>(entity).map(|edit| {
        (
            edit.kind.clone(),
            edit.name.clone(),
            edit.value.clone(),
            edit.path.clone(),
        )
    })?;
    save_definition(world, &kind, &name, &value, &path)
}

/// Write the definition a file path names back to that file, without taking
/// the inspector off whatever it is showing.
fn save_definition_at(world: &mut World, path: &Path) -> Option<String> {
    if let Some(entry) = world.resource::<DefinitionRegistry>().by_path(path) {
        let (kind, name, value) = (entry.kind.clone(), entry.name.clone(), entry.value.clone());
        return save_definition(world, &kind, &name, &value, path);
    }
    let Some(definition) = kind_of_file(world, path) else {
        warn!("asset.save: {} is not an asset file", path.display());
        return None;
    };
    let name = definition_name_of(path);
    let Some(value) = world
        .resource::<DefinitionRegistry>()
        .get(&definition.kind, &name)
        .map(|entry| entry.value.clone())
    else {
        warn!(
            "asset.save: no {} named '{name}' is loaded",
            definition.kind
        );
        return None;
    };
    save_definition(world, &definition.kind, &name, &value, path)
}

/// Write one definition back to the file it came from and record where it
/// landed.
fn save_definition(
    world: &mut World,
    kind: &str,
    name: &str,
    value: &DefinitionValue,
    path: &Path,
) -> Option<String> {
    let path = match write_definition_file(world, name, value, path) {
        Ok(path) => path,
        Err(err) => {
            warn!("asset.save: failed to write '{name}': {err}");
            return None;
        }
    };
    if let Some(open) = edit_of_value(world, value)
        && let Some(mut edit) = world.get_mut::<DefinitionAssetEdit>(open)
    {
        edit.dirty = false;
        edit.path = path.clone();
    }
    world
        .resource_mut::<DefinitionRegistry>()
        .insert(DefinitionEntry {
            kind: kind.to_string(),
            name: name.to_string(),
            value: value.clone(),
            path,
        });
    Some(name.to_string())
}

// -- Operators --------------------------------------------------------------

pub(crate) fn add_to_extension(ctx: &mut ExtensionContext) {
    ctx.register_operator::<AssetNewOp>()
        .register_operator::<AssetOpenOp>()
        .register_operator::<AssetSaveOp>()
        .register_operator::<AssetDeleteOp>()
        .register_operator::<AssetSetOp>()
        .register_operator::<AssetListOp>();
}

/// The kind saved materials list under, so a material is browsed, opened and
/// edited through the same registry as any other asset.
pub const MATERIAL_KIND: &str = "material";

/// The kind an animation graph file holds.
pub const ANIMATION_GRAPH_KIND: &str = "animation_graph";

/// The kind a packed prefab holds.
pub const PREFAB_KIND: &str = "prefab";

/// The types the editor has compiled in, each loaded by the panel that owns
/// it rather than by the asset scan.
fn compiled_kinds() -> [AssetKind; 3] {
    use jackdaw_api_internal::lucide_icons::Icon;
    [
        AssetKind::compiled(
            MATERIAL_KIND,
            "Material",
            "bevy_pbr::pbr_material::StandardMaterial",
        )
        .with_icon(Icon::Palette),
        AssetKind::compiled(
            ANIMATION_GRAPH_KIND,
            "Animation Graph",
            <jackdaw_animation_runtime::graph::AnimationGraphDef as bevy::reflect::TypePath>::type_path(),
        )
        .with_icon(Icon::Workflow),
        AssetKind::compiled(PREFAB_KIND, "Prefab", jackdaw_prefab::components::PREFAB_TYPE)
            .with_icon(Icon::Package),
    ]
}

pub(crate) fn plugin(app: &mut App) {
    {
        let mut kinds = app.world_mut().get_resource_or_init::<AssetKinds>();
        for kind in compiled_kinds() {
            kinds.register(kind);
        }
    }
    app.init_resource::<DefinitionRegistry>()
        .init_resource::<crate::asset_files::AssetKindCache>()
        .init_resource::<DefinitionValues>()
        .init_resource::<DefinitionEditSession>()
        .init_resource::<OpenDefinition>()
        .init_resource::<DefinitionScanPending>()
        .add_systems(OnEnter(crate::AppState::Editor), watch_definition_files)
        .add_systems(
            Update,
            (
                follow_registered_types.run_if(resource_changed::<AssetKinds>),
                poll_definition_watcher,
                apply_definition_scan,
            )
                .chain()
                .run_if(in_state(crate::AppState::Editor)),
        )
        .add_systems(
            Update,
            mirror_material_definitions
                .run_if(resource_exists_and_changed::<crate::material_assets::MaterialRegistry>),
        );
}

/// Publish the saved materials into the definition registry, so the material
/// directory browses and opens like any other kind.
fn mirror_material_definitions(world: &mut World) {
    if definition_of_kind(world, MATERIAL_KIND).is_none() {
        return;
    }
    let saved: Vec<(String, UntypedHandle)> = world
        .resource::<crate::material_assets::MaterialRegistry>()
        .saved_entries()
        .map(|entry| {
            (
                sanitize_definition_name(&entry.name),
                entry.handle.clone().untyped(),
            )
        })
        .collect();
    let paths: Vec<PathBuf> = {
        let Some(project) = world.get_resource::<ProjectRoot>() else {
            return;
        };
        saved
            .iter()
            .map(|(name, _)| crate::material_assets::material_file_path(project, name))
            .collect()
    };

    let mut registry = world.resource_mut::<DefinitionRegistry>();
    registry.entries.retain(|entry| entry.kind != MATERIAL_KIND);
    for ((name, handle), path) in saved.into_iter().zip(paths) {
        registry.insert(DefinitionEntry {
            kind: MATERIAL_KIND.to_string(),
            name,
            value: DefinitionValue::Asset(handle),
            path,
        });
    }
}

/// Watches the project's assets directory so a definition file written by
/// another tool registers without reopening the project.
#[derive(Resource)]
struct DefinitionFileWatcher {
    _watcher: notify::RecommendedWatcher,
    receiver: Mutex<mpsc::Receiver<()>>,
}

#[derive(Resource, Default)]
struct DefinitionScanPending(bool);

fn watch_definition_files(
    project: Option<Res<ProjectRoot>>,
    mut pending: ResMut<DefinitionScanPending>,
    mut commands: Commands,
) {
    pending.0 = true;
    let Some(assets) = project.map(|project| project.assets_dir()) else {
        return;
    };
    let (sender, receiver) = mpsc::channel();
    let watcher =
        notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
            use notify::EventKind;
            if let Ok(event) = event
                && matches!(
                    event.kind,
                    EventKind::Create(_)
                        | EventKind::Remove(_)
                        | EventKind::Modify(notify::event::ModifyKind::Name(_))
                )
            {
                let _ = sender.send(());
            }
        });
    if let Ok(mut watcher) = watcher {
        use notify::Watcher as _;
        if watcher
            .watch(&assets, notify::RecursiveMode::Recursive)
            .is_ok()
        {
            commands.insert_resource(DefinitionFileWatcher {
                _watcher: watcher,
                receiver: Mutex::new(receiver),
            });
        }
    }
}

fn poll_definition_watcher(
    watcher: Option<Res<DefinitionFileWatcher>>,
    mut pending: ResMut<DefinitionScanPending>,
) {
    let Some(watcher) = watcher else { return };
    let Ok(receiver) = watcher.receiver.lock() else {
        return;
    };
    if receiver.try_recv().is_ok() {
        while receiver.try_recv().is_ok() {}
        pending.0 = true;
    }
}

/// Follow the registered types: a kind that has gone takes its loaded entries
/// and its open card with it, and a kind that has arrived gets its directory
/// scanned.
fn follow_registered_types(world: &mut World) {
    let kinds: Vec<String> = registered_types(world)
        .into_iter()
        .map(|definition| definition.kind)
        .collect();
    world
        .resource_mut::<DefinitionRegistry>()
        .entries
        .retain(|entry| kinds.contains(&entry.kind));
    world
        .get_resource_or_init::<DefinitionValues>()
        .values
        .retain(|(kind, _), _| kinds.contains(kind));
    let open_kind = world
        .resource::<OpenDefinition>()
        .0
        .and_then(|entity| world.get::<DefinitionAssetEdit>(entity))
        .map(|edit| edit.kind.clone());
    if let Some(kind) = open_kind
        && !kinds.contains(&kind)
    {
        close_open_definition(world);
    }
    world.resource_mut::<DefinitionScanPending>().0 = true;
}

fn apply_definition_scan(world: &mut World) {
    if !std::mem::take(&mut world.resource_mut::<DefinitionScanPending>().0) {
        return;
    }
    let scan = rescan_definitions(world);
    if !scan.added.is_empty() {
        info!("Loaded {} definition files", scan.added.len());
    }
}

fn definition_of_kind(world: &World, kind: &str) -> Option<AssetKind> {
    world
        .get_resource::<AssetKinds>()
        .and_then(|types| types.by_kind(kind))
        .cloned()
}

/// Create a definition file of a registered type and open it.
#[operator(
    id = "asset.new",
    label = "New Definition",
    description = "Create a definition file of a registered type and open it in the inspector.",
    allows_undo = false,
    params(
        r#type(String, doc = "Kind of asset to create, as its type registered it."),
        name(
            String,
            doc = "Name to create it under. Defaults to the next free name."
        ),
        path(
            String,
            doc = "Folder to create it in, or the file to write. Defaults to the \
                   folder the browser is showing."
        )
    )
)]
pub fn asset_new(params: In<OperatorParameters>, mut commands: Commands) -> OperatorResult {
    let Some(kind) = params.as_str("type").map(str::to_owned) else {
        warn!("asset.new: no type given");
        return OperatorResult::Cancelled;
    };
    let name = params.as_str("name").map(str::to_owned);
    let path = params.as_str("path").map(PathBuf::from);
    commands.queue(move |world: &mut World| {
        let dir = path.as_ref().map(|path| resolve_project_path(world, path));
        new_definition(world, &kind, name.as_deref(), dir.as_deref());
    });
    OperatorResult::Finished
}

fn new_definition(world: &mut World, kind: &str, name: Option<&str>, dir: Option<&Path>) {
    let Some((definition, name, value, path)) = create_definition(world, kind, name, dir) else {
        return;
    };
    show_definition(world, &definition, &name, value, path);
    report_to_caller(world, format!("Created {kind} '{name}'"));
}

/// Write a fresh default value of a registered kind to its file and register
/// it.
fn create_definition(
    world: &mut World,
    kind: &str,
    name: Option<&str>,
    dir: Option<&Path>,
) -> Option<(AssetKind, String, DefinitionValue, PathBuf)> {
    let Some(definition) = definition_of_kind(world, kind) else {
        warn!("asset.new: '{kind}' is not a registered definition type");
        return None;
    };
    if !definition.scanned() {
        warn!("asset.new: {kind} definitions are created by whoever loads them");
        return None;
    }
    let (dir, file) = match dir {
        Some(file) if names_a_file(file) => (
            file.parent().unwrap_or(file).to_path_buf(),
            Some(file.to_path_buf()),
        ),
        Some(dir) => (dir.to_path_buf(), None),
        None => {
            let Some(dir) = new_definition_dir(world) else {
                warn!("asset.new: no project is open");
                return None;
            };
            (dir, None)
        }
    };
    let asked_for = name.map(sanitize_definition_name);
    let named_by_path = file.as_deref().map(definition_name_of);
    let name = match named_by_path.or_else(|| asked_for.clone()) {
        Some(name) => sanitize_definition_name(&name),
        None => next_free_name(world, kind, &dir),
    };
    if asked_for.is_some_and(|asked| asked != name) {
        warn!("asset.new: the file asked for names this {kind} '{name}'");
    }
    if world
        .resource::<DefinitionRegistry>()
        .get(kind, &name)
        .is_some()
    {
        warn!("asset.new: a {kind} named '{name}' already exists");
        return None;
    }
    let path = file.unwrap_or_else(|| definition_file_path(&dir, &name));
    if path.exists() {
        warn!("asset.new: {} is already there", path.display());
        return None;
    }
    let Some(value) = default_definition_value(world, &definition, &name) else {
        warn!(
            "asset.new: {} has no registered default",
            definition.type_path
        );
        return None;
    };
    let path = match write_definition_file(world, &name, &value, &path) {
        Ok(path) => path,
        Err(err) => {
            warn!("asset.new: failed to write '{name}': {err}");
            return None;
        }
    };
    world
        .resource_mut::<DefinitionRegistry>()
        .insert(DefinitionEntry {
            kind: definition.kind.clone(),
            name: name.clone(),
            value: value.clone(),
            path: path.clone(),
        });
    Some((definition, name, value, path))
}

/// Open a definition file in the inspector.
#[operator(
    id = "asset.open",
    label = "Open Definition",
    description = "Open a definition file in the inspector.",
    allows_undo = false,
    params(path(String, doc = "File to open, as a path under the project."))
)]
pub fn asset_open(params: In<OperatorParameters>, mut commands: Commands) -> OperatorResult {
    let Some(path) = params.as_str("path").map(PathBuf::from) else {
        warn!("asset.open: no path given");
        return OperatorResult::Cancelled;
    };
    commands.queue(move |world: &mut World| {
        let path = resolve_project_path(world, &path);
        if !open_definition_file(world, &path) {
            warn!("asset.open: {} is not a definition file", path.display());
        }
    });
    OperatorResult::Finished
}

/// Write a definition back to its file.
#[operator(
    id = "asset.save",
    label = "Save Definition",
    description = "Write the open definition back to its file.",
    allows_undo = false,
    params(path(
        String,
        doc = "Definition file to save. Defaults to the open definition."
    ))
)]
pub fn asset_save(params: In<OperatorParameters>, mut commands: Commands) -> OperatorResult {
    let path = params.as_str("path").map(PathBuf::from);
    commands.queue(move |world: &mut World| {
        let saved = match &path {
            Some(path) => {
                let path = resolve_project_path(world, path);
                save_definition_at(world, &path)
            }
            None => {
                let Some(entity) = world.resource::<OpenDefinition>().0 else {
                    warn!("asset.save: no definition is open");
                    return;
                };
                save_open_definition(world, entity)
            }
        };
        if let Some(name) = saved {
            report_to_caller(world, format!("Saved '{name}'"));
        }
    });
    OperatorResult::Finished
}

/// Delete a definition file and forget what it held.
#[operator(
    id = "asset.delete",
    label = "Delete Definition",
    description = "Delete a definition file and drop it from this project.",
    allows_undo = false,
    params(path(String, doc = "Definition file to delete."))
)]
pub fn asset_delete(params: In<OperatorParameters>, mut commands: Commands) -> OperatorResult {
    let Some(path) = params.as_str("path").map(PathBuf::from) else {
        warn!("asset.delete: no path given");
        return OperatorResult::Cancelled;
    };
    commands.queue(move |world: &mut World| {
        let path = resolve_project_path(world, &path);
        delete_definition(world, &path);
    });
    OperatorResult::Finished
}

fn delete_definition(world: &mut World, path: &Path) {
    let Some(definition) = kind_of_file(world, path) else {
        warn!("asset.delete: {} is not an asset file", path.display());
        return;
    };
    if !definition.scanned() {
        warn!(
            "asset.delete: a {} is removed by whoever loads it",
            definition.kind
        );
        return;
    }
    let name = definition_name_of(path);
    match std::fs::remove_file(path) {
        Ok(()) => info!("Removed {}", path.display()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            warn!("asset.delete: failed to remove {}: {err}", path.display());
            return;
        }
    }
    world
        .resource_mut::<DefinitionRegistry>()
        .remove(&definition.kind, &name);
    world
        .get_resource_or_init::<DefinitionValues>()
        .remove(&definition.kind, &name);
    let open_is_gone = world
        .resource::<OpenDefinition>()
        .0
        .and_then(|entity| world.get::<DefinitionAssetEdit>(entity))
        .is_some_and(|edit| edit.kind == definition.kind && edit.name == name);
    if open_is_gone {
        close_open_definition(world);
    }
}

/// Set a field of the open definition, for edits driven from outside the
/// inspector.
#[operator(
    id = "asset.set",
    label = "Set Definition Field",
    description = "Set a field of the open definition.",
    allows_undo = false,
    params(
        field(
            String,
            doc = "Field path on the definition, for example 'stack_size'."
        ),
        value(
            String,
            doc = "Value to set: JSON, a plain scalar, an asset path for a slot \
                   that holds one, or a colour as 'r,g,b', a hex code or a name."
        )
    )
)]
pub fn asset_set(params: In<OperatorParameters>, mut commands: Commands) -> OperatorResult {
    let (Some(field), Some(value)) = (
        params.as_str("field").map(str::to_owned),
        params.as_str("value").map(str::to_owned),
    ) else {
        warn!("asset.set: both field and value are required");
        return OperatorResult::Cancelled;
    };
    commands.queue(move |world: &mut World| {
        set_definition_field(world, &field, &value);
    });
    OperatorResult::Finished
}

fn set_definition_field(world: &mut World, field: &str, value: &str) {
    let Some(entity) = world.resource::<OpenDefinition>().0 else {
        warn!("asset.set: no definition is open");
        return;
    };
    let Some(type_path) = world
        .get::<DefinitionAssetEdit>(entity)
        .map(|edit| edit.type_path.clone())
    else {
        return;
    };
    if world
        .get_resource::<crate::selection::Selection>()
        .and_then(crate::selection::Selection::primary)
        != Some(entity)
    {
        crate::selection::select_only(world, entity);
    }
    let json = serde_json::from_str::<serde_json::Value>(value)
        .unwrap_or_else(|_| serde_json::Value::String(value.to_string()));
    if commit_definition_field(world, &type_path, field, &json) {
        report_to_caller(world, format!("Set {field}"));
    } else {
        warn!("asset.set: {type_path} did not take '{value}' for '{field}'");
    }
}

/// Report the definitions of a kind this project holds.
#[operator(
    id = "asset.list",
    label = "List Definitions",
    description = "Report the definitions of a registered type this project holds.",
    allows_undo = false,
    params(r#type(String, doc = "Kind of definition to list."))
)]
pub fn asset_list(params: In<OperatorParameters>, mut commands: Commands) -> OperatorResult {
    let Some(kind) = params.as_str("type").map(str::to_owned) else {
        warn!("asset.list: no type given");
        return OperatorResult::Cancelled;
    };
    commands.queue(move |world: &mut World| {
        if definition_of_kind(world, &kind).is_none() {
            warn!("asset.list: '{kind}' is not a registered definition type");
            return;
        }
        let names = world.resource::<DefinitionRegistry>().names_of(&kind);
        report_to_caller(world, format!("{kind}: {}", names.join(", ")));
    });
    OperatorResult::Finished
}

/// Accept both a path under the project and one relative to its assets
/// directory, so a caller can pass what the asset browser lists.
fn resolve_project_path(world: &World, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let Some(project) = world.get_resource::<ProjectRoot>() else {
        return path.to_path_buf();
    };
    let under_root = project.root.join(path);
    if under_root.exists() {
        return under_root;
    }
    project.assets_dir().join(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::{Asset, AssetPlugin};
    use bevy::reflect::Reflect;

    #[derive(Reflect, Clone, Default, PartialEq, Debug)]
    #[reflect(Default)]
    struct LootRoll {
        item: String,
        weight: u32,
    }

    #[derive(Reflect, Clone, Default, PartialEq, Debug)]
    #[reflect(Default)]
    enum Rarity {
        #[default]
        Common,
        Rare,
    }

    #[derive(Asset, Reflect, Clone, Default)]
    #[reflect(Default)]
    struct ItemDef {
        stack_size: u32,
        rarity: Rarity,
        loot: Vec<LootRoll>,
    }

    fn item_type() -> AssetKind {
        AssetKind::extension("item", "Item", "jackdaw::definition_assets::tests::ItemDef")
    }

    fn item_file(tmp: &tempfile::TempDir, name: &str) -> PathBuf {
        let dir = tmp.path().join("assets/content/items");
        std::fs::create_dir_all(&dir).expect("the directory is made");
        definition_file_path(&dir, name)
    }

    fn definition_app() -> (App, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut app = App::new();
        app.add_plugins((bevy::app::TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<ItemDef>();
        app.register_asset_reflect::<ItemDef>();
        app.register_type::<ItemDef>();
        app.register_type::<LootRoll>();
        app.register_type::<Rarity>();
        app.init_resource::<AssetKinds>();
        app.init_resource::<DefinitionRegistry>();
        app.insert_resource(ProjectRoot {
            root: tmp.path().to_path_buf(),
            config: crate::project::ProjectConfig::default(),
        });
        app.world_mut()
            .resource_mut::<AssetKinds>()
            .register(item_type());
        (app, tmp)
    }

    fn item_of(app: &App, value: &DefinitionValue) -> ItemDef {
        let handle = value.handle().expect("a compiled definition").clone();
        app.world()
            .resource::<Assets<ItemDef>>()
            .get(&handle.typed::<ItemDef>())
            .expect("the definition is in its store")
            .clone()
    }

    #[test]
    fn a_file_names_its_definition_by_the_stem_before_its_first_dot() {
        for (file, expected) in [
            ("torch.item.bsn", "torch"),
            ("torch.bsn", "torch"),
            ("torch", "torch"),
            (".torch.item.bsn", "torch"),
        ] {
            assert_eq!(
                definition_name_of(Path::new(file)),
                expected,
                "{file} names a definition"
            );
        }
    }

    #[test]
    fn the_root_name_a_file_spells_is_read_back_however_it_is_written() {
        assert_eq!(
            root_name_of("#torch\njackdaw::Item {\n}\n").as_deref(),
            Some("torch")
        );
        assert_eq!(
            root_name_of("#\"torch.item\"\njackdaw::Item {\n}\n").as_deref(),
            Some("torch.item"),
            "a quoted name is the name, without its quotes"
        );
        assert_eq!(
            root_name_of("jackdaw::Item {\n}\n"),
            None,
            "a root that names nothing leaves the name to the caller"
        );
    }

    #[test]
    fn a_written_definition_reloads_from_the_folder_it_was_written_in() {
        let (mut app, tmp) = definition_app();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<ItemDef>>()
            .add(ItemDef {
                stack_size: 20,
                rarity: Rarity::Rare,
                loot: vec![LootRoll {
                    item: "coin".into(),
                    weight: 3,
                }],
            });

        let path = item_file(&tmp, "torch");
        write_definition_file(
            app.world(),
            "torch",
            &DefinitionValue::Asset(handle.untyped()),
            &path,
        )
        .expect("the file is written");
        assert!(path.is_file());

        let scan = rescan_definitions(app.world_mut());
        assert_eq!(scan.added, vec![("item".to_string(), "torch".to_string())]);

        let loaded = app
            .world()
            .resource::<DefinitionRegistry>()
            .get("item", "torch")
            .expect("the scan registered it")
            .value
            .clone();
        let item = item_of(&app, &loaded);
        assert_eq!(item.stack_size, 20);
        assert_eq!(item.rarity, Rarity::Rare);
        assert_eq!(
            item.loot,
            vec![LootRoll {
                item: "coin".into(),
                weight: 3
            }]
        );
    }

    #[test]
    fn a_file_deleted_on_disk_drops_its_entry_on_the_next_scan() {
        let (mut app, tmp) = definition_app();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<ItemDef>>()
            .add(ItemDef::default());
        let path = item_file(&tmp, "torch");
        write_definition_file(
            app.world(),
            "torch",
            &DefinitionValue::Asset(handle.untyped()),
            &path,
        )
        .expect("the file is written");
        rescan_definitions(app.world_mut());

        std::fs::remove_file(&path).expect("the file is removed");
        let scan = rescan_definitions(app.world_mut());

        assert_eq!(
            scan.removed,
            vec![("item".to_string(), "torch".to_string())]
        );
        assert!(
            app.world()
                .resource::<DefinitionRegistry>()
                .get("item", "torch")
                .is_none()
        );
    }

    #[test]
    fn a_definition_is_found_in_whatever_folder_it_was_written_in() {
        let (mut app, tmp) = definition_app();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<ItemDef>>()
            .add(ItemDef {
                stack_size: 7,
                ..Default::default()
            });
        let deep = tmp.path().join("assets/somewhere/else");
        std::fs::create_dir_all(&deep).expect("the directory is made");
        write_definition_file(
            app.world(),
            "torch",
            &DefinitionValue::Asset(handle.untyped()),
            &definition_file_path(&deep, "torch"),
        )
        .expect("the file is written");

        let scan = rescan_definitions(app.world_mut());

        assert_eq!(scan.added, vec![("item".to_string(), "torch".to_string())]);
        let loaded = app
            .world()
            .resource::<DefinitionRegistry>()
            .get("item", "torch")
            .expect("the scan registered it")
            .value
            .clone();
        assert_eq!(item_of(&app, &loaded).stack_size, 7);
    }

    #[test]
    fn a_file_a_kind_would_call_the_same_name_is_left_unopened() {
        let (mut app, tmp) = definition_app();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<ItemDef>>()
            .add(ItemDef::default());
        let value = DefinitionValue::Asset(handle.untyped());
        write_definition_file(app.world(), "torch", &value, &item_file(&tmp, "torch"))
            .expect("the file is written");
        let elsewhere = tmp.path().join("assets/other");
        std::fs::create_dir_all(&elsewhere).expect("the directory is made");
        write_definition_file(
            app.world(),
            "torch",
            &value,
            &definition_file_path(&elsewhere, "torch"),
        )
        .expect("the second file is written");

        let scan = rescan_definitions(app.world_mut());

        assert_eq!(
            scan.added,
            vec![("item".to_string(), "torch".to_string())],
            "one name means one file"
        );
        assert_eq!(
            app.world()
                .resource::<DefinitionRegistry>()
                .get("item", "torch")
                .map(|entry| entry.path.clone()),
            Some(item_file(&tmp, "torch")),
            "the first file the walk reaches is the one that opens"
        );
    }

    #[test]
    fn names_sanitize_to_one_file_each() {
        assert_eq!(sanitize_definition_name("torch"), "torch");
        assert_eq!(sanitize_definition_name("a/b"), "a_b");
        assert_eq!(sanitize_definition_name("../escape"), ".._escape");
        assert_eq!(sanitize_definition_name("  "), "definition");
    }
}
