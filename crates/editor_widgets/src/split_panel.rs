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
    mut param_set: ParamSet<(
        Query<(&Node, &Children), With<PanelGroup>>,
        Query<(&mut Node, &Panel)>,
    )>,
) {
    // First pass: collect parent layout info (flex direction + child list)
    let mut group_info: Vec<(FlexDirection, Vec<Entity>)> = Vec::new();
    {
        let groups = param_set.p0();
        for &ChildOf(parent) in &changed_panels {
            if let Ok((node, children)) = groups.get(parent) {
                let children_vec: Vec<Entity> = children.iter().collect();
                group_info.push((node.flex_direction, children_vec));
            }
        }
    }

    // Second pass: apply sizes using the panels query
    let mut panels = param_set.p1();
    for (flex_direction, children) in &group_info {
        let total: f32 = panels
            .iter_many(children.iter())
            .map(|(_, Panel { ratio })| ratio)
            .sum();

        if total <= 0.0 {
            continue;
        }

        let mut iterator = panels.iter_many_mut(children.iter());
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
