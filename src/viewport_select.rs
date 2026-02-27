use bevy::{
    picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility},
    prelude::*,
    ui::UiGlobalTransform,
};
use bevy::input_focus::InputFocus;
use jackdaw_camera::JackdawCameraSettings;
use jackdaw_feathers::context_menu::spawn_context_menu;
use jackdaw_widgets::context_menu::{ContextMenuAction, ContextMenuCloseSet, ContextMenuState};

use crate::{
    entity_ops,
    gizmos::GizmoDragState,
    modal_transform::{ModalTransformState, ViewportDragState},
    selection::{Selected, Selection},
    viewport::SceneViewport,
    EditorEntity,
};

pub struct ViewportSelectPlugin;

impl Plugin for ViewportSelectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BoxSelectState>()
            .add_systems(
                Update,
                (
                    handle_viewport_click,
                    handle_box_select,
                    handle_viewport_right_click.after(ContextMenuCloseSet),
                ),
            )
            .add_observer(on_viewport_context_menu_action);
    }
}

// ---------------------------------------------------------------------------
// Box select state
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct BoxSelectState {
    pub active: bool,
    pub start: Vec2,
    pub current: Vec2,
}

// ---------------------------------------------------------------------------
// Click-to-select in viewport using position-based proximity
// ---------------------------------------------------------------------------

fn handle_viewport_click(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    scene_entities: Query<(Entity, &GlobalTransform), (Without<EditorEntity>, With<Transform>)>,
    parents: Query<&ChildOf>,
    gizmo_drag: Res<GizmoDragState>,
    modal: Res<ModalTransformState>,
    vp_drag: Res<ViewportDragState>,
    mut selection: ResMut<Selection>,
    mut input_focus: ResMut<InputFocus>,
    mut commands: Commands,
    (edit_mode, draw_state): (
        Res<crate::brush::EditMode>,
        Res<crate::draw_brush::DrawBrushState>,
    ),
    mut ray_cast: MeshRayCast,
) {
    // Don't select during gizmo drag, modal ops, viewport drag, brush edit mode, or draw mode
    if !mouse.just_pressed(MouseButton::Left)
        || gizmo_drag.active
        || modal.active.is_some()
        || vp_drag.active.is_some()
        || *edit_mode != crate::brush::EditMode::Object
        || draw_state.active.is_some()
    {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };

    // Check if cursor is within viewport
    let Ok((vp_computed, vp_tf)) = viewport_query.single() else {
        return;
    };
    let scale = vp_computed.inverse_scale_factor();
    let vp_pos = vp_tf.translation * scale;
    let vp_size = vp_computed.size() * scale;
    let vp_top_left = vp_pos - vp_size / 2.0;
    let local_cursor = cursor_pos - vp_top_left;
    if local_cursor.x < 0.0
        || local_cursor.y < 0.0
        || local_cursor.x > vp_size.x
        || local_cursor.y > vp_size.y
    {
        return;
    }

    // Clear input focus so keyboard shortcuts (G/R/S) work after viewport click
    input_focus.0 = None;

    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };

    // Remap from UI-logical space to camera render-target space
    let target_size = camera.logical_viewport_size().unwrap_or(vp_size);
    let local_cursor = local_cursor * target_size / vp_size;

    // Try mesh raycast first for accurate geometry-based selection
    let mut best_entity = None;

    if let Ok(ray) = camera.viewport_to_world(cam_tf, local_cursor) {
        let settings = MeshRayCastSettings::default()
            .with_visibility(RayCastVisibility::Any);
        let hits = ray_cast.cast_ray(ray, &settings);

        // Find the first hit that resolves to a scene entity (skip editor entities)
        for (hit_entity, _) in hits {
            if let Some(ancestor) = find_selectable_ancestor(*hit_entity, &scene_entities, &parents) {
                best_entity = Some(ancestor);
                break;
            }
        }
    }

    // Fall back to screen-space proximity for non-mesh entities (lights, empties)
    if best_entity.is_none() {
        let mut best_dist = 30.0_f32;
        for (entity, global_tf) in &scene_entities {
            let pos = global_tf.translation();
            if let Ok(screen_pos) = camera.world_to_viewport(cam_tf, pos) {
                let dist = (screen_pos - local_cursor).length();
                if dist < best_dist {
                    best_dist = dist;
                    best_entity = Some(entity);
                }
            }
        }
    }

    if let Some(entity) = best_entity {
        let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        if ctrl {
            selection.toggle(&mut commands, entity);
        } else {
            selection.select_single(&mut commands, entity);
        }
    } else {
        // Clicked on empty space — deselect all (unless Ctrl held)
        let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        if !ctrl {
            selection.clear(&mut commands);
        }
    }
}

// ---------------------------------------------------------------------------
// Box select (drag in viewport)
// ---------------------------------------------------------------------------

fn handle_box_select(
    mouse: Res<ButtonInput<MouseButton>>,
    _keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    mut box_state: ResMut<BoxSelectState>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    gizmo_drag: Res<GizmoDragState>,
    edit_mode: Res<crate::brush::EditMode>,
    scene_entities: Query<(Entity, &GlobalTransform), (Without<EditorEntity>, With<Transform>)>,
    mut selection: ResMut<Selection>,
    mut commands: Commands,
) {
    // Don't box-select during gizmo drag or brush edit mode
    if gizmo_drag.active || *edit_mode != crate::brush::EditMode::Object {
        box_state.active = false;
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };

    // Start box select on middle mouse drag
    if mouse.just_pressed(MouseButton::Middle) && !box_state.active {
        box_state.active = true;
        box_state.start = cursor_pos;
        box_state.current = cursor_pos;
        return;
    }

    if box_state.active {
        box_state.current = cursor_pos;

        let released = mouse.just_released(MouseButton::Middle);

        if released {
            box_state.active = false;

            let Ok((camera, cam_tf)) = camera_query.single() else {
                return;
            };
            let Ok((vp_computed, vp_tf)) = viewport_query.single() else {
                return;
            };
            let scale = vp_computed.inverse_scale_factor();
            let vp_pos = vp_tf.translation * scale;
            let vp_size = vp_computed.size() * scale;
            let vp_top_left = vp_pos - vp_size / 2.0;

            // Convert box to viewport-local coords, then remap to camera space
            let target_size = camera.logical_viewport_size().unwrap_or(vp_size);
            let remap = target_size / vp_size;
            let min = (box_state.start - vp_top_left).min(box_state.current - vp_top_left) * remap;
            let max = (box_state.start - vp_top_left).max(box_state.current - vp_top_left) * remap;

            // Find entities within the box
            let mut selected_entities = Vec::new();
            for (entity, global_tf) in &scene_entities {
                let pos = global_tf.translation();
                if let Ok(screen_pos) = camera.world_to_viewport(cam_tf, pos) {
                    if screen_pos.x >= min.x
                        && screen_pos.x <= max.x
                        && screen_pos.y >= min.y
                        && screen_pos.y <= max.y
                    {
                        if !selected_entities.contains(&entity) {
                            selected_entities.push(entity);
                        }
                    }
                }
            }

            if !selected_entities.is_empty() {
                selection.select_multiple(&mut commands, &selected_entities);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Right-click context menu in viewport
// ---------------------------------------------------------------------------

fn handle_viewport_right_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    scene_entities: Query<(Entity, &GlobalTransform), (Without<EditorEntity>, With<Transform>)>,
    parents: Query<&ChildOf>,
    gizmo_drag: Res<GizmoDragState>,
    modal: Res<ModalTransformState>,
    draw_state: Res<crate::draw_brush::DrawBrushState>,
    mut state: ResMut<ContextMenuState>,
    mut selection: ResMut<Selection>,
    mut commands: Commands,
    mut ray_cast: MeshRayCast,
) {
    if !mouse.just_pressed(MouseButton::Right) || gizmo_drag.active || modal.active.is_some() || draw_state.active.is_some() {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };

    // Check if cursor is within viewport
    let Ok((vp_computed, vp_tf)) = viewport_query.single() else {
        return;
    };
    let scale = vp_computed.inverse_scale_factor();
    let vp_pos = vp_tf.translation * scale;
    let vp_size = vp_computed.size() * scale;
    let vp_top_left = vp_pos - vp_size / 2.0;
    let local_cursor = cursor_pos - vp_top_left;
    if local_cursor.x < 0.0
        || local_cursor.y < 0.0
        || local_cursor.x > vp_size.x
        || local_cursor.y > vp_size.y
    {
        return;
    }

    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };

    // Remap from UI-logical space to camera render-target space
    let target_size = camera.logical_viewport_size().unwrap_or(vp_size);
    let local_cursor = local_cursor * target_size / vp_size;

    // Try mesh raycast first
    let mut best_entity = None;

    if let Ok(ray) = camera.viewport_to_world(cam_tf, local_cursor) {
        let settings = MeshRayCastSettings::default()
            .with_visibility(RayCastVisibility::Any);
        let hits = ray_cast.cast_ray(ray, &settings);

        for (hit_entity, _) in hits {
            if let Some(ancestor) = find_selectable_ancestor(*hit_entity, &scene_entities, &parents) {
                best_entity = Some(ancestor);
                break;
            }
        }
    }

    // Fall back to proximity for non-mesh entities
    if best_entity.is_none() {
        let mut best_dist = 30.0_f32;
        for (entity, global_tf) in &scene_entities {
            let pos = global_tf.translation();
            if let Ok(screen_pos) = camera.world_to_viewport(cam_tf, pos) {
                let dist = (screen_pos - local_cursor).length();
                if dist < best_dist {
                    best_dist = dist;
                    best_entity = Some(entity);
                }
            }
        }
    }

    let Some(entity) = best_entity else {
        return; // No entity near cursor — no menu
    };

    // Close any existing context menu
    if let Some(menu) = state.menu_entity.take() {
        if let Ok(mut ec) = commands.get_entity(menu) {
            ec.despawn();
        }
    }

    // Select the entity if not already selected
    if !selection.is_selected(entity) {
        selection.select_single(&mut commands, entity);
    }

    let menu_items = &[
        ("viewport.focus", "Focus                   F"),
        ("viewport.duplicate", "Duplicate        Ctrl+D"),
        ("viewport.delete", "Delete             Del"),
    ];

    let menu = spawn_context_menu(&mut commands, cursor_pos, Some(entity), menu_items);
    state.menu_entity = Some(menu);
    state.target_entity = Some(entity);
}

/// Handle context menu actions for viewport operations.
fn on_viewport_context_menu_action(
    event: On<ContextMenuAction>,
    mut commands: Commands,
    selected_transforms: Query<&GlobalTransform, With<Selected>>,
    mut camera_query: Query<&mut Transform, With<JackdawCameraSettings>>,
) {
    match event.action.as_str() {
        "viewport.focus" => {
            if let Some(target) = event.target_entity {
                if let Ok(global_tf) = selected_transforms.get(target) {
                    let target_pos = global_tf.translation();
                    let scale = global_tf.compute_transform().scale;
                    let dist = (scale.length() * 3.0).max(5.0);

                    for mut transform in &mut camera_query {
                        let forward = transform.forward().as_vec3();
                        transform.translation = target_pos - forward * dist;
                        *transform = transform.looking_at(target_pos, Vec3::Y);
                    }
                }
            }
        }
        "viewport.duplicate" => {
            commands.queue(|world: &mut World| {
                entity_ops::duplicate_selected(world);
            });
        }
        "viewport.delete" => {
            commands.queue(|world: &mut World| {
                entity_ops::delete_selected(world);
            });
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Walk up the `ChildOf` hierarchy from a raycast hit entity to find the
/// top-level scene entity (one that appears in `scene_entities`).
/// Handles GLTF child meshes and brush face children.
fn find_selectable_ancestor(
    mut entity: Entity,
    scene_entities: &Query<(Entity, &GlobalTransform), (Without<EditorEntity>, With<Transform>)>,
    parents: &Query<&ChildOf>,
) -> Option<Entity> {
    // Walk up until we find a scene entity (one that has Transform and is not EditorEntity)
    // Start with the hit entity itself — it may already be a scene entity
    loop {
        if scene_entities.contains(entity) {
            // Check if this entity has a parent that's also a scene entity;
            // if so, prefer the parent (handles GLTF sub-meshes).
            if let Ok(child_of) = parents.get(entity) {
                let parent = child_of.0;
                if scene_entities.contains(parent) {
                    // Keep walking up — the parent is also selectable
                    entity = parent;
                    continue;
                }
            }
            return Some(entity);
        }
        if let Ok(child_of) = parents.get(entity) {
            entity = child_of.0;
        } else {
            return None;
        }
    }
}
