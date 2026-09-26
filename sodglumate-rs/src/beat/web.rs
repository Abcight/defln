use crate::reactor::{Command, ComponentResponse};
use std::time::Instant;

/// Web builds do not provide system-audio capture. This preserves the
/// application-facing beat API while the web UI omits audio controls.
pub struct SystemBeat {
	selected_device: Option<String>,
	last_beat: Instant,
}

#[allow(dead_code)]
impl SystemBeat {
	pub fn new(selected_device: Option<String>, _enabled: bool) -> Self {
		Self {
			selected_device,
			last_beat: Instant::now(),
		}
	}

	pub fn poll(&mut self) -> ComponentResponse {
		ComponentResponse::none()
	}

	pub fn handle_command(&mut self, command: &Command) -> ComponentResponse {
		if let Command::SetAudioDevice(device) = command {
			self.selected_device = device.clone();
		}
		ComponentResponse::none()
	}

	pub fn device_names(&self) -> &[String] {
		&[]
	}

	pub fn selected_device(&self) -> &Option<String> {
		&self.selected_device
	}

	pub fn selected_device_label(&self) -> &str {
		"Unavailable on web"
	}

	pub fn is_active(&self) -> bool {
		false
	}

	pub fn latest_beat(&self) -> (Instant, f32) {
		(self.last_beat, 0.0)
	}
}

impl Default for SystemBeat {
	fn default() -> Self {
		Self::new(None, false)
	}
}
