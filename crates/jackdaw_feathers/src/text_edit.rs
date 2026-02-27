use bevy::input_focus::InputFocus;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{FontFeatureTag, FontFeatures};
use bevy_ui_text_input::actions::{TextInputAction, TextInputEdit};
use bevy_ui_text_input::*;

use crate::cursor::{ActiveCursor, HoverCursor};
use crate::icons::EditorFont;
use crate::tokens::{BORDER_COLOR, PRIMARY_COLOR, TEXT_BODY_COLOR, TEXT_MUTED_COLOR, TEXT_SIZE, TEXT_SIZE_SM};

pub fn plugin(app: &mut App) {
    if !app.is_plugin_added::<TextInputPlugin>() {
        app.add_plugins(TextInputPlugin);
    }
    app.add_systems(Update, setup_text_edit_input)
        .add_systems(
            Update,
            (
                handle_focus_style,
                handle_numeric_increment,
                handle_unfocus,
                handle_drag_value,
                handle_click_to_focus,
                handle_clamp_on_unfocus,
            ),
        )
        .add_systems(PostUpdate, (apply_default_value, handle_suffix).chain());
}

pub fn set_text_input_value(queue: &mut TextInputQueue, text: String) {
    queue.add(TextInputAction::Edit(TextInputEdit::SelectAll));
    queue.add(TextInputAction::Edit(TextInputEdit::Paste(text)));
}

#[derive(Event)]
pub struct TextEditCommitEvent {
    pub entity: Entity,
    pub text: String,
}

const INPUT_HEIGHT: f32 = 28.0;
const AFFIX_SIZE: u64 = 16;

#[derive(Component)]
pub struct EditorTextEdit;

#[derive(Component)]
struct TextEditWrapper(Entity);

#[derive(Component, Default, Clone, Copy, PartialEq)]
pub enum TextEditVariant {
    #[default]
    Default,
    NumericF32,
    NumericI32,
}

impl TextEditVariant {
    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::NumericF32 | Self::NumericI32)
    }
}

#[derive(Clone)]
pub enum TextEditPrefix {
    Label { label: String, size: f32 },
}

#[derive(Component)]
struct TextEditSuffix(String);

#[derive(Component)]
struct TextEditSuffixNode(Entity);

#[derive(Component)]
struct TextEditDefaultValue(String);

#[derive(Component, Default)]
struct DragHitbox {
    dragging: bool,
    start_x: f32,
    start_value: f64,
}

#[derive(Component, Clone, Copy)]
struct NumericRange {
    min: f64,
    max: f64,
}

#[derive(Component)]
struct AllowEmpty;

#[derive(Clone)]
pub enum FilterType {
    Decimal,
    Integer,
}

#[derive(Component)]
struct TextEditConfig {
    label: Option<String>,
    variant: TextEditVariant,
    filter: Option<FilterType>,
    prefix: Option<TextEditPrefix>,
    suffix: Option<String>,
    placeholder: String,
    default_value: Option<String>,
    min: f64,
    max: f64,
    allow_empty: bool,
    drag_bottom: bool,
    initialized: bool,
}

pub struct TextEditProps {
    pub label: Option<String>,
    pub placeholder: String,
    pub default_value: Option<String>,
    pub variant: TextEditVariant,
    pub filter: Option<FilterType>,
    pub prefix: Option<TextEditPrefix>,
    pub suffix: Option<String>,
    pub min: f64,
    pub max: f64,
    pub allow_empty: bool,
    pub drag_bottom: bool,
}

impl Default for TextEditProps {
    fn default() -> Self {
        Self {
            label: None,
            placeholder: String::new(),
            default_value: None,
            variant: TextEditVariant::Default,
            filter: None,
            prefix: None,
            suffix: None,
            min: f64::MIN,
            max: f64::MAX,
            allow_empty: false,
            drag_bottom: false,
        }
    }
}

impl TextEditProps {
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }
    pub fn with_prefix(mut self, prefix: TextEditPrefix) -> Self {
        self.prefix = Some(prefix);
        self
    }
    pub fn with_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }
    pub fn with_default_value(mut self, value: impl Into<String>) -> Self {
        self.default_value = Some(value.into());
        self
    }
    pub fn with_min(mut self, min: f64) -> Self {
        self.min = min;
        self
    }
    pub fn with_max(mut self, max: f64) -> Self {
        self.max = max;
        self
    }
    pub fn allow_empty(mut self) -> Self {
        self.allow_empty = true;
        self
    }
    pub fn drag_bottom(mut self) -> Self {
        self.drag_bottom = true;
        self
    }
    pub fn numeric_f32(mut self) -> Self {
        self.variant = TextEditVariant::NumericF32;
        self.filter = Some(FilterType::Decimal);
        self.prefix = Some(TextEditPrefix::Label {
            label: "↔".to_string(),
            size: TEXT_SIZE,
        });
        self.min = f32::MIN as f64;
        self.max = f32::MAX as f64;
        self
    }
    pub fn numeric_i32(mut self) -> Self {
        self.variant = TextEditVariant::NumericI32;
        self.filter = Some(FilterType::Integer);
        self.prefix = Some(TextEditPrefix::Label {
            label: "↔".to_string(),
            size: TEXT_SIZE,
        });
        self.min = i32::MIN as f64;
        self.max = i32::MAX as f64;
        self
    }
}

pub fn text_edit(props: TextEditProps) -> impl Bundle {
    let TextEditProps {
        label,
        placeholder,
        default_value,
        variant,
        filter,
        prefix,
        suffix,
        min,
        max,
        allow_empty,
        drag_bottom,
    } = props;

    (
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            flex_grow: 1.0,
            flex_shrink: 1.0,
            flex_basis: px(0),
            ..default()
        },
        TextEditConfig {
            label,
            variant,
            filter,
            prefix,
            suffix,
            placeholder,
            default_value,
            min,
            max,
            allow_empty,
            drag_bottom,
            initialized: false,
        },
    )
}

fn setup_text_edit_input(
    mut commands: Commands,
    editor_font: Res<EditorFont>,
    mut configs: Query<(Entity, &mut TextEditConfig)>,
) {
    let font = editor_font.0.clone();
    let tabular_figures: FontFeatures = [FontFeatureTag::TABULAR_FIGURES].into();

    for (entity, mut config) in &mut configs {
        if config.initialized {
            continue;
        }
        config.initialized = true;

        if let Some(ref label) = config.label {
            let label_entity = commands
                .spawn((
                    Text::new(label),
                    TextFont {
                        font: font.clone(),
                        font_size: TEXT_SIZE_SM,
                        weight: FontWeight::MEDIUM,
                        ..default()
                    },
                    TextColor(TEXT_MUTED_COLOR.into()),
                ))
                .id();
            commands.entity(entity).add_child(label_entity);
        }

        let is_numeric = config.variant.is_numeric();
        let filter = config.filter.as_ref().map(|f| match f {
            FilterType::Decimal => TextInputFilter::Decimal,
            FilterType::Integer => TextInputFilter::Integer,
        });

        let wrapper_entity = commands
            .spawn((
                Node {
                    width: percent(100),
                    height: px(INPUT_HEIGHT),
                    padding: UiRect::all(px(6)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(2)),
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::all(BORDER_COLOR),
                Interaction::None,
                Hovered::default(),
                HoverCursor(bevy::window::SystemCursorIcon::Text),
            ))
            .id();

        commands.entity(entity).add_child(wrapper_entity);

        if is_numeric && !config.drag_bottom {
            const HITBOX_WIDTH: f32 = INPUT_HEIGHT * 0.9;
            let hitbox = commands
                .spawn((
                    DragHitbox::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(HITBOX_WIDTH),
                        height: px(INPUT_HEIGHT),
                        left: px(0),
                        ..default()
                    },
                    ZIndex(10),
                    Interaction::None,
                    Hovered::default(),
                    HoverCursor(bevy::window::SystemCursorIcon::ColResize),
                ))
                .id();
            commands.entity(wrapper_entity).add_child(hitbox);
        }

        if let Some(ref prefix) = config.prefix {
            let prefix_entity = match prefix {
                TextEditPrefix::Label { label, size } => commands
                    .spawn((
                        Text::new(label),
                        TextFont {
                            font: font.clone(),
                            font_size: *size,
                            ..default()
                        },
                        TextColor(TEXT_BODY_COLOR.with_alpha(0.5).into()),
                        TextLayout::new_with_justify(Justify::Center),
                        Node {
                            width: px(AFFIX_SIZE),
                            ..default()
                        },
                    ))
                    .id(),
            };
            commands.entity(wrapper_entity).add_child(prefix_entity);
        }

        let placeholder = config
            .suffix
            .as_ref()
            .map(|s| format!("{}{}", config.placeholder, s))
            .unwrap_or_else(|| config.placeholder.clone());

        let mut text_input = commands.spawn((
            EditorTextEdit,
            config.variant,
            TextInputNode {
                mode: TextInputMode::SingleLine,
                clear_on_submit: false,
                unfocus_on_submit: true,
                ..default()
            },
            TextFont {
                font: font.clone(),
                font_size: TEXT_SIZE,
                font_features: tabular_figures.clone(),
                ..default()
            },
            TextColor(TEXT_BODY_COLOR.into()),
            TextInputStyle {
                cursor_color: TEXT_BODY_COLOR.into(),
                cursor_width: 1.0,
                selection_color: PRIMARY_COLOR.with_alpha(0.3).into(),
                ..default()
            },
            TextInputPrompt {
                text: placeholder,
                color: Some(TEXT_BODY_COLOR.with_alpha(0.2).into()),
                ..default()
            },
            Node {
                flex_grow: 1.0,
                height: percent(100),
                justify_content: JustifyContent::Center,
                overflow: Overflow::clip(),
                ..default()
            },
        ));

        if let Some(filter) = filter {
            text_input.insert(filter);
        }

        if let Some(ref suffix) = config.suffix {
            text_input.insert(TextEditSuffix(suffix.clone()));
        }

        if let Some(ref default_value) = config.default_value {
            text_input.insert(TextEditDefaultValue(default_value.clone()));
        }

        if is_numeric {
            text_input.insert(NumericRange {
                min: config.min,
                max: config.max,
            });
        }

        if config.allow_empty {
            text_input.insert(AllowEmpty);
        }

        let text_input_entity = text_input.id();

        commands.entity(wrapper_entity).add_child(text_input_entity);

        if let Some(ref suffix) = config.suffix {
            let suffix_entity = commands
                .spawn((
                    TextEditSuffixNode(text_input_entity),
                    Text::new(suffix.clone()),
                    TextFont {
                        font: font.clone(),
                        font_size: TEXT_SIZE,
                        font_features: tabular_figures.clone(),
                        ..default()
                    },
                    TextColor(TEXT_MUTED_COLOR.into()),
                    Node {
                        position_type: PositionType::Absolute,
                        top: px(5.5),
                        display: Display::None,
                        ..default()
                    },
                ))
                .id();
            commands.entity(wrapper_entity).add_child(suffix_entity);
        }
        commands
            .entity(wrapper_entity)
            .insert(TextEditWrapper(text_input_entity));
    }
}

fn handle_focus_style(
    focus: Res<InputFocus>,
    mut wrappers: Query<(&TextEditWrapper, &mut BorderColor, &Hovered)>,
) {
    for (wrapper, mut border_color, hovered) in &mut wrappers {
        let color = match (focus.0 == Some(wrapper.0), hovered.get()) {
            (true, _) => PRIMARY_COLOR,
            (_, true) => BORDER_COLOR.lighter(0.05),
            _ => BORDER_COLOR,
        };
        *border_color = BorderColor::all(color);
    }
}

fn apply_default_value(
    mut commands: Commands,
    mut text_edits: Query<(
        Entity,
        &TextEditDefaultValue,
        &TextEditVariant,
        &TextInputBuffer,
        &mut TextInputQueue,
        Option<&NumericRange>,
    )>,
) {
    for (entity, default_value, variant, buffer, mut queue, range) in &mut text_edits {
        if buffer.get_text().is_empty() {
            let text = if variant.is_numeric() {
                let value = clamp_value(default_value.0.parse().unwrap_or(0.0), range);
                format_numeric_value(value, *variant)
            } else {
                default_value.0.clone()
            };
            queue.add(TextInputAction::Edit(TextInputEdit::Paste(text)));
        }
        commands.entity(entity).remove::<TextEditDefaultValue>();
    }
}

fn handle_suffix(
    focus: Res<InputFocus>,
    text_edits: Query<
        (Entity, &TextInputBuffer, &TextInputLayoutInfo, &ChildOf),
        With<TextEditSuffix>,
    >,
    mut suffix_nodes: Query<(&TextEditSuffixNode, &mut Node), Without<TextEditWrapper>>,
    parents: Query<&ChildOf>,
    configs: Query<&TextEditConfig>,
) {
    const WRAPPER_PADDING: f32 = 8.0;
    const PREFIX_EXTRA: f32 = AFFIX_SIZE as f32 + 6.0;
    for (entity, buffer, layout_info, child_of) in &text_edits {
        let Some((_, mut node)) = suffix_nodes.iter_mut().find(|(link, _)| link.0 == entity)
        else {
            continue;
        };

        let has_prefix = parents
            .get(child_of.parent())
            .ok()
            .and_then(|wrapper_parent| configs.get(wrapper_parent.parent()).ok())
            .is_some_and(|config| config.prefix.is_some());

        let offset = WRAPPER_PADDING + if has_prefix { PREFIX_EXTRA } else { 0.0 };

        let show = focus.0 != Some(entity) && !buffer.get_text().is_empty();
        node.left = px(layout_info.size.x + offset);
        node.display = if show { Display::Flex } else { Display::None };
    }
}

fn handle_click_to_focus(
    mut focus: ResMut<InputFocus>,
    mouse: Res<ButtonInput<MouseButton>>,
    wrappers: Query<(&TextEditWrapper, &Interaction, &Children)>,
    drag_hitboxes: Query<&DragHitbox>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    for (wrapper, interaction, children) in &wrappers {
        let is_dragging = children
            .iter()
            .any(|c| drag_hitboxes.get(c).is_ok_and(|d| d.dragging));
        if *interaction == Interaction::Pressed && !is_dragging {
            focus.0 = Some(wrapper.0);
        }
    }
}

fn handle_unfocus(
    mut focus: ResMut<InputFocus>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    text_edits: Query<&ChildOf, With<EditorTextEdit>>,
    wrappers: Query<&Interaction, With<TextEditWrapper>>,
) {
    let Some(focused_entity) = focus.0 else {
        return;
    };
    let Ok(child_of) = text_edits.get(focused_entity) else {
        return;
    };
    let Ok(interaction) = wrappers.get(child_of.parent()) else {
        return;
    };

    let clicked_outside =
        mouse.get_just_pressed().next().is_some() && *interaction == Interaction::None;
    let key_dismiss = keyboard.just_pressed(KeyCode::Escape)
        || keyboard.just_pressed(KeyCode::Enter)
        || keyboard.just_pressed(KeyCode::NumpadEnter);

    if clicked_outside || key_dismiss {
        focus.0 = None;
    }
}

fn handle_clamp_on_unfocus(
    mut commands: Commands,
    focus: Res<InputFocus>,
    mut prev_focus: Local<Option<Entity>>,
    mut text_edits: Query<
        (
            &TextEditVariant,
            &TextInputBuffer,
            &mut TextInputQueue,
            Option<&TextEditSuffix>,
            Option<&NumericRange>,
            Option<&AllowEmpty>,
        ),
        With<EditorTextEdit>,
    >,
) {
    let prev = *prev_focus;
    *prev_focus = focus.0;

    let Some(was_focused) = prev else { return };
    if focus.0 == Some(was_focused) {
        return;
    }

    let Ok((variant, buffer, mut queue, suffix, range, allow_empty)) =
        text_edits.get_mut(was_focused)
    else {
        return;
    };

    let text = strip_suffix(&buffer.get_text(), suffix);

    commands.trigger(TextEditCommitEvent {
        entity: was_focused,
        text: text.clone(),
    });

    if !variant.is_numeric() {
        return;
    }

    if text.is_empty() && allow_empty.is_some() {
        return;
    }

    let value = text.parse().unwrap_or(0.0);
    update_input_value(&mut queue, value, *variant, range);
}

fn handle_numeric_increment(
    focus: Res<InputFocus>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut text_edits: Query<
        (
            Entity,
            &TextEditVariant,
            &TextInputBuffer,
            &mut TextInputQueue,
            Option<&TextEditSuffix>,
            Option<&NumericRange>,
        ),
        With<EditorTextEdit>,
    >,
) {
    let Some(focused_entity) = focus.0 else {
        return;
    };
    let Ok((_, variant, buffer, mut queue, suffix, range)) = text_edits.get_mut(focused_entity)
    else {
        return;
    };
    if !variant.is_numeric() {
        return;
    }

    let direction = match (
        keyboard.just_pressed(KeyCode::ArrowUp),
        keyboard.just_pressed(KeyCode::ArrowDown),
    ) {
        (true, _) => 1.0,
        (_, true) => -1.0,
        _ => return,
    };

    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let step = if shift { 10.0 } else { 1.0 };
    let new_value = parse_numeric_value(&buffer.get_text(), suffix) + (direction * step);
    let rounded = (new_value * 100.0).round() / 100.0;

    update_input_value(&mut queue, rounded, *variant, range);
}

fn handle_drag_value(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut drag_hitboxes: Query<(Entity, &mut DragHitbox, &Interaction, &ChildOf)>,
    wrappers: Query<&TextEditWrapper>,
    mut text_edits: Query<
        (
            &TextEditVariant,
            &TextInputBuffer,
            &mut TextInputQueue,
            Option<&TextEditSuffix>,
            Option<&NumericRange>,
        ),
        With<EditorTextEdit>,
    >,
) {
    let Ok(window) = windows.single() else { return };
    let cursor_pos = window.cursor_position();

    for (entity, mut hitbox, interaction, child_of) in &mut drag_hitboxes {
        let Ok(wrapper) = wrappers.get(child_of.parent()) else {
            continue;
        };
        let input_entity = wrapper.0;

        if mouse.just_pressed(MouseButton::Left) && *interaction == Interaction::Pressed {
            if let Some(pos) = cursor_pos {
                let Ok((_, buffer, _, suffix, _)) = text_edits.get(input_entity) else {
                    continue;
                };
                hitbox.dragging = true;
                hitbox.start_x = pos.x;
                hitbox.start_value = parse_numeric_value(&buffer.get_text(), suffix);
                commands
                    .entity(entity)
                    .insert(ActiveCursor(bevy::window::SystemCursorIcon::ColResize));
            }
        }

        if mouse.just_released(MouseButton::Left) {
            if hitbox.dragging {
                if let Ok((_, buffer, _, suffix, _)) = text_edits.get(input_entity) {
                    let text = strip_suffix(&buffer.get_text(), suffix);
                    commands.trigger(TextEditCommitEvent {
                        entity: input_entity,
                        text,
                    });
                }
            }
            hitbox.dragging = false;
            commands.entity(entity).remove::<ActiveCursor>();
        }

        if hitbox.dragging {
            if let Some(pos) = cursor_pos {
                let Ok((variant, _, mut queue, _, range)) = text_edits.get_mut(input_entity) else {
                    continue;
                };

                let alt_mode = keyboard.pressed(KeyCode::SuperLeft)
                    || keyboard.pressed(KeyCode::SuperRight)
                    || keyboard.pressed(KeyCode::AltLeft)
                    || keyboard.pressed(KeyCode::AltRight);

                let (amount, sensitivity) = match (*variant, alt_mode) {
                    (TextEditVariant::NumericI32, false) => (1.0, 5.0),
                    (TextEditVariant::NumericI32, true) => (10.0, 10.0),
                    (_, false) => (0.1, 5.0),
                    (_, true) => (1.0, 10.0),
                };

                let steps = ((pos.x - hitbox.start_x) / sensitivity).floor() as f64;
                let new_value = hitbox.start_value + (steps * amount);
                let rounded = (new_value * 100.0).round() / 100.0;

                update_input_value(&mut queue, rounded, *variant, range);
            }
        }
    }
}

fn strip_suffix(text: &str, suffix: Option<&TextEditSuffix>) -> String {
    suffix
        .and_then(|s| text.strip_suffix(&format!(" {}", s.0)))
        .unwrap_or(text)
        .to_string()
}

fn parse_numeric_value(text: &str, suffix: Option<&TextEditSuffix>) -> f64 {
    strip_suffix(text, suffix).parse().unwrap_or(0.0)
}

fn format_numeric_value(value: f64, variant: TextEditVariant) -> String {
    match variant {
        TextEditVariant::NumericI32 => (value.round() as i32).to_string(),
        TextEditVariant::NumericF32 => {
            let mut text = value.to_string();
            if !text.contains('.') {
                text.push_str(".0");
            }
            text
        }
        TextEditVariant::Default => value.to_string(),
    }
}

fn clamp_value(value: f64, range: Option<&NumericRange>) -> f64 {
    match range {
        Some(r) => value.clamp(r.min, r.max),
        None => value,
    }
}

fn update_input_value(
    queue: &mut TextInputQueue,
    value: f64,
    variant: TextEditVariant,
    range: Option<&NumericRange>,
) {
    let clamped = clamp_value(value, range);
    set_text_input_value(queue, format_numeric_value(clamped, variant));
}
