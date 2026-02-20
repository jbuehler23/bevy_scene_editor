use bevy::prelude::*;

pub struct CollapsiblePlugin;

impl Plugin for CollapsiblePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(toggle_collapsible);
    }
}

/// Marker on a collapsible section root.
#[derive(Component)]
pub struct CollapsibleSection {
    pub collapsed: bool,
}

/// Marker on the clickable header bar.
#[derive(Component)]
pub struct CollapsibleHeader;

/// Marker on the collapsible body container.
#[derive(Component)]
pub struct CollapsibleBody;

/// Event to toggle a collapsible section.
#[derive(EntityEvent)]
pub struct ToggleCollapsible {
    pub entity: Entity,
}

fn toggle_collapsible(
    event: On<ToggleCollapsible>,
    mut sections: Query<(&mut CollapsibleSection, &Children)>,
    mut visibility: Query<&mut Visibility, With<CollapsibleBody>>,
) {
    let target = event.entity;
    let Ok((mut section, children)) = sections.get_mut(target) else {
        return;
    };

    section.collapsed = !section.collapsed;

    for child in children.iter() {
        if let Ok(mut vis) = visibility.get_mut(child) {
            *vis = if section.collapsed {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
    }
}
