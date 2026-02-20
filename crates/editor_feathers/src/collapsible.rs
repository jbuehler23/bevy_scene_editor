use bevy::{feathers::theme::ThemedText, prelude::*};
use editor_widgets::collapsible::{
    CollapsibleBody, CollapsibleHeader, CollapsibleSection, ToggleCollapsible,
};
use lucide_icons::Icon;

use crate::tokens;

/// Spawn a styled collapsible section. Returns (section_entity, body_entity).
pub fn collapsible_section(
    commands: &mut Commands,
    title: &str,
    icon_font: &Handle<Font>,
    parent: Entity,
) -> (Entity, Entity) {
    let font = icon_font.clone();

    let body = commands
        .spawn((
            CollapsibleBody,
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::left(Val::Px(tokens::SPACING_MD)),
                width: Val::Percent(100.0),
                ..Default::default()
            },
        ))
        .id();

    let section = commands
        .spawn((
            CollapsibleSection { collapsed: false },
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Percent(100.0),
                ..Default::default()
            },
            ChildOf(parent),
        ))
        .id();

    // Header
    let title_owned = title.to_string();
    let header = commands
        .spawn((
            CollapsibleHeader,
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(tokens::SPACING_SM), Val::Px(tokens::SPACING_XS)),
                column_gap: Val::Px(tokens::SPACING_SM),
                ..Default::default()
            },
            BackgroundColor(tokens::PANEL_HEADER_BG),
            ChildOf(section),
        ))
        .id();

    // Chevron icon
    commands.spawn((
        Text::new(String::from(Icon::ChevronDown.unicode())),
        TextFont {
            font: font.clone(),
            font_size: tokens::FONT_SM,
            ..Default::default()
        },
        TextColor(tokens::TEXT_SECONDARY),
        ChildOf(header),
    ));

    // Title text
    commands.spawn((
        Text::new(title_owned),
        TextFont {
            font_size: tokens::FONT_MD,
            ..Default::default()
        },
        ThemedText,
        ChildOf(header),
    ));

    // Toggle on click
    let section_entity = section;
    commands.entity(header).observe(
        move |_: On<Pointer<Click>>, mut commands: Commands| {
            commands.trigger(ToggleCollapsible {
                entity: section_entity,
            });
        },
    );

    // Hover effect on header
    commands.entity(header).observe(
        |hover: On<Pointer<Over>>, mut bg: Query<&mut BackgroundColor, With<CollapsibleHeader>>| {
            if let Ok(mut bg) = bg.get_mut(hover.event_target()) {
                bg.0 = tokens::HOVER_BG;
            }
        },
    );
    commands.entity(header).observe(
        |out: On<Pointer<Out>>, mut bg: Query<&mut BackgroundColor, With<CollapsibleHeader>>| {
            if let Ok(mut bg) = bg.get_mut(out.event_target()) {
                bg.0 = tokens::PANEL_HEADER_BG;
            }
        },
    );

    // Attach body to section
    commands.entity(body).insert(ChildOf(section));

    (section, body)
}
