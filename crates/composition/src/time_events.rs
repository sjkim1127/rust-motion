//! Immutable named frame markers for deterministic, random-access compositions.

use dioxuscut_project::TimeEvent;
use std::collections::BTreeMap;
use thiserror::Error;

/// A validated, immutable set of named events for one composition timeline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimeEventSchedule {
    events: Vec<TimeEvent>,
    frames: BTreeMap<String, u32>,
}

/// Invalid lookups or definitions in a named event schedule.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum TimeEventError {
    #[error("time event name cannot be empty")]
    EmptyName,
    #[error("time event '{0}' is duplicated")]
    Duplicate(String),
    #[error("time event '{0}' is missing from this schedule")]
    Missing(String),
    #[error("time event '{end}' at frame {end_frame} precedes '{start}' at frame {start_frame}")]
    EndBeforeStart {
        start: String,
        start_frame: u32,
        end: String,
        end_frame: u32,
    },
}

impl TimeEventSchedule {
    /// Build a schedule from project events or composition defaults.
    pub fn new(events: impl IntoIterator<Item = TimeEvent>) -> Result<Self, TimeEventError> {
        let mut ordered = Vec::new();
        let mut frames = BTreeMap::new();
        for event in events {
            let id = event.id.clone();
            if id.trim().is_empty() {
                return Err(TimeEventError::EmptyName);
            }
            if frames.insert(id.clone(), event.frame).is_some() {
                return Err(TimeEventError::Duplicate(id));
            }
            ordered.push(event);
        }
        Ok(Self {
            events: ordered,
            frames,
        })
    }

    /// Return the frame at which a named event occurs.
    pub fn frame(&self, name: &str) -> Result<u32, TimeEventError> {
        self.frames
            .get(name)
            .copied()
            .ok_or_else(|| TimeEventError::Missing(name.to_string()))
    }

    /// Resolve a named wait point into an absolute frame.
    pub fn wait_until(&self, name: &str) -> Result<u32, TimeEventError> {
        self.frame(name)
    }

    /// Resolve the non-negative frame duration between two named events.
    pub fn duration_between(&self, start: &str, end: &str) -> Result<u32, TimeEventError> {
        let start_frame = self.frame(start)?;
        let end_frame = self.frame(end)?;
        end_frame
            .checked_sub(start_frame)
            .ok_or_else(|| TimeEventError::EndBeforeStart {
                start: start.to_string(),
                start_frame,
                end: end.to_string(),
                end_frame,
            })
    }

    /// Iterate event names and frames in their declared project order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, u32)> + '_ {
        self.events
            .iter()
            .map(|event| (event.id.as_str(), event.frame))
    }

    /// Convert the schedule back to persisted project event records.
    pub fn to_events(&self) -> Vec<TimeEvent> {
        self.events.clone()
    }

    /// Return a composition-local view containing markers inside its clip.
    pub fn within_clip(&self, start: u32, duration: u32) -> Self {
        let end = start.saturating_add(duration);
        let events: Vec<TimeEvent> = self
            .events
            .iter()
            // Keep the clip's terminal frame marker so a composition can use
            // it as the end of a duration interval.
            .filter(|event| event.frame >= start && event.frame <= end)
            .map(|event| TimeEvent {
                id: event.id.clone(),
                frame: event.frame - start,
            })
            .collect();
        let frames = events
            .iter()
            .map(|event| (event.id.clone(), event.frame))
            .collect();
        Self { events, frames }
    }
}
