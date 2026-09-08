//! Named moments in an authored clip, and the message they send as playback
//! reaches them.
//!
//! An event is a child of the clip it belongs to, so it travels with the clip
//! through the document the same way a keyframe does. What plays the clip
//! writes [`ClipPlayhead`] on the clip entity; this module turns the span
//! between one write and the next into the events it covered.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// A named moment in a clip, as a child of the clip entity.
///
/// `time` is in seconds from the clip's start, on the same scale as a
/// keyframe's.
#[derive(Component, Reflect, Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[reflect(Component, Serialize, Deserialize, Default)]
pub struct ClipEvent {
    /// Seconds from the start of the clip.
    pub time: f32,
    /// What the message carries, for whatever is listening to name.
    pub name: String,
}

/// How far through its clip playback has come, written each frame by
/// whatever plays it; never saved.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ClipPlayhead {
    /// Where playback stood when events last fired.
    pub last: f32,
    /// Where it stands now.
    pub now: f32,
    /// How playback travelled from one to the other.
    pub pass: ClipPass,
}

/// The way playback moved through a clip between two readings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClipPass {
    /// Onwards through the clip.
    #[default]
    Forward,
    /// Onwards, off the end of the clip and on again from the start.
    ForwardWrapped,
    /// Back through the clip.
    Backward,
    /// Back off the start of the clip and on again from the end.
    BackwardWrapped,
}

impl ClipPlayhead {
    /// Move the playhead to `now`, keeping where it came from.
    pub fn advance_to(&mut self, now: f32, pass: ClipPass) {
        self.last = self.now;
        self.now = now;
        self.pass = pass;
    }

    /// Whether the span since the last reading covers `time`: closed at the end
    /// playback moves towards, open at the one it left, both ends on a wrap.
    fn covers(&self, time: f32) -> bool {
        match self.pass {
            ClipPass::Forward => time >= self.last && time < self.now,
            ClipPass::ForwardWrapped => time >= self.last || time < self.now,
            ClipPass::Backward => time >= self.now && time < self.last,
            ClipPass::BackwardWrapped => time >= self.now || time < self.last,
        }
    }
}

/// Sent as playback crosses a [`ClipEvent`].
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct AnimationEvent {
    /// The entity the clip animates, or the clip itself when it has no parent.
    pub entity: Entity,
    /// The name the crossed [`ClipEvent`] carries.
    pub name: String,
}

/// Send an [`AnimationEvent`] for every [`ClipEvent`] the playhead has just
/// passed, then leave the playhead where it stands so the next tick reads the
/// span after this one.
pub fn fire_clip_events(
    mut clips: Query<(Entity, &mut ClipPlayhead, &Children)>,
    events: Query<&ClipEvent>,
    parents: Query<&ChildOf>,
    mut out: MessageWriter<AnimationEvent>,
) {
    for (clip, mut playhead, children) in &mut clips {
        let animated = parents.get(clip).map_or(clip, ChildOf::parent);
        for child in children.iter() {
            let Ok(event) = events.get(child) else {
                continue;
            };
            if playhead.covers(event.time) {
                out.write(AnimationEvent {
                    entity: animated,
                    name: event.name.clone(),
                });
            }
        }
        playhead.last = playhead.now;
        playhead.pass = ClipPass::Forward;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world holding one clip with one event on it, under a named target.
    fn world_with_an_event_at(time: f32) -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_message::<AnimationEvent>()
            .add_systems(Update, fire_clip_events);
        let target = app.world_mut().spawn(Name::new("Door")).id();
        let clip = app
            .world_mut()
            .spawn((ClipPlayhead::default(), ChildOf(target)))
            .id();
        app.world_mut().spawn((
            ClipEvent {
                time,
                name: "step".to_string(),
            },
            ChildOf(clip),
        ));
        (app, target, clip)
    }

    fn events_after(
        app: &mut App,
        cursor: &mut bevy::ecs::message::MessageCursor<AnimationEvent>,
        clip: Entity,
        now: f32,
    ) -> Vec<AnimationEvent> {
        moved_to(app, cursor, clip, now, ClipPass::Forward)
    }

    fn moved_to(
        app: &mut App,
        cursor: &mut bevy::ecs::message::MessageCursor<AnimationEvent>,
        clip: Entity,
        now: f32,
        pass: ClipPass,
    ) -> Vec<AnimationEvent> {
        app.world_mut()
            .get_mut::<ClipPlayhead>(clip)
            .expect("the clip carries a playhead")
            .advance_to(now, pass);
        app.update();
        cursor
            .read(app.world().resource::<Messages<AnimationEvent>>())
            .cloned()
            .collect()
    }

    fn cursor(app: &App) -> bevy::ecs::message::MessageCursor<AnimationEvent> {
        app.world()
            .resource::<Messages<AnimationEvent>>()
            .get_cursor()
    }

    #[test]
    fn an_event_key_fires_once_when_playback_crosses_it() {
        let (mut app, target, clip) = world_with_an_event_at(0.5);
        let mut seen = cursor(&app);

        assert!(
            events_after(&mut app, &mut seen, clip, 0.4).is_empty(),
            "playback short of the key should say nothing"
        );
        assert_eq!(
            events_after(&mut app, &mut seen, clip, 0.6),
            vec![AnimationEvent {
                entity: target,
                name: "step".to_string(),
            }],
            "the span that covers the key should send its name once"
        );
        assert!(
            events_after(&mut app, &mut seen, clip, 0.7).is_empty(),
            "a key already passed must not send again"
        );
    }

    #[test]
    fn a_clip_that_wrapped_fires_the_keys_on_both_sides_of_the_wrap() {
        let (mut app, _, clip) = world_with_an_event_at(0.1);
        let mut seen = cursor(&app);
        events_after(&mut app, &mut seen, clip, 0.9);

        let fired = moved_to(&mut app, &mut seen, clip, 0.2, ClipPass::ForwardWrapped);

        assert_eq!(
            fired.len(),
            1,
            "wrapping past the key should send it: {fired:?}"
        );
    }

    #[test]
    fn a_key_on_the_first_frame_of_a_clip_fires_as_playback_leaves_it() {
        let (mut app, _, clip) = world_with_an_event_at(0.0);
        let mut seen = cursor(&app);

        let fired = events_after(&mut app, &mut seen, clip, 0.1);

        assert_eq!(
            fired.len(),
            1,
            "a key at the very start of the clip should send once the first \
             span leaves it: {fired:?}"
        );
    }

    #[test]
    fn a_wrap_that_lands_where_it_started_fires_the_whole_clip() {
        let (mut app, _, clip) = world_with_an_event_at(0.6);
        let mut seen = cursor(&app);
        events_after(&mut app, &mut seen, clip, 0.2);

        let fired = moved_to(&mut app, &mut seen, clip, 0.2, ClipPass::ForwardWrapped);

        assert_eq!(
            fired.len(),
            1,
            "a clip shorter than the frame that ran it comes back to the same \
             time having passed every key: {fired:?}"
        );
    }
}
