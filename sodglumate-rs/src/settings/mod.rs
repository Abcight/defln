use crate::breathing::BreathingOverlay;
use crate::reactor::{Command, ComponentResponse, Event, Message};
use crate::types::{BreathingPhase, NavDirection};
use std::time::{Duration, Instant};

pub struct SettingsManager {
	auto_play: bool,
	auto_play_delay: Duration,
	slideshow_scheduled: bool,
	cap_by_breathing: bool,
	last_advance_time: Instant,
}

impl SettingsManager {
	pub fn new(
		auto_play: bool,
		auto_play_delay: Duration,
		cap_by_breathing: bool,
	) -> Self {
		Self {
			auto_play,
			auto_play_delay,
			slideshow_scheduled: false,
			cap_by_breathing,
			last_advance_time: Instant::now(),
		}
	}

	pub fn handle_command(
		&mut self,
		command: &Command,
		breathing: &BreathingOverlay,
	) -> ComponentResponse {
		match command {
			Command::ToggleAutoPlay => {
				self.auto_play = !self.auto_play;
				if self.auto_play {
					self.last_advance_time = Instant::now();
					if !self.slideshow_scheduled {
						self.slideshow_scheduled = true;
						return ComponentResponse::schedule_command(
							Command::AdvanceSlideshow,
							self.auto_play_delay,
						);
					}
				}
				ComponentResponse::none()
			}
			Command::SetAutoPlayDelay(duration) => {
				self.auto_play_delay = *duration;
				ComponentResponse::none()
			}
			Command::AdjustAutoPlayDelay(delta_secs) => {
				let current_secs = self.auto_play_delay.as_secs() as i64;
				let new_secs = (current_secs + delta_secs).clamp(1, 60);
				self.auto_play_delay = Duration::from_secs(new_secs as u64);
				ComponentResponse::none()
			}
			Command::ToggleCapByBreathing => {
				self.cap_by_breathing = !self.cap_by_breathing;
				ComponentResponse::none()
			}
			Command::AdvanceSlideshow => {
				self.slideshow_scheduled = false;
				if self.auto_play {
					let elapsed = self.last_advance_time.elapsed();
					if elapsed < self.auto_play_delay {
						// We haven't waited long enough since the last manual navigation or advance
						self.slideshow_scheduled = true;
						return ComponentResponse::schedule_command(
							Command::AdvanceSlideshow,
							self.auto_play_delay - elapsed,
						);
					}

					// Check breathing cap
					if self.cap_by_breathing && breathing.is_visible() {
						let phase = breathing.state().phase;
						if matches!(
							phase,
							BreathingPhase::Inhale | BreathingPhase::Hold
						) {
							// Blocked by breathing, reschedule to check again shortly
							self.slideshow_scheduled = true;
							return ComponentResponse::schedule_command(
								Command::AdvanceSlideshow,
								Duration::from_secs(1),
							);
						}
					}

					// Navigate to next and schedule another advance
					self.slideshow_scheduled = true;
					self.last_advance_time = Instant::now();
					let mut response = ComponentResponse::command(Command::Navigate(
						NavDirection::Next,
					));
					response.scheduled.push((
						Message::Command(Command::AdvanceSlideshow),
						self.auto_play_delay,
					));
					return response;
				}
				ComponentResponse::none()
			}
			_ => ComponentResponse::none(),
		}
	}

	pub fn observe(
		&mut self,
		event: &Event,
		breathing: &BreathingOverlay,
	) -> ComponentResponse {
		match event {
			Event::BreathingPhaseStarted(phase)
				if self.auto_play
					&& self.cap_by_breathing
					&& breathing.is_visible()
					&& matches!(
						phase,
						BreathingPhase::Prepare | BreathingPhase::Release
					) =>
			{
				ComponentResponse::command(Command::Navigate(NavDirection::Next))
			}
			Event::Navigated(_) if self.auto_play => {
				self.last_advance_time = Instant::now();
				if !self.slideshow_scheduled {
					self.slideshow_scheduled = true;
					return ComponentResponse::schedule_command(
						Command::AdvanceSlideshow,
						self.auto_play_delay,
					);
				}
				ComponentResponse::none()
			}
			_ => ComponentResponse::none(),
		}
	}

	// Accessors for ViewManager/UI
	pub fn auto_play(&self) -> bool {
		self.auto_play
	}

	pub fn cap_by_breathing(&self) -> bool {
		self.cap_by_breathing
	}

	pub fn auto_play_delay(&self) -> Duration {
		self.auto_play_delay
	}
}

impl Default for SettingsManager {
	fn default() -> Self {
		Self::new(false, Duration::from_secs(16), false)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn enabling_autoplay_schedules_an_advance() {
		let mut settings = SettingsManager::new(false, Duration::from_secs(5), false);
		let breathing = BreathingOverlay::default();

		let response = settings.handle_command(&Command::ToggleAutoPlay, &breathing);

		assert!(settings.auto_play());
		assert_eq!(response.scheduled.len(), 1);
	}

	#[test]
	fn delay_adjustment_is_clamped() {
		let mut settings = SettingsManager::new(true, Duration::from_secs(5), false);
		let breathing = BreathingOverlay::default();

		settings.handle_command(&Command::AdjustAutoPlayDelay(-100), &breathing);
		assert_eq!(settings.auto_play_delay(), Duration::from_secs(1));

		settings.handle_command(&Command::AdjustAutoPlayDelay(100), &breathing);
		assert_eq!(settings.auto_play_delay(), Duration::from_secs(60));
	}
}
