use bevy::{
    camera::RenderTarget,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    ui::{widget::ViewportNode, UiGlobalTransform},
};
use bevy_infinite_grid::InfiniteGridPlugin;
use bevy_panorbit_camera::{ActiveCameraData, PanOrbitCamera, PanOrbitCameraPlugin};

use crate::selection::{Selected, Selection};
use editor_widgets::file_browser::FileBrowserItem;

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
        .add_systems(Startup, setup_viewport.after(crate::spawn_layout))
        .add_systems(Update, (update_viewport_focus, handle_camera_keys));
    }
}

fn setup_viewport(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    viewport_query: Single<Entity, With<SceneViewport>>,
) {
    // Create render-target image
    let size = Extent3d {
        width: 1280,
        height: 720,
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
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
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
    // Convert window cursor to viewport-local coordinates
    let viewport_cursor = if let Ok((computed, vp_transform)) = viewport_query.single() {
        let scale = computed.inverse_scale_factor();
        let vp_pos = vp_transform.translation * scale;
        let vp_size = computed.size() * scale;
        let vp_top_left = vp_pos - vp_size / 2.0;
        cursor_pos - vp_top_left
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

    // Disable camera orbit during modal operations (right-click = cancel, not orbit)
    let modal_active = modal.active.is_some();
    let should_enable = hovered && !modal_active;

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
}

// ---------------------------------------------------------------------------
// Camera key handling: F = focus on selected, 1-9 = bookmarks
// ---------------------------------------------------------------------------

fn handle_camera_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    selection: Res<Selection>,
    selected_transforms: Query<&GlobalTransform, With<Selected>>,
    mut camera_query: Query<(&mut PanOrbitCamera, &mut Transform)>,
    mut bookmarks: ResMut<CameraBookmarks>,
) {
    let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);

    // F key: focus on selected entity
    if keyboard.just_pressed(KeyCode::KeyF) {
        if let Some(primary) = selection.primary() {
            if let Ok(global_tf) = selected_transforms.get(primary) {
                let target = global_tf.translation();
                let scale = global_tf.compute_transform().scale;
                let dist = (scale.length() * 3.0).max(5.0);

                for (mut cam, mut transform) in &mut camera_query {
                    cam.focus = target;
                    let dir = (transform.translation - target).normalize_or_zero();
                    transform.translation = target + dir * dist;
                }
            }
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
                    });
                }
            } else {
                // Restore bookmark
                if let Some(bookmark) = bookmarks.slots[index] {
                    for (mut cam, mut transform) in &mut camera_query {
                        cam.focus = bookmark.focus;
                        *transform = bookmark.transform;
                    }
                }
            }
        }
    }
}
