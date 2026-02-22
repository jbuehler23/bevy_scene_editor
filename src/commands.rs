use std::any::TypeId;

use bevy::{
    ecs::{
        component::ComponentId,
        reflect::{AppTypeRegistry, ReflectComponent},
    },
    prelude::*,
};

use crate::EditorEntity;

pub struct CommandHistoryPlugin;

impl Plugin for CommandHistoryPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CommandHistory::default())
            .add_systems(Update, handle_undo_redo_keys);
    }
}

// ---------------------------------------------------------------------------
// EditorCommand trait
// ---------------------------------------------------------------------------

pub trait EditorCommand: Send + Sync + 'static {
    fn execute(&self, world: &mut World);
    fn undo(&self, world: &mut World);
    fn description(&self) -> &str;
}

// ---------------------------------------------------------------------------
// CommandHistory resource
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct CommandHistory {
    pub undo_stack: Vec<Box<dyn EditorCommand>>,
    pub redo_stack: Vec<Box<dyn EditorCommand>>,
}

impl CommandHistory {
    pub fn execute(&mut self, command: Box<dyn EditorCommand>, world: &mut World) {
        command.execute(world);
        self.undo_stack.push(command);
        self.redo_stack.clear();
    }

    pub fn undo(&mut self, world: &mut World) {
        if let Some(command) = self.undo_stack.pop() {
            command.undo(world);
            self.redo_stack.push(command);
        }
    }

    pub fn redo(&mut self, world: &mut World) {
        if let Some(command) = self.redo_stack.pop() {
            command.execute(world);
            self.undo_stack.push(command);
        }
    }
}

// ---------------------------------------------------------------------------
// CommandGroup — atomic multi-step command
// ---------------------------------------------------------------------------

pub struct CommandGroup {
    pub commands: Vec<Box<dyn EditorCommand>>,
    pub label: String,
}

impl EditorCommand for CommandGroup {
    fn execute(&self, world: &mut World) {
        for cmd in &self.commands {
            cmd.execute(world);
        }
    }

    fn undo(&self, world: &mut World) {
        for cmd in self.commands.iter().rev() {
            cmd.undo(world);
        }
    }

    fn description(&self) -> &str {
        &self.label
    }
}

// ---------------------------------------------------------------------------
// SetComponentField
// ---------------------------------------------------------------------------

pub struct SetComponentField {
    pub entity: Entity,
    pub component_type_id: TypeId,
    pub field_path: String,
    pub old_value: Box<dyn PartialReflect>,
    pub new_value: Box<dyn PartialReflect>,
}

impl EditorCommand for SetComponentField {
    fn execute(&self, world: &mut World) {
        apply_reflected_value(
            world,
            self.entity,
            self.component_type_id,
            &self.field_path,
            &*self.new_value,
        );
    }

    fn undo(&self, world: &mut World) {
        apply_reflected_value(
            world,
            self.entity,
            self.component_type_id,
            &self.field_path,
            &*self.old_value,
        );
    }

    fn description(&self) -> &str {
        "Set component field"
    }
}

fn apply_reflected_value(
    world: &mut World,
    entity: Entity,
    component_type_id: TypeId,
    field_path: &str,
    value: &dyn PartialReflect,
) {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();

    let Some(registration) = registry.get(component_type_id) else {
        return;
    };
    let Some(reflect_component) = registration.data::<ReflectComponent>() else {
        return;
    };

    let Some(reflected) = reflect_component.reflect_mut(world.entity_mut(entity)) else {
        return;
    };

    if field_path.is_empty() {
        // Apply to the entire component (e.g. a top-level enum component)
        reflected.into_inner().apply(value);
    } else {
        let Ok(field) = reflected.into_inner().reflect_path_mut(field_path) else {
            return;
        };
        field.apply(value);
    }
}

// ---------------------------------------------------------------------------
// SetTransform
// ---------------------------------------------------------------------------

pub struct SetTransform {
    pub entity: Entity,
    pub old_transform: Transform,
    pub new_transform: Transform,
}

impl EditorCommand for SetTransform {
    fn execute(&self, world: &mut World) {
        if let Some(mut transform) = world.get_mut::<Transform>(self.entity) {
            *transform = self.new_transform;
        }
    }

    fn undo(&self, world: &mut World) {
        if let Some(mut transform) = world.get_mut::<Transform>(self.entity) {
            *transform = self.old_transform;
        }
    }

    fn description(&self) -> &str {
        "Set transform"
    }
}

// ---------------------------------------------------------------------------
// ReparentEntity
// ---------------------------------------------------------------------------

pub struct ReparentEntity {
    pub entity: Entity,
    pub old_parent: Option<Entity>,
    pub new_parent: Option<Entity>,
}

impl EditorCommand for ReparentEntity {
    fn execute(&self, world: &mut World) {
        set_parent(world, self.entity, self.new_parent);
    }

    fn undo(&self, world: &mut World) {
        set_parent(world, self.entity, self.old_parent);
    }

    fn description(&self) -> &str {
        "Reparent entity"
    }
}

fn set_parent(world: &mut World, entity: Entity, parent: Option<Entity>) {
    match parent {
        Some(p) => {
            world.entity_mut(entity).insert(ChildOf(p));
        }
        None => {
            world.entity_mut(entity).remove::<ChildOf>();
        }
    }
}

// ---------------------------------------------------------------------------
// AddComponent — insert a component via reflection
// ---------------------------------------------------------------------------

pub struct AddComponent {
    pub entity: Entity,
    pub type_id: TypeId,
    pub component_id: ComponentId,
}

impl EditorCommand for AddComponent {
    fn execute(&self, world: &mut World) {
        let registry = world.resource::<AppTypeRegistry>().clone();
        let registry = registry.read();

        let Some(registration) = registry.get(self.type_id) else {
            return;
        };

        // Create default value
        let Some(reflect_default) = registration.data::<ReflectDefault>() else {
            warn!("No ReflectDefault for component — cannot add");
            return;
        };
        let default_value = reflect_default.default();
        let Some(reflect_component) = registration.data::<ReflectComponent>() else {
            return;
        };

        reflect_component.insert(
            &mut world.entity_mut(self.entity),
            default_value.as_partial_reflect(),
            &registry,
        );
    }

    fn undo(&self, world: &mut World) {
        if let Ok(mut entity) = world.get_entity_mut(self.entity) {
            entity.remove_by_id(self.component_id);
        }
    }

    fn description(&self) -> &str {
        "Add component"
    }
}

// ---------------------------------------------------------------------------
// RemoveComponent — remove a component via reflection (for undo of add)
// ---------------------------------------------------------------------------

pub struct RemoveComponent {
    pub entity: Entity,
    pub type_id: TypeId,
    pub component_id: ComponentId,
    /// Snapshot of the component's value before removal, for undo.
    pub snapshot: Box<dyn PartialReflect>,
}

impl EditorCommand for RemoveComponent {
    fn execute(&self, world: &mut World) {
        if let Ok(mut entity) = world.get_entity_mut(self.entity) {
            entity.remove_by_id(self.component_id);
        }
    }

    fn undo(&self, world: &mut World) {
        let registry = world.resource::<AppTypeRegistry>().clone();
        let registry = registry.read();

        let Some(registration) = registry.get(self.type_id) else {
            return;
        };
        let Some(reflect_component) = registration.data::<ReflectComponent>() else {
            return;
        };

        reflect_component.insert(
            &mut world.entity_mut(self.entity),
            &*self.snapshot,
            &registry,
        );
    }

    fn description(&self) -> &str {
        "Remove component"
    }
}

// ---------------------------------------------------------------------------
// SpawnEntity — stores bundles of components for undo
// ---------------------------------------------------------------------------

pub struct SpawnEntity {
    /// The entity that was spawned (set after first execute).
    pub spawned: Option<Entity>,
    /// Builder function that spawns the entity and returns its Entity id.
    pub spawn_fn: Box<dyn Fn(&mut World) -> Entity + Send + Sync>,
    pub label: String,
}

impl EditorCommand for SpawnEntity {
    fn execute(&self, world: &mut World) {
        let _entity = (self.spawn_fn)(world);
    }

    fn undo(&self, _world: &mut World) {
        // TODO: Track spawned entity for despawn on undo
    }

    fn description(&self) -> &str {
        &self.label
    }
}

// ---------------------------------------------------------------------------
// DespawnEntity — snapshots entity state via DynamicScene for undo
// ---------------------------------------------------------------------------

pub struct DespawnEntity {
    pub entity: Entity,
    pub scene_snapshot: DynamicScene,
    pub parent: Option<Entity>,
    pub label: String,
}

impl DespawnEntity {
    pub fn from_world(world: &World, entity: Entity) -> Self {
        let parent = world.get::<ChildOf>(entity).map(|c| c.0);
        let scene = snapshot_entity(world, entity);
        Self {
            entity,
            scene_snapshot: scene,
            parent,
            label: format!("Despawn entity {entity}"),
        }
    }
}

impl EditorCommand for DespawnEntity {
    fn execute(&self, world: &mut World) {
        if let Ok(entity_mut) = world.get_entity_mut(self.entity) {
            entity_mut.despawn();
        }
    }

    fn undo(&self, world: &mut World) {
        // Re-build the scene from scratch and write it back
        let scene = snapshot_rebuild(&self.scene_snapshot);
        let _result = scene.write_to_world(world, &mut Default::default());
    }

    fn description(&self) -> &str {
        &self.label
    }
}

/// Create a DynamicScene snapshot of a single entity and all its descendants.
pub(crate) fn snapshot_entity(world: &World, entity: Entity) -> DynamicScene {
    let mut entities = Vec::new();
    collect_entity_ids(world, entity, &mut entities);
    DynamicSceneBuilder::from_world(world)
        .extract_entities(entities.into_iter())
        .build()
}

pub fn collect_entity_ids(world: &World, entity: Entity, out: &mut Vec<Entity>) {
    out.push(entity);
    if let Some(children) = world.get::<Children>(entity) {
        for child in children.iter() {
            if world.get::<EditorEntity>(child).is_none() {
                collect_entity_ids(world, child, out);
            }
        }
    }
}

/// Rebuild a DynamicScene by copying its entity data (since DynamicScene doesn't impl Clone).
pub(crate) fn snapshot_rebuild(scene: &DynamicScene) -> DynamicScene {
    DynamicScene {
        resources: scene
            .resources
            .iter()
            .map(|r| r.to_dynamic())
            .collect(),
        entities: scene
            .entities
            .iter()
            .map(|e| bevy::scene::DynamicEntity {
                entity: e.entity,
                components: e.components.iter().map(|c| c.to_dynamic()).collect(),
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Keyboard shortcut system
// ---------------------------------------------------------------------------

fn handle_undo_redo_keys(world: &mut World) {
    let keyboard = world.resource::<ButtonInput<KeyCode>>();
    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let z_pressed = keyboard.just_pressed(KeyCode::KeyZ);

    if !ctrl || !z_pressed {
        return;
    }

    let mut history = world.resource_mut::<CommandHistory>();
    // Take ownership to avoid borrow conflict with world
    let command = if shift {
        history.redo_stack.pop()
    } else {
        history.undo_stack.pop()
    };

    if let Some(command) = command {
        if shift {
            command.execute(world);
            world
                .resource_mut::<CommandHistory>()
                .undo_stack
                .push(command);
        } else {
            command.undo(world);
            world
                .resource_mut::<CommandHistory>()
                .redo_stack
                .push(command);
        }
    }
}
