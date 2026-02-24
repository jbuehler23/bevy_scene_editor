use bevy::{
    input_focus::InputFocus,
    picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility},
    prelude::*,
    ui::UiGlobalTransform,
};

use bevy_jsn::{Brush, BrushFaceData, BrushPlane};
use bevy_jsn_geometry::{
    brush_planes_to_world, brushes_intersect, clean_degenerate_faces, compute_brush_geometry,
    compute_face_tangent_axes, subtract_brush,
};
use crate::{
    brush::BrushFaceEntity,
    commands::{CommandHistory, EditorCommand, snapshot_entity, snapshot_rebuild},
    selection::{Selected, Selection},
    snapping::SnapSettings,
    viewport::SceneViewport,
    viewport_util::window_to_viewport_cursor,
    EditorEntity,
};

const EXTRUDE_DEPTH_SENSITIVITY: f32 = 0.003;
const MIN_FOOTPRINT_SIZE: f32 = 0.01;
const MIN_EXTRUDE_DEPTH: f32 = 0.01;

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrawPhase {
    PlacingFirstCorner,
    DrawingFootprint,
    ExtrudingDepth,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DrawMode {
    #[default]
    Add,
    Cut,
}

#[derive(Clone, Debug)]
pub struct DrawPlane {
    pub origin: Vec3,
    pub normal: Vec3,
    pub axis_u: Vec3,
    pub axis_v: Vec3,
}

#[derive(Clone, Debug)]
pub struct ActiveDraw {
    pub corner1: Vec3,
    pub corner2: Vec3,
    pub depth: f32,
    pub phase: DrawPhase,
    pub mode: DrawMode,
    pub plane: DrawPlane,
    pub extrude_start_cursor: Vec2,
    pub plane_locked: bool,
    /// World-space cursor position on the drawing plane (for crosshair preview).
    pub cursor_on_plane: Option<Vec3>,
}

#[derive(Resource, Default)]
pub struct DrawBrushState {
    pub active: Option<ActiveDraw>,
}

// ---------------------------------------------------------------------------
// Undo command
// ---------------------------------------------------------------------------

pub struct CreateBrushCommand {
    pub entity: Entity,
    pub scene_snapshot: DynamicScene,
}

impl EditorCommand for CreateBrushCommand {
    fn execute(&self, world: &mut World) {
        // Redo: respawn from snapshot
        let scene = snapshot_rebuild(&self.scene_snapshot);
        let _result = scene.write_to_world(world, &mut Default::default());
    }

    fn undo(&self, world: &mut World) {
        if let Ok(entity_mut) = world.get_entity_mut(self.entity) {
            entity_mut.despawn();
        }
    }

    fn description(&self) -> &str {
        "Draw brush"
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct DrawBrushPlugin;

impl Plugin for DrawBrushPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DrawBrushState>()
            .add_systems(
                Update,
                (
                    draw_brush_activate,
                    draw_brush_update,
                    draw_brush_confirm,
                    draw_brush_cancel,
                    draw_brush_preview,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// Activate: B key enters draw mode
// ---------------------------------------------------------------------------

fn draw_brush_activate(
    keyboard: Res<ButtonInput<KeyCode>>,
    input_focus: Res<InputFocus>,
    mut draw_state: ResMut<DrawBrushState>,
    modal: Res<crate::modal_transform::ModalTransformState>,
    edit_mode: Res<crate::brush::EditMode>,
    walk_mode: Res<crate::viewport::WalkModeState>,
) {
    // Handle Tab toggle while in draw mode
    if let Some(ref mut active) = draw_state.active {
        if keyboard.just_pressed(KeyCode::Tab) && active.phase == DrawPhase::PlacingFirstCorner {
            active.mode = match active.mode {
                DrawMode::Add => DrawMode::Cut,
                DrawMode::Cut => DrawMode::Add,
            };
        }
        return;
    }

    if !keyboard.just_pressed(KeyCode::KeyB) {
        return;
    }
    // Standard guards
    if input_focus.0.is_some()
        || modal.active.is_some()
        || *edit_mode != crate::brush::EditMode::Object
        || walk_mode.active
    {
        return;
    }

    draw_state.active = Some(ActiveDraw {
        corner1: Vec3::ZERO,
        corner2: Vec3::ZERO,
        depth: 0.0,
        phase: DrawPhase::PlacingFirstCorner,
        mode: DrawMode::Add,
        plane: DrawPlane {
            origin: Vec3::ZERO,
            normal: Vec3::Y,
            axis_u: Vec3::X,
            axis_v: Vec3::Z,
        },
        extrude_start_cursor: Vec2::ZERO,
        plane_locked: false,
        cursor_on_plane: None,
    });
}

// ---------------------------------------------------------------------------
// Update: track surface / project to plane / compute depth
// ---------------------------------------------------------------------------

fn draw_brush_update(
    mut draw_state: ResMut<DrawBrushState>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    snap_settings: Res<SnapSettings>,
    mut ray_cast: MeshRayCast,
    brush_faces: Query<(&BrushFaceEntity, &GlobalTransform)>,
    brushes: Query<(&Brush, &GlobalTransform)>,
) {
    let Some(ref mut active) = draw_state.active else {
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
    let Some(viewport_cursor) = window_to_viewport_cursor(cursor_pos, camera, &viewport_query)
    else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(cam_tf, viewport_cursor) else {
        return;
    };

    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);

    match active.phase {
        DrawPhase::PlacingFirstCorner => {
            // Ctrl toggles plane lock
            if ctrl {
                active.plane_locked = true;
            } else {
                active.plane_locked = false;
            }

            if !active.plane_locked {
                // Raycast against brush face meshes
                let settings = MeshRayCastSettings::default()
                    .with_visibility(RayCastVisibility::Any);
                let hits = ray_cast.cast_ray(ray, &settings);

                let mut found_face = false;
                for (hit_entity, hit_data) in hits {
                    if let Ok((face_ent, _face_tf)) = brush_faces.get(*hit_entity) {
                        if let Ok((brush, brush_tf)) = brushes.get(face_ent.brush_entity) {
                            let face = &brush.faces[face_ent.face_index];
                            let (_, brush_rot, _) = brush_tf.to_scale_rotation_translation();
                            let world_normal = (brush_rot * face.plane.normal).normalize();
                            let hit_point = hit_data.point;

                            let (u, v) = compute_face_tangent_axes(world_normal);
                            active.plane = DrawPlane {
                                origin: hit_point,
                                normal: world_normal,
                                axis_u: u,
                                axis_v: v,
                            };
                            found_face = true;
                            break;
                        }
                    }
                }

                if !found_face {
                    // Fall back to Y=0 ground plane
                    if let Some(ground_hit) = ray_plane_intersection(ray, Vec3::ZERO, Vec3::Y) {
                        active.plane = DrawPlane {
                            origin: ground_hit,
                            normal: Vec3::Y,
                            axis_u: Vec3::X,
                            axis_v: Vec3::Z,
                        };
                    }
                }
            }

            // Project cursor onto current plane
            if let Some(hit) = ray_plane_intersection(ray, active.plane.origin, active.plane.normal)
            {
                let snapped = snap_to_plane_grid(hit, &active.plane, &snap_settings, ctrl);
                active.cursor_on_plane = Some(snapped);
            }
        }
        DrawPhase::DrawingFootprint => {
            // Project cursor onto the locked drawing plane
            if let Some(hit) = ray_plane_intersection(ray, active.plane.origin, active.plane.normal)
            {
                let snapped = snap_to_plane_grid(hit, &active.plane, &snap_settings, ctrl);
                active.corner2 = snapped;
            }
        }
        DrawPhase::ExtrudingDepth => {
            let center = (active.corner1 + active.corner2) / 2.0;
            let cam_dist = (cam_tf.translation() - center).length();

            // Project the plane normal to screen space to determine drag direction
            if let (Ok(origin_screen), Ok(normal_screen)) = (
                camera.world_to_viewport(cam_tf, center),
                camera.world_to_viewport(cam_tf, center + active.plane.normal),
            ) {
                let screen_dir = (normal_screen - origin_screen).normalize_or_zero();
                let mouse_delta = viewport_cursor - active.extrude_start_cursor;
                let projected = mouse_delta.dot(screen_dir);
                let raw_depth = projected * cam_dist * EXTRUDE_DEPTH_SENSITIVITY;

                // Snap depth
                let depth = if snap_settings.translate_active(ctrl)
                    && snap_settings.translate_increment > 0.0
                {
                    (raw_depth / snap_settings.translate_increment).round()
                        * snap_settings.translate_increment
                } else {
                    raw_depth
                };
                active.depth = depth;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Confirm: left-click advances phase or spawns brush
// ---------------------------------------------------------------------------

fn draw_brush_confirm(
    mouse: Res<ButtonInput<MouseButton>>,
    mut draw_state: ResMut<DrawBrushState>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    mut commands: Commands,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Some(ref mut active) = draw_state.active else {
        return;
    };

    // Verify cursor is in viewport
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, _)) = camera_query.single() else {
        return;
    };
    let Some(viewport_cursor) = window_to_viewport_cursor(cursor_pos, camera, &viewport_query)
    else {
        return;
    };

    match active.phase {
        DrawPhase::PlacingFirstCorner => {
            if let Some(pos) = active.cursor_on_plane {
                active.corner1 = pos;
                active.corner2 = pos;
                active.phase = DrawPhase::DrawingFootprint;
            }
        }
        DrawPhase::DrawingFootprint => {
            // Enforce minimum size
            let delta = active.corner2 - active.corner1;
            let u_size = delta.dot(active.plane.axis_u).abs();
            let v_size = delta.dot(active.plane.axis_v).abs();
            if u_size < MIN_FOOTPRINT_SIZE || v_size < MIN_FOOTPRINT_SIZE {
                return; // Too small, keep drawing
            }
            active.phase = DrawPhase::ExtrudingDepth;
            active.extrude_start_cursor = viewport_cursor;
            active.depth = 0.0;
        }
        DrawPhase::ExtrudingDepth => {
            if active.depth.abs() < MIN_EXTRUDE_DEPTH {
                return; // No depth, keep extruding
            }
            let active_owned = active.clone();
            match active_owned.mode {
                DrawMode::Add => {
                    draw_state.active = None;
                    spawn_drawn_brush(&active_owned, &mut commands);
                }
                DrawMode::Cut => {
                    // Defer clearing draw state to command application so that
                    // handle_viewport_click still sees draw mode as active during
                    // this frame and skips the click (preventing entity-despawn races).
                    subtract_drawn_brush(&active_owned, &mut commands);
                    commands.queue(|world: &mut World| {
                        world.resource_mut::<DrawBrushState>().active = None;
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cancel: Escape or right-click
// ---------------------------------------------------------------------------

fn draw_brush_cancel(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut draw_state: ResMut<DrawBrushState>,
) {
    if draw_state.active.is_none() {
        return;
    }
    if keyboard.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right) {
        draw_state.active = None;
    }
}

// ---------------------------------------------------------------------------
// Preview wireframe using Gizmos
// ---------------------------------------------------------------------------

const DRAW_COLOR: Color = Color::srgb(1.0, 0.6, 0.0);
const CUT_COLOR: Color = Color::srgb(1.0, 0.2, 0.2);

fn draw_brush_preview(
    draw_state: Res<DrawBrushState>,
    snap_settings: Res<SnapSettings>,
    mut gizmos: Gizmos,
    brushes: Query<(&Brush, &GlobalTransform)>,
) {
    let Some(ref active) = draw_state.active else {
        return;
    };

    let color = match active.mode {
        DrawMode::Add => DRAW_COLOR,
        DrawMode::Cut => CUT_COLOR,
    };

    match active.phase {
        DrawPhase::PlacingFirstCorner => {
            // Crosshair at cursor on surface
            if let Some(pos) = active.cursor_on_plane {
                let size = 0.3;
                gizmos.line(
                    pos - active.plane.axis_u * size,
                    pos + active.plane.axis_u * size,
                    color,
                );
                gizmos.line(
                    pos - active.plane.axis_v * size,
                    pos + active.plane.axis_v * size,
                    color,
                );

                // Draw plane grid overlay
                draw_plane_grid(&mut gizmos, &active.plane, pos, &snap_settings);
            }
        }
        DrawPhase::DrawingFootprint => {
            // Rectangle on the plane from corner1 to corner2
            let corners = footprint_corners(active);
            for i in 0..4 {
                gizmos.line(corners[i], corners[(i + 1) % 4], color);
            }

            // Draw plane grid overlay centered on midpoint of footprint
            let mid = (active.corner1 + active.corner2) / 2.0;
            draw_plane_grid(&mut gizmos, &active.plane, mid, &snap_settings);
        }
        DrawPhase::ExtrudingDepth => {
            // Cuboid wireframe
            let base = footprint_corners(active);
            let offset = active.plane.normal * active.depth;
            let top: [Vec3; 4] = [
                base[0] + offset,
                base[1] + offset,
                base[2] + offset,
                base[3] + offset,
            ];
            // Base rectangle
            for i in 0..4 {
                gizmos.line(base[i], base[(i + 1) % 4], color);
            }
            // Top rectangle
            for i in 0..4 {
                gizmos.line(top[i], top[(i + 1) % 4], color);
            }
            // Connecting edges
            for i in 0..4 {
                gizmos.line(base[i], top[i], color);
            }

            // Cut mode: show intersection outlines on affected brushes.
            // The intersection volume's edges lie on brush surfaces, so they're
            // visible even when the cutter is inside solid geometry.
            if active.mode == DrawMode::Cut {
                let cutter_planes = build_cutter_planes(active);
                for (brush, brush_tf) in &brushes {
                    let (_, rotation, translation) = brush_tf.to_scale_rotation_translation();
                    let world_target = brush_planes_to_world(&brush.faces, rotation, translation);
                    let mut combined = world_target;
                    combined.extend_from_slice(&cutter_planes);
                    let (verts, polys) = compute_brush_geometry(&combined);
                    if verts.len() < 4 {
                        continue;
                    }
                    for polygon in &polys {
                        if polygon.len() < 2 {
                            continue;
                        }
                        for i in 0..polygon.len() {
                            let a = verts[polygon[i]];
                            let b = verts[polygon[(i + 1) % polygon.len()]];
                            gizmos.line(a, b, color);
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Brush spawning
// ---------------------------------------------------------------------------

fn spawn_drawn_brush(active: &ActiveDraw, commands: &mut Commands) {
    let plane = &active.plane;

    // Decompose corners into plane-local u/v coordinates
    let c1_u = (active.corner1 - plane.origin).dot(plane.axis_u);
    let c1_v = (active.corner1 - plane.origin).dot(plane.axis_v);
    let c2_u = (active.corner2 - plane.origin).dot(plane.axis_u);
    let c2_v = (active.corner2 - plane.origin).dot(plane.axis_v);

    let min_u = c1_u.min(c2_u);
    let max_u = c1_u.max(c2_u);
    let min_v = c1_v.min(c2_v);
    let max_v = c1_v.max(c2_v);

    let half_u = (max_u - min_u) / 2.0;
    let half_v = (max_v - min_v) / 2.0;
    let half_depth = active.depth.abs() / 2.0;

    // Center on the plane
    let center_on_plane = plane.origin
        + plane.axis_u * (min_u + max_u) / 2.0
        + plane.axis_v * (min_v + max_v) / 2.0;
    let center = center_on_plane + plane.normal * active.depth / 2.0;

    // For ground-plane (normal=Y): axis_u=X, axis_v=Z, normal=Y
    // Brush::cuboid uses half_x, half_y, half_z in local space
    // We need to map: local X -> axis_u, local Y -> normal, local Z -> axis_v
    let brush = Brush::cuboid(half_u, half_depth, half_v);

    // Build rotation that maps local (X,Y,Z) -> (axis_u, normal, axis_v)
    let rotation = if plane.normal == Vec3::Y {
        Quat::IDENTITY
    } else if plane.normal == Vec3::NEG_Y {
        Quat::from_rotation_x(std::f32::consts::PI)
    } else {
        let target_mat = Mat3::from_cols(plane.axis_u, plane.normal, -plane.axis_v);
        Quat::from_mat3(&target_mat)
    };

    commands.queue(move |world: &mut World| {
        let entity = world
            .spawn((
                Name::new("Brush"),
                brush,
                Transform {
                    translation: center,
                    rotation,
                    scale: Vec3::ONE,
                },
                Visibility::default(),
            ))
            .id();

        // Select the new brush
        {
            // Deselect current selection
            let selection = world.resource::<Selection>();
            let old_selected: Vec<Entity> = selection.entities.clone();
            for &e in &old_selected {
                if let Ok(mut ec) = world.get_entity_mut(e) {
                    ec.remove::<Selected>();
                }
            }
            let mut selection = world.resource_mut::<Selection>();
            selection.entities = vec![entity];
            world.entity_mut(entity).insert(Selected);
        }

        // Snapshot for undo
        let snapshot = snapshot_entity(world, entity);
        let cmd = CreateBrushCommand {
            entity,
            scene_snapshot: snapshot,
        };
        let mut history = world.resource_mut::<CommandHistory>();
        history.undo_stack.push(Box::new(cmd));
        history.redo_stack.clear();
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Intersect a ray with a plane defined by a point and normal.
fn ray_plane_intersection(ray: Ray3d, plane_point: Vec3, plane_normal: Vec3) -> Option<Vec3> {
    let denom = ray.direction.dot(plane_normal);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = (plane_point - ray.origin).dot(plane_normal) / denom;
    if t < 0.0 {
        return None;
    }
    Some(ray.origin + *ray.direction * t)
}

/// Draw a grid of small crosses on the drawing plane, centered near `center`.
/// Grid points are world-aligned (fixed at world-space multiples of `inc`),
/// so only the visible window moves with the cursor — individual crosses stay put.
fn draw_plane_grid(
    gizmos: &mut Gizmos,
    plane: &DrawPlane,
    center: Vec3,
    snap_settings: &SnapSettings,
) {
    let inc = snap_settings.grid_size();
    let cross_size = inc * 0.1;
    let range = 10_i32;
    let fade_radius = range as f32 * inc;

    // World-aligned: project center directly onto axes (not relative to plane.origin)
    let u_center = (center.dot(plane.axis_u) / inc).round() as i32;
    let v_center = (center.dot(plane.axis_v) / inc).round() as i32;

    // Distance of the plane from the world origin along its normal
    let plane_d = plane.origin.dot(plane.normal);

    for du in -range..=range {
        for dv in -range..=range {
            let u = (u_center + du) as f32 * inc;
            let v = (v_center + dv) as f32 * inc;
            let pt = plane.axis_u * u + plane.axis_v * v + plane.normal * plane_d;

            // Distance-based alpha fade from cursor
            let dist = (pt - center).length();
            let alpha = (1.0 - dist / fade_radius).clamp(0.0, 0.3);
            if alpha <= 0.0 {
                continue;
            }
            let grid_color = Color::srgba(0.5, 0.5, 0.5, alpha);

            gizmos.line(
                pt - plane.axis_u * cross_size,
                pt + plane.axis_u * cross_size,
                grid_color,
            );
            gizmos.line(
                pt - plane.axis_v * cross_size,
                pt + plane.axis_v * cross_size,
                grid_color,
            );
        }
    }
}

/// Snap a world-space hit point to a world-aligned grid on the drawing plane.
fn snap_to_plane_grid(
    hit: Vec3,
    plane: &DrawPlane,
    snap_settings: &SnapSettings,
    ctrl: bool,
) -> Vec3 {
    if !snap_settings.translate_active(ctrl) || snap_settings.translate_increment <= 0.0 {
        return hit;
    }
    let inc = snap_settings.translate_increment;
    // World-aligned: snap using world-space projections onto axes
    let u = hit.dot(plane.axis_u);
    let v = hit.dot(plane.axis_v);
    let snapped_u = (u / inc).round() * inc;
    let snapped_v = (v / inc).round() * inc;
    let plane_d = plane.origin.dot(plane.normal);
    plane.axis_u * snapped_u + plane.axis_v * snapped_v + plane.normal * plane_d
}

/// Compute the 4 world-space corners of the footprint rectangle.
fn footprint_corners(active: &ActiveDraw) -> [Vec3; 4] {
    let plane = &active.plane;
    let c1_u = (active.corner1 - plane.origin).dot(plane.axis_u);
    let c1_v = (active.corner1 - plane.origin).dot(plane.axis_v);
    let c2_u = (active.corner2 - plane.origin).dot(plane.axis_u);
    let c2_v = (active.corner2 - plane.origin).dot(plane.axis_v);

    let min_u = c1_u.min(c2_u);
    let max_u = c1_u.max(c2_u);
    let min_v = c1_v.min(c2_v);
    let max_v = c1_v.max(c2_v);

    [
        plane.origin + plane.axis_u * min_u + plane.axis_v * min_v,
        plane.origin + plane.axis_u * max_u + plane.axis_v * min_v,
        plane.origin + plane.axis_u * max_u + plane.axis_v * max_v,
        plane.origin + plane.axis_u * min_u + plane.axis_v * max_v,
    ]
}

// ---------------------------------------------------------------------------
// CSG subtraction (Cut mode)
// ---------------------------------------------------------------------------

/// Build 6 world-space cutter planes from the ActiveDraw cuboid.
fn build_cutter_planes(active: &ActiveDraw) -> Vec<BrushFaceData> {
    let plane = &active.plane;

    let c1_u = (active.corner1 - plane.origin).dot(plane.axis_u);
    let c1_v = (active.corner1 - plane.origin).dot(plane.axis_v);
    let c2_u = (active.corner2 - plane.origin).dot(plane.axis_u);
    let c2_v = (active.corner2 - plane.origin).dot(plane.axis_v);

    let min_u = c1_u.min(c2_u);
    let max_u = c1_u.max(c2_u);
    let min_v = c1_v.min(c2_v);
    let max_v = c1_v.max(c2_v);

    let half_u = (max_u - min_u) / 2.0;
    let half_v = (max_v - min_v) / 2.0;
    let half_depth = active.depth.abs() / 2.0;

    let center_on_plane = plane.origin
        + plane.axis_u * (min_u + max_u) / 2.0
        + plane.axis_v * (min_v + max_v) / 2.0;
    let center = center_on_plane + plane.normal * active.depth / 2.0;

    vec![
        // +U face
        BrushFaceData {
            plane: BrushPlane {
                normal: plane.axis_u,
                distance: plane.axis_u.dot(center) + half_u,
            },
            uv_scale: Vec2::ONE,
            ..default()
        },
        // -U face
        BrushFaceData {
            plane: BrushPlane {
                normal: -plane.axis_u,
                distance: (-plane.axis_u).dot(center) + half_u,
            },
            uv_scale: Vec2::ONE,
            ..default()
        },
        // +V face
        BrushFaceData {
            plane: BrushPlane {
                normal: plane.axis_v,
                distance: plane.axis_v.dot(center) + half_v,
            },
            uv_scale: Vec2::ONE,
            ..default()
        },
        // -V face
        BrushFaceData {
            plane: BrushPlane {
                normal: -plane.axis_v,
                distance: (-plane.axis_v).dot(center) + half_v,
            },
            uv_scale: Vec2::ONE,
            ..default()
        },
        // +Normal face (depth direction)
        BrushFaceData {
            plane: BrushPlane {
                normal: plane.normal,
                distance: plane.normal.dot(center) + half_depth,
            },
            uv_scale: Vec2::ONE,
            ..default()
        },
        // -Normal face
        BrushFaceData {
            plane: BrushPlane {
                normal: -plane.normal,
                distance: (-plane.normal).dot(center) + half_depth,
            },
            uv_scale: Vec2::ONE,
            ..default()
        },
    ]
}

/// Perform CSG subtraction: subtract the drawn cuboid from all intersecting brushes.
fn subtract_drawn_brush(active: &ActiveDraw, commands: &mut Commands) {
    let cutter_planes = build_cutter_planes(active);

    commands.queue(move |world: &mut World| {
        // Phase 1: Collect all brush entities and their data
        let mut query = world.query::<(Entity, &Brush, &GlobalTransform)>();
        let targets: Vec<(Entity, Brush, GlobalTransform)> = query
            .iter(world)
            .map(|(e, b, gt)| (e, b.clone(), *gt))
            .collect();

        // Phase 2: Compute subtractions (pure computation)
        struct SubtractionResult {
            original_entity: Entity,
            fragments: Vec<(Brush, Transform)>,
        }

        let mut results: Vec<SubtractionResult> = Vec::new();

        for (entity, brush, global_transform) in &targets {
            // Transform target planes to world space
            let (_, rotation, translation) = global_transform.to_scale_rotation_translation();
            let world_target = brush_planes_to_world(&brush.faces, rotation, translation);

            // Check intersection
            if !brushes_intersect(&world_target, &cutter_planes) {
                continue;
            }

            // Perform subtraction
            let raw_fragments = subtract_brush(&world_target, &cutter_planes);

            let mut fragment_data: Vec<(Brush, Transform)> = Vec::new();
            for fragment_faces in &raw_fragments {
                // Compute vertices to find centroid (world space)
                let (world_verts, _) = compute_brush_geometry(fragment_faces);
                if world_verts.len() < 4 {
                    continue;
                }
                let centroid: Vec3 =
                    world_verts.iter().sum::<Vec3>() / world_verts.len() as f32;

                // Convert to local space around centroid
                let local_faces: Vec<BrushFaceData> = fragment_faces
                    .iter()
                    .map(|f| BrushFaceData {
                        plane: BrushPlane {
                            normal: f.plane.normal,
                            distance: f.plane.distance - f.plane.normal.dot(centroid),
                        },
                        ..f.clone()
                    })
                    .collect();

                // Clean degenerate faces
                let clean = clean_degenerate_faces(&local_faces);
                if clean.len() < 4 {
                    continue;
                }

                fragment_data.push((
                    Brush { faces: clean },
                    Transform::from_translation(centroid),
                ));
            }

            results.push(SubtractionResult {
                original_entity: *entity,
                fragments: fragment_data,
            });
        }

        if results.is_empty() {
            return;
        }

        // Phase 3: Snapshot originals (just the entity, not children — children are rebuilt
        // automatically by regenerate_brush_meshes when Brush component changes)
        let mut original_snapshots: Vec<(Entity, DynamicScene)> = Vec::new();
        for result in &results {
            let snapshot = DynamicSceneBuilder::from_world(world)
                .extract_entities(std::iter::once(result.original_entity))
                .build();
            original_snapshots.push((result.original_entity, snapshot));
        }

        // Clean up selection: remove originals that are about to be despawned
        {
            let mut selection = world.resource_mut::<Selection>();
            let despawning: Vec<Entity> =
                original_snapshots.iter().map(|(e, _)| *e).collect();
            selection.entities.retain(|e| !despawning.contains(e));
        }
        for (entity, _) in &original_snapshots {
            if let Ok(mut e) = world.get_entity_mut(*entity) {
                e.remove::<Selected>();
            }
        }

        // Despawn originals
        for (entity, _) in &original_snapshots {
            if let Ok(e) = world.get_entity_mut(*entity) {
                e.despawn();
            }
        }

        // Spawn fragments
        let mut fragment_snapshots: Vec<(Entity, DynamicScene)> = Vec::new();
        for result in &results {
            for (brush, transform) in &result.fragments {
                let entity = world
                    .spawn((
                        Name::new("Brush"),
                        brush.clone(),
                        *transform,
                        Visibility::default(),
                    ))
                    .id();
                let snapshot = DynamicSceneBuilder::from_world(world)
                    .extract_entities(std::iter::once(entity))
                    .build();
                fragment_snapshots.push((entity, snapshot));
            }
        }

        // Push undo command
        let cmd = SubtractBrushCommand {
            originals: original_snapshots,
            fragments: fragment_snapshots,
        };
        let mut history = world.resource_mut::<CommandHistory>();
        history.undo_stack.push(Box::new(cmd));
        history.redo_stack.clear();
    });
}

// ---------------------------------------------------------------------------
// Undo command for CSG subtraction
// ---------------------------------------------------------------------------

struct SubtractBrushCommand {
    /// Original brushes to restore on undo (entity + snapshot).
    originals: Vec<(Entity, DynamicScene)>,
    /// Fragment brushes spawned by the subtraction (entity + snapshot).
    fragments: Vec<(Entity, DynamicScene)>,
}

impl EditorCommand for SubtractBrushCommand {
    fn execute(&self, world: &mut World) {
        // Redo: clean up selection, despawn originals, respawn fragments
        {
            let entities: Vec<Entity> = self.originals.iter().map(|(e, _)| *e).collect();
            let mut selection = world.resource_mut::<Selection>();
            selection.entities.retain(|e| !entities.contains(e));
        }
        for (entity, _) in &self.originals {
            if let Ok(mut e) = world.get_entity_mut(*entity) {
                e.remove::<Selected>();
            }
        }
        for (entity, _) in &self.originals {
            if let Ok(e) = world.get_entity_mut(*entity) {
                e.despawn();
            }
        }
        for (_, snapshot) in &self.fragments {
            let scene = snapshot_rebuild(snapshot);
            let _ = scene.write_to_world(world, &mut Default::default());
        }
    }

    fn undo(&self, world: &mut World) {
        // Undo: clean up selection, despawn fragments, respawn originals
        {
            let entities: Vec<Entity> = self.fragments.iter().map(|(e, _)| *e).collect();
            let mut selection = world.resource_mut::<Selection>();
            selection.entities.retain(|e| !entities.contains(e));
        }
        for (entity, _) in &self.fragments {
            if let Ok(mut e) = world.get_entity_mut(*entity) {
                e.remove::<Selected>();
            }
        }
        for (entity, _) in &self.fragments {
            if let Ok(e) = world.get_entity_mut(*entity) {
                e.despawn();
            }
        }
        for (_, snapshot) in &self.originals {
            let scene = snapshot_rebuild(snapshot);
            let _ = scene.write_to_world(world, &mut Default::default());
        }
    }

    fn description(&self) -> &str {
        "Subtract brush"
    }
}
