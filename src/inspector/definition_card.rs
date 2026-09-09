//! The inspector card for the open definition asset.
//!
//! The card is the generic component card with the definition's reflected
//! fields in its body, so scalars, enum menus and list rows behave exactly as
//! they do on a component. Its header carries the Save action and says when
//! the definition has unsaved edits.

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use bevy::reflect::ReflectFromReflect;
use jackdaw_api::op::Operator as _;
use jackdaw_api::prelude::DefinitionAssetTypes;
use jackdaw_feathers::{
    button::{ButtonOperatorCall, ButtonProps, ButtonSize, ButtonVariant, button},
    icons::{EditorFont, IconFont},
    tokens,
};
use jackdaw_widgets::collapsible::CollapsibleHeader;

use crate::definition_assets::{AssetSaveOp, DefinitionAssetEdit};

use super::component_display::{ComponentDisplaySpec, spawn_component_display};

/// Build the card for the definition `source` is editing under `inspector`.
pub(crate) fn fill_definition_card(world: &mut World, inspector: Entity, source: Entity) {
    let Some((kind, name, type_path, dirty)) =
        world.get::<DefinitionAssetEdit>(source).map(|edit| {
            (
                edit.kind.clone(),
                edit.name.clone(),
                edit.type_path.clone(),
                edit.dirty,
            )
        })
    else {
        return;
    };
    let label = world
        .get_resource::<DefinitionAssetTypes>()
        .and_then(|types| types.by_kind(&kind))
        .map_or_else(|| kind.clone(), |definition| definition.label.clone());

    let Some(value) = definition_snapshot(world, source, &type_path) else {
        return;
    };

    let registry = world.resource::<AppTypeRegistry>().clone();
    let icon_font = world.resource::<IconFont>().0.clone();
    let editor_font = world.resource::<EditorFont>().0.clone();
    let collapse_state =
        super::InspectorCollapseState(world.resource::<super::InspectorCollapseState>().0.clone());

    let card_name = format!("{name} ({label})");
    let mut state: SystemState<(Commands, Query<&Name>)> = SystemState::new(world);
    let Ok((mut commands, names)) = state.get_mut(world) else {
        return;
    };
    let card = spawn_component_display(
        &mut commands,
        ComponentDisplaySpec {
            name: &card_name,
            type_path: &type_path,
            entity: source,
            component: None,
            is_overridden: false,
            is_derived: false,
            prefab_ctx: None,
            revert_through_prefab: false,
            icon_font: &icon_font,
            editor_font: &editor_font,
            collapse_state: &collapse_state,
        },
    );
    jackdaw_feathers::utils::attach_or_despawn(&mut commands, inspector, card.section);
    super::reflect_fields::spawn_reflected_fields(
        &mut commands,
        card.body,
        value.as_ref(),
        0,
        String::new(),
        source,
        &type_path,
        &names,
        &registry,
        &editor_font,
        &icon_font,
    );
    state.apply(world);

    spawn_save_action(world, card.section, source, dirty);
}

/// The definition's value, owned, so the field rows can be spawned while the
/// world is borrowed for commands.
fn definition_snapshot(world: &World, source: Entity, type_path: &str) -> Option<Box<dyn Reflect>> {
    let registry = world.resource::<AppTypeRegistry>().read();
    let value = crate::definition_assets::definition_value(world, source, type_path, &registry)?;
    registry
        .get_with_type_path(type_path)?
        .data::<ReflectFromReflect>()?
        .from_reflect(value.as_partial_reflect())
}

/// The header's unsaved-edits marker, following the definition it names.
#[derive(Component)]
pub(crate) struct UnsavedMarker(Entity);

/// Show the marker exactly while the definition it names has unsaved edits.
pub(crate) fn keep_unsaved_marker_in_step(
    mut markers: Query<(&UnsavedMarker, &mut Node)>,
    edits: Query<&DefinitionAssetEdit>,
) {
    for (marker, mut node) in &mut markers {
        let display = if edits.get(marker.0).is_ok_and(|edit| edit.dirty) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

/// Put Save, and the unsaved-edits marker, in the card's header.
fn spawn_save_action(world: &mut World, section: Entity, source: Entity, dirty: bool) {
    let Some(header) = world.get::<Children>(section).and_then(|children| {
        children
            .iter()
            .find(|&child| world.get::<CollapsibleHeader>(child).is_some())
    }) else {
        return;
    };
    let row = world
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(tokens::SPACING_XS),
                ..default()
            },
            ChildOf(header),
        ))
        .id();
    world.spawn((
        Text::new("Unsaved"),
        TextFont {
            font_size: tokens::TEXT_SIZE_XS,
            ..default()
        },
        TextColor(tokens::TEXT_SECONDARY),
        Node {
            display: if dirty { Display::Flex } else { Display::None },
            ..default()
        },
        UnsavedMarker(source),
        ChildOf(row),
    ));
    let save = world
        .spawn((
            button(
                ButtonProps::new("Save")
                    .with_variant(ButtonVariant::Default)
                    .with_size(ButtonSize::MD),
            ),
            ButtonOperatorCall::new(AssetSaveOp::ID),
        ))
        .id();
    world.entity_mut(save).insert(ChildOf(row));
}
