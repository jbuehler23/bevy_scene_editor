use bevy::{
    ecs::{query::QueryFilter, spawn::SpawnableList},
    prelude::*,
};
use editor_widgets::split_panel::{Panel, PanelGroup, PanelHandle};

const HANDLE_SIZE: f32 = 4.0;
const HANDLE_COLOR: Color = Color::srgba(0.3, 0.3, 0.3, 1.0);
const HANDLE_HOVER_COLOR: Color = Color::srgba(0.5, 0.5, 0.5, 1.0);

pub fn panel_group<C: SpawnableList<ChildOf> + Send + Sync + 'static>(
    min_ratio: f32,
    panels: C,
) -> impl Bundle {
    (PanelGroup { min_ratio }, Children::spawn(panels))
}

pub fn panel(ratio: impl ValNum) -> impl Bundle {
    Panel {
        ratio: ratio.val_num_f32(),
    }
}

pub fn panel_handle() -> impl Bundle {
    (
        PanelHandle,
        Node {
            min_width: px(HANDLE_SIZE),
            min_height: px(HANDLE_SIZE),
            ..default()
        },
        BackgroundColor::from(HANDLE_COLOR),
    )
}

pub struct SplitPanelPlugin;

impl Plugin for SplitPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(set_background_on_with::<Pointer<Over>, With<PanelHandle>>(
            HANDLE_HOVER_COLOR,
        ))
        .add_observer(set_background_on_with::<Pointer<Out>, With<PanelHandle>>(
            HANDLE_COLOR,
        ));
    }
}

fn set_background_on_with<E: EntityEvent, F: QueryFilter>(
    color: Color,
) -> impl Fn(On<E>, Commands, Query<(), F>) {
    move |event, mut commands, filter| {
        if filter.contains(event.event_target()) {
            commands
                .entity(event.event_target())
                .insert(BackgroundColor(color));
        }
    }
}
