use bevy::prelude::*;
use bevy_infinite_grid::{InfiniteGrid, InfiniteGridSettings};

pub struct SnappingPlugin;

impl Plugin for SnappingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SnapSettings>()
            .init_resource::<GridSettings>()
            .add_systems(Update, sync_grid_settings);
    }
}

// ---------------------------------------------------------------------------
// Grid settings
// ---------------------------------------------------------------------------

#[derive(Resource)]
pub struct GridSettings {
    pub visible: bool,
    pub scale: f32,
    pub major_line_color: Color,
    pub minor_line_color: Color,
    pub fadeout_distance: f32,
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            visible: true,
            scale: 1.0,
            major_line_color: Color::srgb(0.25, 0.25, 0.25),
            minor_line_color: Color::srgb(0.1, 0.1, 0.1),
            fadeout_distance: 100.0,
        }
    }
}

fn sync_grid_settings(
    grid: Res<GridSettings>,
    mut grids: Query<(&mut InfiniteGridSettings, &mut Visibility), With<InfiniteGrid>>,
) {
    if !grid.is_changed() {
        return;
    }
    for (mut settings, mut visibility) in &mut grids {
        settings.scale = grid.scale;
        settings.major_line_color = grid.major_line_color;
        settings.minor_line_color = grid.minor_line_color;
        settings.fadeout_distance = grid.fadeout_distance;
        *visibility = if grid.visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

// ---------------------------------------------------------------------------
// Snap settings
// ---------------------------------------------------------------------------

#[derive(Resource)]
pub struct SnapSettings {
    pub translate_snap: bool,
    pub translate_increment: f32,
    pub rotate_snap: bool,
    pub rotate_increment: f32,
    pub scale_snap: bool,
    pub scale_increment: f32,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self {
            translate_snap: false,
            translate_increment: 0.25,
            rotate_snap: false,
            rotate_increment: 15.0_f32.to_radians(),
            scale_snap: false,
            scale_increment: 0.1,
        }
    }
}

impl SnapSettings {
    /// Snap a translation value to the nearest increment.
    pub fn snap_translate(&self, value: f32) -> f32 {
        if self.translate_snap && self.translate_increment > 0.0 {
            (value / self.translate_increment).round() * self.translate_increment
        } else {
            value
        }
    }

    /// Snap a translation vector.
    pub fn snap_translate_vec3(&self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.snap_translate(v.x),
            self.snap_translate(v.y),
            self.snap_translate(v.z),
        )
    }

    /// Snap a rotation angle to the nearest increment.
    pub fn snap_rotate(&self, angle: f32) -> f32 {
        if self.rotate_snap && self.rotate_increment > 0.0 {
            (angle / self.rotate_increment).round() * self.rotate_increment
        } else {
            angle
        }
    }

    /// Snap a scale value to the nearest increment.
    pub fn snap_scale(&self, value: f32) -> f32 {
        if self.scale_snap && self.scale_increment > 0.0 {
            (value / self.scale_increment).round() * self.scale_increment
        } else {
            value
        }
    }

    /// Snap a scale vector.
    pub fn snap_scale_vec3(&self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.snap_scale(v.x),
            self.snap_scale(v.y),
            self.snap_scale(v.z),
        )
    }

    /// Check if translate snapping should be active (Ctrl held = toggle snap).
    pub fn translate_active(&self, ctrl_held: bool) -> bool {
        self.translate_snap ^ ctrl_held
    }

    /// Check if rotate snapping should be active (Ctrl held = toggle snap).
    pub fn rotate_active(&self, ctrl_held: bool) -> bool {
        self.rotate_snap ^ ctrl_held
    }

    /// Check if scale snapping should be active (Ctrl held = toggle snap).
    pub fn scale_active(&self, ctrl_held: bool) -> bool {
        self.scale_snap ^ ctrl_held
    }

    /// Conditionally snap a translation vector based on Ctrl state.
    pub fn snap_translate_vec3_if(&self, v: Vec3, ctrl_held: bool) -> Vec3 {
        if self.translate_active(ctrl_held) && self.translate_increment > 0.0 {
            Vec3::new(
                (v.x / self.translate_increment).round() * self.translate_increment,
                (v.y / self.translate_increment).round() * self.translate_increment,
                (v.z / self.translate_increment).round() * self.translate_increment,
            )
        } else {
            v
        }
    }

    /// Conditionally snap a rotation angle based on Ctrl state.
    pub fn snap_rotate_if(&self, angle: f32, ctrl_held: bool) -> f32 {
        if self.rotate_active(ctrl_held) && self.rotate_increment > 0.0 {
            (angle / self.rotate_increment).round() * self.rotate_increment
        } else {
            angle
        }
    }

    /// Conditionally snap a scale vector based on Ctrl state.
    pub fn snap_scale_vec3_if(&self, v: Vec3, ctrl_held: bool) -> Vec3 {
        if self.scale_active(ctrl_held) && self.scale_increment > 0.0 {
            Vec3::new(
                (v.x / self.scale_increment).round() * self.scale_increment,
                (v.y / self.scale_increment).round() * self.scale_increment,
                (v.z / self.scale_increment).round() * self.scale_increment,
            )
        } else {
            v
        }
    }
}
