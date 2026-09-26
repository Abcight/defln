use crate::types::{BreathingPhaseMultipliers, BreathingStyle, ImageFillMode};
use serde::{Deserialize, Serialize};

const DEFAULT_SEARCH_QUERY: &str = "~gay ~male solo abs wolf order:score";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSettings {
	pub search_query: String,
	#[serde(default)]
	pub search_query_presets: Vec<String>,
	pub search_page_input: String,
	pub auto_play: bool,
	pub auto_play_delay_secs: f32,
	pub cap_by_breathing: bool,
	#[serde(default = "default_breathing_multiplier")]
	pub breathing_prepare_multiplier: f32,
	#[serde(default = "default_breathing_multiplier")]
	pub breathing_inhale_multiplier: f32,
	#[serde(default = "default_breathing_multiplier")]
	pub breathing_hold_multiplier: f32,
	#[serde(default = "default_breathing_multiplier")]
	pub breathing_release_multiplier: f32,
	#[serde(default = "default_breathing_multiplier")]
	pub breathing_idle_multiplier: f32,
	pub breathing_style: BreathingStyle,
	pub auto_pan_cycle_duration: f32,
	pub selected_audio_device: Option<String>,
	pub beat_pulse_enabled: bool,
	pub beat_pulse_scale: f32,
	pub image_fill_mode: ImageFillMode,
}

fn default_breathing_multiplier() -> f32 {
	1.0
}

impl Default for SavedSettings {
	fn default() -> Self {
		Self {
			search_query: DEFAULT_SEARCH_QUERY.to_owned(),
			search_query_presets: vec![DEFAULT_SEARCH_QUERY.to_owned()],
			search_page_input: "1".to_owned(),
			auto_play: false,
			auto_play_delay_secs: 16.0,
			cap_by_breathing: false,
			breathing_prepare_multiplier: 1.0,
			breathing_inhale_multiplier: 1.0,
			breathing_hold_multiplier: 1.0,
			breathing_release_multiplier: 1.0,
			breathing_idle_multiplier: 1.0,
			breathing_style: BreathingStyle::Immersive,
			auto_pan_cycle_duration: 10.0,
			selected_audio_device: None,
			beat_pulse_enabled: false,
			beat_pulse_scale: 0.03,
			image_fill_mode: ImageFillMode::Fit,
		}
	}
}

impl SavedSettings {
	pub fn normalized(mut self) -> Self {
		self.auto_play_delay_secs =
			finite_clamped(self.auto_play_delay_secs, 1.0, 60.0, 16.0);
		self.auto_pan_cycle_duration =
			finite_clamped(self.auto_pan_cycle_duration, 10.0, 120.0, 10.0);
		self.beat_pulse_scale =
			finite_clamped(self.beat_pulse_scale, 0.01, 0.15, 0.03);
		self.breathing_prepare_multiplier =
			finite_clamped(self.breathing_prepare_multiplier, 0.1, 10.0, 1.0);
		self.breathing_inhale_multiplier =
			finite_clamped(self.breathing_inhale_multiplier, 0.1, 10.0, 1.0);
		self.breathing_hold_multiplier =
			finite_clamped(self.breathing_hold_multiplier, 0.1, 10.0, 1.0);
		self.breathing_release_multiplier =
			finite_clamped(self.breathing_release_multiplier, 0.1, 10.0, 1.0);
		self.breathing_idle_multiplier =
			finite_clamped(self.breathing_idle_multiplier, 0.1, 10.0, 1.0);
		self
	}

	pub fn breathing_phase_multipliers(&self) -> BreathingPhaseMultipliers {
		BreathingPhaseMultipliers {
			prepare: self.breathing_prepare_multiplier,
			inhale: self.breathing_inhale_multiplier,
			hold: self.breathing_hold_multiplier,
			release: self.breathing_release_multiplier,
			idle: self.breathing_idle_multiplier,
		}
		.normalized()
	}
}

fn finite_clamped(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
	if value.is_finite() {
		value.clamp(min, max)
	} else {
		fallback
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn normalized_settings_reject_invalid_numeric_values() {
		let settings = SavedSettings {
			auto_play_delay_secs: f32::NAN,
			auto_pan_cycle_duration: -1.0,
			beat_pulse_scale: 99.0,
			..SavedSettings::default()
		};

		let normalized = settings.normalized();

		assert_eq!(normalized.auto_play_delay_secs, 16.0);
		assert_eq!(normalized.auto_pan_cycle_duration, 10.0);
		assert_eq!(normalized.beat_pulse_scale, 0.15);
	}
}
