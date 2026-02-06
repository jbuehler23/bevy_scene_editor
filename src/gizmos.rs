use bevy::prelude::*;

use crate::inspector::SelectedEntity;

const AXIS_LENGTH: f32 = 1.5;
const AXIS_TIP_LENGTH: f32 = 0.3;

pub struct TransformGizmosPlugin;

impl Plugin for TransformGizmosPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, draw_transform_gizmos);
    }
}

fn draw_transform_gizmos(
    mut gizmos: Gizmos,
    selected: Query<&GlobalTransform, With<SelectedEntity>>,
) {
    let Ok(global_transform) = selected.single() else {
        return;
    };

    let pos = global_transform.translation();
    let (rotation, _, _) = global_transform.to_scale_rotation_translation();

    let right = rotation * Vec3::X;
    let up = rotation * Vec3::Y;
    let forward = rotation * Vec3::Z;

    // X axis — red
    gizmos.arrow(pos, pos + right * AXIS_LENGTH, Color::srgb(1.0, 0.2, 0.2))
        .with_tip_length(AXIS_TIP_LENGTH);

    // Y axis — green
    gizmos.arrow(pos, pos + up * AXIS_LENGTH, Color::srgb(0.2, 1.0, 0.2))
        .with_tip_length(AXIS_TIP_LENGTH);

    // Z axis — blue
    gizmos.arrow(pos, pos + forward * AXIS_LENGTH, Color::srgb(0.2, 0.4, 1.0))
        .with_tip_length(AXIS_TIP_LENGTH);
}
