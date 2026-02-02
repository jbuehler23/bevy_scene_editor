use bevy::prelude::*;
use crate::headless::*;

const INPUT_BG: Color = Color::srgba(0.15, 0.15, 0.15, 1.0);
const INPUT_BORDER: Color = Color::srgba(0.3, 0.3, 0.3, 1.0);
const PLACEHOLDER_COLOR: Color = Color::srgba(0.5, 0.5, 0.5, 1.0);

pub fn text_input_field(placeholder: &str) -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(28.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor::from(INPUT_BG),
        BorderColor::all(INPUT_BORDER),
        Interaction::default(),
        TextInput {
            placeholder: placeholder.to_string(),
            ..default()
        },
        children![
            (
                Text::new(placeholder.to_string()),
                TextFont { font_size: 13.0, ..default() },
                TextColor(PLACEHOLDER_COLOR),
                TextInputDisplay,
            ),
        ],
    )
}
