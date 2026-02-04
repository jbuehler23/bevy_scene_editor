use bevy::{
    feathers::{theme::ThemeBackgroundColor, tokens},
    prelude::*,
};
use editor_feathers::{split_panel, text_input};

use crate::{EditorEntity, inspector::Inspector};

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
        EditorEntity,
        Node {
            height: percent(100),
            padding: percent(0.2).all(),
            ..Default::default()
        },
        BackgroundColor(PANEL_BG),
        children![text_input::text_input("Filter entities")],
    )
}

fn scene_view() -> impl Bundle {
    (
        EditorEntity,
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
                ..Default::default()
            }
        )],
    )
}
