use bevy::{prelude::*, ui_widgets::observe};
use jackdaw_feathers::{
    button::{self, ButtonProps, ButtonVariant},
    tokens,
};

use super::{
    brp_client::GetNavmeshInput,
    build::BuildNavmesh,
    save_load::{LoadNavmesh, SaveNavmesh},
};
use crate::{EditorEntity, selection::Selection};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, toggle_toolbar_visibility);
}

/// Marker for the navmesh contextual toolbar node.
#[derive(Component)]
pub struct NavmeshToolbar;

/// Builds the navmesh toolbar UI node. Starts hidden (`Display::None`).
pub fn navmesh_toolbar() -> impl Bundle {
    (
        NavmeshToolbar,
        EditorEntity,
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(tokens::SPACING_MD), px(tokens::SPACING_SM)),
            column_gap: px(tokens::SPACING_SM),
            width: percent(100),
            height: px(32.0),
            flex_shrink: 0.0,
            display: Display::None,
            ..Default::default()
        },
        BackgroundColor(tokens::TOOLBAR_BG),
        children![
            (
                Text::new("Navmesh"),
                TextFont {
                    font_size: tokens::FONT_SM,
                    ..Default::default()
                },
                TextColor(tokens::TEXT_SECONDARY),
            ),
            (
                button::button(
                    ButtonProps::new("Fetch Scene").with_variant(ButtonVariant::Primary),
                ),
                observe(|_: On<Pointer<Click>>, mut commands: Commands| {
                    commands.trigger(GetNavmeshInput);
                }),
            ),
            (
                button::button(ButtonProps::new("Build").with_variant(ButtonVariant::Default)),
                observe(|_: On<Pointer<Click>>, mut commands: Commands| {
                    commands.trigger(BuildNavmesh);
                }),
            ),
            (
                button::button(ButtonProps::new("Save").with_variant(ButtonVariant::Default)),
                observe(|_: On<Pointer<Click>>, mut commands: Commands| {
                    commands.trigger(SaveNavmesh);
                }),
            ),
            (
                button::button(ButtonProps::new("Load").with_variant(ButtonVariant::Default)),
                observe(|_: On<Pointer<Click>>, mut commands: Commands| {
                    commands.trigger(LoadNavmesh);
                }),
            ),
        ],
    )
}

fn toggle_toolbar_visibility(
    selection: Res<Selection>,
    regions: Query<(), With<jackdaw_jsn::NavmeshRegion>>,
    mut toolbar: Query<&mut Node, With<NavmeshToolbar>>,
) {
    if !selection.is_changed() {
        return;
    }

    let should_show = selection
        .primary()
        .is_some_and(|e| regions.contains(e));

    for mut node in &mut toolbar {
        node.display = if should_show {
            Display::Flex
        } else {
            Display::None
        };
    }
}
