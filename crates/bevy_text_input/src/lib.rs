pub mod feathers;
pub mod headless;
pub mod prelude;

use crate::prelude::*;
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    input_focus::{FocusedInput, InputDispatchPlugin, InputFocus},
    prelude::*,
};
use bevy_notify::prelude::*;

pub struct TextInputPlugin;

impl Plugin for TextInputPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<InputDispatchPlugin>() {
            app.add_plugins(InputDispatchPlugin);
        }
        app.add_observer(set_text_input_focus)
            .add_observer(update_text_input)
            .add_observer(update_text_input_display);
    }
}

fn set_text_input_focus(
    click: On<Pointer<Click>>,
    text_input: Query<(), With<TextInput>>,
    mut input_focus: ResMut<InputFocus>,
) {
    if !text_input.contains(click.entity) {
        return;
    }
    input_focus.set(click.entity);
}

fn update_text_input(
    key_event: On<FocusedInput<KeyboardInput>>,
    mut text_input: Query<&mut TextInput>,
) {
    let Ok(mut text_input) = text_input.get_mut(key_event.focused_entity) else {
        return;
    };

    if key_event.input.state.is_pressed() {
        if let Some(text) = &key_event.input.text
            && text.chars().all(|c| !c.is_control())
        {
            let cursor = text_input.cursor;
            text_input.value.insert_str(cursor, text);
            text_input.cursor += text.len();
        }

        match &key_event.input.logical_key {
            Key::Backspace => {
                let cursor = text_input.cursor;
                if cursor > 0 {
                    text_input.value.remove(cursor - 1);
                    text_input.cursor -= 1;
                }
            }
            Key::Delete => {
                let cursor = text_input.cursor;
                if cursor < text_input.value.len() {
                    text_input.value.remove(cursor);
                }
            }
            Key::ArrowLeft => {
                if text_input.cursor > 0 {
                    text_input.cursor -= 1;
                }
            }
            Key::ArrowRight => {
                if text_input.cursor < text_input.value.len() {
                    text_input.cursor += 1;
                }
            }
            Key::Home => {
                text_input.cursor = 0;
            }
            Key::End => {
                text_input.cursor = text_input.value.len();
            }
            _ => {}
        }
    }
}

fn update_text_input_display(
    mutation: On<Mutation<TextInput>>,
    text_input: Query<(&TextInput, &Children, Option<&TextInputPlaceholder>)>,
    mut display: Query<&mut Text, With<TextInputDisplay>>,
) -> Result<(), BevyError> {
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
}
