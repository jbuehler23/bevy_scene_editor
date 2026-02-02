use crate::{headless, prelude::*};
use bevy::prelude::*;
use bevy_notify::prelude::*;

const INPUT_BG: Color = Color::srgba(0.15, 0.15, 0.15, 1.0);
const INPUT_BORDER: Color = Color::srgba(0.3, 0.3, 0.3, 1.0);
const PLACEHOLDER_COLOR: Color = Color::srgba(0.5, 0.5, 0.5, 1.0);

pub fn text_input() -> impl Bundle {
    (
        headless::text_input_framing(),
        Node {
            width: percent(100.0),
            height: px(28),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            align_items: AlignItems::Center,
            border: px(1).all(),
            ..default()
        },
        BackgroundColor::from(INPUT_BG),
        BorderColor::all(INPUT_BORDER),
        children![(
            TextInputDisplay,
            Text::default(),
            TextColor(PLACEHOLDER_COLOR),
            TextFont {
                font_size: 13.0,
                ..default()
            },
        )],
    )
}
