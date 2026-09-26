use crate::platform::Instant;
use eframe::egui;
use std::time::Duration;

/// Loaded media content
pub enum LoadedMedia {
	Image {
		texture: egui::TextureHandle,
	},
	AnimatedImage {
		frames: Vec<AnimatedFrame>,
		started_at: Instant,
		complete: bool,
	},
}

pub struct AnimatedFrame {
	pub texture: egui::TextureHandle,
	pub duration: Duration,
}

impl LoadedMedia {
	pub fn texture(&self) -> &egui::TextureHandle {
		match self {
			Self::Image { texture } => texture,
			Self::AnimatedImage {
				frames,
				started_at,
				complete,
			} => {
				let elapsed = started_at.elapsed();
				let total_duration: Duration =
					frames.iter().map(|frame| frame.duration).sum();
				let elapsed = if total_duration.is_zero() {
					Duration::ZERO
				} else if *complete {
					Duration::from_nanos(
						(elapsed.as_nanos() % total_duration.as_nanos()) as u64,
					)
				} else {
					// Do not loop over only the frames received so far. Until the
					// stream completes, the last available frame is a buffer edge.
					elapsed
				};
				let mut frame_elapsed = Duration::ZERO;
				for frame in frames {
					frame_elapsed += frame.duration;
					if elapsed < frame_elapsed {
						return &frame.texture;
					}
				}
				&frames[frames.len() - 1].texture
			}
		}
	}

	pub fn is_animated(&self) -> bool {
		matches!(self, Self::AnimatedImage { .. })
	}
}

use serde::{Deserialize, Serialize};

/// Rendering backend selected for a post's media.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
	Image,
	Playable,
}

impl MediaKind {
	pub fn from_extension(extension: &str) -> Option<Self> {
		match extension.to_ascii_lowercase().as_str() {
			"jpg" | "jpeg" | "png" | "webp" => Some(Self::Image),
			"gif" | "mp4" | "webm" | "m3u8" => Some(Self::Playable),
			_ => None,
		}
	}

	pub fn is_playable(self) -> bool {
		matches!(self, Self::Playable)
	}
}

/// Breathing overlay display style
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum BreathingStyle {
	#[default]
	Immersive, // Full progress bar overlay
	Classic, // Quick pop-in animation
}

/// How to fill the image in the view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ImageFillMode {
	Cover,
	Fit,
	#[default]
	FitToGallery,
}

/// Breathing timer phases
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BreathingPhase {
	Prepare,
	Inhale,
	Hold,
	Release,
	Idle,
}

/// Duration multipliers for each breathing timer phase.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BreathingPhaseMultipliers {
	pub prepare: f32,
	pub inhale: f32,
	pub hold: f32,
	pub release: f32,
	pub idle: f32,
}

impl BreathingPhaseMultipliers {
	pub fn multiplier_for(&self, phase: BreathingPhase) -> f32 {
		match phase {
			BreathingPhase::Prepare => self.prepare,
			BreathingPhase::Inhale => self.inhale,
			BreathingPhase::Hold => self.hold,
			BreathingPhase::Release => self.release,
			BreathingPhase::Idle => self.idle,
		}
	}

	pub fn set_multiplier_for(&mut self, phase: BreathingPhase, value: f32) {
		let value = sanitize_breathing_multiplier(value);
		match phase {
			BreathingPhase::Prepare => self.prepare = value,
			BreathingPhase::Inhale => self.inhale = value,
			BreathingPhase::Hold => self.hold = value,
			BreathingPhase::Release => self.release = value,
			BreathingPhase::Idle => self.idle = value,
		}
	}

	pub fn normalized(self) -> Self {
		Self {
			prepare: sanitize_breathing_multiplier(self.prepare),
			inhale: sanitize_breathing_multiplier(self.inhale),
			hold: sanitize_breathing_multiplier(self.hold),
			release: sanitize_breathing_multiplier(self.release),
			idle: sanitize_breathing_multiplier(self.idle),
		}
	}
}

impl Default for BreathingPhaseMultipliers {
	fn default() -> Self {
		Self {
			prepare: 1.0,
			inhale: 1.0,
			hold: 1.0,
			release: 1.0,
			idle: 1.0,
		}
	}
}

fn sanitize_breathing_multiplier(value: f32) -> f32 {
	if value.is_finite() {
		value.clamp(0.1, 10.0)
	} else {
		1.0
	}
}

/// Navigation direction
#[derive(Debug, Clone, Copy)]
pub enum NavDirection {
	Next,
	Prev,
	Skip(i32),
}
