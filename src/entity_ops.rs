use std::any::TypeId;
use std::path::Path;

use bevy::{
    ecs::{
        reflect::{AppTypeRegistry, ReflectComponent},
        system::SystemState,
    },
    gltf::GltfAssetLabel,
    prelude::*,
};

use crate::{
    commands::{CommandHistory, DespawnEntity, EditorCommand},
    selection::{Selected, Selection},
    EditorEntity,
};
use bevy::input_focus::InputFocus;

// ---------------------------------------------------------------------------
// Component clipboard
// ---------------------------------------------------------------------------

/// Resource storing copied component data for paste operations.
#[derive(Resource, Default)]
pub struct ComponentClipboard {
    /// Snapshots of component data: (type_id, reflected_data)
    pub data: Vec<(TypeId, Box<dyn PartialReflect>)>,
}

// Re-export from bevy_jsn
pub use bevy_jsn::GltfSource;

pub struct EntityOpsPlugin;

impl Plugin for EntityOpsPlugin {
    fn build(&self, app: &mut App) {
        // Note: GltfSource type registration is handled by JsnPlugin
        app.init_resource::<ComponentClipboard>()
            .add_systems(Update, handle_entity_keys);
    }
}

// ---------------------------------------------------------------------------
// Entity templates
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityTemplate {
    Empty,
    Cube,
    Sphere,
    PointLight,
    DirectionalLight,
    SpotLight,
    Camera3d,
}

impl EntityTemplate {
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "Empty Entity",
            Self::Cube => "Cube",
            Self::Sphere => "Sphere",
            Self::PointLight => "Point Light",
            Self::DirectionalLight => "Directional Light",
            Self::SpotLight => "Spot Light",
            Self::Camera3d => "Camera",
        }
    }
}

// ---------------------------------------------------------------------------
// Create entity
// ---------------------------------------------------------------------------

pub fn create_entity(
    commands: &mut Commands,
    template: EntityTemplate,
    selection: &mut Selection,
) -> Entity {
    let entity = match template {
        EntityTemplate::Empty => commands
            .spawn((Name::new("Empty"), Transform::default()))
            .id(),
        EntityTemplate::Cube => commands
            .spawn((
                Name::new("Cube"),
                crate::brush::Brush::cuboid(0.5, 0.5, 0.5),
                Transform::default(),
                Visibility::default(),
            ))
            .id(),
        EntityTemplate::Sphere => commands
            .spawn((
                Name::new("Sphere"),
                crate::brush::Brush::sphere(0.5),
                Transform::default(),
                Visibility::default(),
            ))
            .id(),
        EntityTemplate::PointLight => commands
            .spawn((
                Name::new("Point Light"),
                PointLight {
                    shadows_enabled: true,
                    ..default()
                },
                Transform::from_xyz(0.0, 3.0, 0.0),
            ))
            .id(),
        EntityTemplate::DirectionalLight => commands
            .spawn((
                Name::new("Directional Light"),
                DirectionalLight {
                    shadows_enabled: true,
                    ..default()
                },
                Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, 0.4, 0.0)),
            ))
            .id(),
        EntityTemplate::SpotLight => commands
            .spawn((
                Name::new("Spot Light"),
                SpotLight {
                    shadows_enabled: true,
                    ..default()
                },
                Transform::from_xyz(0.0, 3.0, 0.0).looking_at(Vec3::ZERO, Vec3::Y),
            ))
            .id(),
        EntityTemplate::Camera3d => commands
            .spawn((
                Name::new("Camera"),
                Camera3d::default(),
                Transform::from_xyz(0.0, 2.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
            ))
            .id(),
    };

    selection.select_single(commands, entity);
    entity
}

/// World-access version of `create_entity` — used from menu actions and other deferred contexts.
pub fn create_entity_in_world(world: &mut World, template: EntityTemplate) {
    let mut system_state: SystemState<(
        Commands,
        ResMut<Selection>,
    )> = SystemState::new(world);
    let (mut commands, mut selection) = system_state.get_mut(world);
    create_entity(&mut commands, template, &mut selection);
    system_state.apply(world);
}

// ---------------------------------------------------------------------------
// GLTF loading
// ---------------------------------------------------------------------------

pub fn spawn_gltf(
    commands: &mut Commands,
    asset_server: &AssetServer,
    path: &str,
    position: Vec3,
    selection: &mut Selection,
) -> Entity {
    let file_name = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "GLTF Model".to_string());
    let scene_index = 0;
    let asset_path = to_asset_path(path);
    let scene = asset_server.load(GltfAssetLabel::Scene(scene_index).from_asset(asset_path));
    let entity = commands
        .spawn((
            Name::new(file_name),
            GltfSource {
                path: path.to_string(),
                scene_index,
            },
            SceneRoot(scene),
            Transform::from_translation(position),
        ))
        .id();
    selection.select_single(commands, entity);
    entity
}

pub fn spawn_gltf_in_world(world: &mut World, path: &str, position: Vec3) {
    let mut system_state: SystemState<(Commands, Res<AssetServer>, ResMut<Selection>)> =
        SystemState::new(world);
    let (mut commands, asset_server, mut selection) = system_state.get_mut(world);
    spawn_gltf(&mut commands, &asset_server, path, position, &mut selection);
    system_state.apply(world);
}

// ---------------------------------------------------------------------------
// Delete entity
// ---------------------------------------------------------------------------

pub fn delete_selected(world: &mut World) {
    let selection = world.resource::<Selection>();
    let entities: Vec<Entity> = selection.entities.clone();

    if entities.is_empty() {
        return;
    }

    // Build commands for each entity
    let mut cmds: Vec<Box<dyn EditorCommand>> = Vec::new();
    for &entity in &entities {
        if world.get_entity(entity).is_err() {
            continue;
        }
        if world.get::<EditorEntity>(entity).is_some() {
            continue;
        }
        cmds.push(Box::new(DespawnEntity::from_world(world, entity)));
    }

    // Clear selection
    let mut selection = world.resource_mut::<Selection>();
    selection.entities.clear();

    // Execute all despawn commands
    for cmd in &cmds {
        cmd.execute(world);
    }

    // Push as a single group command
    if !cmds.is_empty() {
        let group = crate::commands::CommandGroup {
            commands: cmds,
            label: "Delete entities".to_string(),
        };
        let mut history = world.resource_mut::<CommandHistory>();
        history.undo_stack.push(Box::new(group));
        history.redo_stack.clear();
    }
}

// ---------------------------------------------------------------------------
// Duplicate entity
// ---------------------------------------------------------------------------

pub fn duplicate_selected(world: &mut World) {
    let selection = world.resource::<Selection>();
    let entities: Vec<Entity> = selection.entities.clone();

    if entities.is_empty() {
        return;
    }

    // Deselect current entities first
    for &entity in &entities {
        if let Ok(mut ec) = world.get_entity_mut(entity) {
            ec.remove::<Selected>();
        }
    }

    let mut new_entities = Vec::new();

    for &entity in &entities {
        if world.get_entity(entity).is_err() {
            continue;
        }
        if world.get::<EditorEntity>(entity).is_some() {
            continue;
        }

        // Snapshot the entity (and descendants) via DynamicSceneBuilder
        let mut snapshot_entities = Vec::new();
        crate::commands::collect_entity_ids(world, entity, &mut snapshot_entities);
        let scene = DynamicSceneBuilder::from_world(world)
            .extract_entities(snapshot_entities.into_iter())
            .build();

        // Write the snapshot back to create a clone
        let mut entity_map = Default::default();
        if scene.write_to_world(world, &mut entity_map).is_err() {
            continue;
        }

        // Find the cloned root entity
        let Some(&new_root) = entity_map.get(&entity) else {
            continue;
        };

        // Rename to "<name> (Copy)"
        if let Some(name) = world.get::<Name>(new_root) {
            let new_name = format!("{} (Copy)", name.as_str());
            world.entity_mut(new_root).insert(Name::new(new_name));
        }

        // Offset transform slightly so it's not on top
        if let Some(mut transform) = world.get_mut::<Transform>(new_root) {
            transform.translation += Vec3::new(0.5, 0.0, 0.5);
        }

        // Preserve parent relationship from original
        let parent = world.get::<ChildOf>(entity).map(|c| c.0);
        if let Some(parent) = parent {
            world.entity_mut(new_root).insert(ChildOf(parent));
        } else {
            // Original was a root entity — remove any ChildOf the scene write may have added
            world.entity_mut(new_root).remove::<ChildOf>();
        }

        new_entities.push(new_root);
    }

    // Select the new entities
    let mut selection = world.resource_mut::<Selection>();
    selection.entities = new_entities;
    for &entity in &selection.entities.clone() {
        world.entity_mut(entity).insert(Selected);
    }
}

// ---------------------------------------------------------------------------
// Keyboard shortcuts
// ---------------------------------------------------------------------------

fn handle_entity_keys(world: &mut World) {
    // Don't process entity keys when a text input is focused
    let has_input_focus = world.resource::<InputFocus>().0.is_some();
    if has_input_focus {
        return;
    }

    // Don't process entity keys during modal transform operations
    let modal_active = world
        .resource::<crate::modal_transform::ModalTransformState>()
        .active
        .is_some();
    if modal_active {
        return;
    }

    let keyboard = world.resource::<ButtonInput<KeyCode>>();
    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let alt = keyboard.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let delete_pressed = keyboard.just_pressed(KeyCode::Delete)
        || keyboard.just_pressed(KeyCode::Backspace);
    let d_pressed = keyboard.just_pressed(KeyCode::KeyD);
    let g_pressed = keyboard.just_pressed(KeyCode::KeyG);
    let r_pressed = keyboard.just_pressed(KeyCode::KeyR);
    let s_pressed = keyboard.just_pressed(KeyCode::KeyS);
    let h_pressed = keyboard.just_pressed(KeyCode::KeyH);
    let c_pressed = keyboard.just_pressed(KeyCode::KeyC);
    let v_pressed = keyboard.just_pressed(KeyCode::KeyV);

    if delete_pressed {
        delete_selected(world);
    } else if ctrl && d_pressed {
        duplicate_selected(world);
    } else if ctrl && c_pressed {
        copy_components(world);
    } else if ctrl && v_pressed {
        paste_components(world);
    } else if alt && g_pressed {
        reset_transform_selected(world, TransformReset::Position);
    } else if alt && r_pressed {
        reset_transform_selected(world, TransformReset::Rotation);
    } else if alt && s_pressed {
        reset_transform_selected(world, TransformReset::Scale);
    } else if h_pressed && !ctrl && !alt {
        toggle_visibility_selected(world);
    }
}

// ---------------------------------------------------------------------------
// Transform reset
// ---------------------------------------------------------------------------

enum TransformReset {
    Position,
    Rotation,
    Scale,
}

fn reset_transform_selected(world: &mut World, reset: TransformReset) {
    let selection = world.resource::<Selection>();
    let entities: Vec<Entity> = selection.entities.clone();

    if entities.is_empty() {
        return;
    }

    let mut cmds: Vec<Box<dyn EditorCommand>> = Vec::new();

    for &entity in &entities {
        if world.get_entity(entity).is_err() {
            continue;
        }
        let Some(&old_transform) = world.get::<Transform>(entity) else {
            continue;
        };

        let new_transform = match reset {
            TransformReset::Position => Transform {
                translation: Vec3::ZERO,
                ..old_transform
            },
            TransformReset::Rotation => Transform {
                rotation: Quat::IDENTITY,
                ..old_transform
            },
            TransformReset::Scale => Transform {
                scale: Vec3::ONE,
                ..old_transform
            },
        };

        if old_transform == new_transform {
            continue;
        }

        let cmd = crate::commands::SetTransform {
            entity,
            old_transform,
            new_transform,
        };
        cmd.execute(world);
        cmds.push(Box::new(cmd));
    }

    if !cmds.is_empty() {
        let label = match reset {
            TransformReset::Position => "Reset position",
            TransformReset::Rotation => "Reset rotation",
            TransformReset::Scale => "Reset scale",
        };
        let group = crate::commands::CommandGroup {
            commands: cmds,
            label: label.to_string(),
        };
        let mut history = world.resource_mut::<CommandHistory>();
        history.undo_stack.push(Box::new(group));
        history.redo_stack.clear();
    }
}

// ---------------------------------------------------------------------------
// Copy/Paste components
// ---------------------------------------------------------------------------

/// Copy all reflected components from the primary selected entity to the clipboard.
fn copy_components(world: &mut World) {
    let selection = world.resource::<Selection>();
    let Some(primary) = selection.primary() else {
        return;
    };

    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    let Ok(entity_ref) = world.get_entity(primary) else {
        return;
    };

    let mut data = Vec::new();
    for registration in registry.iter() {
        let type_id = registration.type_id();
        let Some(reflect_component) = registration.data::<ReflectComponent>() else {
            continue;
        };
        let Some(reflected) = reflect_component.reflect(entity_ref) else {
            continue;
        };

        // Skip internal types
        let path = registration.type_info().type_path_table().path();
        if path.starts_with("bevy_scene_editor")
            || path.contains("ChildOf")
            || path.contains("Children")
        {
            continue;
        }

        data.push((type_id, reflected.to_dynamic()));
    }

    drop(registry);

    let mut clipboard = world.resource_mut::<ComponentClipboard>();
    clipboard.data = data;
}

/// Paste component values from clipboard onto all selected entities.
fn paste_components(world: &mut World) {
    let clipboard_data: Vec<(TypeId, Box<dyn PartialReflect>)> = {
        let clipboard = world.resource::<ComponentClipboard>();
        if clipboard.data.is_empty() {
            return;
        }
        clipboard
            .data
            .iter()
            .map(|(tid, val)| (*tid, val.to_dynamic()))
            .collect()
    };

    let selection = world.resource::<Selection>();
    let entities: Vec<Entity> = selection.entities.clone();

    if entities.is_empty() {
        return;
    }

    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    for &entity in &entities {
        if world.get_entity(entity).is_err() {
            continue;
        }

        for (type_id, value) in &clipboard_data {
            let Some(registration) = registry.get(*type_id) else {
                continue;
            };
            let Some(reflect_component) = registration.data::<ReflectComponent>() else {
                continue;
            };

            // Apply: if component exists, update it; if not, insert it
            let has_component = {
                let entity_ref = world.get_entity(entity).unwrap();
                reflect_component.reflect(entity_ref).is_some()
            };
            if has_component {
                let existing = reflect_component
                    .reflect_mut(world.entity_mut(entity))
                    .unwrap();
                existing.into_inner().apply(value.as_ref());
            } else {
                reflect_component.insert(
                    &mut world.entity_mut(entity),
                    value.as_ref(),
                    &registry,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Visibility toggle
// ---------------------------------------------------------------------------

fn toggle_visibility_selected(world: &mut World) {
    let selection = world.resource::<Selection>();
    let entities: Vec<Entity> = selection.entities.clone();

    if entities.is_empty() {
        return;
    }

    let mut cmds: Vec<Box<dyn EditorCommand>> = Vec::new();

    for &entity in &entities {
        let current = world
            .get::<Visibility>(entity)
            .copied()
            .unwrap_or(Visibility::Inherited);

        let new_visibility = match current {
            Visibility::Hidden => Visibility::Inherited,
            _ => Visibility::Hidden,
        };

        let cmd = crate::commands::SetComponentField {
            entity,
            component_type_id: std::any::TypeId::of::<Visibility>(),
            field_path: String::new(),
            old_value: Box::new(current),
            new_value: Box::new(new_visibility),
        };
        cmd.execute(world);
        cmds.push(Box::new(cmd));
    }

    if !cmds.is_empty() {
        let group = crate::commands::CommandGroup {
            commands: cmds,
            label: "Toggle visibility".to_string(),
        };
        let mut history = world.resource_mut::<CommandHistory>();
        history.undo_stack.push(Box::new(group));
        history.redo_stack.clear();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a filesystem path to a Bevy asset path (relative to the assets directory).
///
/// Bevy's default asset source reads from `<base>/assets/` where `<base>` is
/// `BEVY_ASSET_ROOT`, `CARGO_MANIFEST_DIR`, or the executable's parent directory.
fn to_asset_path(path: &str) -> String {
    let path = Path::new(path);
    if let Some(assets_dir) = get_assets_base_dir() {
        if let Ok(relative) = path.strip_prefix(&assets_dir) {
            return relative.to_string_lossy().to_string();
        }
    }
    // Fallback: if already a simple relative path, use as-is
    if !path.is_absolute() {
        return path.to_string_lossy().to_string();
    }
    warn!(
        "Cannot load '{}': file is outside the assets directory. \
         Move it into your project's assets/ folder.",
        path.display()
    );
    path.to_string_lossy().to_string()
}

/// Get the absolute path of Bevy's assets directory, matching FileAssetReader logic.
fn get_assets_base_dir() -> Option<std::path::PathBuf> {
    let base = if let Ok(dir) = std::env::var("BEVY_ASSET_ROOT") {
        std::path::PathBuf::from(dir)
    } else if let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") {
        std::path::PathBuf::from(dir)
    } else {
        std::env::current_exe()
            .ok()?
            .parent()?
            .to_path_buf()
    };
    Some(base.join("assets"))
}
