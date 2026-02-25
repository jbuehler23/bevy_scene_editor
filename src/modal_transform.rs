use bevy::{input_focus::InputFocus, prelude::*, ui::UiGlobalTransform};

use crate::{
    commands::{CommandHistory, SetTransform},
    gizmos::{GizmoAxis, GizmoDragState, GizmoHoverState, GizmoMode},
    selection::{Selected, Selection},
    snapping::SnapSettings,
    viewport::SceneViewport,
    viewport_util::window_to_viewport_cursor,
    EditorEntity,
};

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModalOp {
    Grab,
    Rotate,
    Scale,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ModalConstraint {
    #[default]
    Free,
    Axis(GizmoAxis),
    /// Constrains to a plane by excluding this axis.
    Plane(GizmoAxis),
}

#[derive(Resource, Default)]
pub struct ModalTransformState {
    pub active: Option<ActiveModal>,
}

pub struct ActiveModal {
    pub op: ModalOp,
    pub entity: Entity,
    pub start_transform: Transform,
    pub constraint: ModalConstraint,
    pub start_cursor: Vec2,
}

#[derive(Resource, Default)]
pub struct ViewportDragState {
    pub pending: Option<PendingDrag>,
    pub active: Option<ActiveDrag>,
}

pub struct PendingDrag {
    pub entity: Entity,
    pub start_transform: Transform,
    pub click_pos: Vec2,
    /// Viewport-local cursor position at drag start.
    pub start_viewport_cursor: Vec2,
}

pub struct ActiveDrag {
    pub entity: Entity,
    pub start_transform: Transform,
    /// Viewport-local cursor position at drag start.
    pub start_viewport_cursor: Vec2,
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct ModalTransformPlugin;

impl Plugin for ModalTransformPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModalTransformState>()
            .init_resource::<ViewportDragState>()
            .add_systems(
                Update,
                (
                    modal_activate,
                    modal_constrain,
                    modal_update,
                    modal_confirm,
                    modal_cancel,
                    snap_toggle,
                    viewport_drag_detect,
                    viewport_drag_update,
                    viewport_drag_finish,
                    modal_draw,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// Modal Activate: G/R/S keys start modal transform
// ---------------------------------------------------------------------------

fn modal_activate(
    keyboard: Res<ButtonInput<KeyCode>>,
    input_focus: Res<InputFocus>,
    selection: Res<Selection>,
    transforms: Query<&Transform, With<Selected>>,
    gizmo_drag: Res<GizmoDragState>,
    mut modal: ResMut<ModalTransformState>,
    mut gizmo_mode: ResMut<GizmoMode>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    edit_mode: Res<crate::brush::EditMode>,
    draw_state: Res<crate::draw_brush::DrawBrushState>,
) {
    if modal.active.is_some() || gizmo_drag.active || input_focus.0.is_some() {
        return;
    }

    // Don't start modal transforms in brush edit mode or draw mode
    if *edit_mode != crate::brush::EditMode::Object || draw_state.active.is_some() {
        return;
    }

    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);

    let op = if keyboard.just_pressed(KeyCode::KeyG) {
        Some(ModalOp::Grab)
    } else if keyboard.just_pressed(KeyCode::KeyR) && !ctrl {
        Some(ModalOp::Rotate)
    } else if keyboard.just_pressed(KeyCode::KeyS) && !ctrl {
        Some(ModalOp::Scale)
    } else {
        None
    };

    let Some(op) = op else {
        return;
    };
    let Some(primary) = selection.primary() else {
        return;
    };
    let Ok(transform) = transforms.get(primary) else {
        return;
    };

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, _)) = camera_query.single() else {
        return;
    };
    let viewport_cursor =
        window_to_viewport_cursor(cursor_pos, camera, &viewport_query).unwrap_or(cursor_pos);

    modal.active = Some(ActiveModal {
        op,
        entity: primary,
        start_transform: *transform,
        constraint: ModalConstraint::Free,
        start_cursor: viewport_cursor,
    });

    // Sync gizmo mode to match modal operation so the gizmo mode is consistent when modal ends
    match op {
        ModalOp::Grab => *gizmo_mode = GizmoMode::Translate,
        ModalOp::Rotate => *gizmo_mode = GizmoMode::Rotate,
        ModalOp::Scale => *gizmo_mode = GizmoMode::Scale,
    }
}

// ---------------------------------------------------------------------------
// Modal Constrain: X/Y/Z keys set axis constraint
// ---------------------------------------------------------------------------

fn modal_constrain(keyboard: Res<ButtonInput<KeyCode>>, mut modal: ResMut<ModalTransformState>) {
    let Some(ref mut active) = modal.active else {
        return;
    };

    let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);

    if keyboard.just_pressed(KeyCode::KeyX) {
        active.constraint = if shift {
            ModalConstraint::Plane(GizmoAxis::X)
        } else {
            ModalConstraint::Axis(GizmoAxis::X)
        };
    } else if keyboard.just_pressed(KeyCode::KeyY) {
        active.constraint = if shift {
            ModalConstraint::Plane(GizmoAxis::Y)
        } else {
            ModalConstraint::Axis(GizmoAxis::Y)
        };
    } else if keyboard.just_pressed(KeyCode::KeyZ) {
        active.constraint = if shift {
            ModalConstraint::Plane(GizmoAxis::Z)
        } else {
            ModalConstraint::Axis(GizmoAxis::Z)
        };
    }
}

// ---------------------------------------------------------------------------
// Modal Update: apply transform changes each frame
// ---------------------------------------------------------------------------

fn modal_update(
    modal: Res<ModalTransformState>,
    mut transforms: Query<&mut Transform, With<Selected>>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    windows: Query<&Window>,
    keyboard: Res<ButtonInput<KeyCode>>,
    snap_settings: Res<SnapSettings>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
) {
    let Some(ref active) = modal.active else {
        return;
    };
    let Ok(mut transform) = transforms.get_mut(active.entity) else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };
    let viewport_cursor =
        window_to_viewport_cursor(cursor_pos, camera, &viewport_query).unwrap_or(cursor_pos);
    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);

    match active.op {
        ModalOp::Grab => {
            modal_grab(
                active,
                &mut transform,
                viewport_cursor,
                cursor_pos,
                camera,
                cam_tf,
                &snap_settings,
                &viewport_query,
                ctrl,
            );
        }
        ModalOp::Rotate => {
            let mouse_delta = viewport_cursor - active.start_cursor;
            let raw_angle = mouse_delta.x * 0.01;
            let angle = snap_settings.snap_rotate_if(raw_angle, ctrl);

            let axis_dir = match active.constraint {
                ModalConstraint::Free | ModalConstraint::Plane(_) => Vec3::Y,
                ModalConstraint::Axis(axis) => axis_to_vec3(axis),
            };

            let rotation_delta = Quat::from_axis_angle(axis_dir, angle);
            transform.rotation = rotation_delta * active.start_transform.rotation;
        }
        ModalOp::Scale => {
            let mouse_delta = viewport_cursor - active.start_cursor;
            let factor = 1.0 + mouse_delta.x * 0.005;

            let mut new_scale = active.start_transform.scale;
            match active.constraint {
                ModalConstraint::Free => {
                    new_scale *= factor;
                }
                ModalConstraint::Axis(axis) => match axis {
                    GizmoAxis::X => {
                        new_scale.x = (active.start_transform.scale.x * factor).max(0.01)
                    }
                    GizmoAxis::Y => {
                        new_scale.y = (active.start_transform.scale.y * factor).max(0.01)
                    }
                    GizmoAxis::Z => {
                        new_scale.z = (active.start_transform.scale.z * factor).max(0.01)
                    }
                },
                ModalConstraint::Plane(excluded) => {
                    if excluded != GizmoAxis::X {
                        new_scale.x = (active.start_transform.scale.x * factor).max(0.01);
                    }
                    if excluded != GizmoAxis::Y {
                        new_scale.y = (active.start_transform.scale.y * factor).max(0.01);
                    }
                    if excluded != GizmoAxis::Z {
                        new_scale.z = (active.start_transform.scale.z * factor).max(0.01);
                    }
                }
            }
            new_scale = new_scale.max(Vec3::splat(0.01));
            transform.scale = snap_settings.snap_scale_vec3_if(new_scale, ctrl);
        }
    }
}

fn modal_grab(
    active: &ActiveModal,
    transform: &mut Transform,
    viewport_cursor: Vec2,
    _cursor_pos: Vec2,
    camera: &Camera,
    cam_tf: &GlobalTransform,
    snap_settings: &SnapSettings,
    _viewport_query: &Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,  // kept for API compat

    ctrl: bool,
) {
    match active.constraint {
        ModalConstraint::Free => {
            let start_pos = active.start_transform.translation;
            let cam_dist = (cam_tf.translation() - start_pos).length();
            let scale = cam_dist * 0.003;
            let mouse_delta = viewport_cursor - active.start_cursor;

            // Project camera right/forward onto the horizontal plane
            let cam_right = cam_tf.right().as_vec3();
            let cam_forward = cam_tf.forward().as_vec3();
            let right_h = Vec3::new(cam_right.x, 0.0, cam_right.z).normalize_or_zero();
            let forward_h = Vec3::new(cam_forward.x, 0.0, cam_forward.z).normalize_or_zero();

            let offset = right_h * mouse_delta.x * scale + forward_h * (-mouse_delta.y) * scale;
            let snapped_offset = snap_settings.snap_translate_vec3_if(offset, ctrl);
            transform.translation = start_pos + snapped_offset;
        }
        ModalConstraint::Axis(axis) => {
            let axis_dir = axis_to_vec3(axis);
            let gizmo_pos = active.start_transform.translation;

            let Ok(origin_screen) = camera.world_to_viewport(cam_tf, gizmo_pos) else {
                return;
            };
            let Ok(axis_screen) = camera.world_to_viewport(cam_tf, gizmo_pos + axis_dir) else {
                return;
            };
            let screen_axis: Vec2 = (axis_screen - origin_screen).normalize_or_zero();
            let mouse_delta = viewport_cursor - active.start_cursor;
            let projected = mouse_delta.dot(screen_axis);

            let cam_dist = (cam_tf.translation() - gizmo_pos).length();
            let scale = cam_dist * 0.003;

            let raw_delta = axis_dir * projected * scale;
            let snapped_delta = snap_settings.snap_translate_vec3_if(raw_delta, ctrl);
            transform.translation = active.start_transform.translation + snapped_delta;
        }
        ModalConstraint::Plane(excluded_axis) => {
            let gizmo_pos = active.start_transform.translation;
            let cam_dist = (cam_tf.translation() - gizmo_pos).length();
            let scale = cam_dist * 0.003;
            let mouse_delta = viewport_cursor - active.start_cursor;

            let axes: [Vec3; 2] = match excluded_axis {
                GizmoAxis::X => [Vec3::Y, Vec3::Z],
                GizmoAxis::Y => [Vec3::X, Vec3::Z],
                GizmoAxis::Z => [Vec3::X, Vec3::Y],
            };

            let mut offset = Vec3::ZERO;
            for dir in &axes {
                let Ok(origin_screen) = camera.world_to_viewport(cam_tf, gizmo_pos) else {
                    continue;
                };
                let Ok(axis_screen) = camera.world_to_viewport(cam_tf, gizmo_pos + *dir) else {
                    continue;
                };
                let screen_axis: Vec2 = (axis_screen - origin_screen).normalize_or_zero();
                let projected = mouse_delta.dot(screen_axis);
                offset += *dir * projected * scale;
            }

            let snapped_offset = snap_settings.snap_translate_vec3_if(offset, ctrl);
            transform.translation = active.start_transform.translation + snapped_offset;
        }
    }
}

// ---------------------------------------------------------------------------
// Modal Confirm: Left-click or Enter
// ---------------------------------------------------------------------------

fn modal_confirm(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut modal: ResMut<ModalTransformState>,
    transforms: Query<&Transform, With<Selected>>,
    mut history: ResMut<CommandHistory>,
) {
    let Some(ref active) = modal.active else {
        return;
    };

    if !mouse.just_pressed(MouseButton::Left) && !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }

    if let Ok(transform) = transforms.get(active.entity) {
        let cmd = SetTransform {
            entity: active.entity,
            old_transform: active.start_transform,
            new_transform: *transform,
        };
        history.undo_stack.push(Box::new(cmd));
        history.redo_stack.clear();
    }

    modal.active = None;
}

// ---------------------------------------------------------------------------
// Modal Cancel: Right-click or Esc
// ---------------------------------------------------------------------------

fn modal_cancel(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut modal: ResMut<ModalTransformState>,
    mut transforms: Query<&mut Transform, With<Selected>>,
) {
    let Some(ref active) = modal.active else {
        return;
    };

    if !mouse.just_pressed(MouseButton::Right) && !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }

    // Restore original transform
    if let Ok(mut transform) = transforms.get_mut(active.entity) {
        *transform = active.start_transform;
    }

    modal.active = None;
}

// ---------------------------------------------------------------------------
// Snap toggle: period key
// ---------------------------------------------------------------------------

fn snap_toggle(
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<GizmoMode>,
    modal: Res<ModalTransformState>,
    mut snap_settings: ResMut<SnapSettings>,
) {
    if modal.active.is_some() {
        return;
    }

    if keyboard.just_pressed(KeyCode::Period) {
        match *mode {
            GizmoMode::Translate => snap_settings.translate_snap = !snap_settings.translate_snap,
            GizmoMode::Rotate => snap_settings.rotate_snap = !snap_settings.rotate_snap,
            GizmoMode::Scale => snap_settings.scale_snap = !snap_settings.scale_snap,
        }
    }
}

// ---------------------------------------------------------------------------
// Viewport drag detect
// ---------------------------------------------------------------------------

fn viewport_drag_detect(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    selection: Res<Selection>,
    transforms: Query<(&GlobalTransform, &Transform), With<Selected>>,
    gizmo_drag: Res<GizmoDragState>,
    modal: Res<ModalTransformState>,
    gizmo_hover: Res<GizmoHoverState>,
    mut drag_state: ResMut<ViewportDragState>,
) {
    if modal.active.is_some() || gizmo_drag.active || gizmo_hover.hovered_axis.is_some() {
        return;
    }

    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Some(primary) = selection.primary() else {
        return;
    };
    let Ok((global_tf, local_tf)) = transforms.get(primary) else {
        return;
    };

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };

    let Some(viewport_cursor) = window_to_viewport_cursor(cursor_pos, camera, &viewport_query) else {
        return;
    };

    // Check screen-space proximity to the selected entity
    let entity_pos = global_tf.translation();
    let Ok(entity_screen) = camera.world_to_viewport(cam_tf, entity_pos) else {
        return;
    };
    let dist = (entity_screen - viewport_cursor).length();

    if dist < 30.0 {
        drag_state.pending = Some(PendingDrag {
            entity: primary,
            start_transform: *local_tf,
            click_pos: cursor_pos,
            start_viewport_cursor: viewport_cursor,
        });
    }
}

// ---------------------------------------------------------------------------
// Viewport drag update
// ---------------------------------------------------------------------------

fn viewport_drag_update(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    snap_settings: Res<SnapSettings>,
    mut drag_state: ResMut<ViewportDragState>,
    mut transforms: Query<&mut Transform>,
) {
    if !mouse.pressed(MouseButton::Left) {
        drag_state.pending = None;
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };

    // Check pending -> active promotion
    if let Some(ref pending) = drag_state.pending {
        let dist = (cursor_pos - pending.click_pos).length();
        if dist > 5.0 {
            let active = ActiveDrag {
                entity: pending.entity,
                start_transform: pending.start_transform,
                start_viewport_cursor: pending.start_viewport_cursor,
            };
            drag_state.active = Some(active);
            drag_state.pending = None;
        } else {
            return;
        }
    }

    // Update active drag
    let Some(ref active) = drag_state.active else {
        return;
    };
    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };
    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);

    let viewport_cursor = window_to_viewport_cursor(cursor_pos, camera, &viewport_query)
        .unwrap_or(cursor_pos);

    let start_pos = active.start_transform.translation;
    let cam_dist = (cam_tf.translation() - start_pos).length();
    let scale = cam_dist * 0.003;
    let mouse_delta = viewport_cursor - active.start_viewport_cursor;

    // Project camera right/forward onto the horizontal plane
    let cam_right = cam_tf.right().as_vec3();
    let cam_forward = cam_tf.forward().as_vec3();
    let right_h = Vec3::new(cam_right.x, 0.0, cam_right.z).normalize_or_zero();
    let forward_h = Vec3::new(cam_forward.x, 0.0, cam_forward.z).normalize_or_zero();

    let offset = right_h * mouse_delta.x * scale + forward_h * (-mouse_delta.y) * scale;
    let snapped_offset = snap_settings.snap_translate_vec3_if(offset, ctrl);

    if let Ok(mut transform) = transforms.get_mut(active.entity) {
        transform.translation = start_pos + snapped_offset;
    }
}

// ---------------------------------------------------------------------------
// Viewport drag finish
// ---------------------------------------------------------------------------

fn viewport_drag_finish(
    mouse: Res<ButtonInput<MouseButton>>,
    mut drag_state: ResMut<ViewportDragState>,
    transforms: Query<&Transform>,
    mut history: ResMut<CommandHistory>,
) {
    if !mouse.just_released(MouseButton::Left) {
        return;
    }

    drag_state.pending = None;

    let Some(active) = drag_state.active.take() else {
        return;
    };

    if let Ok(transform) = transforms.get(active.entity) {
        let cmd = SetTransform {
            entity: active.entity,
            old_transform: active.start_transform,
            new_transform: *transform,
        };
        history.undo_stack.push(Box::new(cmd));
        history.redo_stack.clear();
    }
}

// ---------------------------------------------------------------------------
// Modal Draw: show constraint axis line
// ---------------------------------------------------------------------------

fn modal_draw(
    modal: Res<ModalTransformState>,
    mut gizmos: Gizmos,
    transforms: Query<&GlobalTransform, With<Selected>>,
) {
    let Some(ref active) = modal.active else {
        return;
    };
    let Ok(global_tf) = transforms.get(active.entity) else {
        return;
    };
    let pos = global_tf.translation();

    let line_length = 50.0;

    match active.constraint {
        ModalConstraint::Free => {}
        ModalConstraint::Axis(axis) => {
            let dir = axis_to_vec3(axis);
            let color = axis_color(axis);
            gizmos.line(pos - dir * line_length, pos + dir * line_length, color);
        }
        ModalConstraint::Plane(excluded) => {
            for axis in [GizmoAxis::X, GizmoAxis::Y, GizmoAxis::Z] {
                if axis != excluded {
                    let dir = axis_to_vec3(axis);
                    let color = axis_color(axis);
                    gizmos.line(
                        pos - dir * line_length,
                        pos + dir * line_length,
                        color.with_alpha(0.4),
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn axis_to_vec3(axis: GizmoAxis) -> Vec3 {
    match axis {
        GizmoAxis::X => Vec3::X,
        GizmoAxis::Y => Vec3::Y,
        GizmoAxis::Z => Vec3::Z,
    }
}

fn axis_color(axis: GizmoAxis) -> Color {
    match axis {
        GizmoAxis::X => Color::srgb(1.0, 0.2, 0.2),
        GizmoAxis::Y => Color::srgb(0.2, 1.0, 0.2),
        GizmoAxis::Z => Color::srgb(0.2, 0.4, 1.0),
    }
}
