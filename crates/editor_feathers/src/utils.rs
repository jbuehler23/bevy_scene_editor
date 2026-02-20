use bevy::prelude::*;

pub fn is_descendant_of(entity: Entity, ancestor: Entity, parents: &Query<&ChildOf>) -> bool {
    let mut current = entity;
    for _ in 0..50 {
        if current == ancestor {
            return true;
        }
        if let Ok(child_of) = parents.get(current) {
            current = child_of.parent();
        } else {
            return false;
        }
    }
    false
}

pub fn find_ancestor<'a, C: Component>(
    entity: Entity,
    query: &'a Query<&C>,
    parents: &Query<&ChildOf>,
) -> Option<(Entity, &'a C)> {
    let mut current = entity;
    for _ in 0..50 {
        if let Ok(component) = query.get(current) {
            return Some((current, component));
        }
        if let Ok(child_of) = parents.get(current) {
            current = child_of.parent();
        } else {
            return None;
        }
    }
    None
}
