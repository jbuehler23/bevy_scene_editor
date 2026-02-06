use bevy::{
    feathers::{theme::ThemeBackgroundColor, tokens},
    prelude::*,
};
use editor_feathers::{split_panel, text_input, tree_view::tree_container_drop_observers};

use crate::{EditorEntity, hierarchy::{HierarchyPanel, HierarchyTreeContainer}, inspector::Inspector, viewport::SceneViewport};

/// Marker on the hierarchy filter text input
#[derive(Component)]
pub struct HierarchyFilter;

const PANEL_BG: Color = Color::srgba(0.12, 0.12, 0.12, 1.0);

pub fn editor_layout() -> impl Bundle {
    (
        EditorEntity,
        ThemeBackgroundColor(tokens::WINDOW_BG),
        Node {
            width: percent(100),
            height: percent(100),
            ..Default::default()
        },
        split_panel::panel_group(
            0.2,
            (
                Spawn((split_panel::panel(1), entity_heiarchy())),
                Spawn(split_panel::panel_handle()),
                Spawn((split_panel::panel(4), scene_view())),
                Spawn(split_panel::panel_handle()),
                Spawn((split_panel::panel(1), entity_inspector())),
            ),
        ),
    )
}

fn entity_heiarchy() -> impl Bundle {
    (
        HierarchyPanel,
        Node {
            height: percent(100),
            flex_direction: FlexDirection::Column,
            padding: percent(0.2).all(),
            ..Default::default()
        },
        BackgroundColor(PANEL_BG),
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
                    margin: UiRect::top(px(8.0)),
                    ..Default::default()
                },
                BackgroundColor(Color::NONE),
                tree_container_drop_observers(),
            )
        ],
    )
}

fn scene_view() -> impl Bundle {
    (
        EditorEntity,
        SceneViewport,
        Node {
            height: percent(100),
            ..Default::default()
        },
    )
}

fn entity_inspector() -> impl Bundle {
    (
        Node {
            height: percent(100),
            padding: percent(0.2).all(),
            ..Default::default()
        },
        BackgroundColor(PANEL_BG),
        children![(
            Inspector,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                overflow: Overflow::scroll_y(),
                ..Default::default()
            }
        )],
    )
}
