// TODO: Add support for mouse dragging.

use bevy::prelude::*;

#[derive(Component)]
pub struct PanelGroup {
    pub min_ratio: f32,
}

#[derive(Component)]
pub struct Panel {
    pub ratio: f32,
}

#[derive(Component)]
pub struct PanelHandle;

pub struct SplitPanelPlugin;

impl Plugin for SplitPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, apply_panel_size);
    }
}

fn apply_panel_size(
    changed_panels: Populated<&ChildOf, Changed<Panel>>,
    panel_group: Query<(&Node, &Children), Without<Panel>>,
    mut panels: Query<(&mut Node, &Panel)>,
) {
    for &ChildOf(parent) in changed_panels {
        let (Node { flex_direction, .. }, children) = panel_group.get(parent).unwrap();

        let total = panels
            .iter_many(children.collection())
            .map(|(_, Panel { ratio })| ratio)
            .sum::<f32>();

        let mut iterator = panels.iter_many_mut(children.collection());
        while let Some((mut node, Panel { ratio })) = iterator.fetch_next() {
            match flex_direction {
                FlexDirection::Row | FlexDirection::RowReverse => {
                    node.width = percent((ratio / total) * 100.);
                }
                FlexDirection::Column | FlexDirection::ColumnReverse => {
                    node.height = percent((ratio / total) * 100.);
                }
            }
        }
    }
}
