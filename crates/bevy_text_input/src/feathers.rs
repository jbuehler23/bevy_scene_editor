use crate::headless::*;
use bevy::{prelude::*, ui_widgets::observe};
use bevy_notify::prelude::*;

const INPUT_BG: Color = Color::srgba(0.15, 0.15, 0.15, 1.0);
const INPUT_BORDER: Color = Color::srgba(0.3, 0.3, 0.3, 1.0);
const PLACEHOLDER_COLOR: Color = Color::srgba(0.5, 0.5, 0.5, 1.0);

pub fn text_input() -> impl Bundle {
    (
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
        TextInput::default(),
        MonitorSelf,
        NotifyChanged::<TextInput>::default(),
        observe(
            |mutation: On<Mutation<TextInput>>,
             text_input: Query<(&TextInput, &Children, Option<&TextInputPlaceholder>)>,
             mut display: Query<&mut Text, With<TextInputDisplay>>|
             -> Result<(), BevyError> {
                let (text_input, children, placeholder) = text_input.get(mutation.entity)?;

                let new_text = if text_input.value.is_empty() {
                    placeholder.map(ToString::to_string).unwrap_or_default()
                } else {
                    text_input.value.clone()
                };

                if let Some(entity) = children.iter().find(|&entity| display.contains(entity)) {
                    let mut display = display.get_mut(entity).unwrap();

                    display.0 = new_text;
                }

                Ok(())
            },
        ),
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
