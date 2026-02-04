use bevy::{
    input::keyboard::{Key, KeyboardInput},
    input_focus::FocusedInput,
    prelude::*,
};

#[derive(Component, Default)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
}
impl TextInput {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            cursor: value.len(),
            value,
        }
    }
}

#[derive(Component, Default)]
pub struct TextInputPlacholder(pub String);

#[derive(Component)]
#[require(Text)]
pub struct TextInputDisplay;

#[derive(EntityEvent)]
pub struct EnteredText {
    pub entity: Entity,
    pub value: String,
}

pub struct TextInputPlugin;

impl Plugin for TextInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(update_text_input)
            .add_systems(Update, update_text_input_display);
    }
}

fn update_text_input(
    key_event: On<FocusedInput<KeyboardInput>>,
    mut commands: Commands,
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
            Key::Enter => commands.trigger(EnteredText {
                entity: key_event.focused_entity,
                value: text_input.value.clone(),
            }),
            _ => {}
        }
    }
}

fn update_text_input_display(
    text_input: Populated<(&TextInput, &TextInputPlacholder, &Children), Changed<TextInput>>,
    mut display: Query<&mut Text, With<TextInputDisplay>>,
) {
    for (text_input, TextInputPlacholder(placeholder), children) in &text_input {
        let new_text = if text_input.value.is_empty() {
            placeholder.clone()
        } else {
            text_input.value.clone()
        };

        if let Some(entity) = children.iter().find(|&entity| display.contains(entity)) {
            let mut display = display.get_mut(entity).unwrap();

            display.0 = new_text;
        }
    }
}
