use bevy::{
    feathers::{
        theme::{ThemeBackgroundColor, ThemedText},
        tokens as bevy_tokens,
    },
    prelude::*,
    ui_widgets::observe,
};
use editor_feathers::{icons::{Icon, IconFont}, menu_bar, panel_header, separator, split_panel, status_bar, text_input, tokens, tree_view::tree_container_drop_observers};

use crate::{
    EditorEntity,
    asset_browser,
    entity_ops::{EntityTemplate, create_entity},
    gizmos::{GizmoMode, GizmoSpace},
    hierarchy::{HierarchyPanel, HierarchyTreeContainer},
    inspector::Inspector,
    viewport::SceneViewport,
};

/// Marker on the hierarchy filter text input
#[derive(Component)]
pub struct HierarchyFilter;

/// Marker for the toolbar
#[derive(Component)]
pub struct Toolbar;

/// Marker for gizmo mode buttons
#[derive(Component)]
pub struct GizmoModeButton(pub GizmoMode);

/// Marker for gizmo space toggle
#[derive(Component)]
pub struct GizmoSpaceButton;

pub fn editor_layout(icon_font: &IconFont) -> impl Bundle {
    let font = icon_font.0.clone();
    (
        EditorEntity,
        ThemeBackgroundColor(bevy_tokens::WINDOW_BG),
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            ..Default::default()
        },
        children![
            // Menu bar (fixed height, populated in spawn_layout)
            menu_bar::menu_bar_shell(),
            // Main content (flex grow)
            (
                EditorEntity,
                Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    min_height: px(0.0),
                    flex_direction: FlexDirection::Column,
                    ..Default::default()
                },
                // Vertical split: main area (top) + asset browser (bottom)
                split_panel::panel_group(
                    0.15,
                    (
                        Spawn((split_panel::panel(4), main_area(font))),
                        Spawn(split_panel::panel_handle()),
                        Spawn((split_panel::panel(1), asset_browser::asset_browser_panel())),
                    ),
                ),
            ),
            // Status bar (fixed height)
            status_bar::status_bar()
        ],
    )
}

fn main_area(icon_font: Handle<Font>) -> impl Bundle {
    (
        EditorEntity,
        Node {
            width: percent(100),
            height: percent(100),
            ..Default::default()
        },
        // Horizontal split: hierarchy | viewport | inspector
        split_panel::panel_group(
            0.2,
            (
                Spawn((split_panel::panel(1), entity_heiarchy())),
                Spawn(split_panel::panel_handle()),
                Spawn((split_panel::panel(4), viewport_with_toolbar(icon_font))),
                Spawn(split_panel::panel_handle()),
                Spawn((split_panel::panel(1), entity_inspector())),
            ),
        ),
    )
}

fn viewport_with_toolbar(icon_font: Handle<Font>) -> impl Bundle {
    (
        EditorEntity,
        Node {
            height: percent(100),
            flex_direction: FlexDirection::Column,
            ..Default::default()
        },
        children![toolbar(icon_font), scene_view()],
    )
}

fn toolbar(icon_font: Handle<Font>) -> impl Bundle {
    let f = icon_font.clone();
    (
        Toolbar,
        EditorEntity,
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(tokens::SPACING_MD), px(tokens::SPACING_SM)),
            column_gap: px(tokens::SPACING_SM),
            width: percent(100),
            height: px(32.0),
            flex_shrink: 0.0,
            ..Default::default()
        },
        BackgroundColor(tokens::TOOLBAR_BG),
        children![
            // Gizmo mode buttons
            toolbar_button(Icon::Move, "W", GizmoMode::Translate, icon_font.clone()),
            toolbar_button(Icon::RotateCw, "E", GizmoMode::Rotate, icon_font.clone()),
            toolbar_button(Icon::Scaling, "R", GizmoMode::Scale, icon_font.clone()),
            // Separator
            separator::separator(separator::SeparatorProps::vertical()),
            // Space toggle
            toolbar_space_button(f.clone()),
            // Separator
            separator::separator(separator::SeparatorProps::vertical()),
            // Entity creation
            toolbar_create_button(Icon::Box, "Cube", EntityTemplate::Mesh3dCube, f.clone()),
            toolbar_create_button(Icon::Circle, "Sphere", EntityTemplate::Mesh3dSphere, f.clone()),
            toolbar_create_button(Icon::Lightbulb, "Light", EntityTemplate::PointLight, f.clone()),
            toolbar_create_button(Icon::Plus, "Empty", EntityTemplate::Empty, f),
        ],
    )
}

fn toolbar_button(icon: Icon, label: &str, mode: GizmoMode, font: Handle<Font>) -> impl Bundle {
    let label = label.to_string();
    (
        GizmoModeButton(mode),
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(tokens::SPACING_XS),
            padding: UiRect::axes(px(tokens::SPACING_MD), px(tokens::SPACING_XS)),
            border_radius: BorderRadius::all(px(tokens::BORDER_RADIUS_SM)),
            ..Default::default()
        },
        BackgroundColor(tokens::TOOLBAR_BUTTON_BG),
        children![
            (
                Text::new(String::from(icon.unicode())),
                TextFont {
                    font,
                    font_size: tokens::FONT_MD,
                    ..Default::default()
                },
                TextColor(tokens::TEXT_SECONDARY),
            ),
            (
                Text::new(label),
                TextFont {
                    font_size: tokens::FONT_SM,
                    ..Default::default()
                },
                ThemedText,
            )
        ],
        observe(move |_: On<Pointer<Click>>, mut gizmo_mode: ResMut<GizmoMode>| {
            *gizmo_mode = mode;
        }),
    )
}

fn toolbar_space_button(icon_font: Handle<Font>) -> impl Bundle {
    (
        GizmoSpaceButton,
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(tokens::SPACING_XS),
            padding: UiRect::axes(px(tokens::SPACING_MD), px(tokens::SPACING_XS)),
            border_radius: BorderRadius::all(px(tokens::BORDER_RADIUS_SM)),
            ..Default::default()
        },
        BackgroundColor(tokens::TOOLBAR_BUTTON_BG),
        children![
            (
                Text::new(String::from(Icon::Globe.unicode())),
                TextFont {
                    font: icon_font,
                    font_size: tokens::FONT_MD,
                    ..Default::default()
                },
                TextColor(tokens::TEXT_SECONDARY),
            ),
            (
                Text::new("World"),
                TextFont {
                    font_size: tokens::FONT_SM,
                    ..Default::default()
                },
                ThemedText,
            )
        ],
        observe(
            |_: On<Pointer<Click>>, mut space: ResMut<GizmoSpace>| {
                *space = match *space {
                    GizmoSpace::World => GizmoSpace::Local,
                    GizmoSpace::Local => GizmoSpace::World,
                };
            },
        ),
    )
}

fn toolbar_create_button(icon: Icon, label: &str, template: EntityTemplate, font: Handle<Font>) -> impl Bundle {
    let label = label.to_string();
    (
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(tokens::SPACING_XS),
            padding: UiRect::axes(px(6.0), px(tokens::SPACING_XS)),
            border_radius: BorderRadius::all(px(tokens::BORDER_RADIUS_SM)),
            ..Default::default()
        },
        BackgroundColor(tokens::TOOLBAR_BUTTON_BG),
        children![
            (
                Text::new(String::from(icon.unicode())),
                TextFont {
                    font,
                    font_size: tokens::FONT_SM,
                    ..Default::default()
                },
                TextColor(tokens::TEXT_SECONDARY),
            ),
            (
                Text::new(label),
                TextFont {
                    font_size: tokens::FONT_SM,
                    ..Default::default()
                },
                ThemedText,
            )
        ],
        observe(
            move |_: On<Pointer<Click>>,
                  mut commands: Commands,
                  mut meshes: ResMut<Assets<Mesh>>,
                  mut materials: ResMut<Assets<StandardMaterial>>,
                  mut selection: ResMut<crate::selection::Selection>| {
                create_entity(&mut commands, template, &mut meshes, &mut materials, &mut selection);
            },
        ),
    )
}

fn entity_heiarchy() -> impl Bundle {
    (
        HierarchyPanel,
        Node {
            height: percent(100),
            flex_direction: FlexDirection::Column,
            ..Default::default()
        },
        BackgroundColor(tokens::PANEL_BG),
        children![
            panel_header::panel_header("Hierarchy"),
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    min_height: px(0.0),
                    padding: UiRect::all(px(tokens::SPACING_SM)),
                    ..Default::default()
                },
                children![
                    (HierarchyFilter, text_input::text_input("Filter entities")),
                    (
                        HierarchyTreeContainer,
                        Node {
                            flex_direction: FlexDirection::Column,
                            width: percent(100),
                            flex_grow: 1.0,
                            min_height: px(0.0),
                            overflow: Overflow::scroll_y(),
                            margin: UiRect::top(px(tokens::SPACING_SM)),
                            ..Default::default()
                        },
                        BackgroundColor(Color::NONE),
                        tree_container_drop_observers(),
                    )
                ],
            )
        ],
    )
}

fn scene_view() -> impl Bundle {
    (
        EditorEntity,
        SceneViewport,
        Node {
            width: percent(100),
            flex_grow: 1.0,
            ..Default::default()
        },
    )
}

/// Updates toolbar button backgrounds to highlight the active gizmo mode.
pub fn update_toolbar_highlights(
    mode: Res<GizmoMode>,
    mut buttons: Query<(&GizmoModeButton, &mut BackgroundColor)>,
) {
    if !mode.is_changed() {
        return;
    }
    for (button, mut bg) in &mut buttons {
        bg.0 = if button.0 == *mode {
            tokens::SELECTED_BG
        } else {
            tokens::TOOLBAR_BUTTON_BG
        };
    }
}

/// Updates the gizmo space toggle button label.
pub fn update_space_toggle_label(
    space: Res<GizmoSpace>,
    buttons: Query<&Children, With<GizmoSpaceButton>>,
    mut texts: Query<&mut Text, With<ThemedText>>,
) {
    if !space.is_changed() {
        return;
    }
    let label = match *space {
        GizmoSpace::World => "World",
        GizmoSpace::Local => "Local",
    };
    for children in &buttons {
        for child in children.iter() {
            if let Ok(mut text) = texts.get_mut(child) {
                text.0 = label.to_string();
                return;
            }
        }
    }
}

fn entity_inspector() -> impl Bundle {
    (
        Node {
            height: percent(100),
            flex_direction: FlexDirection::Column,
            ..Default::default()
        },
        BackgroundColor(tokens::PANEL_BG),
        children![
            panel_header::panel_header("Inspector"),
            (
                Inspector,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(tokens::SPACING_SM),
                    overflow: Overflow::scroll_y(),
                    flex_grow: 1.0,
                    min_height: px(0.0),
                    padding: UiRect::all(px(tokens::SPACING_SM)),
                    ..Default::default()
                }
            )
        ],
    )
}
