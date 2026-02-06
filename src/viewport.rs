use bevy::{
    camera::RenderTarget,
    image::ImageSampler,
    picking::hover::HoverMap,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    ui::widget::ViewportNode,
};
use bevy_infinite_grid::InfiniteGridPlugin;
use bevy_panorbit_camera::{PanOrbitCamera, PanOrbitCameraPlugin};

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
        .add_systems(Startup, setup_viewport.after(crate::spawn_layout))
        .add_systems(Update, update_viewport_focus);
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

    // Spawn 3D camera
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: -1,
                ..default()
            },
            RenderTarget::Image(image_handle.into()),
            Transform::from_xyz(0.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
            PanOrbitCamera {
                focus: Vec3::ZERO,
                ..default()
            },
        ))
        .id();

    // Spawn infinite grid
    commands.spawn(bevy_infinite_grid::InfiniteGrid);

    // Attach ViewportNode to the SceneViewport UI entity
    commands.entity(*viewport_query).insert(ViewportNode::new(camera));
}

fn update_viewport_focus(
    hover_map: Res<HoverMap>,
    viewport_query: Single<Entity, With<SceneViewport>>,
    mut camera_query: Query<&mut PanOrbitCamera>,
) {
    let viewport_entity = *viewport_query;

    // Check if any pointer is hovering over the viewport node or its descendants
    let hovered = hover_map.values().any(|pointer_map| {
        pointer_map.keys().any(|&entity| entity == viewport_entity)
    });

    for mut cam in &mut camera_query {
        cam.enabled = hovered;
    }
}
