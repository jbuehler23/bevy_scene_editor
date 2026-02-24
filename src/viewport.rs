use bevy::{
    camera::RenderTarget,
    image::ImageSampler,
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    ui::{widget::ViewportNode, UiGlobalTransform},
};
use bevy_infinite_grid::InfiniteGridPlugin;
use bevy_panorbit_camera::{ActiveCameraData, PanOrbitCamera, PanOrbitCameraPlugin};

use crate::selection::{Selected, Selection};
use editor_widgets::file_browser::FileBrowserItem;

const DEFAULT_VIEWPORT_WIDTH: u32 = 1280;
const DEFAULT_VIEWPORT_HEIGHT: u32 = 720;

/// Marker on the center-panel UI node that hosts the 3D viewport.
#[derive(Component)]
pub struct SceneViewport;

pub struct ViewportPlugin;

impl Plugin for ViewportPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            PanOrbitCameraPlugin,
            InfiniteGridPlugin,
        ))
        .init_resource::<CameraBookmarks>()
        .init_resource::<WalkModeState>()
        .init_resource::<OrbitCenterVisibility>()
        .add_systems(Startup, setup_viewport.after(crate::spawn_layout))
        .add_systems(Update, (
            update_viewport_focus,
            handle_camera_keys,
            walk_mode_update,
            draw_orbit_center,
        ));
    }
}

fn setup_viewport(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    viewport_query: Single<Entity, With<SceneViewport>>,
) {
    // Create render-target image
    let size = Extent3d {
        width: DEFAULT_VIEWPORT_WIDTH,
        height: DEFAULT_VIEWPORT_HEIGHT,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Bgra8UnormSrgb,
        default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = ImageSampler::linear();
    let image_handle = images.add(image);

    // Spawn 3D camera (marked EditorEntity so it's hidden from hierarchy and undeletable)
    let camera = commands
        .spawn((
            crate::EditorEntity,
            Camera3d::default(),
            Camera {
                order: -1,
                ..default()
            },
            RenderTarget::Image(image_handle.into()),
            Transform::from_xyz(0.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
            PanOrbitCamera {
                focus: Vec3::ZERO,
                button_orbit: MouseButton::Middle,
                button_pan: MouseButton::Middle,
                modifier_pan: Some(KeyCode::ShiftLeft),
                ..default()
            },
        ))
        .id();

    // Spawn infinite grid (marked EditorEntity so it's hidden from hierarchy and undeletable)
    commands.spawn((crate::EditorEntity, bevy_infinite_grid::InfiniteGridBundle::default()));

    // Attach ViewportNode to the SceneViewport UI entity
    commands
        .entity(*viewport_query)
        .insert(ViewportNode::new(camera))
        .observe(handle_viewport_drop);
}

/// Handle files dropped from the asset browser onto the viewport.
fn handle_viewport_drop(
    event: On<Pointer<DragDrop>>,
    file_items: Query<&FileBrowserItem>,
    parents: Query<&ChildOf>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<crate::EditorEntity>)>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    snap_settings: Res<crate::snapping::SnapSettings>,
    mut commands: Commands,
) {
    // Walk up the hierarchy to find the FileBrowserItem component
    let item = find_ancestor_component(event.dropped, &file_items, &parents);
    let Some(item) = item else {
        return;
    };

    let path_lower = item.path.to_lowercase();
    let is_gltf = path_lower.ends_with(".gltf") || path_lower.ends_with(".glb");
    let is_template = path_lower.ends_with(".template.json");

    if !is_gltf && !is_template {
        return;
    }

    // Get cursor position and raycast to ground plane
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_tf)) = camera_query.single() else {
        return;
    };

    let position = cursor_to_ground_plane(cursor_pos, camera, cam_tf, &viewport_query)
        .unwrap_or(Vec3::ZERO);

    let ctrl = false; // No Ctrl check needed for drop placement
    let snapped_pos = snap_settings.snap_translate_vec3_if(position, ctrl);

    let path = item.path.clone();
    if is_template {
        commands.queue(move |world: &mut World| {
            crate::entity_templates::instantiate_template(world, &path, snapped_pos);
        });
    } else {
        commands.queue(move |world: &mut World| {
            crate::entity_ops::spawn_gltf_in_world(world, &path, snapped_pos);
        });
    }
}

/// Raycast from screen cursor to the Y=0 ground plane.
pub(crate) fn cursor_to_ground_plane(
    cursor_pos: Vec2,
    camera: &Camera,
    cam_tf: &GlobalTransform,
    viewport_query: &Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
) -> Option<Vec3> {
    // Convert window cursor to viewport-local coordinates, remapped to camera space
    let viewport_cursor = if let Ok((computed, vp_transform)) = viewport_query.single() {
        let scale = computed.inverse_scale_factor();
        let vp_pos = vp_transform.translation * scale;
        let vp_size = computed.size() * scale;
        let vp_top_left = vp_pos - vp_size / 2.0;
        let local = cursor_pos - vp_top_left;
        // Remap from UI-logical space to camera render-target space
        let target_size = camera.logical_viewport_size().unwrap_or(vp_size);
        local * target_size / vp_size
    } else {
        cursor_pos
    };

    let ray = camera.viewport_to_world(cam_tf, viewport_cursor).ok()?;

    // Intersect with Y=0 plane
    if ray.direction.y.abs() < 1e-6 {
        return None; // Ray parallel to ground
    }
    let t = -ray.origin.y / ray.direction.y;
    if t < 0.0 {
        return None; // Ground behind camera
    }
    Some(ray.origin + *ray.direction * t)
}

/// Walk up the entity hierarchy to find a component.
fn find_ancestor_component<'a, C: Component>(
    mut entity: Entity,
    query: &'a Query<&C>,
    parents: &Query<&ChildOf>,
) -> Option<&'a C> {
    loop {
        if let Ok(component) = query.get(entity) {
            return Some(component);
        }
        if let Ok(child_of) = parents.get(entity) {
            entity = child_of.0;
        } else {
            return None;
        }
    }
}

fn update_viewport_focus(
    windows: Query<&Window>,
    viewport_node: Single<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    mut camera_query: Query<(Entity, &mut PanOrbitCamera)>,
    modal: Res<crate::modal_transform::ModalTransformState>,
    walk_mode: Res<WalkModeState>,
    mut active_cam: ResMut<ActiveCameraData>,
) {
    // Use manual mode so the plugin doesn't overwrite our data
    active_cam.manual = true;

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };

    let (computed, vp_transform) = *viewport_node;
    // Convert from physical pixels to logical pixels to match cursor_position()
    let scale = computed.inverse_scale_factor();
    let vp_pos = vp_transform.translation * scale;
    let vp_size = computed.size() * scale;
    let vp_top_left = vp_pos - vp_size / 2.0;
    let vp_bottom_right = vp_pos + vp_size / 2.0;

    let hovered = cursor_pos.x >= vp_top_left.x
        && cursor_pos.x <= vp_bottom_right.x
        && cursor_pos.y >= vp_top_left.y
        && cursor_pos.y <= vp_bottom_right.y;

    // Disable camera orbit during modal operations, walk mode (right-click = cancel, not orbit)
    let modal_active = modal.active.is_some();
    let should_enable = hovered && !modal_active && !walk_mode.active;

    for (entity, mut cam) in &mut camera_query {
        cam.enabled = should_enable;
        if should_enable {
            *active_cam = ActiveCameraData {
                entity: Some(entity),
                viewport_size: Some(vp_size),
                window_size: Some(Vec2::new(window.width(), window.height())),
                manual: true,
            };
        }
    }
}

// ---------------------------------------------------------------------------
// Walk mode (Shift+F, like Blender)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct WalkModeState {
    pub active: bool,
    pub speed: f32,
    /// Camera transform when walk mode was entered (for cancel).
    saved_transform: Option<Transform>,
    saved_focus: Option<Vec3>,
    saved_target_focus: Option<Vec3>,
    saved_target_radius: Option<f32>,
    saved_target_yaw: Option<f32>,
    saved_target_pitch: Option<f32>,
}

fn walk_mode_update(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    mut scroll_events: MessageReader<MouseWheel>,
    time: Res<Time>,
    mut walk_mode: ResMut<WalkModeState>,
    mut camera_query: Query<(&mut PanOrbitCamera, &mut Transform)>,
) {
    let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);

    // Enter walk mode: Shift+F
    if !walk_mode.active {
        if shift && keyboard.just_pressed(KeyCode::KeyF) {
            walk_mode.active = true;
            walk_mode.speed = 5.0;
            for (cam, transform) in &camera_query {
                walk_mode.saved_transform = Some(*transform);
                walk_mode.saved_focus = Some(cam.focus);
                walk_mode.saved_target_focus = Some(cam.target_focus);
                walk_mode.saved_target_radius = Some(cam.target_radius);
                walk_mode.saved_target_yaw = Some(cam.target_yaw);
                walk_mode.saved_target_pitch = Some(cam.target_pitch);
            }
        }
        return;
    }

    // Exit: Escape or right-click = cancel (restore saved transform)
    if keyboard.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right) {
        if let (Some(saved_tf), Some(saved_focus)) = (walk_mode.saved_transform, walk_mode.saved_focus) {
            for (mut cam, mut transform) in &mut camera_query {
                *transform = saved_tf;
                cam.focus = saved_focus;
                if let Some(tf) = walk_mode.saved_target_focus {
                    cam.target_focus = tf;
                }
                if let Some(r) = walk_mode.saved_target_radius {
                    cam.target_radius = r;
                }
                if let Some(y) = walk_mode.saved_target_yaw {
                    cam.target_yaw = y;
                }
                if let Some(p) = walk_mode.saved_target_pitch {
                    cam.target_pitch = p;
                }
                cam.initialized = false;
            }
        }
        walk_mode.active = false;
        return;
    }

    // Exit: left-click or Enter = confirm (keep current position)
    if mouse.just_pressed(MouseButton::Left) || keyboard.just_pressed(KeyCode::Enter) {
        // Update focus to be in front of camera
        for (mut cam, transform) in &mut camera_query {
            let forward_point = transform.translation + transform.forward().as_vec3() * 5.0;
            cam.target_focus = forward_point;
            cam.focus = forward_point;
            cam.initialized = false;
        }
        walk_mode.active = false;
        return;
    }

    // Scroll wheel adjusts speed
    for event in scroll_events.read() {
        let delta = match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y * 0.01,
        };
        walk_mode.speed = (walk_mode.speed * (1.0 + delta * 0.1)).clamp(0.5, 100.0);
    }

    // Mouse look (yaw/pitch)
    let mut mouse_delta = Vec2::ZERO;
    for motion in mouse_motion.read() {
        mouse_delta += motion.delta;
    }

    let dt = time.delta_secs();

    for (_cam, mut transform) in &mut camera_query {
        // Apply mouse look
        if mouse_delta != Vec2::ZERO {
            let sensitivity = 0.003;
            let (mut yaw, mut pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
            yaw -= mouse_delta.x * sensitivity;
            pitch -= mouse_delta.y * sensitivity;
            pitch = pitch.clamp(-std::f32::consts::FRAC_PI_2 + 0.01, std::f32::consts::FRAC_PI_2 - 0.01);
            transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
        }

        // WASD + QE movement
        let mut movement = Vec3::ZERO;
        if keyboard.pressed(KeyCode::KeyW) {
            movement += transform.forward().as_vec3();
        }
        if keyboard.pressed(KeyCode::KeyS) {
            movement -= transform.forward().as_vec3();
        }
        if keyboard.pressed(KeyCode::KeyA) {
            movement -= transform.right().as_vec3();
        }
        if keyboard.pressed(KeyCode::KeyD) {
            movement += transform.right().as_vec3();
        }
        if keyboard.pressed(KeyCode::KeyE) {
            movement += Vec3::Y;
        }
        if keyboard.pressed(KeyCode::KeyQ) {
            movement -= Vec3::Y;
        }

        if movement != Vec3::ZERO {
            let speed_mult = if shift { 2.0 } else { 1.0 };
            transform.translation += movement.normalize() * walk_mode.speed * speed_mult * dt;
        }
    }
}

// ---------------------------------------------------------------------------
// Camera bookmarks
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct CameraBookmarks {
    pub slots: [Option<CameraBookmark>; 9],
}

#[derive(Clone, Copy)]
pub struct CameraBookmark {
    pub focus: Vec3,
    pub transform: Transform,
    pub target_focus: Vec3,
    pub target_radius: f32,
    pub target_yaw: f32,
    pub target_pitch: f32,
}

// ---------------------------------------------------------------------------
// Orbit center visibility (brief flash after change)
// ---------------------------------------------------------------------------

#[derive(Resource)]
pub struct OrbitCenterVisibility {
    pub timer: Timer,
    pub active: bool,
}

impl Default for OrbitCenterVisibility {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(1.5, TimerMode::Once),
            active: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Camera key handling: F = focus, Shift+F = walk, Numpad. = orbit center, bookmarks
// ---------------------------------------------------------------------------

/// Instantly reposition the camera to orbit around `new_focus` at `new_radius`.
/// Sets both current and target state so PanOrbitCamera doesn't interpolate back.
fn focus_camera(cam: &mut PanOrbitCamera, new_focus: Vec3, new_radius: f32) {
    cam.target_focus = new_focus;
    cam.focus = new_focus;
    cam.target_radius = new_radius;
    cam.radius = Some(new_radius);
    cam.force_update = true;
}

fn handle_camera_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    selection: Res<Selection>,
    selected_transforms: Query<&GlobalTransform, With<Selected>>,
    mut camera_query: Query<(&mut PanOrbitCamera, &mut Transform)>,
    mut bookmarks: ResMut<CameraBookmarks>,
    walk_mode: Res<WalkModeState>,
    modal: Res<crate::modal_transform::ModalTransformState>,
    mut orbit_vis: ResMut<OrbitCenterVisibility>,
    windows: Query<&Window>,
    viewport_query: Query<(&ComputedNode, &UiGlobalTransform), With<SceneViewport>>,
    camera_global: Query<(&Camera, &GlobalTransform), (With<Camera3d>, With<crate::EditorEntity>)>,
) {
    // Don't handle camera keys during walk mode or modal transform (G/R/S)
    if walk_mode.active || modal.active.is_some() {
        return;
    }

    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);

    // F key (without Shift): focus on selected entity
    if keyboard.just_pressed(KeyCode::KeyF) && !shift {
        if let Some(primary) = selection.primary() {
            if let Ok(global_tf) = selected_transforms.get(primary) {
                let target = global_tf.translation();
                let scale = global_tf.compute_transform().scale;
                let dist = (scale.length() * 3.0).max(5.0);

                for (mut cam, _) in &mut camera_query {
                    focus_camera(&mut cam, target, dist);
                }
            }
        }
    }

    // Numpad Period (.): center orbit on current selection (move focus only, keep distance)
    if keyboard.just_pressed(KeyCode::NumpadDecimal) {
        if let Some(primary) = selection.primary() {
            if let Ok(global_tf) = selected_transforms.get(primary) {
                let target = global_tf.translation();
                for (mut cam, _) in &mut camera_query {
                    cam.target_focus = target;
                    cam.focus = target;
                    cam.force_update = true;
                }
                orbit_vis.active = true;
                orbit_vis.timer.reset();
            }
        }
    }

    // Shift+Middle Click: set orbit center to 3D point under cursor
    if shift && mouse.just_pressed(MouseButton::Middle) {
        let Ok(window) = windows.single() else {
            return;
        };
        let Some(cursor_pos) = window.cursor_position() else {
            return;
        };
        let Ok((camera, cam_tf)) = camera_global.single() else {
            return;
        };

        // Try ground plane intersection
        if let Some(hit_point) = cursor_to_ground_plane(cursor_pos, camera, cam_tf, &viewport_query) {
            for (mut cam, _) in &mut camera_query {
                cam.target_focus = hit_point;
                cam.focus = hit_point;
                cam.force_update = true;
            }
            orbit_vis.active = true;
            orbit_vis.timer.reset();
        }
    }

    // Number keys: camera bookmarks
    let bookmark_keys = [
        (KeyCode::Digit1, 0),
        (KeyCode::Digit2, 1),
        (KeyCode::Digit3, 2),
        (KeyCode::Digit4, 3),
        (KeyCode::Digit5, 4),
        (KeyCode::Digit6, 5),
        (KeyCode::Digit7, 6),
        (KeyCode::Digit8, 7),
        (KeyCode::Digit9, 8),
    ];

    for (key, index) in bookmark_keys {
        if keyboard.just_pressed(key) {
            if ctrl {
                // Save bookmark
                for (cam, transform) in &camera_query {
                    bookmarks.slots[index] = Some(CameraBookmark {
                        focus: cam.focus,
                        transform: *transform,
                        target_focus: cam.target_focus,
                        target_radius: cam.target_radius,
                        target_yaw: cam.target_yaw,
                        target_pitch: cam.target_pitch,
                    });
                }
            } else {
                // Restore bookmark
                if let Some(bookmark) = bookmarks.slots[index] {
                    for (mut cam, mut transform) in &mut camera_query {
                        cam.focus = bookmark.focus;
                        cam.target_focus = bookmark.target_focus;
                        cam.target_radius = bookmark.target_radius;
                        cam.target_yaw = bookmark.target_yaw;
                        cam.target_pitch = bookmark.target_pitch;
                        *transform = bookmark.transform;
                        cam.initialized = false;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Orbit center marker gizmo
// ---------------------------------------------------------------------------

fn draw_orbit_center(
    camera_query: Query<&PanOrbitCamera>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut orbit_vis: ResMut<OrbitCenterVisibility>,
    time: Res<Time>,
    mut gizmos: Gizmos,
) {
    let Ok(cam) = camera_query.single() else {
        return;
    };

    // Show while middle-mouse is held (orbiting) or during visibility timer
    let orbiting = mouse.pressed(MouseButton::Middle);

    orbit_vis.timer.tick(time.delta());
    let timer_active = orbit_vis.active && !orbit_vis.timer.is_finished();

    if !orbiting && !timer_active {
        if orbit_vis.active && orbit_vis.timer.is_finished() {
            orbit_vis.active = false;
        }
        return;
    }

    let focus = cam.focus;
    let size = 0.1;
    let alpha = if timer_active && !orbiting {
        // Fade out over the last 0.5 seconds
        let remaining = orbit_vis.timer.remaining_secs();
        (remaining / 0.5).min(1.0)
    } else {
        0.6
    };

    let color = Color::srgba(1.0, 1.0, 1.0, alpha);
    gizmos.line(focus - Vec3::X * size, focus + Vec3::X * size, color);
    gizmos.line(focus - Vec3::Y * size, focus + Vec3::Y * size, color);
    gizmos.line(focus - Vec3::Z * size, focus + Vec3::Z * size, color);
}
