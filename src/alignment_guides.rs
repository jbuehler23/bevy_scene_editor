use bevy::prelude::*;

use crate::brush::BrushMeshCache;
use crate::gizmos::{GizmoDragState, GizmoMode};
use crate::modal_transform::{ModalOp, ModalTransformState, ViewportDragState};
use crate::selection::Selected;
use crate::viewport_overlays::{self, OverlaySettings};

const SPIKE_LENGTH_SCALE: f32 = 0.8;
const SPIKE_LENGTH_MIN: f32 = 10.0;
const SPIKE_COLOR: Color = Color::srgba(1.0, 1.0, 0.0, 0.5);
const ALIGN_THRESHOLD_FACTOR: f32 = 0.005;
const SNAP_THRESHOLD_FACTOR: f32 = 0.003;
const ALIGN_COLOR: Color = Color::srgba(0.0, 1.0, 1.0, 0.6);
const OVERLAP_FRACTION: f32 = 0.25;

pub struct AlignmentGuidesPlugin;

impl Plugin for AlignmentGuidesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AlignmentGuideState>()
            .add_systems(
                Update,
                (cache_reference_aabbs, draw_alignment_guides).chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

pub struct EntityAabb {
    pub entity: Entity,
    pub min: Vec3,
    pub max: Vec3,
}

#[derive(Resource, Default)]
pub struct AlignmentGuideState {
    pub reference_aabbs: Vec<EntityAabb>,
    pub cache_valid: bool,
}

// ---------------------------------------------------------------------------
// Drag detection
// ---------------------------------------------------------------------------

/// Returns true if a translation drag is currently active.
fn is_translate_drag_active(
    gizmo_drag: &GizmoDragState,
    gizmo_mode: &GizmoMode,
    modal_state: &ModalTransformState,
    viewport_drag: &ViewportDragState,
) -> bool {
    if gizmo_drag.active && *gizmo_mode == GizmoMode::Translate {
        return true;
    }
    if let Some(ref active) = modal_state.active {
        if active.op == ModalOp::Grab {
            return true;
        }
    }
    viewport_drag.active.is_some()
}

/// Returns the entity being dragged and its current world position.
fn dragged_entity_position(
    gizmo_drag: &GizmoDragState,
    gizmo_mode: &GizmoMode,
    modal_state: &ModalTransformState,
    viewport_drag: &ViewportDragState,
    transforms: &Query<&GlobalTransform>,
) -> Option<(Entity, Vec3)> {
    // Gizmo translate
    if gizmo_drag.active && *gizmo_mode == GizmoMode::Translate {
        if let Some(e) = gizmo_drag.entity {
            if let Ok(gt) = transforms.get(e) {
                return Some((e, gt.translation()));
            }
        }
    }
    // Modal grab
    if let Some(ref active) = modal_state.active {
        if active.op == ModalOp::Grab {
            if let Ok(gt) = transforms.get(active.entity) {
                return Some((active.entity, gt.translation()));
            }
        }
    }
    // Viewport drag
    if let Some(ref active) = viewport_drag.active {
        if let Ok(gt) = transforms.get(active.entity) {
            return Some((active.entity, gt.translation()));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Cache AABBs for all non-selected entities at drag start; clear on drag end.
fn cache_reference_aabbs(
    mut state: ResMut<AlignmentGuideState>,
    settings: Res<OverlaySettings>,
    gizmo_drag: Res<GizmoDragState>,
    gizmo_mode: Res<GizmoMode>,
    modal_state: Res<ModalTransformState>,
    viewport_drag: Res<ViewportDragState>,
    non_selected: Query<
        (Entity, &GlobalTransform, Option<&BrushMeshCache>),
        Without<Selected>,
    >,
    children_query: Query<&Children>,
    mesh_query: Query<(&Mesh3d, &GlobalTransform)>,
    meshes: Res<Assets<Mesh>>,
) {
    if !settings.show_alignment_guides {
        state.cache_valid = false;
        state.reference_aabbs.clear();
        return;
    }

    let dragging = is_translate_drag_active(
        &gizmo_drag, &gizmo_mode, &modal_state, &viewport_drag,
    );

    if !dragging {
        state.cache_valid = false;
        state.reference_aabbs.clear();
        return;
    }

    if state.cache_valid {
        return;
    }

    // Build cache
    state.reference_aabbs.clear();
    for (entity, global_tf, maybe_brush) in &non_selected {
        let world_verts = if let Some(cache) = maybe_brush {
            if cache.vertices.is_empty() {
                continue;
            }
            cache
                .vertices
                .iter()
                .map(|v| global_tf.transform_point(*v))
                .collect::<Vec<Vec3>>()
        } else {
            let mut verts = Vec::new();
            viewport_overlays::collect_descendant_mesh_world_vertices(
                entity,
                &children_query,
                &mesh_query,
                &meshes,
                &mut verts,
            );
            if verts.is_empty() {
                continue;
            }
            verts
        };

        let (min, max) = viewport_overlays::aabb_from_points(&world_verts);
        state.reference_aabbs.push(EntityAabb { entity, min, max });
    }
    state.cache_valid = true;
}

/// Draw spike guides and object-to-object alignment lines during drags.
fn draw_alignment_guides(
    mut gizmos: Gizmos,
    state: Res<AlignmentGuideState>,
    settings: Res<OverlaySettings>,
    gizmo_drag: Res<GizmoDragState>,
    gizmo_mode: Res<GizmoMode>,
    modal_state: Res<ModalTransformState>,
    viewport_drag: Res<ViewportDragState>,
    transforms: Query<&GlobalTransform>,
    camera_query: Query<&GlobalTransform, (With<Camera3d>, With<crate::EditorEntity>)>,
    selected: Query<
        (Entity, &GlobalTransform, Option<&BrushMeshCache>),
        With<Selected>,
    >,
    mut selected_transforms: Query<&mut Transform, With<Selected>>,
    children_query: Query<&Children>,
    mesh_query: Query<(&Mesh3d, &GlobalTransform)>,
    meshes: Res<Assets<Mesh>>,
) {
    if !settings.show_alignment_guides {
        return;
    }

    let Some((dragged_entity, drag_pos)) = dragged_entity_position(
        &gizmo_drag, &gizmo_mode, &modal_state, &viewport_drag, &transforms,
    ) else {
        return;
    };

    let cam_distance = camera_query
        .single()
        .map(|ct| ct.translation().distance(drag_pos))
        .unwrap_or(10.0);

    // --- Compute dragged entity AABB first (needed for spike origin + alignment) ---
    let dragged_aabb = {
        let mut verts = Vec::new();
        for (entity, global_tf, maybe_brush) in &selected {
            if entity != dragged_entity {
                continue;
            }
            if let Some(cache) = maybe_brush {
                for v in &cache.vertices {
                    verts.push(global_tf.transform_point(*v));
                }
            } else {
                viewport_overlays::collect_descendant_mesh_world_vertices(
                    entity,
                    &children_query,
                    &mesh_query,
                    &meshes,
                    &mut verts,
                );
            }
        }
        if verts.is_empty() {
            return;
        }
        viewport_overlays::aabb_from_points(&verts)
    };

    let (d_min, d_max) = dragged_aabb;
    let d_center = (d_min + d_max) * 0.5;

    // --- Spike guides (from AABB center, not transform origin) ---
    let spike_length = (cam_distance * SPIKE_LENGTH_SCALE).max(SPIKE_LENGTH_MIN);
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];

    for axis in &axes {
        for sign in [-1.0f32, 1.0] {
            let end = d_center + *axis * sign * spike_length;
            gizmos.line(d_center, end, SPIKE_COLOR);
        }
    }

    // --- Object-to-object alignment ---
    let threshold = cam_distance * ALIGN_THRESHOLD_FACTOR;
    let snap_threshold = cam_distance * SNAP_THRESHOLD_FACTOR;

    // Track best snap per axis: (delta, r_val for line drawing, ref AABB)
    let mut best_snap: [Option<(f32, f32, f32, Vec3, Vec3)>; 3] = [None; 3]; // (abs_delta, delta, aligned_val, ref_min, ref_max)

    let d_vals = [
        [d_min.x, d_max.x, d_center.x, d_min.x, d_max.x],
        [d_min.y, d_max.y, d_center.y, d_min.y, d_max.y],
        [d_min.z, d_max.z, d_center.z, d_min.z, d_max.z],
    ];

    for ref_aabb in &state.reference_aabbs {
        let r_center = (ref_aabb.min + ref_aabb.max) * 0.5;
        let r_vals = [
            [ref_aabb.max.x, ref_aabb.min.x, r_center.x, ref_aabb.min.x, ref_aabb.max.x],
            [ref_aabb.max.y, ref_aabb.min.y, r_center.y, ref_aabb.min.y, ref_aabb.max.y],
            [ref_aabb.max.z, ref_aabb.min.z, r_center.z, ref_aabb.min.z, ref_aabb.max.z],
        ];

        for axis_idx in 0..3 {
            // Meaningful pairs: (d_min, r_max), (d_max, r_min), (d_center, r_center), (d_min, r_min), (d_max, r_max)
            for pair_idx in 0..5 {
                let d_val = d_vals[axis_idx][pair_idx];
                let r_val = r_vals[axis_idx][pair_idx];
                let delta = r_val - d_val;
                let abs_delta = delta.abs();

                if abs_delta < threshold {
                    let aligned_val = r_val;

                    // Draw guide line between the two entities along the perpendicular axes
                    draw_alignment_line(
                        &mut gizmos,
                        axis_idx,
                        aligned_val,
                        d_min,
                        d_max,
                        ref_aabb.min,
                        ref_aabb.max,
                        ALIGN_COLOR,
                    );

                    // Track closest snap per axis
                    if abs_delta < snap_threshold {
                        let is_better = match best_snap[axis_idx] {
                            Some((prev_abs, _, _, _, _)) => abs_delta < prev_abs,
                            None => true,
                        };
                        if is_better {
                            best_snap[axis_idx] = Some((abs_delta, delta, aligned_val, ref_aabb.min, ref_aabb.max));
                        }
                    }
                }
            }
        }
    }

    // Apply snaps
    if let Ok(mut transform) = selected_transforms.get_mut(dragged_entity) {
        for axis_idx in 0..3 {
            if let Some((_, delta, _, _, _)) = best_snap[axis_idx] {
                match axis_idx {
                    0 => transform.translation.x += delta,
                    1 => transform.translation.y += delta,
                    2 => transform.translation.z += delta,
                    _ => {}
                }
            }
        }
    }
}

/// Draw a straight guide line at the aligned coordinate value, spanning between
/// the two entities' AABBs on the most relevant perpendicular axis.
fn draw_alignment_line(
    gizmos: &mut Gizmos,
    axis_idx: usize,
    aligned_val: f32,
    d_min: Vec3,
    d_max: Vec3,
    r_min: Vec3,
    r_max: Vec3,
    color: Color,
) {
    // Pick the perpendicular axis with the largest combined extent to draw along
    let perp_axes: [(usize, usize); 3] = [(1, 2), (0, 2), (0, 1)];
    let (perp_a, perp_b) = perp_axes[axis_idx];

    // For each perpendicular axis, find the span that connects both AABBs
    for &perp in &[perp_a, perp_b] {
        let d_perp_center = (d_min[perp] + d_max[perp]) * 0.5;
        let r_perp_center = (r_min[perp] + r_max[perp]) * 0.5;

        // Only draw along this perp axis if the entities are separated along it
        let d_perp_extent = d_max[perp] - d_min[perp];
        let r_perp_extent = r_max[perp] - r_min[perp];
        let separation = (d_perp_center - r_perp_center).abs();
        if separation < (d_perp_extent + r_perp_extent) * OVERLAP_FRACTION {
            continue; // entities overlap on this axis, skip
        }

        // Line from dragged entity edge to reference entity edge
        let (line_start_perp, line_end_perp) = if d_perp_center < r_perp_center {
            (d_max[perp], r_min[perp])
        } else {
            (d_min[perp], r_max[perp])
        };

        let mut start = Vec3::ZERO;
        let mut end = Vec3::ZERO;
        start[axis_idx] = aligned_val;
        end[axis_idx] = aligned_val;
        start[perp] = line_start_perp;
        end[perp] = line_end_perp;
        // Set the other perpendicular axis to the midpoint of both AABBs
        let other_perp = if perp == perp_a { perp_b } else { perp_a };
        let other_mid = (d_min[other_perp] + d_max[other_perp] + r_min[other_perp] + r_max[other_perp]) * 0.25;
        start[other_perp] = other_mid;
        end[other_perp] = other_mid;

        gizmos.line(start, end, color);
        return; // Only draw along one perp axis
    }

    // Fallback: if entities overlap on all perp axes, draw a short line along the aligned axis
    let mut start = (d_min + d_max) * 0.5;
    let mut end = (r_min + r_max) * 0.5;
    start[axis_idx] = aligned_val;
    end[axis_idx] = aligned_val;
    gizmos.line(start, end, color);
}
