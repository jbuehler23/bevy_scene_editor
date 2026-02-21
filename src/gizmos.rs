use bevy::{prelude::*, ui::UiGlobalTransform};

use crate::{
    commands::{CommandHistory, SetTransform},
    modal_transform::ModalTransformState,
    selection::{Selected, Selection},
    snapping::SnapSettings,
    viewport::SceneViewport,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const AXIS_LENGTH: f32 = 1.5;
const AXIS_TIP_LENGTH: f32 = 0.3;
const ROTATE_RING_RADIUS: f32 = 1.2;
const SCALE_CUBE_SIZE: f32 = 0.15;

const COLOR_X: Color = Color::srgb(1.0, 0.2, 0.2);
const COLOR_Y: Color = Color::srgb(0.2, 1.0, 0.2);
const COLOR_Z: Color = Color::srgb(0.2, 0.4, 1.0);
const COLOR_X_BRIGHT: Color = Color::srgb(1.0, 0.5, 0.5);
const COLOR_Y_BRIGHT: Color = Color::srgb(0.5, 1.0, 0.5);
const COLOR_Z_BRIGHT: Color = Color::srgb(0.5, 0.7, 1.0);

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

#[derive(Resource, Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum GizmoMode {
    #[default]
    Translate,
    Rotate,
    Scale,
}

#[derive(Resource, Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum GizmoSpace {
    #[default]
    World,
    Local,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GizmoAxis {
    X,
    Y,
    Z,
}

#[derive(Resource, Default)]
pub struct GizmoDragState {
    pub active: bool,
    pub axis: Option<GizmoAxis>,
    pub drag_start_screen: Vec2,
    pub start_transform: Transform,
    pub entity: Option<Entity>,
    pub accumulated_delta: f32,
}

#[derive(Resource, Default)]
pub struct GizmoHoverState {
    pub hovered_axis: Option<GizmoAxis>,
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct TransformGizmosPlugin;

impl Plugin for TransformGizmosPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GizmoMode>()
            .init_resource::<GizmoSpace>()
            .init_resource::<GizmoDragState>()
            .init_resource::<GizmoHoverState>()
            .add_systems(
                Update,
                (
                    handle_gizmo_mode_keys,
                    handle_gizmo_hover,
                    handle_gizmo_drag,
                    draw_gizmos,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// Key bindings: W/E/R switch gizmo mode
// ---------------------------------------------------------------------------

fn handle_gizmo_mode_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<GizmoMode>,
    mut space: ResMut<GizmoSpace>,
    drag_state: Res<GizmoDragState>,
    modal: Res<ModalTransformState>,
    selection: Res<Selection>,
) {
    // Don't switch modes while dragging or during modal ops
    if drag_state.active || modal.active.is_some() {
        return;
    }

    let has_selection = selection.primary().is_some();

    if keyboard.just_pressed(KeyCode::KeyW) {
        *mode = GizmoMode::Translate;
    }
    if keyboard.just_pressed(KeyCode::KeyE) {
        *mode = GizmoMode::Rotate;
    }
    // R and S are reserved for modal ops when an entity is selected
    if keyboard.just_pressed(KeyCode::KeyR) && !has_selection {
        *mode = GizmoMode::Scale;
    }
    // Toggle world/local space (skip during modal — X is axis constraint)
    if keyboard.just_pressed(KeyCode::KeyX) {
        *space = match *space {
            GizmoSpace::World => GizmoSpace::Local,
            GizmoSpace::Local => GizmoSpace::World,
        };
    }
}

// ---------------------------------------------------------------------------
// Hover detection — find which axis the cursor is near
// ---------------------------------------------------------------------------

fn handle_gizmo_hover(
    selection: Res<Selection>,
    transforms: Query<&GlobalTransform, With<Selected>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    windows: Query<&Window>,
    mode: Res<GizmoMode>,
    space: Res<GizmoSpace>,
    mut hover: ResMut<GizmoHoverState>,
    drag_state: Res<GizmoDragState>,
    modal: Res<ModalTransformState>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    edit_mode: Res<crate::brush::EditMode>,
    walk_mode: Res<crate::viewport::WalkModeState>,
) {
    hover.hovered_axis = None;

    if drag_state.active || modal.active.is_some() || walk_mode.active {
        return;
    }

    // Don't show gizmo hover in brush edit mode
    if *edit_mode != crate::brush::EditMode::Object {
        return;
    }

    let Some(primary) = selection.primary() else {
        return;
    };
    let Ok(global_tf) = transforms.get(primary) else {
        return;
    };
    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };

    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };

    // Convert window cursor to viewport-local coordinates
    let Some(viewport_cursor) = window_to_viewport_cursor(cursor_pos, &viewport_query) else {
        return;
    };

    let gizmo_pos = global_tf.translation();
    let rotation = gizmo_rotation(global_tf, &space);

    let axes = [
        (GizmoAxis::X, rotation * Vec3::X),
        (GizmoAxis::Y, rotation * Vec3::Y),
        (GizmoAxis::Z, rotation * Vec3::Z),
    ];

    // Project gizmo origin and axis endpoints to screen space, find closest axis
    let Some(origin_screen) = camera.world_to_viewport(cam_tf, gizmo_pos).ok() else {
        return;
    };

    let mut best_axis = None;
    let mut best_dist = f32::MAX;
    let threshold = 20.0; // pixels

    for (axis, dir) in &axes {
        let endpoint = match *mode {
            GizmoMode::Translate | GizmoMode::Scale => gizmo_pos + *dir * AXIS_LENGTH,
            GizmoMode::Rotate => gizmo_pos + *dir * ROTATE_RING_RADIUS,
        };
        let Some(end_screen) = camera.world_to_viewport(cam_tf, endpoint).ok() else {
            continue;
        };
        let dist = point_to_segment_dist(viewport_cursor, origin_screen, end_screen);
        if dist < threshold && dist < best_dist {
            best_dist = dist;
            best_axis = Some(*axis);
        }
    }

    hover.hovered_axis = best_axis;
}

// ---------------------------------------------------------------------------
// Drag handling
// ---------------------------------------------------------------------------

fn handle_gizmo_drag(
    selection: Res<Selection>,
    mut transforms: Query<(&GlobalTransform, &mut Transform), With<Selected>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<GizmoMode>,
    space: Res<GizmoSpace>,
    hover: Res<GizmoHoverState>,
    mut drag_state: ResMut<GizmoDragState>,
    mut history: ResMut<CommandHistory>,
    snap_settings: Res<SnapSettings>,
    modal: Res<ModalTransformState>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    edit_mode: Res<crate::brush::EditMode>,
    walk_mode: Res<crate::viewport::WalkModeState>,
) {
    // Suppress gizmo drag during modal operations, brush edit mode, or walk mode
    if modal.active.is_some() || *edit_mode != crate::brush::EditMode::Object || walk_mode.active {
        if drag_state.active {
            drag_state.active = false;
        }
        return;
    }

    let Some(primary) = selection.primary() else {
        if drag_state.active {
            drag_state.active = false;
        }
        return;
    };

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Some(viewport_cursor) = window_to_viewport_cursor(cursor_pos, &viewport_query) else {
        return;
    };

    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };

    // Start drag
    if mouse.just_pressed(MouseButton::Left) && !drag_state.active {
        if let Some(axis) = hover.hovered_axis {
            if let Ok((_, transform)) = transforms.get(primary) {
                drag_state.active = true;
                drag_state.axis = Some(axis);
                drag_state.drag_start_screen = viewport_cursor;
                drag_state.start_transform = *transform;
                drag_state.entity = Some(primary);
                drag_state.accumulated_delta = 0.0;
            }
        }
        return;
    }

    // Continue drag
    if drag_state.active && mouse.pressed(MouseButton::Left) {
        let Some(entity) = drag_state.entity else {
            return;
        };
        let Ok((global_tf, mut transform)) = transforms.get_mut(entity) else {
            return;
        };
        let Some(axis) = drag_state.axis else {
            return;
        };

        let rotation = gizmo_rotation(global_tf, &space);
        let axis_dir = match axis {
            GizmoAxis::X => rotation * Vec3::X,
            GizmoAxis::Y => rotation * Vec3::Y,
            GizmoAxis::Z => rotation * Vec3::Z,
        };

        let gizmo_pos = global_tf.translation();

        match *mode {
            GizmoMode::Translate => {
                // Project mouse movement onto axis in screen space
                let Some(origin_screen) = camera.world_to_viewport(cam_tf, gizmo_pos).ok()
                else {
                    return;
                };
                let Some(axis_screen) =
                    camera.world_to_viewport(cam_tf, gizmo_pos + axis_dir).ok()
                else {
                    return;
                };
                let screen_axis = (axis_screen - origin_screen).normalize_or_zero();
                let mouse_delta = viewport_cursor - drag_state.drag_start_screen;
                let projected = mouse_delta.dot(screen_axis);

                // Scale by distance to camera for consistent feel
                let cam_dist = (cam_tf.translation() - gizmo_pos).length();
                let scale = cam_dist * 0.003;

                let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
                let new_pos =
                    drag_state.start_transform.translation + axis_dir * projected * scale;
                transform.translation = snap_settings.snap_translate_vec3_if(new_pos, ctrl);
            }
            GizmoMode::Rotate => {
                let mouse_delta = viewport_cursor - drag_state.drag_start_screen;
                let screen_axis = match axis {
                    GizmoAxis::X => Vec2::Y,
                    GizmoAxis::Y => Vec2::X,
                    GizmoAxis::Z => Vec2::X,
                };
                let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
                let raw_angle = mouse_delta.dot(screen_axis) * 0.01;
                let angle = snap_settings.snap_rotate_if(raw_angle, ctrl);
                let rotation_delta = Quat::from_axis_angle(axis_dir, angle);
                transform.rotation = rotation_delta * drag_state.start_transform.rotation;
            }
            GizmoMode::Scale => {
                let Some(origin_screen) = camera.world_to_viewport(cam_tf, gizmo_pos).ok()
                else {
                    return;
                };
                let Some(axis_screen) =
                    camera.world_to_viewport(cam_tf, gizmo_pos + axis_dir).ok()
                else {
                    return;
                };
                let screen_axis = (axis_screen - origin_screen).normalize_or_zero();
                let mouse_delta = viewport_cursor - drag_state.drag_start_screen;
                let projected = mouse_delta.dot(screen_axis) * 0.005;

                let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
                let mut new_scale = drag_state.start_transform.scale;
                match axis {
                    GizmoAxis::X => new_scale.x = (new_scale.x + projected).max(0.01),
                    GizmoAxis::Y => new_scale.y = (new_scale.y + projected).max(0.01),
                    GizmoAxis::Z => new_scale.z = (new_scale.z + projected).max(0.01),
                }
                transform.scale = snap_settings.snap_scale_vec3_if(new_scale, ctrl);
            }
        }
        return;
    }

    // End drag — push undo command
    if drag_state.active && mouse.just_released(MouseButton::Left) {
        if let Some(entity) = drag_state.entity {
            if let Ok((_, transform)) = transforms.get(entity) {
                let cmd = SetTransform {
                    entity,
                    old_transform: drag_state.start_transform,
                    new_transform: *transform,
                };
                history.undo_stack.push(Box::new(cmd));
                history.redo_stack.clear();
            }
        }
        drag_state.active = false;
        drag_state.axis = None;
        drag_state.entity = None;
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn draw_gizmos(
    mut gizmos: Gizmos,
    selection: Res<Selection>,
    transforms: Query<&GlobalTransform, With<Selected>>,
    mode: Res<GizmoMode>,
    space: Res<GizmoSpace>,
    hover: Res<GizmoHoverState>,
    drag_state: Res<GizmoDragState>,
    modal: Res<ModalTransformState>,
    edit_mode: Res<crate::brush::EditMode>,
    walk_mode: Res<crate::viewport::WalkModeState>,
) {
    // Hide gizmo during modal operations, brush edit mode, or walk mode
    if modal.active.is_some() || *edit_mode != crate::brush::EditMode::Object || walk_mode.active {
        return;
    }

    let Some(primary) = selection.primary() else {
        return;
    };
    let Ok(global_tf) = transforms.get(primary) else {
        return;
    };

    let pos = global_tf.translation();
    let rotation = gizmo_rotation(global_tf, &space);

    let right = rotation * Vec3::X;
    let up = rotation * Vec3::Y;
    let forward = rotation * Vec3::Z;

    let active_axis = if drag_state.active {
        drag_state.axis
    } else {
        hover.hovered_axis
    };

    let x_color = axis_color(GizmoAxis::X, active_axis);
    let y_color = axis_color(GizmoAxis::Y, active_axis);
    let z_color = axis_color(GizmoAxis::Z, active_axis);

    match *mode {
        GizmoMode::Translate => {
            gizmos
                .arrow(pos, pos + right * AXIS_LENGTH, x_color)
                .with_tip_length(AXIS_TIP_LENGTH);
            gizmos
                .arrow(pos, pos + up * AXIS_LENGTH, y_color)
                .with_tip_length(AXIS_TIP_LENGTH);
            gizmos
                .arrow(pos, pos + forward * AXIS_LENGTH, z_color)
                .with_tip_length(AXIS_TIP_LENGTH);
        }
        GizmoMode::Rotate => {
            // Draw rotation rings
            gizmos.circle(
                Isometry3d::new(pos, Quat::from_rotation_arc(Vec3::Z, right)),
                ROTATE_RING_RADIUS,
                x_color,
            );
            gizmos.circle(
                Isometry3d::new(pos, Quat::from_rotation_arc(Vec3::Z, up)),
                ROTATE_RING_RADIUS,
                y_color,
            );
            gizmos.circle(
                Isometry3d::new(pos, Quat::from_rotation_arc(Vec3::Z, forward)),
                ROTATE_RING_RADIUS,
                z_color,
            );
        }
        GizmoMode::Scale => {
            // Draw scale handles: lines with cubes at the end
            let cube_half = SCALE_CUBE_SIZE;
            for (dir, color) in [(right, x_color), (up, y_color), (forward, z_color)] {
                let end = pos + dir * AXIS_LENGTH;
                gizmos.line(pos, end, color);
                // Draw a small cube at the end using lines
                let x = Vec3::X * cube_half;
                let y = Vec3::Y * cube_half;
                let z = Vec3::Z * cube_half;
                let corners = [
                    end - x - y - z,
                    end + x - y - z,
                    end + x + y - z,
                    end - x + y - z,
                    end - x - y + z,
                    end + x - y + z,
                    end + x + y + z,
                    end - x + y + z,
                ];
                // Bottom face
                gizmos.line(corners[0], corners[1], color);
                gizmos.line(corners[1], corners[2], color);
                gizmos.line(corners[2], corners[3], color);
                gizmos.line(corners[3], corners[0], color);
                // Top face
                gizmos.line(corners[4], corners[5], color);
                gizmos.line(corners[5], corners[6], color);
                gizmos.line(corners[6], corners[7], color);
                gizmos.line(corners[7], corners[4], color);
                // Verticals
                gizmos.line(corners[0], corners[4], color);
                gizmos.line(corners[1], corners[5], color);
                gizmos.line(corners[2], corners[6], color);
                gizmos.line(corners[3], corners[7], color);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn gizmo_rotation(global_tf: &GlobalTransform, space: &GizmoSpace) -> Quat {
    match space {
        GizmoSpace::World => Quat::IDENTITY,
        GizmoSpace::Local => {
            let (_, rotation, _) = global_tf.to_scale_rotation_translation();
            rotation
        }
    }
}

fn axis_color(axis: GizmoAxis, active: Option<GizmoAxis>) -> Color {
    let is_active = active == Some(axis);
    match axis {
        GizmoAxis::X => {
            if is_active {
                COLOR_X_BRIGHT
            } else {
                COLOR_X
            }
        }
        GizmoAxis::Y => {
            if is_active {
                COLOR_Y_BRIGHT
            } else {
                COLOR_Y
            }
        }
        GizmoAxis::Z => {
            if is_active {
                COLOR_Z_BRIGHT
            } else {
                COLOR_Z
            }
        }
    }
}

/// Convert window cursor position to viewport-local coordinates.
pub(crate) fn window_to_viewport_cursor(
    cursor_pos: Vec2,
    viewport_query: &Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
) -> Option<Vec2> {
    let Ok((computed, vp_transform)) = viewport_query.single() else {
        return Some(cursor_pos);
    };
    // Convert from physical pixels to logical pixels to match cursor_position()
    let scale = computed.inverse_scale_factor();
    let vp_pos = vp_transform.translation * scale;
    let vp_size = computed.size() * scale;
    // ComputedNode position is the center, convert to top-left
    let vp_top_left = vp_pos - vp_size / 2.0;
    let local = cursor_pos - vp_top_left;
    if local.x >= 0.0 && local.y >= 0.0 && local.x <= vp_size.x && local.y <= vp_size.y {
        Some(local)
    } else {
        None
    }
}

/// Distance from a point to a line segment.
pub(crate) fn point_to_segment_dist(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let ap = point - a;
    let t = (ap.dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
    let closest = a + ab * t;
    (point - closest).length()
}
