use crate::booru::BooruSource;
use crate::breathing::BreathingOverlay;
use crate::config::{BooruCredentials, DapiCredentials, SavedSettings};
use crate::platform::Instant;
use crate::reactor::{Command, ComponentResponse, Event, Message};
use crate::types::{BreathingPhase, ImageFillMode, NavDirection};
use std::time::Duration;

pub struct SettingsManager {
	auto_play: bool,
	auto_play_delay: Duration,
	slideshow_scheduled: bool,
	cap_by_breathing: bool,
	last_advance_time: Instant,
	search_query: String,
	search_query_presets: Vec<String>,
	search_page_input: String,
	auto_pan_cycle_duration: f32,
	beat_pulse_enabled: bool,
	beat_pulse_scale: f32,
	image_fill_mode: ImageFillMode,
	booru_credentials: BooruCredentials,
}

impl SettingsManager {
	#[cfg(test)]
	fn new(
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
			search_query: String::new(),
			search_query_presets: Vec::new(),
			search_page_input: "1".to_owned(),
			auto_pan_cycle_duration: 10.0,
			beat_pulse_enabled: false,
			beat_pulse_scale: 0.03,
			image_fill_mode: ImageFillMode::default(),
			booru_credentials: BooruCredentials::default(),
		}
	}

	pub fn from_saved(saved: &SavedSettings) -> Self {
		let saved = saved.clone().normalized();
		Self {
			auto_play: saved.auto_play,
			auto_play_delay: Duration::from_secs_f32(saved.auto_play_delay_secs),
			slideshow_scheduled: false,
			cap_by_breathing: saved.cap_by_breathing,
			last_advance_time: Instant::now(),
			search_query: saved.search_query.clone(),
			search_query_presets: normalize_search_query_presets(
				saved.search_query_presets.clone(),
			),
			search_page_input: saved.search_page_input.clone(),
			auto_pan_cycle_duration: saved.auto_pan_cycle_duration,
			beat_pulse_enabled: saved.beat_pulse_enabled,
			beat_pulse_scale: saved.beat_pulse_scale,
			image_fill_mode: saved.image_fill_mode,
			booru_credentials: saved.booru_credentials,
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
				self.auto_play_delay = (*duration)
					.clamp(Duration::from_secs(1), Duration::from_secs(60));
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
			Command::SetSearchPreferences {
				query,
				presets,
				page_input,
			} => {
				self.search_query = query.clone();
				self.search_query_presets =
					normalize_search_query_presets(presets.clone());
				self.search_page_input = page_input.clone();
				ComponentResponse::none()
			}
			Command::SetDapiCredentials {
				source,
				user_id,
				api_key,
			} => {
				let credentials = DapiCredentials {
					user_id: user_id.clone(),
					api_key: api_key.clone(),
				}
				.normalized();
				match source {
					BooruSource::Rule34 => {
						self.booru_credentials.rule34 = credentials
					}
					BooruSource::Gelbooru => {
						self.booru_credentials.gelbooru = credentials
					}
					BooruSource::E621 => {}
				}
				ComponentResponse::none()
			}
			Command::SetAutoPanCycleDuration(duration) => {
				self.auto_pan_cycle_duration =
					finite_clamped(*duration, 10.0, 120.0, 10.0);
				ComponentResponse::none()
			}
			Command::SetBeatPulseEnabled(enabled) => {
				self.beat_pulse_enabled = *enabled;
				ComponentResponse::none()
			}
			Command::SetBeatPulseScale(scale) => {
				self.beat_pulse_scale = finite_clamped(*scale, 0.01, 0.15, 0.03);
				ComponentResponse::none()
			}
			Command::SetImageFillMode(mode) => {
				self.image_fill_mode = *mode;
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
			Event::Navigated if self.auto_play => {
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

	pub fn search_query(&self) -> &str {
		&self.search_query
	}

	pub fn search_query_presets(&self) -> &[String] {
		&self.search_query_presets
	}

	pub fn search_page_input(&self) -> &str {
		&self.search_page_input
	}

	pub fn auto_pan_cycle_duration(&self) -> f32 {
		self.auto_pan_cycle_duration
	}

	pub fn beat_pulse_enabled(&self) -> bool {
		self.beat_pulse_enabled
	}

	pub fn beat_pulse_scale(&self) -> f32 {
		self.beat_pulse_scale
	}

	pub fn image_fill_mode(&self) -> ImageFillMode {
		self.image_fill_mode
	}

	pub fn dapi_credentials(&self, source: BooruSource) -> DapiCredentials {
		match source {
			BooruSource::Rule34 => self.booru_credentials.rule34.clone(),
			BooruSource::Gelbooru => self.booru_credentials.gelbooru.clone(),
			BooruSource::E621 => DapiCredentials::default(),
		}
	}

	pub fn booru_credentials(&self) -> &BooruCredentials {
		&self.booru_credentials
	}
}

impl Default for SettingsManager {
	fn default() -> Self {
		Self::from_saved(&SavedSettings::default())
	}
}

fn finite_clamped(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
	if value.is_finite() {
		value.clamp(min, max)
	} else {
		fallback
	}
}

fn normalize_search_query_presets(presets: Vec<String>) -> Vec<String> {
	let mut normalized = Vec::new();
	for preset in presets {
		let preset = preset.trim();
		if !preset.is_empty() && !normalized.iter().any(|existing| existing == preset)
		{
			normalized.push(preset.to_owned());
		}
	}
	normalized
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

	#[test]
	fn persisted_view_preferences_are_canonical_settings_state() {
		let saved = SavedSettings {
			search_query: "wolves".to_owned(),
			search_query_presets: vec![
				" wolves ".to_owned(),
				"wolves".to_owned(),
				String::new(),
			],
			auto_pan_cycle_duration: f32::NAN,
			beat_pulse_scale: 99.0,
			image_fill_mode: ImageFillMode::FitToGallery,
			..SavedSettings::default()
		};

		let settings = SettingsManager::from_saved(&saved);

		assert_eq!(settings.search_query(), "wolves");
		assert_eq!(settings.search_query_presets(), &["wolves"]);
		assert_eq!(settings.auto_pan_cycle_duration(), 10.0);
		assert_eq!(settings.beat_pulse_scale(), 0.15);
		assert_eq!(settings.image_fill_mode(), ImageFillMode::FitToGallery);
	}
}
