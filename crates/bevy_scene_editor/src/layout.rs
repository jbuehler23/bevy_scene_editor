use bevy::prelude::*;
use bevy_split_panel::*;
use crate::state::EditorEntity;

/// Marker for the hierarchy panel container.
#[derive(Component)]
pub struct HierarchyPanel;

/// Marker for the viewport panel container.
#[derive(Component)]
pub struct ViewportPanel;

/// Marker for the inspector panel container.
#[derive(Component)]
pub struct InspectorPanel;

const PANEL_BG: Color = Color::srgba(0.12, 0.12, 0.12, 1.0);

pub fn spawn_editor_layout(mut commands: Commands) {
    commands.spawn((
        EditorEntity,
        split_panel_horizontal(
            0.2,
            150.0,
            // Left: hierarchy panel
            hierarchy_panel(),
            // Right: viewport + inspector split
            split_panel_horizontal(
                0.75,
                200.0,
                // Center: viewport
                viewport_panel(),
                // Right: inspector
                inspector_panel(),
            ),
        ),
    ));
}

fn hierarchy_panel() -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        BackgroundColor(PANEL_BG),
        HierarchyPanel,
    )
}

fn viewport_panel() -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        BackgroundColor(Color::BLACK),
        ViewportPanel,
    )
}

fn inspector_panel() -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        BackgroundColor(PANEL_BG),
        InspectorPanel,
    )
}

