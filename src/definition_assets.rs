//! Definition assets: one reflected asset value per file, under the directory
//! and suffix its type registered.
//!
//! A registered type (`assets/content/items` with `.item.bsn`, say) is scanned
//! at project open and watched while the editor runs. Loading goes through
//! [`jackdaw_bsn::load_bsn_assets`] and saving through
//! [`jackdaw_bsn::serialize_assets_to_bsn`], so a file holds only what the
//! asset changes from its default.
//!
//! Opening a definition puts it in the inspector: the open definition rides on
//! an editor entity carrying [`DefinitionAssetEdit`], the reflected field rows
//! read the asset behind its handle instead of a component, and their edits
//! come back here as [`SetDefinitionField`] undo entries.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};

use bevy::asset::{ReflectAsset, UntypedHandle};
use bevy::prelude::*;
use bevy::reflect::{GetPath, ReflectRef, prelude::ReflectDefault};
use jackdaw_api::prelude::{DefinitionAssetType, DefinitionAssetTypes};
use jackdaw_api_internal::operator::report_to_caller;
use jackdaw_bsn::CatalogAssetRef;
use jackdaw_commands::CommandHistory;

use crate::EditorEntity;
use crate::commands::EditorCommand;
use crate::prelude::*;
use crate::project::ProjectRoot;

/// A definition loaded from a file, by the name its file stem gives it.
pub struct DefinitionEntry {
    pub kind: String,
    pub name: String,
    pub handle: UntypedHandle,
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
}

/// The first `<Label>_N` name of this kind with neither an entry nor a file.
fn next_free_name(world: &World, definition: &DefinitionAssetType) -> String {
    let registry = world.resource::<DefinitionRegistry>();
    let project = world.get_resource::<ProjectRoot>();
    let mut index = 1u32;
    loop {
        let candidate = sanitize_definition_name(&format!("{}_{index}", definition.label));
        let taken = registry.get(&definition.kind, &candidate).is_some()
            || project
                .is_some_and(|project| definition_path(project, definition, &candidate).exists());
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
    pub handle: UntypedHandle,
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

pub fn definition_dir(project: &ProjectRoot, definition: &DefinitionAssetType) -> PathBuf {
    project.assets_dir().join(&definition.directory)
}

pub fn definition_path(
    project: &ProjectRoot,
    definition: &DefinitionAssetType,
    name: &str,
) -> PathBuf {
    definition_dir(project, definition).join(definition.file_name(&sanitize_definition_name(name)))
}

/// Every file of this kind as `(name, path)`, sorted by path so scan order is
/// stable.
pub fn definition_files(world: &World, definition: &DefinitionAssetType) -> Vec<(String, PathBuf)> {
    let Some(project) = world.get_resource::<ProjectRoot>() else {
        return Vec::new();
    };
    let Ok(read_dir) = std::fs::read_dir(definition_dir(project, definition)) else {
        return Vec::new();
    };
    let mut files: Vec<(String, PathBuf)> = read_dir
        .flatten()
        .map(|entry| entry.path())
        .filter_map(|path| Some((definition.name_of_file(&path)?, path)))
        .collect();
    files.sort_by(|a, b| a.1.cmp(&b.1));
    files
}

/// Load one file into its `Assets<T>` store. A file that cannot be read or
/// parsed, or that holds a value of another type, is reported and skipped.
pub fn load_definition_file(
    world: &mut World,
    definition: &DefinitionAssetType,
    path: &Path,
) -> Option<UntypedHandle> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            warn!("Failed to read {}: {err}", path.display());
            return None;
        }
    };
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
    Some(entry.handle)
}

/// Write one definition to the file its name gives it.
pub fn write_definition_file(
    world: &World,
    definition: &DefinitionAssetType,
    name: &str,
    handle: &UntypedHandle,
) -> std::io::Result<PathBuf> {
    let project = world
        .get_resource::<ProjectRoot>()
        .ok_or_else(|| std::io::Error::other("no project root"))?;
    let path = definition_path(project, definition, name);
    write_definition_at(world, name, handle, &path)
}

/// Write one definition to a named file, which is where it was opened from
/// rather than where its name would put it. An identical rewrite is skipped so
/// the asset watcher does not reload behind an unchanged save.
fn write_definition_at(
    world: &World,
    name: &str,
    handle: &UntypedHandle,
    path: &Path,
) -> std::io::Result<PathBuf> {
    let text = jackdaw_bsn::serialize_assets_to_bsn(
        world,
        &[CatalogAssetRef {
            name: sanitize_definition_name(name),
            type_id: handle.type_id(),
            asset_id: handle.id(),
        }],
    );
    if text.trim().is_empty() {
        return Err(std::io::Error::other(format!(
            "nothing to write for '{name}'"
        )));
    }
    if std::fs::read_to_string(path).is_ok_and(|existing| existing == text) {
        return Ok(path.to_path_buf());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::scene_io::save::write_atomic(path, text.as_bytes())?;
    Ok(path.to_path_buf())
}

/// Add a default value of the type to its `Assets<T>` store.
pub fn default_definition_value(
    world: &mut World,
    definition: &DefinitionAssetType,
) -> Option<UntypedHandle> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let registration = registry.get_with_type_path(&definition.type_path)?;
    let reflect_asset = registration.data::<ReflectAsset>()?;
    let value = registration.data::<ReflectDefault>()?.default();
    Some(reflect_asset.add(world, value.as_partial_reflect()))
}

fn registered_type_id(world: &World, type_path: &str) -> Option<std::any::TypeId> {
    let registry = world.resource::<AppTypeRegistry>().read();
    Some(registry.get_with_type_path(type_path)?.type_id())
}

fn registered_types(world: &World) -> Vec<DefinitionAssetType> {
    world
        .get_resource::<DefinitionAssetTypes>()
        .map(|types| types.iter().cloned().collect())
        .unwrap_or_default()
}

/// What a rescan of the registered directories found.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct DefinitionRescan {
    /// `(kind, name)` of files that appeared since the last scan.
    pub added: Vec<(String, String)>,
    /// `(kind, name)` of entries whose file has gone.
    pub removed: Vec<(String, String)>,
}

/// Scan every registered directory: files that appeared are loaded, entries
/// whose file has gone are dropped.
///
/// A file changed in place is not reloaded, and a definition open in the
/// inspector keeps its value when its file disappears: the loaded value is
/// what the editor is editing, and its Save writes the file again.
pub fn rescan_definitions(world: &mut World) -> DefinitionRescan {
    let mut scan = DefinitionRescan::default();
    for definition in registered_types(world) {
        if !definition.scanned {
            continue;
        }
        let files = definition_files(world, &definition);
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
            let Some(handle) = load_definition_file(world, &definition, &path) else {
                continue;
            };
            world
                .resource_mut::<DefinitionRegistry>()
                .insert(DefinitionEntry {
                    kind: definition.kind.clone(),
                    name: name.clone(),
                    handle,
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
    reflect_asset.get(world, edit.handle.id())
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
    let registry = world.resource::<AppTypeRegistry>().read();
    let value = definition_value(world, entity, type_path, &registry)?;
    let field = if field_path.is_empty() {
        value.as_partial_reflect()
    } else {
        value.reflect_path(field_path).ok()?
    };
    crate::inspector::reflect_fields::reflect_to_json(field, &registry)
}

/// Write one field of the asset behind `handle`, and mark whatever definition
/// is editing it as having unsaved changes.
fn write_field(
    world: &mut World,
    handle: &UntypedHandle,
    type_path: &str,
    field_path: &str,
    json: &serde_json::Value,
) -> bool {
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
    let written = if field_path.is_empty() {
        crate::commands::apply_json_to_reflect(value.as_partial_reflect_mut(), json, &registry);
        true
    } else if let Ok(field) = value.reflect_path_mut(field_path) {
        crate::commands::apply_json_to_reflect(field, json, &registry);
        true
    } else {
        false
    };
    drop(registry);
    if written {
        mark_dirty(world, handle);
    }
    written
}

/// The entity editing the asset behind `handle`, if one is open.
fn edit_of_handle(world: &mut World, handle: &UntypedHandle) -> Option<Entity> {
    let mut edits = world.query::<(Entity, &DefinitionAssetEdit)>();
    edits
        .iter(world)
        .find(|(_, edit)| edit.handle == *handle)
        .map(|(entity, _)| entity)
}

fn mark_dirty(world: &mut World, handle: &UntypedHandle) {
    let Some(entity) = edit_of_handle(world, handle) else {
        return;
    };
    if let Some(mut edit) = world.get_mut::<DefinitionAssetEdit>(entity) {
        edit.dirty = true;
    }
}

/// Set one field of a definition, as one undo entry.
///
/// Keyed by handle rather than by the entity the definition was open on, so
/// undo still reaches the asset after the card has been closed and reopened.
pub struct SetDefinitionField {
    pub handle: UntypedHandle,
    pub type_path: String,
    pub field_path: String,
    pub old_json: serde_json::Value,
    pub new_json: serde_json::Value,
    /// Whether applying this edit changes which rows the inspector shows, as
    /// an enum variant or a list length does.
    pub rebuilds_rows: bool,
}

impl SetDefinitionField {
    fn apply(&self, world: &mut World, json: &serde_json::Value) {
        if !write_field(world, &self.handle, &self.type_path, &self.field_path, json) {
            return;
        }
        let open = edit_of_handle(world, &self.handle);
        if self.rebuilds_rows
            && let Some(open) = open
            && let Some(mut pending) =
                world.get_resource_mut::<crate::inspector::PendingInspectorRebuild>()
        {
            pending.0 = Some(open);
        }
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
/// Returns whether the edit belonged to a definition; a caller whose edit is
/// refused here writes to the selection as usual.
pub(crate) fn commit_definition_field(
    world: &mut World,
    type_path: &str,
    field_path: &str,
    new_json: &serde_json::Value,
) -> bool {
    let Some(entity) = open_edit_of(world, type_path) else {
        return false;
    };
    let Some(handle) = world
        .get::<DefinitionAssetEdit>(entity)
        .map(|edit| edit.handle.clone())
    else {
        return false;
    };
    let Some(current) = field_as_json(world, entity, type_path, field_path) else {
        return false;
    };
    let old_json = take_baseline(world, type_path, field_path).unwrap_or(current);
    let rebuilds_rows = field_rebuilds_rows(world, entity, type_path, field_path);
    let mut command: Box<dyn EditorCommand> = Box::new(SetDefinitionField {
        handle,
        type_path: type_path.to_string(),
        field_path: field_path.to_string(),
        old_json,
        new_json: new_json.clone(),
        rebuilds_rows,
    });
    command.execute(world);
    world
        .resource_mut::<CommandHistory>()
        .push_executed(command);
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
    let Some(handle) = world
        .get::<DefinitionAssetEdit>(entity)
        .map(|edit| edit.handle.clone())
    else {
        return false;
    };
    remember_baseline(world, entity, type_path, field_path);
    write_field(world, &handle, type_path, field_path, new_json)
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

// -- Opening, saving and creating -------------------------------------------

/// Load a definition file, or reuse the loaded one, and put it in the
/// inspector. A kind loaded elsewhere is only ever shown through the handle
/// its owner published.
pub fn open_definition_file(world: &mut World, path: &Path) -> bool {
    let Some(definition) = world
        .get_resource::<DefinitionAssetTypes>()
        .and_then(|types| types.for_file(path))
        .cloned()
    else {
        return false;
    };
    let Some(name) = definition.name_of_file(path) else {
        return false;
    };

    let known = world
        .resource::<DefinitionRegistry>()
        .get(&definition.kind, &name)
        .map(|entry| entry.handle.clone());
    let handle = match known {
        Some(handle) => handle,
        None => {
            if !definition.scanned {
                warn!("No {} named '{name}' is loaded", definition.kind);
                return false;
            }
            let Some(handle) = load_definition_file(world, &definition, path) else {
                return false;
            };
            world
                .resource_mut::<DefinitionRegistry>()
                .insert(DefinitionEntry {
                    kind: definition.kind.clone(),
                    name: name.clone(),
                    handle: handle.clone(),
                    path: path.to_path_buf(),
                });
            handle
        }
    };

    show_definition(world, &definition, &name, handle, path.to_path_buf());
    true
}

fn show_definition(
    world: &mut World,
    definition: &DefinitionAssetType,
    name: &str,
    handle: UntypedHandle,
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
                handle,
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
    let (kind, name, handle, path) = world.get::<DefinitionAssetEdit>(entity).map(|edit| {
        (
            edit.kind.clone(),
            edit.name.clone(),
            edit.handle.clone(),
            edit.path.clone(),
        )
    })?;
    save_definition(world, &kind, &name, &handle, &path)
}

/// Write the definition a file path names back to that file, without taking
/// the inspector off whatever it is showing.
fn save_definition_at(world: &mut World, path: &Path) -> Option<String> {
    let definition = world
        .get_resource::<DefinitionAssetTypes>()
        .and_then(|types| types.for_file(path))
        .cloned();
    let Some(definition) = definition else {
        warn!("asset.save: {} is not a definition file", path.display());
        return None;
    };
    let name = definition.name_of_file(path)?;
    let Some(handle) = world
        .resource::<DefinitionRegistry>()
        .get(&definition.kind, &name)
        .map(|entry| entry.handle.clone())
    else {
        warn!(
            "asset.save: no {} named '{name}' is loaded",
            definition.kind
        );
        return None;
    };
    save_definition(world, &definition.kind, &name, &handle, path)
}

/// Write one definition back to the file it came from and record where it
/// landed.
fn save_definition(
    world: &mut World,
    kind: &str,
    name: &str,
    handle: &UntypedHandle,
    path: &Path,
) -> Option<String> {
    let path = match write_definition_at(world, name, handle, path) {
        Ok(path) => path,
        Err(err) => {
            warn!("asset.save: failed to write '{name}': {err}");
            return None;
        }
    };
    if let Some(open) = edit_of_handle(world, handle)
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
            handle: handle.clone(),
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

/// The kind saved materials list under, so the material directory is browsed,
/// opened and edited through the same registry as any other definition.
pub const MATERIAL_KIND: &str = "material";

pub(crate) fn plugin(app: &mut App) {
    app.world_mut()
        .get_resource_or_init::<DefinitionAssetTypes>()
        .register(
            DefinitionAssetType::new(
                MATERIAL_KIND,
                "Material",
                "bevy_pbr::pbr_material::StandardMaterial",
                crate::material_assets::MATERIALS_DIR,
                crate::material_assets::MATERIAL_FILE_SUFFIX,
            )
            .loaded_elsewhere(),
        );
    app.init_resource::<DefinitionRegistry>()
        .init_resource::<DefinitionEditSession>()
        .init_resource::<OpenDefinition>()
        .init_resource::<DefinitionScanPending>()
        .add_systems(OnEnter(crate::AppState::Editor), watch_definition_files)
        .add_systems(
            Update,
            (
                follow_registered_types.run_if(resource_changed::<DefinitionAssetTypes>),
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
    let Some(definition) = definition_of_kind(world, MATERIAL_KIND) else {
        return;
    };
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
            .map(|(name, _)| definition_path(project, &definition, name))
            .collect()
    };

    let mut registry = world.resource_mut::<DefinitionRegistry>();
    registry.entries.retain(|entry| entry.kind != MATERIAL_KIND);
    for ((name, handle), path) in saved.into_iter().zip(paths) {
        registry.insert(DefinitionEntry {
            kind: MATERIAL_KIND.to_string(),
            name,
            handle,
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

fn definition_of_kind(world: &World, kind: &str) -> Option<DefinitionAssetType> {
    world
        .get_resource::<DefinitionAssetTypes>()
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
        r#type(
            String,
            doc = "Kind of definition to create, as its type registered it."
        ),
        name(
            String,
            doc = "Name to create it under. Defaults to the next free name."
        )
    )
)]
pub fn asset_new(params: In<OperatorParameters>, mut commands: Commands) -> OperatorResult {
    let Some(kind) = params.as_str("type").map(str::to_owned) else {
        warn!("asset.new: no type given");
        return OperatorResult::Cancelled;
    };
    let name = params.as_str("name").map(str::to_owned);
    commands.queue(move |world: &mut World| {
        new_definition(world, &kind, name.as_deref());
    });
    OperatorResult::Finished
}

fn new_definition(world: &mut World, kind: &str, name: Option<&str>) {
    let Some((definition, name, handle, path)) = create_definition(world, kind, name) else {
        return;
    };
    show_definition(world, &definition, &name, handle, path);
    report_to_caller(world, format!("Created {kind} '{name}'"));
}

/// Write a fresh default value of a registered kind to its file and register
/// it.
fn create_definition(
    world: &mut World,
    kind: &str,
    name: Option<&str>,
) -> Option<(DefinitionAssetType, String, UntypedHandle, PathBuf)> {
    let Some(definition) = definition_of_kind(world, kind) else {
        warn!("asset.new: '{kind}' is not a registered definition type");
        return None;
    };
    if !definition.scanned {
        warn!("asset.new: {kind} definitions are created by whoever loads them");
        return None;
    }
    let name = match name {
        Some(name) => sanitize_definition_name(name),
        None => next_free_name(world, &definition),
    };
    if world
        .resource::<DefinitionRegistry>()
        .get(kind, &name)
        .is_some()
    {
        warn!("asset.new: a {kind} named '{name}' already exists");
        return None;
    }
    if world
        .get_resource::<ProjectRoot>()
        .is_some_and(|project| definition_path(project, &definition, &name).exists())
    {
        warn!("asset.new: a file for the {kind} '{name}' is already there");
        return None;
    }
    let Some(handle) = default_definition_value(world, &definition) else {
        warn!(
            "asset.new: {} has no registered default",
            definition.type_path
        );
        return None;
    };
    let path = match write_definition_file(world, &definition, &name, &handle) {
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
            handle: handle.clone(),
            path: path.clone(),
        });
    Some((definition, name, handle, path))
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
    let Some(definition) = world
        .get_resource::<DefinitionAssetTypes>()
        .and_then(|types| types.for_file(path))
        .cloned()
    else {
        warn!("asset.delete: {} is not a definition file", path.display());
        return;
    };
    if !definition.scanned {
        warn!(
            "asset.delete: {} definitions are removed by whoever loads them",
            definition.kind
        );
        return;
    }
    let Some(name) = definition.name_of_file(path) else {
        return;
    };
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
        value(String, doc = "Value to set, as JSON or as a plain scalar.")
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
        warn!("asset.set: '{field}' is not a field of {type_path}");
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

    fn item_type() -> DefinitionAssetType {
        DefinitionAssetType::new(
            "item",
            "Item",
            "jackdaw::definition_assets::tests::ItemDef",
            "content/items",
            ".item.bsn",
        )
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
        app.init_resource::<DefinitionAssetTypes>();
        app.init_resource::<DefinitionRegistry>();
        app.insert_resource(ProjectRoot {
            root: tmp.path().to_path_buf(),
            config: crate::project::ProjectConfig::default(),
        });
        app.world_mut()
            .resource_mut::<DefinitionAssetTypes>()
            .register(item_type());
        (app, tmp)
    }

    fn item_of(app: &App, handle: &UntypedHandle) -> ItemDef {
        app.world()
            .resource::<Assets<ItemDef>>()
            .get(&handle.clone().typed::<ItemDef>())
            .expect("the definition is in its store")
            .clone()
    }

    #[test]
    fn a_written_definition_reloads_from_its_directory() {
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

        write_definition_file(app.world(), &item_type(), "torch", &handle.untyped())
            .expect("the file is written");
        assert!(
            tmp.path()
                .join("assets/content/items/torch.item.bsn")
                .is_file()
        );

        let scan = rescan_definitions(app.world_mut());
        assert_eq!(scan.added, vec![("item".to_string(), "torch".to_string())]);

        let loaded = app
            .world()
            .resource::<DefinitionRegistry>()
            .get("item", "torch")
            .expect("the scan registered it")
            .handle
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
        write_definition_file(app.world(), &item_type(), "torch", &handle.untyped())
            .expect("the file is written");
        rescan_definitions(app.world_mut());

        std::fs::remove_file(tmp.path().join("assets/content/items/torch.item.bsn"))
            .expect("the file is removed");
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
    fn two_kinds_sharing_a_name_keep_their_own_files() {
        let (mut app, tmp) = definition_app();
        app.world_mut()
            .resource_mut::<DefinitionAssetTypes>()
            .register(DefinitionAssetType::new(
                "mob",
                "Mob",
                "jackdaw::definition_assets::tests::ItemDef",
                "content/mobs",
                ".mob.bsn",
            ));
        let handle = app
            .world_mut()
            .resource_mut::<Assets<ItemDef>>()
            .add(ItemDef::default());

        create_definition(app.world_mut(), "item", Some("rat")).expect("the item is created");
        create_definition(app.world_mut(), "mob", Some("rat")).expect("the mob is created");
        let _ = handle;

        assert!(
            tmp.path()
                .join("assets/content/items/rat.item.bsn")
                .is_file()
        );
        assert!(tmp.path().join("assets/content/mobs/rat.mob.bsn").is_file());
        let registry = app.world().resource::<DefinitionRegistry>();
        assert!(registry.get("item", "rat").is_some());
        assert!(registry.get("mob", "rat").is_some());
    }

    #[test]
    fn names_sanitize_to_one_file_each() {
        assert_eq!(sanitize_definition_name("torch"), "torch");
        assert_eq!(sanitize_definition_name("a/b"), "a_b");
        assert_eq!(sanitize_definition_name("../escape"), ".._escape");
        assert_eq!(sanitize_definition_name("  "), "definition");
    }
}
