use crate::api::Post;
use crate::types::{BreathingPhase, BreathingStyle, MediaKind, NavDirection};
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum Event {
	Source(SourceEvent),
	Gateway(GatewayEvent),
	Browser(BrowserEvent),
	Media(MediaEvent),
	Breathing(BreathingEvent),
	Settings(SettingsEvent),
	Beat(BeatEvent),
}

impl Event {
	pub fn priority(&self) -> Priority {
		match self {
			Event::Source(_) => Priority::High,
			Event::Gateway(_) => Priority::Normal,
			Event::Browser(_) => Priority::Normal,
			Event::Media(MediaEvent::Prefetch { .. }) => Priority::Low,
			Event::Media(_) => Priority::Normal,
			Event::Breathing(_) => Priority::Low,
			Event::Beat(_) => Priority::Low,
			Event::Settings(SettingsEvent::SlideshowAdvance) => Priority::Normal,
			Event::Settings(_) => Priority::Normal,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
	High = 0,
	Normal = 1,
	Low = 2,
}

impl Priority {
	pub fn as_index(&self) -> usize {
		*self as usize
	}
}

#[derive(Clone, Debug)]
pub enum SourceEvent {
	Search {
		query: String,
		page: u32,
	},
	Navigate(NavDirection),
	ConfigureCoach {
		enabled: bool,
		model: Option<String>,
		preset: Option<String>,
	},
}

#[derive(Clone, Debug)]
pub enum GatewayEvent {
	SearchRequest {
		query: String,
		page: u32,
		limit: u32,
	},
	FetchNextPage,
}

#[derive(Clone, Debug)]
pub enum BrowserEvent {
	PostsReceived {
		posts: Vec<Post>,
		page: u32,
		is_new: bool,
	},
	Navigate {
		direction: NavDirection,
	},
}

#[derive(Clone, Debug)]
pub enum MediaEvent {
	LoadRequest {
		sample_url: Option<String>,
		full_url: Option<String>,
		kind: MediaKind,
	},
	Prefetch {
		urls: Vec<(Option<String>, Option<String>, MediaKind)>, // (sample_url, full_url, kind)
	},
	/// The current media was drawn during the latest UI pass.
	Painted,
}

#[derive(Clone, Debug)]
pub enum BreathingEvent {
	Toggle,
	PhaseComplete,
	SetPhaseMultiplier { phase: BreathingPhase, value: f32 },
	SetStyle { style: BreathingStyle },
	PhaseStarted(BreathingPhase),
}

#[derive(Clone, Debug)]
pub enum SettingsEvent {
	/// Toggle auto-play
	ToggleAutoPlay,
	/// Set auto-play delay
	SetDelay {
		duration: Duration,
	},
	/// Adjust auto-play delay by delta
	AdjustDelay {
		delta_secs: i64,
	},
	/// Timer fired, advance slideshow
	SlideshowAdvance,
	ToggleCapByBreathing,
}

#[derive(Clone, Debug)]
pub enum BeatEvent {
	/// Switch capture device (None = system default)
	SetDevice { name: Option<String> },
}

/// Response from component.handle()
#[derive(Default)]
pub struct ComponentResponse {
	/// Events to dispatch immediately
	pub events: Vec<Event>,
	/// Events to schedule (event, delay)
	pub scheduled: Vec<(Event, Duration)>,
}

impl ComponentResponse {
	pub fn none() -> Self {
		Self::default()
	}

	pub fn emit(event: Event) -> Self {
		Self {
			events: vec![event],
			scheduled: vec![],
		}
	}

	pub fn emit_many(events: Vec<Event>) -> Self {
		Self {
			events,
			scheduled: vec![],
		}
	}

	pub fn schedule(event: Event, delay: Duration) -> Self {
		Self {
			events: vec![],
			scheduled: vec![(event, delay)],
		}
	}
}
