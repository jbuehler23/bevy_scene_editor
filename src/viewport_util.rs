use bevy::{prelude::*, ui::UiGlobalTransform};

use crate::viewport::SceneViewport;

/// Convert window cursor position to viewport-local coordinates in camera space.
///
/// The camera renders to an off-screen image whose logical size may differ from
/// the UI node's logical size (they diverge on HiDPI/fractional-scaling displays).
/// This function remaps from UI-logical space into the camera's viewport space so
/// that `camera.viewport_to_world()` and `camera.world_to_viewport()` produce
/// correct results.
pub(crate) fn window_to_viewport_cursor(
    cursor_pos: Vec2,
    camera: &Camera,
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
        // Remap from UI-logical space to camera render-target space
        let target_size = camera.logical_viewport_size().unwrap_or(vp_size);
        Some(local * target_size / vp_size)
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
