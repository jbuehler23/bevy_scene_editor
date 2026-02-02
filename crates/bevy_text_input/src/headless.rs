use crate::{TextInput, TextInputDisplay, TextInputPlaceholder};
use bevy::{
    ecs::{lifecycle::HookContext, world::DeferredWorld},
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    ui_widgets::observe,
};
use bevy_notify::prelude::*;

pub fn text_input_framing() -> impl Bundle {
    (
        TextInput::default(),
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
    )
}

#[derive(Component)]
#[component(on_add)]
// TODO: Rework into a global focused component.
pub struct TextInputFocused;
impl TextInputFocused {
    fn on_add(mut world: DeferredWorld, ctx: HookContext) {
        world.commands().queue(move |world: &mut World| {
            let old_entities = world
                .try_query_filtered::<Entity, With<Self>>()
                .unwrap()
                .iter(world)
                .filter(|entity| *entity != ctx.entity)
                // Collect is to ensure that there is no longer an exclusive reference to world
                .collect::<Vec<_>>();

            for entity in old_entities {
                world.entity_mut(entity).remove::<Self>();
            }
        });
    }
}

pub fn text_input_focus(
    click: On<Pointer<Click>>,
    mut commands: Commands,
    text_input: Query<(), With<TextInput>>,
) -> Result<(), BevyError> {
    if !text_input.contains(click.entity) {
        return Ok(());
    }

    commands.entity(click.entity).insert(TextInputFocused);

    Ok(())
}

pub fn text_input_keyboard_system(
    mut keyboard_events: MessageReader<KeyboardInput>,
    selected_input: Single<&mut TextInput, With<TextInputFocused>>,
) {
    let mut input = selected_input.into_inner();

    for event in keyboard_events
        .read()
        .filter(|event| event.state.is_pressed())
    {
        if let Some(text) = &event.text
            && text.chars().all(|c| !c.is_control())
        {
            let cursor = input.cursor;
            input.value.insert_str(cursor, text);
            input.cursor += text.len();
        }

        match &event.logical_key {
            Key::Backspace => {
                let cursor = input.cursor;
                if cursor > 0 {
                    input.value.remove(cursor - 1);
                    input.cursor -= 1;
                }
            }
            Key::Delete => {
                let cursor = input.cursor;
                if cursor < input.value.len() {
                    input.value.remove(cursor);
                }
            }
            Key::ArrowLeft => {
                if input.cursor > 0 {
                    input.cursor -= 1;
                }
            }
            Key::ArrowRight => {
                if input.cursor < input.value.len() {
                    input.cursor += 1;
                }
            }
            Key::Home => {
                input.cursor = 0;
            }
            Key::End => {
                input.cursor = input.value.len();
            }
            _ => {}
        }
    }
}
