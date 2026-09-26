use crate::api::Post;
use crate::types::{
	BreathingPhase, BreathingStyle, ImageFillMode, MediaKind, NavDirection,
};
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum Message {
	Command(Command),
	Event(Event),
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum Command {
	Search {
		query: String,
		page: u32,
	},
	FetchNextPage,
	PrepareLinks {
		source_id: u64,
	},
	FetchLinkCandidate {
		post_id: u64,
		generation: u64,
	},
	OpenLinkedPost {
		source_id: u64,
		target_id: u64,
	},
	FetchLinkedPost {
		post_id: u64,
	},
	Navigate(NavDirection),
	LoadMedia {
		sample_url: Option<String>,
		full_url: Option<String>,
		kind: MediaKind,
	},
	PrefetchMedia {
		urls: Vec<(Option<String>, Option<String>, MediaKind)>,
	},
	PrefetchRelatedMedia {
		urls: Vec<(String, MediaKind)>,
	},
	ToggleBreathing,
	CompleteBreathingPhase,
	SetBreathingPhaseMultiplier {
		phase: BreathingPhase,
		value: f32,
	},
	SetBreathingStyle(BreathingStyle),
	ToggleAutoPlay,
	SetAutoPlayDelay(Duration),
	AdjustAutoPlayDelay(i64),
	AdvanceSlideshow,
	ToggleCapByBreathing,
	SetAudioDevice(Option<String>),
	SetSearchPreferences {
		query: String,
		presets: Vec<String>,
		page_input: String,
	},
	SetAutoPanCycleDuration(f32),
	SetBeatPulseEnabled(bool),
	SetBeatPulseScale(f32),
	SetImageFillMode(ImageFillMode),
}

#[derive(Clone, Debug)]
pub enum Event {
	LinkCandidateLoaded {
		post_id: u64,
		generation: u64,
		result: Result<Box<Post>, String>,
	},
	LinkedPostLoaded {
		post_id: u64,
		result: Result<Box<Post>, String>,
	},
	SearchCompleted {
		posts: Vec<Post>,
		page: u32,
		is_new: bool,
	},
	Navigated,
	BreathingPhaseStarted(BreathingPhase),
	MediaPainted,
}

/// Ordered messages and delayed messages produced while handling one message.
#[derive(Default)]
pub struct ComponentResponse {
	pub messages: Vec<Message>,
	pub scheduled: Vec<(Message, Duration)>,
}

impl ComponentResponse {
	pub fn none() -> Self {
		Self::default()
	}

	pub fn command(command: Command) -> Self {
		Self::message(Message::Command(command))
	}

	pub fn event(event: Event) -> Self {
		Self::message(Message::Event(event))
	}

	pub fn messages(messages: Vec<Message>) -> Self {
		Self {
			messages,
			scheduled: Vec::new(),
		}
	}

	pub fn schedule_command(command: Command, delay: Duration) -> Self {
		Self {
			messages: Vec::new(),
			scheduled: vec![(Message::Command(command), delay)],
		}
	}

	fn message(message: Message) -> Self {
		Self {
			messages: vec![message],
			scheduled: Vec::new(),
		}
	}
}

#[derive(Default)]
pub struct ViewOutput {
	messages: Vec<Message>,
}

impl ViewOutput {
	pub fn command(&mut self, command: Command) {
		self.messages.push(Message::Command(command));
	}

	pub fn event(&mut self, event: Event) {
		self.messages.push(Message::Event(event));
	}

	pub fn into_messages(self) -> impl Iterator<Item = Message> {
		self.messages.into_iter()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn view_output_preserves_command_and_event_order() {
		let mut output = ViewOutput::default();
		output.event(Event::MediaPainted);
		output.command(Command::Navigate(NavDirection::Next));

		assert!(matches!(
			output.messages[0],
			Message::Event(Event::MediaPainted)
		));
		assert!(matches!(
			output.messages[1],
			Message::Command(Command::Navigate(NavDirection::Next))
		));
	}
}
