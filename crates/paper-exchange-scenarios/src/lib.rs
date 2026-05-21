//! Scenario timelines and overlay effects (Drop, Rally, …) for modulating a baseline market engine.
//! No I/O or async — the server schedules events and calls into [`OverlayState`].

mod overlay;
mod presets;
mod timeline;

pub use overlay::OverlayState;
pub use presets::{preset_by_name, preset_names};
pub use timeline::{EffectSpec, ScenarioTimeline, TimedEffect};
