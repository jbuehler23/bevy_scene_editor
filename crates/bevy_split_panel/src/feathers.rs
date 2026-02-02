use bevy::prelude::*;
use crate::headless::*;

const HANDLE_SIZE: f32 = 4.0;
const HANDLE_COLOR: Color = Color::srgba(0.3, 0.3, 0.3, 1.0);
const HANDLE_HOVER_COLOR: Color = Color::srgba(0.5, 0.5, 0.5, 1.0);

pub fn split_panel_horizontal(
    ratio: f32,
    min_size: f32,
    first: impl Bundle,
    second: impl Bundle,
) -> impl Bundle {
    let left_pct = ratio * 100.0;
    let right_pct = (1.0 - ratio) * 100.0;

    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            ..default()
        },
        SplitPanel {
            direction: SplitDirection::Horizontal,
            ratio,
            min_size,
        },
        children![
            // First panel slot
            (
                Node {
                    width: Val::Percent(left_pct),
                    height: Val::Percent(100.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                SplitPanelFirst,
                children![first],
            ),
            // Handle
            (
                Node {
                    width: Val::Px(HANDLE_SIZE),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor::from(HANDLE_COLOR),
                SplitHandle,
                Interaction::default(),
            ),
            // Second panel slot
            (
                Node {
                    width: Val::Percent(right_pct),
                    height: Val::Percent(100.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                SplitPanelSecond,
                children![second],
            ),
        ],
    )
}

pub fn split_panel_vertical(
    ratio: f32,
    min_size: f32,
    first: impl Bundle,
    second: impl Bundle,
) -> impl Bundle {
    let top_pct = ratio * 100.0;
    let bottom_pct = (1.0 - ratio) * 100.0;

    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        SplitPanel {
            direction: SplitDirection::Vertical,
            ratio,
            min_size,
        },
        children![
            (
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(top_pct),
                    overflow: Overflow::clip(),
                    ..default()
                },
                SplitPanelFirst,
                children![first],
            ),
            (
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(HANDLE_SIZE),
                    ..default()
                },
                BackgroundColor::from(HANDLE_COLOR),
                SplitHandle,
                Interaction::default(),
            ),
            (
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(bottom_pct),
                    overflow: Overflow::clip(),
                    ..default()
                },
                SplitPanelSecond,
                children![second],
            ),
        ],
    )
}

pub fn split_handle_hover_system(
    mut query: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<SplitHandle>)>,
) {
    for (interaction, mut bg) in &mut query {
        *bg = match interaction {
            Interaction::Hovered | Interaction::Pressed => BackgroundColor::from(HANDLE_HOVER_COLOR),
            Interaction::None => BackgroundColor::from(HANDLE_COLOR),
        };
    }
}
