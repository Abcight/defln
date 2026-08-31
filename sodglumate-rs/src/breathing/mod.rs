use crate::reactor::{BreathingEvent, ComponentResponse, Event};
use crate::types::{BreathingPhase, BreathingPhaseMultipliers, BreathingStyle};
use rand::Rng;
use std::time::{Duration, Instant};

pub struct BreathingState {
	pub phase: BreathingPhase,
	pub start_time: Instant,
	pub base_duration: Duration,
	pub duration: Duration,
}

pub struct BreathingOverlay {
	state: BreathingState,
	show_overlay: bool,
	phase_multipliers: BreathingPhaseMultipliers,
	style: BreathingStyle,
}

impl BreathingOverlay {
	pub fn new(
		show_overlay: bool,
		phase_multipliers: BreathingPhaseMultipliers,
		style: BreathingStyle,
	) -> Self {
		let phase_multipliers = phase_multipliers.normalized();
		let phase = BreathingPhase::Prepare;
		let base_duration = Duration::from_secs(5);
		Self {
			state: BreathingState {
				phase,
				start_time: Instant::now(),
				base_duration,
				duration: scaled_duration(base_duration, phase_multipliers.multiplier_for(phase)),
			},
			show_overlay,
			phase_multipliers,
			style,
		}
	}

	pub fn init(&self) -> ComponentResponse {
		let mut response = ComponentResponse::emit(Event::Breathing(BreathingEvent::PhaseStarted(
			self.state.phase,
		)));
		response.scheduled.push((
			Event::Breathing(BreathingEvent::PhaseComplete),
			self.state.duration,
		));
		response
	}

	pub fn handle(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::Breathing(BreathingEvent::Toggle) => {
				self.show_overlay = !self.show_overlay;
				ComponentResponse::none()
			}
			Event::Breathing(BreathingEvent::PhaseComplete) => {
				// Transition to next phase
				let (next_phase, base_duration, duration) = self.transition_phase();
				self.state = BreathingState {
					phase: next_phase,
					start_time: Instant::now(),
					base_duration,
					duration,
				};

				let mut response = ComponentResponse::emit(Event::Breathing(
					BreathingEvent::PhaseStarted(next_phase),
				));
				response
					.scheduled
					.push((Event::Breathing(BreathingEvent::PhaseComplete), duration));
				response
			}
			Event::Breathing(BreathingEvent::SetPhaseMultiplier { phase, value }) => {
				self.phase_multipliers.set_multiplier_for(*phase, *value);
				ComponentResponse::none()
			}
			Event::Breathing(BreathingEvent::SetStyle { style }) => {
				self.style = *style;
				ComponentResponse::none()
			}
			_ => ComponentResponse::none(),
		}
	}

	fn transition_phase(&self) -> (BreathingPhase, Duration, Duration) {
		let mut rng = rand::rng();

		let (next_phase, base_duration) = match self.state.phase {
			BreathingPhase::Prepare => {
				// -> Inhale (5-10s)
				(
					BreathingPhase::Inhale,
					Duration::from_secs(rng.random_range(5..=10)),
				)
			}
			BreathingPhase::Inhale => {
				// -> Hold (same base length as Inhale, independently scaled)
				(BreathingPhase::Hold, self.state.base_duration)
			}
			BreathingPhase::Hold => {
				// -> Release (4s)
				(BreathingPhase::Release, Duration::from_secs(4))
			}
			BreathingPhase::Release => {
				// Always pause between exercises to prevent back-to-back cycles.
				(
					BreathingPhase::Idle,
					Duration::from_secs(rng.random_range(17..=28)),
				)
			}
			BreathingPhase::Idle => {
				// -> Prepare (5s)
				(BreathingPhase::Prepare, Duration::from_secs(5))
			}
		};

		(
			next_phase,
			base_duration,
			scaled_duration(
				base_duration,
				self.phase_multipliers.multiplier_for(next_phase),
			),
		)
	}

	// Accessors for ViewManager
	pub fn is_visible(&self) -> bool {
		self.show_overlay
	}

	pub fn state(&self) -> &BreathingState {
		&self.state
	}

	pub fn phase_multipliers(&self) -> BreathingPhaseMultipliers {
		self.phase_multipliers
	}

	pub fn style(&self) -> BreathingStyle {
		self.style
	}
}

impl Default for BreathingOverlay {
	fn default() -> Self {
		Self::new(
			false,
			BreathingPhaseMultipliers::default(),
			BreathingStyle::default(),
		)
	}
}

fn scaled_duration(base_duration: Duration, multiplier: f32) -> Duration {
	Duration::from_secs_f32(base_duration.as_secs_f32() * multiplier)
}
