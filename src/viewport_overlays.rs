use bevy::{camera::primitives::Aabb, prelude::*};

use crate::selection::Selected;

pub struct ViewportOverlaysPlugin;

impl Plugin for ViewportOverlaysPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverlaySettings>()
            .add_systems(Update, (draw_selection_bounding_boxes, draw_coordinate_indicator));
    }
}

#[derive(Resource)]
pub struct OverlaySettings {
    pub show_bounding_boxes: bool,
    pub show_coordinate_indicator: bool,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            show_bounding_boxes: true,
            show_coordinate_indicator: true,
        }
    }
}

/// Draw wireframe bounding boxes around selected entities.
fn draw_selection_bounding_boxes(
    mut gizmos: Gizmos,
    settings: Res<OverlaySettings>,
    selected: Query<(Entity, &GlobalTransform, Option<&Aabb>), With<Selected>>,
    children_query: Query<&Children>,
    aabb_query: Query<&Aabb>,
) {
    if !settings.show_bounding_boxes {
        return;
    }

    for (entity, global_tf, maybe_aabb) in &selected {
        let computed: Transform = global_tf.compute_transform();
        let pos = computed.translation;
        let rotation = computed.rotation;
        let scale = computed.scale;

        // Try to find an Aabb: on the entity itself, or on a descendant (for GLTF models)
        let child_aabb = find_child_aabb(entity, &children_query, &aabb_query);
        let resolved_aabb: Option<&Aabb> = maybe_aabb.or(child_aabb);

        let (center, half) = if let Some(aabb) = resolved_aabb {
            let he = Vec3::from(aabb.half_extents) * scale;
            let c = pos + rotation * (Vec3::from(aabb.center) * scale);
            (c, he)
        } else {
            // Fallback for entities without meshes (lights, empties)
            (pos, Vec3::splat(0.25))
        };

        draw_wireframe_box(&mut gizmos, center, half, rotation, Color::srgba(1.0, 1.0, 0.0, 0.5));
    }
}

/// Walk children recursively to find the first entity with an `Aabb` component.
fn find_child_aabb<'a>(
    entity: Entity,
    children_query: &Query<&Children>,
    aabb_query: &'a Query<&Aabb>,
) -> Option<&'a Aabb> {
    let Ok(children) = children_query.get(entity) else {
        return None;
    };
    for child in children.iter() {
        if let Ok(aabb) = aabb_query.get(child) {
            return Some(aabb);
        }
        if let Some(aabb) = find_child_aabb(child, children_query, aabb_query) {
            return Some(aabb);
        }
    }
    None
}

/// Draw a wireframe box centered at `center` with given half extents and rotation.
fn draw_wireframe_box(gizmos: &mut Gizmos, center: Vec3, half: Vec3, rotation: Quat, color: Color) {
    let corners = [
        center + rotation * Vec3::new(-half.x, -half.y, -half.z),
        center + rotation * Vec3::new(half.x, -half.y, -half.z),
        center + rotation * Vec3::new(half.x, half.y, -half.z),
        center + rotation * Vec3::new(-half.x, half.y, -half.z),
        center + rotation * Vec3::new(-half.x, -half.y, half.z),
        center + rotation * Vec3::new(half.x, -half.y, half.z),
        center + rotation * Vec3::new(half.x, half.y, half.z),
        center + rotation * Vec3::new(-half.x, half.y, half.z),
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

/// Draw a small coordinate indicator showing camera orientation.
fn draw_coordinate_indicator(
    mut gizmos: Gizmos,
    settings: Res<OverlaySettings>,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
) {
    if !settings.show_coordinate_indicator {
        return;
    }

    let Ok(cam_tf) = camera_query.single() else {
        return;
    };

    let cam_pos = cam_tf.translation();
    let cam_forward = cam_tf.forward().as_vec3();

    // Place the indicator in front of the camera, offset to bottom-left
    let indicator_pos = cam_pos
        + cam_forward * 2.0
        + cam_tf.right().as_vec3() * -0.8
        + cam_tf.up().as_vec3() * -0.5;
    let size = 0.1;

    gizmos.line(indicator_pos, indicator_pos + Vec3::X * size, Color::srgb(1.0, 0.2, 0.2));
    gizmos.line(indicator_pos, indicator_pos + Vec3::Y * size, Color::srgb(0.2, 1.0, 0.2));
    gizmos.line(indicator_pos, indicator_pos + Vec3::Z * size, Color::srgb(0.2, 0.4, 1.0));
}
