use bevy::prelude::*;
use jackdaw_feathers::status_bar::{StatusBarCenter, StatusBarLeft, StatusBarRight};

use crate::{
    brush::{BrushEditMode, ClipState, EditMode, VertexDragState, VertexDragConstraint},
    draw_brush::{DrawBrushState, DrawMode, DrawPhase},
    gizmos::{GizmoMode, GizmoSpace},
    modal_transform::{ModalConstraint, ModalOp, ModalTransformState},
    scene_io::SceneFilePath,
    selection::{Selected, Selection},
    snapping::SnapSettings,
    EditorEntity,
};

pub struct StatusBarPlugin;

impl Plugin for StatusBarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (update_status_left, update_status_center, update_status_right),
        );
    }
}

fn update_status_left(
    selection: Res<Selection>,
    selected: Query<Option<&Name>, With<Selected>>,
    transforms: Query<&Transform>,
    mut text_query: Query<&mut Text, With<StatusBarLeft>>,
) {
    let Ok(mut text) = text_query.single_mut() else {
        return;
    };

    let count = selection.entities.len();
    if count == 0 {
        if selection.is_changed() {
            text.0 = "No selection".to_string();
        }
    } else if count == 1 {
        if let Some(primary) = selection.primary() {
            let name = selected
                .get(primary)
                .ok()
                .flatten()
                .map(|n| n.as_str().to_string())
                .unwrap_or_else(|| format!("{primary}"));
            let pos_str = transforms
                .get(primary)
                .map(|t| {
                    let p = t.translation;
                    format!("  Pos: ({:.2}, {:.2}, {:.2})", p.x, p.y, p.z)
                })
                .unwrap_or_default();
            let new_text = format!("{name}{pos_str}");
            if text.0 != new_text {
                text.0 = new_text;
            }
        }
    } else if selection.is_changed() {
        text.0 = format!("{count} entities selected");
    }
}

fn update_status_center(
    scene_entities: Query<Entity, (With<Transform>, Without<EditorEntity>)>,
    meshes: Query<(), (With<Mesh3d>, Without<EditorEntity>)>,
    point_lights: Query<(), (With<PointLight>, Without<EditorEntity>)>,
    dir_lights: Query<(), (With<DirectionalLight>, Without<EditorEntity>)>,
    spot_lights: Query<(), (With<SpotLight>, Without<EditorEntity>)>,
    cameras: Query<(), (With<Camera3d>, Without<EditorEntity>)>,
    mut text_query: Query<&mut Text, With<StatusBarCenter>>,
) {
    let Ok(mut text) = text_query.single_mut() else {
        return;
    };

    let total = scene_entities.iter().count();
    let mesh_count = meshes.iter().count();
    let light_count = point_lights.iter().count()
        + dir_lights.iter().count()
        + spot_lights.iter().count();
    let camera_count = cameras.iter().count();

    let new_text = format!(
        "Entities: {total}  |  Meshes: {mesh_count}  |  Lights: {light_count}  |  Cameras: {camera_count}"
    );
    if text.0 != new_text {
        text.0 = new_text;
    }
}

fn update_status_right(
    mode: Res<GizmoMode>,
    space: Res<GizmoSpace>,
    scene_path: Res<SceneFilePath>,
    snap_settings: Res<SnapSettings>,
    modal: Res<ModalTransformState>,
    edit_mode: Res<EditMode>,
    vertex_drag: Res<VertexDragState>,
    clip_state: Res<ClipState>,
    draw_state: Res<DrawBrushState>,
    mut text_query: Query<&mut Text, With<StatusBarRight>>,
) {
    if !mode.is_changed()
        && !space.is_changed()
        && !snap_settings.is_changed()
        && !modal.is_changed()
        && !edit_mode.is_changed()
        && !vertex_drag.is_changed()
        && !clip_state.is_changed()
        && !draw_state.is_changed()
    {
        return;
    }
    let Ok(mut text) = text_query.single_mut() else {
        return;
    };

    // Show draw brush mode status
    if let Some(ref active) = draw_state.active {
        let mode_label = match (active.mode, active.append_target.is_some()) {
            (DrawMode::Add, true) => "APPEND",
            (DrawMode::Add, false) => "ADD",
            (DrawMode::Cut, _) => "CUT",
        };
        text.0 = match active.phase {
            DrawPhase::PlacingFirstCorner => {
                format!("DRAW BRUSH ({mode_label}): Click to place first corner (Ctrl lock plane, Tab toggle mode) | Esc cancel")
            }
            DrawPhase::DrawingFootprint => {
                format!("DRAW BRUSH ({mode_label}): Move to size, click to lock footprint | Esc cancel")
            }
            DrawPhase::ExtrudingDepth => {
                format!(
                    "DRAW BRUSH ({mode_label}): Move for depth ({:.2}), click to create | Esc cancel",
                    active.depth
                )
            }
        };
        return;
    }

    // Show brush edit mode info
    if let EditMode::BrushEdit(sub_mode) = *edit_mode {
        let sub_str = match sub_mode {
            BrushEditMode::Face => "Face",
            BrushEditMode::Vertex => "Vertex",
            BrushEditMode::Edge => "Edge",
            BrushEditMode::Clip => "Clip",
        };
        let extra = if vertex_drag.active {
            let c = match vertex_drag.constraint {
                VertexDragConstraint::Free => "Free",
                VertexDragConstraint::AxisX => "X",
                VertexDragConstraint::AxisY => "Y",
                VertexDragConstraint::AxisZ => "Z",
            };
            format!(" | Dragging ({c}) X/Y/Z constrain")
        } else if sub_mode == BrushEditMode::Clip {
            let n = clip_state.points.len();
            if n < 2 {
                format!(" | Click {}-3 points, Enter apply, Esc cancel", n + 1)
            } else {
                " | Enter apply, Esc cancel".to_string()
            }
        } else {
            String::new()
        };
        let base_hint = if sub_mode == BrushEditMode::Vertex {
            "Drag move  Shift+Drag split edge  Del remove"
        } else {
            "Drag to move  Del remove"
        };
        text.0 = format!("EDIT MODE: {sub_str} | 1 Vert  2 Edge  3 Face  4 Clip | {base_hint}{extra}");
        return;
    }

    // Show modal operation info when active
    if let Some(ref active) = modal.active {
        let op_str = match active.op {
            ModalOp::Grab => "Grab",
            ModalOp::Rotate => "Rotate",
            ModalOp::Scale => "Scale",
        };
        let constraint_str = match active.constraint {
            ModalConstraint::Free => "Free".to_string(),
            ModalConstraint::Axis(axis) => format!("{axis:?} axis"),
            ModalConstraint::Plane(excluded) => format!("{excluded:?} plane"),
        };
        text.0 = format!("{op_str}: {constraint_str} | LMB confirm, RMB/Esc cancel");
        return;
    }

    let mode_str = match *mode {
        GizmoMode::Translate => "Translate",
        GizmoMode::Rotate => "Rotate",
        GizmoMode::Scale => "Scale",
    };
    let space_str = match *space {
        GizmoSpace::World => "World",
        GizmoSpace::Local => "Local",
    };

    let snap_str = match *mode {
        GizmoMode::Translate => {
            if snap_settings.translate_snap {
                format!("Snap: {:.2}", snap_settings.translate_increment)
            } else {
                "Snap: Off".to_string()
            }
        }
        GizmoMode::Rotate => {
            if snap_settings.rotate_snap {
                format!(
                    "Snap: {:.0}\u{00b0}",
                    snap_settings.rotate_increment.to_degrees()
                )
            } else {
                "Snap: Off".to_string()
            }
        }
        GizmoMode::Scale => {
            if snap_settings.scale_snap {
                format!("Snap: {:.2}", snap_settings.scale_increment)
            } else {
                "Snap: Off".to_string()
            }
        }
    };

    let path_str = scene_path
        .path
        .as_deref()
        .map(|p| format!(" | {p}"))
        .unwrap_or_default();

    text.0 = format!("{mode_str} ({space_str}) | {snap_str}{path_str}");
}
