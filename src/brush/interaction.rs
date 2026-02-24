use std::collections::HashSet;

use bevy::{
    input_focus::InputFocus,
    prelude::*,
};

use crate::{
    commands::CommandHistory,
    selection::Selection,
    viewport::SceneViewport,
    viewport_util::{point_to_segment_dist, window_to_viewport_cursor},
    EditorEntity,
};

use super::{
    Brush, BrushEditMode, BrushFaceData, BrushMeshCache, BrushPlane, BrushSelection, EditMode,
    SetBrush, EPSILON,
};
use super::geometry::{compute_face_tangent_axes, point_inside_all_planes};
use super::hull::rebuild_brush_from_vertices;

// ---------------------------------------------------------------------------
// Edit mode toggle
// ---------------------------------------------------------------------------

pub(super) fn handle_edit_mode_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    input_focus: Res<InputFocus>,
    selection: Res<Selection>,
    brushes: Query<(), With<Brush>>,
    mut edit_mode: ResMut<EditMode>,
    mut brush_selection: ResMut<BrushSelection>,
    modal: Res<crate::modal_transform::ModalTransformState>,
) {
    if input_focus.0.is_some() || modal.active.is_some() {
        return;
    }

    // Backtick (`) toggles in/out of brush edit mode
    if keyboard.just_pressed(KeyCode::Backquote) {
        match *edit_mode {
            EditMode::Object => {
                // Enter brush edit mode if a brush is selected
                if let Some(primary) = selection.primary() {
                    if brushes.contains(primary) {
                        *edit_mode = EditMode::BrushEdit(BrushEditMode::Face);
                        brush_selection.entity = Some(primary);
                        brush_selection.faces.clear();
                        brush_selection.vertices.clear();
                        brush_selection.edges.clear();
                    }
                }
            }
            EditMode::BrushEdit(_) => {
                *edit_mode = EditMode::Object;
                brush_selection.entity = None;
                brush_selection.faces.clear();
                brush_selection.vertices.clear();
                brush_selection.edges.clear();
            }
        }
        return;
    }

    // 1/2/3 switch sub-modes within brush edit mode
    if let EditMode::BrushEdit(_) = *edit_mode {
        if keyboard.just_pressed(KeyCode::Digit1) {
            *edit_mode = EditMode::BrushEdit(BrushEditMode::Vertex);
            brush_selection.faces.clear();
            brush_selection.vertices.clear();
            brush_selection.edges.clear();
        } else if keyboard.just_pressed(KeyCode::Digit2) {
            *edit_mode = EditMode::BrushEdit(BrushEditMode::Edge);
            brush_selection.faces.clear();
            brush_selection.vertices.clear();
            brush_selection.edges.clear();
        } else if keyboard.just_pressed(KeyCode::Digit3) {
            *edit_mode = EditMode::BrushEdit(BrushEditMode::Face);
            brush_selection.faces.clear();
            brush_selection.vertices.clear();
            brush_selection.edges.clear();
        } else if keyboard.just_pressed(KeyCode::Digit4) {
            *edit_mode = EditMode::BrushEdit(BrushEditMode::Clip);
            brush_selection.faces.clear();
            brush_selection.vertices.clear();
            brush_selection.edges.clear();
        }
    }

    // Exit brush edit mode if the brush entity gets deselected
    if let EditMode::BrushEdit(_) = *edit_mode {
        if let Some(brush_entity) = brush_selection.entity {
            if selection.primary() != Some(brush_entity) {
                *edit_mode = EditMode::Object;
                brush_selection.entity = None;
                brush_selection.faces.clear();
                brush_selection.vertices.clear();
                brush_selection.edges.clear();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Face selection (in face edit mode, click on face child entities)
// ---------------------------------------------------------------------------

pub(super) fn brush_face_select(
    edit_mode: Res<EditMode>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    face_entities: Query<(Entity, &super::BrushFaceEntity, &GlobalTransform)>,
    mut brush_selection: ResMut<BrushSelection>,
    brush_caches: Query<&BrushMeshCache>,
) {
    let EditMode::BrushEdit(BrushEditMode::Face) = *edit_mode else {
        return;
    };
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

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

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };

    // Raycast: find face entity closest to click in screen space
    // We check centroids of face polygons projected to screen
    let Ok(cache) = brush_caches.get(brush_entity) else {
        return;
    };

    let mut best_face = None;
    let mut best_dist = 30.0_f32;

    for (_, face_ent, face_global) in &face_entities {
        if face_ent.brush_entity != brush_entity {
            continue;
        }
        let face_idx = face_ent.face_index;
        let polygon = &cache.face_polygons[face_idx];
        if polygon.is_empty() {
            continue;
        }

        // Compute face centroid in world space
        let brush_tf = face_global; // face children have identity local transform
        let centroid: Vec3 = polygon.iter().map(|&vi| cache.vertices[vi]).sum::<Vec3>()
            / polygon.len() as f32;
        let world_centroid = brush_tf.transform_point(centroid);

        if let Ok(screen_pos) = camera.world_to_viewport(cam_tf, world_centroid) {
            let dist = (screen_pos - viewport_cursor).length();
            if dist < best_dist {
                best_dist = dist;
                best_face = Some(face_idx);
            }
        }
    }

    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if let Some(face_idx) = best_face {
        if ctrl {
            if let Some(pos) = brush_selection.faces.iter().position(|&f| f == face_idx) {
                brush_selection.faces.remove(pos);
            } else {
                brush_selection.faces.push(face_idx);
            }
        } else {
            brush_selection.faces = vec![face_idx];
        }
    } else if !ctrl {
        brush_selection.faces.clear();
    }
}

// ---------------------------------------------------------------------------
// Vertex selection
// ---------------------------------------------------------------------------

pub(super) fn brush_vertex_select(
    edit_mode: Res<EditMode>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    brush_transforms: Query<&GlobalTransform>,
    mut brush_selection: ResMut<BrushSelection>,
    brush_caches: Query<&BrushMeshCache>,
) {
    let EditMode::BrushEdit(BrushEditMode::Vertex) = *edit_mode else {
        return;
    };
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

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

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(cache) = brush_caches.get(brush_entity) else {
        return;
    };
    let Ok(brush_global) = brush_transforms.get(brush_entity) else {
        return;
    };

    let mut best_vert = None;
    let mut best_dist = 20.0_f32;

    for (vi, v) in cache.vertices.iter().enumerate() {
        let world_pos = brush_global.transform_point(*v);
        if let Ok(screen_pos) = camera.world_to_viewport(cam_tf, world_pos) {
            let dist = (screen_pos - viewport_cursor).length();
            if dist < best_dist {
                best_dist = dist;
                best_vert = Some(vi);
            }
        }
    }

    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if let Some(vi) = best_vert {
        if ctrl {
            if let Some(pos) = brush_selection.vertices.iter().position(|&v| v == vi) {
                brush_selection.vertices.remove(pos);
            } else {
                brush_selection.vertices.push(vi);
            }
        } else {
            brush_selection.vertices = vec![vi];
        }
    } else if !ctrl {
        brush_selection.vertices.clear();
    }
}

// ---------------------------------------------------------------------------
// Edge selection
// ---------------------------------------------------------------------------

pub(super) fn brush_edge_select(
    edit_mode: Res<EditMode>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    brush_transforms: Query<&GlobalTransform>,
    mut brush_selection: ResMut<BrushSelection>,
    brush_caches: Query<&BrushMeshCache>,
) {
    let EditMode::BrushEdit(BrushEditMode::Edge) = *edit_mode else {
        return;
    };
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

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

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(cache) = brush_caches.get(brush_entity) else {
        return;
    };
    let Ok(brush_global) = brush_transforms.get(brush_entity) else {
        return;
    };

    // Collect unique edges from face polygons
    let mut unique_edges: Vec<(usize, usize)> = Vec::new();
    for polygon in &cache.face_polygons {
        if polygon.len() < 2 {
            continue;
        }
        for i in 0..polygon.len() {
            let a = polygon[i];
            let b = polygon[(i + 1) % polygon.len()];
            let edge = (a.min(b), a.max(b));
            if !unique_edges.contains(&edge) {
                unique_edges.push(edge);
            }
        }
    }

    // Find nearest edge in screen space
    let mut best_edge = None;
    let mut best_dist = 20.0_f32;

    for &(a, b) in &unique_edges {
        let wa = brush_global.transform_point(cache.vertices[a]);
        let wb = brush_global.transform_point(cache.vertices[b]);
        let Ok(sa) = camera.world_to_viewport(cam_tf, wa) else {
            continue;
        };
        let Ok(sb) = camera.world_to_viewport(cam_tf, wb) else {
            continue;
        };
        let dist = point_to_segment_dist(viewport_cursor, sa, sb);
        if dist < best_dist {
            best_dist = dist;
            best_edge = Some((a, b));
        }
    }

    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if let Some(edge) = best_edge {
        if ctrl {
            if let Some(pos) = brush_selection.edges.iter().position(|e| *e == edge) {
                brush_selection.edges.remove(pos);
            } else {
                brush_selection.edges.push(edge);
            }
        } else {
            brush_selection.edges = vec![edge];
        }
    } else if !ctrl {
        brush_selection.edges.clear();
    }
}

// ---------------------------------------------------------------------------
// Face drag (G key moves selected face along its normal)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub(crate) struct BrushDragState {
    pub active: bool,
    start_brush: Option<Brush>,
    start_cursor: Vec2,
    drag_face_normal: Vec3,
}

pub(super) fn handle_face_drag(
    edit_mode: Res<EditMode>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    brush_selection: Res<BrushSelection>,
    mut brushes: Query<(&mut Brush, &GlobalTransform)>,
    mut drag_state: ResMut<BrushDragState>,
    input_focus: Res<InputFocus>,
    mut history: ResMut<CommandHistory>,
) {
    let EditMode::BrushEdit(BrushEditMode::Face) = *edit_mode else {
        drag_state.active = false;
        return;
    };

    if input_focus.0.is_some() {
        return;
    }

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok((mut brush, brush_global)) = brushes.get_mut(brush_entity) else {
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

    // Start drag on G or E (extrude face = same as face move, plane system extends)
    if (keyboard.just_pressed(KeyCode::KeyG) || keyboard.just_pressed(KeyCode::KeyE))
        && !drag_state.active
        && !brush_selection.faces.is_empty()
    {
        drag_state.active = true;
        drag_state.start_brush = Some(brush.clone());
        drag_state.start_cursor = viewport_cursor;
        // Use the first selected face's normal
        let face_idx = brush_selection.faces[0];
        drag_state.drag_face_normal = brush.faces[face_idx].plane.normal;
        return;
    }

    if !drag_state.active {
        return;
    }

    // Cancel on Escape or right-click
    if keyboard.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right) {
        if let Some(ref start) = drag_state.start_brush {
            *brush = start.clone();
        }
        drag_state.active = false;
        return;
    }

    // Confirm on left-click or Enter
    if mouse.just_pressed(MouseButton::Left) || keyboard.just_pressed(KeyCode::Enter) {
        if let Some(ref start) = drag_state.start_brush {
            let cmd = SetBrush {
                entity: brush_entity,
                old: start.clone(),
                new: brush.clone(),
                label: "Move brush face".to_string(),
            };
            history.undo_stack.push(Box::new(cmd));
            history.redo_stack.clear();
        }
        drag_state.active = false;
        return;
    }

    // Continue drag — adjust face plane distance based on mouse movement
    let Some(ref start) = drag_state.start_brush else {
        drag_state.active = false;
        return;
    };

    // Project the face normal to screen space to determine drag direction
    let brush_pos = brush_global.translation();
    let Ok(origin_screen) = camera.world_to_viewport(cam_tf, brush_pos) else {
        return;
    };
    let Ok(normal_screen) = camera.world_to_viewport(cam_tf, brush_pos + drag_state.drag_face_normal) else {
        return;
    };
    let screen_dir = (normal_screen - origin_screen).normalize_or_zero();
    let mouse_delta = viewport_cursor - drag_state.start_cursor;
    let projected = mouse_delta.dot(screen_dir);

    // Scale by camera distance
    let cam_dist = (cam_tf.translation() - brush_pos).length();
    let drag_amount = projected * cam_dist * 0.003;

    // Apply to all selected faces
    for &face_idx in &brush_selection.faces {
        if face_idx < start.faces.len() && face_idx < brush.faces.len() {
            brush.faces[face_idx].plane.distance = start.faces[face_idx].plane.distance + drag_amount;
        }
    }
}

// ---------------------------------------------------------------------------
// Vertex drag (G key moves selected vertices)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum VertexDragConstraint {
    #[default]
    Free,
    AxisX,
    AxisY,
    AxisZ,
}

#[derive(Resource, Default)]
pub(crate) struct VertexDragState {
    pub active: bool,
    pub constraint: VertexDragConstraint,
    start_brush: Option<Brush>,
    start_cursor: Vec2,
    start_vertex_positions: Vec<Vec3>,
    /// Full vertex list at drag start (for hull rebuild).
    start_all_vertices: Vec<Vec3>,
    /// Per-face polygon indices at drag start (for hull rebuild).
    start_face_polygons: Vec<Vec<usize>>,
}

pub(super) fn handle_vertex_drag(
    edit_mode: Res<EditMode>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    brush_selection: Res<BrushSelection>,
    mut brushes: Query<&mut Brush>,
    brush_transforms: Query<&GlobalTransform>,
    brush_caches: Query<&BrushMeshCache>,
    mut drag_state: ResMut<VertexDragState>,
    input_focus: Res<InputFocus>,
    mut history: ResMut<CommandHistory>,
) {
    let EditMode::BrushEdit(BrushEditMode::Vertex) = *edit_mode else {
        drag_state.active = false;
        return;
    };

    if input_focus.0.is_some() {
        return;
    }

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(mut brush) = brushes.get_mut(brush_entity) else {
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

    // Start drag on G or E (extrude = same as move, hull creates transition faces)
    if (keyboard.just_pressed(KeyCode::KeyG) || keyboard.just_pressed(KeyCode::KeyE))
        && !drag_state.active
        && !brush_selection.vertices.is_empty()
    {
        if let Ok(cache) = brush_caches.get(brush_entity) {
            drag_state.active = true;
            drag_state.constraint = VertexDragConstraint::Free;
            drag_state.start_brush = Some(brush.clone());
            drag_state.start_cursor = viewport_cursor;
            drag_state.start_vertex_positions = brush_selection
                .vertices
                .iter()
                .map(|&vi| cache.vertices.get(vi).copied().unwrap_or(Vec3::ZERO))
                .collect();
            drag_state.start_all_vertices = cache.vertices.clone();
            drag_state.start_face_polygons = cache.face_polygons.clone();
        }
        return;
    }

    if !drag_state.active {
        return;
    }

    // Axis constraint toggle: X/Y/Z keys (same key again = back to Free)
    if keyboard.just_pressed(KeyCode::KeyX) {
        drag_state.constraint = if drag_state.constraint == VertexDragConstraint::AxisX {
            VertexDragConstraint::Free
        } else {
            VertexDragConstraint::AxisX
        };
    } else if keyboard.just_pressed(KeyCode::KeyY) {
        drag_state.constraint = if drag_state.constraint == VertexDragConstraint::AxisY {
            VertexDragConstraint::Free
        } else {
            VertexDragConstraint::AxisY
        };
    } else if keyboard.just_pressed(KeyCode::KeyZ) {
        drag_state.constraint = if drag_state.constraint == VertexDragConstraint::AxisZ {
            VertexDragConstraint::Free
        } else {
            VertexDragConstraint::AxisZ
        };
    }

    // Cancel on Escape or right-click
    if keyboard.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right) {
        if let Some(ref start) = drag_state.start_brush {
            *brush = start.clone();
        }
        drag_state.active = false;
        drag_state.constraint = VertexDragConstraint::Free;
        return;
    }

    // Confirm on left-click or Enter
    if mouse.just_pressed(MouseButton::Left) || keyboard.just_pressed(KeyCode::Enter) {
        if let Some(ref start) = drag_state.start_brush {
            let cmd = SetBrush {
                entity: brush_entity,
                old: start.clone(),
                new: brush.clone(),
                label: "Move brush vertex".to_string(),
            };
            history.undo_stack.push(Box::new(cmd));
            history.redo_stack.clear();
        }
        drag_state.active = false;
        drag_state.constraint = VertexDragConstraint::Free;
        return;
    }

    // Continue drag — compute world-space offset from camera-relative mouse movement
    let Some(ref start) = drag_state.start_brush else {
        drag_state.active = false;
        return;
    };
    let Ok(brush_global) = brush_transforms.get(brush_entity) else {
        return;
    };

    let mouse_delta = viewport_cursor - drag_state.start_cursor;
    let Some(local_offset) = compute_brush_drag_offset(
        drag_state.constraint,
        mouse_delta,
        cam_tf,
        camera,
        brush_global,
    ) else {
        return;
    };

    // Rebuild from moved vertices using convex hull
    let mut new_verts = drag_state.start_all_vertices.clone();
    for (sel_idx, &vert_idx) in brush_selection.vertices.iter().enumerate() {
        if sel_idx < drag_state.start_vertex_positions.len() && vert_idx < new_verts.len() {
            new_verts[vert_idx] = drag_state.start_vertex_positions[sel_idx] + local_offset;
        }
    }

    if let Some(new_brush) = rebuild_brush_from_vertices(
        start,
        &drag_state.start_all_vertices,
        &drag_state.start_face_polygons,
        &new_verts,
    ) {
        *brush = new_brush;
    }
}

// ---------------------------------------------------------------------------
// Shared drag offset computation
// ---------------------------------------------------------------------------

/// Compute a local-space offset for brush vertex/edge drag based on mouse movement.
fn compute_brush_drag_offset(
    constraint: VertexDragConstraint,
    mouse_delta: Vec2,
    cam_tf: &GlobalTransform,
    camera: &Camera,
    brush_global: &GlobalTransform,
) -> Option<Vec3> {
    let brush_pos = brush_global.translation();
    let cam_dist = (cam_tf.translation() - brush_pos).length();
    let scale = cam_dist * 0.003;

    let offset = match constraint {
        VertexDragConstraint::Free => {
            let cam_right = cam_tf.right().as_vec3();
            let cam_up = cam_tf.up().as_vec3();
            let world_offset =
                cam_right * mouse_delta.x * scale + cam_up * (-mouse_delta.y) * scale;
            let (_, brush_rot, _) = brush_global.to_scale_rotation_translation();
            brush_rot.inverse() * world_offset
        }
        constraint => {
            let axis_dir = match constraint {
                VertexDragConstraint::AxisX => Vec3::X,
                VertexDragConstraint::AxisY => Vec3::Y,
                VertexDragConstraint::AxisZ => Vec3::Z,
                VertexDragConstraint::Free => unreachable!(),
            };
            let origin_screen = camera.world_to_viewport(cam_tf, brush_pos).ok()?;
            let (_, brush_rot, _) = brush_global.to_scale_rotation_translation();
            let world_axis = brush_rot * axis_dir;
            let axis_screen = camera
                .world_to_viewport(cam_tf, brush_pos + world_axis)
                .ok()?;
            let screen_axis = (axis_screen - origin_screen).normalize_or_zero();
            let projected = mouse_delta.dot(screen_axis);
            axis_dir * projected * scale
        }
    };
    Some(offset)
}

// ---------------------------------------------------------------------------
// Edge drag (G key moves selected edges)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub(crate) struct EdgeDragState {
    pub active: bool,
    pub constraint: VertexDragConstraint,
    start_brush: Option<Brush>,
    start_cursor: Vec2,
    /// Start positions for each selected edge's two endpoints (vertex indices + positions).
    start_edge_vertices: Vec<(usize, Vec3)>,
    /// Full vertex list at drag start (for hull rebuild).
    start_all_vertices: Vec<Vec3>,
    /// Per-face polygon indices at drag start (for hull rebuild).
    start_face_polygons: Vec<Vec<usize>>,
}

pub(super) fn handle_edge_drag(
    edit_mode: Res<EditMode>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    brush_selection: Res<BrushSelection>,
    mut brushes: Query<&mut Brush>,
    brush_transforms: Query<&GlobalTransform>,
    brush_caches: Query<&BrushMeshCache>,
    mut drag_state: ResMut<EdgeDragState>,
    input_focus: Res<InputFocus>,
    mut history: ResMut<CommandHistory>,
) {
    let EditMode::BrushEdit(BrushEditMode::Edge) = *edit_mode else {
        drag_state.active = false;
        return;
    };

    if input_focus.0.is_some() {
        return;
    }

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(mut brush) = brushes.get_mut(brush_entity) else {
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

    // Start drag on G or E (extrude = same as move, hull creates transition faces)
    if (keyboard.just_pressed(KeyCode::KeyG) || keyboard.just_pressed(KeyCode::KeyE))
        && !drag_state.active
        && !brush_selection.edges.is_empty()
    {
        if let Ok(cache) = brush_caches.get(brush_entity) {
            drag_state.active = true;
            drag_state.constraint = VertexDragConstraint::Free;
            drag_state.start_brush = Some(brush.clone());
            drag_state.start_cursor = viewport_cursor;
            drag_state.start_all_vertices = cache.vertices.clone();
            drag_state.start_face_polygons = cache.face_polygons.clone();

            // Collect unique vertex indices and their positions from all selected edges
            let mut seen = HashSet::new();
            let mut edge_verts = Vec::new();
            for &(a, b) in &brush_selection.edges {
                if seen.insert(a) {
                    let pos = cache.vertices.get(a).copied().unwrap_or(Vec3::ZERO);
                    edge_verts.push((a, pos));
                }
                if seen.insert(b) {
                    let pos = cache.vertices.get(b).copied().unwrap_or(Vec3::ZERO);
                    edge_verts.push((b, pos));
                }
            }
            drag_state.start_edge_vertices = edge_verts;
        }
        return;
    }

    if !drag_state.active {
        return;
    }

    // Axis constraint toggle
    if keyboard.just_pressed(KeyCode::KeyX) {
        drag_state.constraint = if drag_state.constraint == VertexDragConstraint::AxisX {
            VertexDragConstraint::Free
        } else {
            VertexDragConstraint::AxisX
        };
    } else if keyboard.just_pressed(KeyCode::KeyY) {
        drag_state.constraint = if drag_state.constraint == VertexDragConstraint::AxisY {
            VertexDragConstraint::Free
        } else {
            VertexDragConstraint::AxisY
        };
    } else if keyboard.just_pressed(KeyCode::KeyZ) {
        drag_state.constraint = if drag_state.constraint == VertexDragConstraint::AxisZ {
            VertexDragConstraint::Free
        } else {
            VertexDragConstraint::AxisZ
        };
    }

    // Cancel
    if keyboard.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right) {
        if let Some(ref start) = drag_state.start_brush {
            *brush = start.clone();
        }
        drag_state.active = false;
        drag_state.constraint = VertexDragConstraint::Free;
        return;
    }

    // Confirm
    if mouse.just_pressed(MouseButton::Left) || keyboard.just_pressed(KeyCode::Enter) {
        if let Some(ref start) = drag_state.start_brush {
            let cmd = SetBrush {
                entity: brush_entity,
                old: start.clone(),
                new: brush.clone(),
                label: "Move brush edge".to_string(),
            };
            history.undo_stack.push(Box::new(cmd));
            history.redo_stack.clear();
        }
        drag_state.active = false;
        drag_state.constraint = VertexDragConstraint::Free;
        return;
    }

    // Continue drag
    let Some(ref start) = drag_state.start_brush else {
        drag_state.active = false;
        return;
    };
    let Ok(brush_global) = brush_transforms.get(brush_entity) else {
        return;
    };

    let mouse_delta = viewport_cursor - drag_state.start_cursor;
    let Some(local_offset) = compute_brush_drag_offset(
        drag_state.constraint,
        mouse_delta,
        cam_tf,
        camera,
        brush_global,
    ) else {
        return;
    };

    // Rebuild from moved edge vertices using convex hull
    let mut new_verts = drag_state.start_all_vertices.clone();
    for &(vi, start_pos) in &drag_state.start_edge_vertices {
        if vi < new_verts.len() {
            new_verts[vi] = start_pos + local_offset;
        }
    }

    if let Some(new_brush) = rebuild_brush_from_vertices(
        start,
        &drag_state.start_all_vertices,
        &drag_state.start_face_polygons,
        &new_verts,
    ) {
        *brush = new_brush;
    }
}

// ---------------------------------------------------------------------------
// Delete selected vertices/edges/faces (Delete key)
// ---------------------------------------------------------------------------

pub(super) fn handle_brush_delete(
    edit_mode: Res<EditMode>,
    keyboard: Res<ButtonInput<KeyCode>>,
    input_focus: Res<InputFocus>,
    mut brush_selection: ResMut<BrushSelection>,
    mut brushes: Query<&mut Brush>,
    brush_caches: Query<&BrushMeshCache>,
    mut history: ResMut<CommandHistory>,
    vertex_drag: Res<VertexDragState>,
    edge_drag: Res<EdgeDragState>,
    face_drag: Res<BrushDragState>,
) {
    let EditMode::BrushEdit(mode) = *edit_mode else {
        return;
    };
    if input_focus.0.is_some() {
        return;
    }
    if !keyboard.just_pressed(KeyCode::Delete) && !keyboard.just_pressed(KeyCode::Backspace) {
        return;
    }
    // Don't delete while dragging
    if vertex_drag.active || edge_drag.active || face_drag.active {
        return;
    }

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(mut brush) = brushes.get_mut(brush_entity) else {
        return;
    };

    match mode {
        BrushEditMode::Vertex => {
            if brush_selection.vertices.is_empty() {
                return;
            }
            let Ok(cache) = brush_caches.get(brush_entity) else {
                return;
            };
            let remove_set: HashSet<usize> = brush_selection.vertices.iter().copied().collect();
            let remaining: Vec<Vec3> = cache
                .vertices
                .iter()
                .enumerate()
                .filter(|(i, _)| !remove_set.contains(i))
                .map(|(_, v)| *v)
                .collect();
            if remaining.len() < 4 {
                return; // need at least a tetrahedron
            }
            let old = brush.clone();
            if let Some(new_brush) = rebuild_brush_from_vertices(
                &old,
                &cache.vertices,
                &cache.face_polygons,
                &remaining,
            ) {
                *brush = new_brush;
                let cmd = SetBrush {
                    entity: brush_entity,
                    old,
                    new: brush.clone(),
                    label: "Remove brush vertex".to_string(),
                };
                history.undo_stack.push(Box::new(cmd));
                history.redo_stack.clear();
                brush_selection.vertices.clear();
            }
        }
        BrushEditMode::Edge => {
            if brush_selection.edges.is_empty() {
                return;
            }
            let Ok(cache) = brush_caches.get(brush_entity) else {
                return;
            };
            let mut remove_set = HashSet::new();
            for &(a, b) in &brush_selection.edges {
                remove_set.insert(a);
                remove_set.insert(b);
            }
            let remaining: Vec<Vec3> = cache
                .vertices
                .iter()
                .enumerate()
                .filter(|(i, _)| !remove_set.contains(i))
                .map(|(_, v)| *v)
                .collect();
            if remaining.len() < 4 {
                return;
            }
            let old = brush.clone();
            if let Some(new_brush) = rebuild_brush_from_vertices(
                &old,
                &cache.vertices,
                &cache.face_polygons,
                &remaining,
            ) {
                *brush = new_brush;
                let cmd = SetBrush {
                    entity: brush_entity,
                    old,
                    new: brush.clone(),
                    label: "Remove brush edge".to_string(),
                };
                history.undo_stack.push(Box::new(cmd));
                history.redo_stack.clear();
                brush_selection.edges.clear();
            }
        }
        BrushEditMode::Face => {
            if brush_selection.faces.is_empty() {
                return;
            }
            let remaining = brush.faces.len() - brush_selection.faces.len();
            if remaining < 4 {
                return;
            }
            let old = brush.clone();
            let remove_set: HashSet<usize> = brush_selection.faces.iter().copied().collect();
            let new_faces: Vec<BrushFaceData> = brush
                .faces
                .iter()
                .enumerate()
                .filter(|(i, _)| !remove_set.contains(i))
                .map(|(_, f)| f.clone())
                .collect();
            brush.faces = new_faces;
            let cmd = SetBrush {
                entity: brush_entity,
                old,
                new: brush.clone(),
                label: "Remove brush face".to_string(),
            };
            history.undo_stack.push(Box::new(cmd));
            history.redo_stack.clear();
            brush_selection.faces.clear();
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Clip mode (4 key — add cutting plane)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub(crate) struct ClipState {
    pub points: Vec<Vec3>,
    pub preview_plane: Option<BrushPlane>,
}

pub(super) fn handle_clip_mode(
    edit_mode: Res<EditMode>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    input_focus: Res<InputFocus>,
    windows: Query<&Window>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    brush_selection: Res<BrushSelection>,
    mut brushes: Query<&mut Brush>,
    brush_transforms: Query<&GlobalTransform>,
    brush_caches: Query<&BrushMeshCache>,
    mut clip_state: ResMut<ClipState>,
    mut history: ResMut<CommandHistory>,
    mut gizmos: Gizmos,
) {
    let EditMode::BrushEdit(BrushEditMode::Clip) = *edit_mode else {
        // Clear clip state when not in clip mode
        if !clip_state.points.is_empty() {
            clip_state.points.clear();
            clip_state.preview_plane = None;
        }
        return;
    };
    if input_focus.0.is_some() {
        return;
    }

    let Some(brush_entity) = brush_selection.entity else {
        return;
    };
    let Ok(brush_global) = brush_transforms.get(brush_entity) else {
        return;
    };

    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };

    // Escape clears clip points
    if keyboard.just_pressed(KeyCode::Escape) {
        clip_state.points.clear();
        clip_state.preview_plane = None;
        return;
    }

    // Left click: add point by raycasting to brush surface
    if mouse.just_pressed(MouseButton::Left) && clip_state.points.len() < 3 {
        let Some(cursor_pos) = window.cursor_position() else {
            return;
        };
        let Some(viewport_cursor) = window_to_viewport_cursor(cursor_pos, camera, &viewport_query) else {
            return;
        };

        // Cast ray from camera through cursor
        let Ok(ray) = camera.viewport_to_world(cam_tf, viewport_cursor) else {
            return;
        };

        let Ok(cache) = brush_caches.get(brush_entity) else {
            return;
        };

        // Find closest intersection with any brush face
        let (_, brush_rot, brush_trans) = brush_global.to_scale_rotation_translation();
        let mut best_t = f32::MAX;
        let mut best_point = None;

        for (face_idx, polygon) in cache.face_polygons.iter().enumerate() {
            if polygon.len() < 3 {
                continue;
            }
            let Ok(brush_ref) = brushes.get(brush_entity) else {
                return;
            };
            let face = &brush_ref.faces[face_idx];
            let world_normal = brush_rot * face.plane.normal;
            let face_centroid: Vec3 =
                polygon.iter().map(|&vi| cache.vertices[vi]).sum::<Vec3>() / polygon.len() as f32;
            let world_centroid = brush_global.transform_point(face_centroid);

            let denom = world_normal.dot(*ray.direction);
            if denom.abs() < EPSILON {
                continue;
            }
            let t = (world_centroid - ray.origin).dot(world_normal) / denom;
            if t > 0.0 && t < best_t {
                let hit = ray.origin + *ray.direction * t;
                // Verify hit is roughly on the brush (within face polygon bounds)
                let local_hit =
                    brush_rot.inverse() * (hit - brush_trans);
                if point_inside_all_planes(local_hit, &brush_ref.faces) {
                    best_t = t;
                    best_point = Some(local_hit);
                }
            }
        }

        if let Some(point) = best_point {
            clip_state.points.push(point);
        }
    }

    // Compute preview plane from collected points
    clip_state.preview_plane = match clip_state.points.len() {
        2 => {
            // Two points + camera forward for orientation
            let dir = clip_state.points[1] - clip_state.points[0];
            let (_, brush_rot, _) = brush_global.to_scale_rotation_translation();
            let local_cam_fwd = brush_rot.inverse() * cam_tf.forward().as_vec3();
            let normal = dir.cross(local_cam_fwd).normalize_or_zero();
            if normal.length_squared() > 0.5 {
                let distance = normal.dot(clip_state.points[0]);
                Some(BrushPlane { normal, distance })
            } else {
                None
            }
        }
        3 => {
            let a = clip_state.points[0];
            let b = clip_state.points[1];
            let c = clip_state.points[2];
            let normal = (b - a).cross(c - a).normalize_or_zero();
            if normal.length_squared() > 0.5 {
                let distance = normal.dot(a);
                Some(BrushPlane { normal, distance })
            } else {
                None
            }
        }
        _ => None,
    };

    // Enter: apply clip plane
    if keyboard.just_pressed(KeyCode::Enter) {
        if let Some(ref plane) = clip_state.preview_plane {
            let Ok(mut brush) = brushes.get_mut(brush_entity) else {
                return;
            };
            let old = brush.clone();
            brush.faces.push(BrushFaceData {
                plane: plane.clone(),
                material_index: 0,
                texture_path: None,
                uv_offset: Vec2::ZERO,
                uv_scale: Vec2::ONE,
                uv_rotation: 0.0,
            });
            let cmd = SetBrush {
                entity: brush_entity,
                old,
                new: brush.clone(),
                label: "Clip brush".to_string(),
            };
            history.undo_stack.push(Box::new(cmd));
            history.redo_stack.clear();
            clip_state.points.clear();
            clip_state.preview_plane = None;
        }
    }

    // Draw clip points and preview
    for (i, point) in clip_state.points.iter().enumerate() {
        let world_pos = brush_global.transform_point(*point);
        let color = Color::srgb(1.0, 0.3, 0.3);
        gizmos.sphere(Isometry3d::from_translation(world_pos), 0.06, color);
        // Draw connecting lines between points
        if i > 0 {
            let prev_world = brush_global.transform_point(clip_state.points[i - 1]);
            gizmos.line(prev_world, world_pos, color);
        }
    }

    // Draw preview plane as a translucent quad
    if let Some(ref plane) = clip_state.preview_plane {
        let (_, brush_rot, _) = brush_global.to_scale_rotation_translation();
        let world_normal = brush_rot * plane.normal;
        let center = brush_global.transform_point(plane.normal * plane.distance);
        let (u, v) = compute_face_tangent_axes(plane.normal);
        let world_u = brush_rot * u * 2.0;
        let world_v = brush_rot * v * 2.0;
        let preview_color = Color::srgba(1.0, 0.3, 0.3, 0.4);
        // Draw a diamond shape
        gizmos.line(center + world_u, center + world_v, preview_color);
        gizmos.line(center + world_v, center - world_u, preview_color);
        gizmos.line(center - world_u, center - world_v, preview_color);
        gizmos.line(center - world_v, center + world_u, preview_color);
        // Draw normal arrow
        gizmos.arrow(center, center + world_normal * 0.5, Color::srgb(1.0, 0.3, 0.3));
    }
}
