use crate::reactor::{Command, ComponentResponse};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::sync::mpsc;
use std::time::Instant;

/// Size of energy analysis window in samples
const WINDOW_SIZE: usize = 441;

/// Number of history windows for rolling average
const HISTORY_LEN: usize = 43;

/// Energy threshold multiplier over rolling average to trigger a beat
const BEAT_THRESHOLD: f32 = 1.5;

/// Minimum time between beats to avoid double-triggers
const BEAT_COOLDOWN_MS: u128 = 200;

pub struct SystemBeat {
	/// Raw audio samples from cpal stream
	sample_rx: mpsc::Receiver<Vec<f32>>,
	/// Sender cloned into cpal stream callback
	sample_tx: mpsc::SyncSender<Vec<f32>>,
	/// Active cpal stream (must be kept alive)
	stream: Option<cpal::Stream>,
	enabled: bool,
	/// Available device names
	device_names: Vec<String>,
	/// Currently selected device name (None = default)
	selected_device: Option<String>,
	/// Energy detection state
	sample_buffer: Vec<f32>,
	energy_history: Vec<f32>,
	history_index: usize,
	last_beat: Instant,
	last_beat_scale: f32,
}

impl SystemBeat {
	pub fn new(selected_device: Option<String>, enabled: bool) -> Self {
		let (sample_tx, sample_rx) = mpsc::sync_channel(32);

		let mut beat = Self {
			sample_rx,
			sample_tx,
			stream: None,
			enabled,
			device_names: Vec::new(),
			selected_device,
			sample_buffer: Vec::with_capacity(WINDOW_SIZE * 2),
			energy_history: vec![0.0; HISTORY_LEN],
			history_index: 0,
			last_beat: Instant::now(),
			last_beat_scale: 0.0,
		};
		beat.restart_capture();
		beat
	}

	/// Enumerate all available input devices
	fn enumerate_devices() -> Vec<String> {
		let host = cpal::default_host();
		let mut names = Vec::new();
		if let Ok(devices) = host.input_devices() {
			for device in devices {
				if let Ok(name) = device.name() {
					names.push(name);
				}
			}
		}
		log::info!("Enumerated {} audio input devices", names.len());
		for name in &names {
			log::debug!("  Audio device: {}", name);
		}
		names
	}

	/// Start capture on the default input device
	fn start_stream_default(tx: &mpsc::SyncSender<Vec<f32>>) -> Option<cpal::Stream> {
		let host = cpal::default_host();
		let device = match host.default_input_device() {
			Some(d) => {
				let name = d.name().unwrap_or_else(|_| "unknown".into());
				log::info!("Using default audio input: {}", name);
				d
			}
			None => {
				log::warn!("No default audio input device found");
				return None;
			}
		};
		Self::start_stream_on_device(&device, tx)
	}

	/// Start capture on a named device
	fn start_stream_named(
		name: &str,
		tx: &mpsc::SyncSender<Vec<f32>>,
	) -> Option<cpal::Stream> {
		let host = cpal::default_host();
		let devices = match host.input_devices() {
			Ok(d) => d,
			Err(e) => {
				log::error!("Failed to enumerate devices: {}", e);
				return None;
			}
		};
		for device in devices {
			if let Ok(dev_name) = device.name()
				&& dev_name == name
			{
				log::info!("Using audio device: {}", name);
				return Self::start_stream_on_device(&device, tx);
			}
		}
		log::warn!("Audio device '{}' not found, falling back to default", name);
		Self::start_stream_default(tx)
	}

	/// Start a cpal input stream on a specific device
	fn start_stream_on_device(
		device: &cpal::Device,
		tx: &mpsc::SyncSender<Vec<f32>>,
	) -> Option<cpal::Stream> {
		let config = match device.default_input_config() {
			Ok(c) => c,
			Err(e) => {
				log::error!("Failed to get input config: {}", e);
				return None;
			}
		};

		log::info!(
			"Audio config: {} channels, {}Hz, {:?}",
			config.channels(),
			config.sample_rate().0,
			config.sample_format()
		);

		let stream_config: cpal::StreamConfig = config.clone().into();
		let channels = config.channels() as usize;

		let stream = match config.sample_format() {
			SampleFormat::I8 => Self::build_input_stream::<i8>(
				device,
				&stream_config,
				channels,
				tx.clone(),
			),
			SampleFormat::I16 => Self::build_input_stream::<i16>(
				device,
				&stream_config,
				channels,
				tx.clone(),
			),
			SampleFormat::I32 => Self::build_input_stream::<i32>(
				device,
				&stream_config,
				channels,
				tx.clone(),
			),
			SampleFormat::I64 => Self::build_input_stream::<i64>(
				device,
				&stream_config,
				channels,
				tx.clone(),
			),
			SampleFormat::U8 => Self::build_input_stream::<u8>(
				device,
				&stream_config,
				channels,
				tx.clone(),
			),
			SampleFormat::U16 => Self::build_input_stream::<u16>(
				device,
				&stream_config,
				channels,
				tx.clone(),
			),
			SampleFormat::U32 => Self::build_input_stream::<u32>(
				device,
				&stream_config,
				channels,
				tx.clone(),
			),
			SampleFormat::U64 => Self::build_input_stream::<u64>(
				device,
				&stream_config,
				channels,
				tx.clone(),
			),
			SampleFormat::F32 => Self::build_input_stream::<f32>(
				device,
				&stream_config,
				channels,
				tx.clone(),
			),
			SampleFormat::F64 => Self::build_input_stream::<f64>(
				device,
				&stream_config,
				channels,
				tx.clone(),
			),
			sample_format => {
				log::error!("Unsupported audio sample format: {}", sample_format);
				return None;
			}
		};

		let stream = match stream {
			Ok(s) => s,
			Err(e) => {
				log::error!("Failed to build audio stream: {}", e);
				return None;
			}
		};

		if let Err(e) = stream.play() {
			log::error!("Failed to start audio stream: {}", e);
			return None;
		}

		Some(stream)
	}

	fn build_input_stream<T>(
		device: &cpal::Device,
		config: &cpal::StreamConfig,
		channels: usize,
		tx: mpsc::SyncSender<Vec<f32>>,
	) -> Result<cpal::Stream, cpal::BuildStreamError>
	where
		T: SizedSample,
		f32: FromSample<T>,
	{
		device.build_input_stream(
			config,
			move |data: &[T], _: &cpal::InputCallbackInfo| {
				let mono: Vec<f32> = data
					.chunks(channels)
					.map(|frame| {
						frame
							.iter()
							.map(|sample| f32::from_sample(*sample))
							.sum::<f32>() / frame.len() as f32
					})
					.collect();
				let _ = tx.try_send(mono);
			},
			move |err| {
				log::error!("Audio stream error: {}", err);
			},
			None,
		)
	}

	/// Poll for new audio data and detect beats
	pub fn poll(&mut self) -> ComponentResponse {
		// Drain all available samples
		while let Ok(samples) = self.sample_rx.try_recv() {
			self.sample_buffer.extend(samples);
		}

		let mut beat_detected = None;

		// Process complete windows
		while self.sample_buffer.len() >= WINDOW_SIZE {
			let window: Vec<f32> = self.sample_buffer.drain(..WINDOW_SIZE).collect();

			// Compute energy for this window
			let energy: f32 =
				window.iter().map(|s| s * s).sum::<f32>() / WINDOW_SIZE as f32;

			// Compute rolling average
			let avg_energy: f32 = self.energy_history.iter().sum::<f32>()
				/ self.energy_history.len() as f32;

			// Update history ring buffer
			self.energy_history[self.history_index] = energy;
			self.history_index = (self.history_index + 1) % HISTORY_LEN;

			// Beat detection with cooldown
			if energy > avg_energy * BEAT_THRESHOLD
				&& avg_energy > 1e-8 // Avoid triggering on silence
				&& self.last_beat.elapsed().as_millis() > BEAT_COOLDOWN_MS
			{
				let scale = (energy / (avg_energy * BEAT_THRESHOLD)).min(3.0);
				beat_detected = Some(scale);
				self.last_beat = Instant::now();
				self.last_beat_scale = scale;
			}
		}

		if let Some(scale) = beat_detected {
			log::debug!("Beat detected! scale={:.2}", scale);
		}
		ComponentResponse::none()
	}

	fn restart_capture(&mut self) {
		self.stream = None;
		// Replace the channel so callbacks from the previous stream cannot leak
		// samples into the next capture session.
		(self.sample_tx, self.sample_rx) = mpsc::sync_channel(32);
		self.sample_buffer.clear();
		self.energy_history.fill(0.0);
		self.history_index = 0;
		self.last_beat = Instant::now();
		self.last_beat_scale = 0.0;
		if self.enabled {
			self.device_names = Self::enumerate_devices();
			self.stream = match self.selected_device.as_deref() {
				Some(name) => Self::start_stream_named(name, &self.sample_tx),
				None => Self::start_stream_default(&self.sample_tx),
			};
		}
	}

	pub fn handle_command(&mut self, command: &Command) -> ComponentResponse {
		match command {
			Command::SetBeatPulseEnabled(enabled) if self.enabled != *enabled => {
				self.enabled = *enabled;
				self.restart_capture();
			}
			Command::SetAudioDevice(name) => {
				self.selected_device = name.clone();
				self.restart_capture();
			}
			_ => {}
		}
		ComponentResponse::none()
	}

	// Accessors for UI
	pub fn device_names(&self) -> &[String] {
		&self.device_names
	}

	pub fn selected_device(&self) -> &Option<String> {
		&self.selected_device
	}

	pub fn selected_device_label(&self) -> &str {
		self.selected_device.as_deref().unwrap_or("Default")
	}

	pub fn is_active(&self) -> bool {
		self.stream.is_some()
	}

	pub fn latest_beat(&self) -> (Instant, f32) {
		(self.last_beat, self.last_beat_scale)
	}
}

impl Default for SystemBeat {
	fn default() -> Self {
		Self::new(None, false)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn disabled_capture_stays_off_when_selecting_a_device() {
		let mut beat = SystemBeat::new(None, false);
		assert!(!beat.is_active());
		beat.handle_command(&Command::SetAudioDevice(Some(
			"unavailable test device".into(),
		)));
		assert!(!beat.enabled);
		assert!(!beat.is_active());
		assert_eq!(beat.selected_device_label(), "unavailable test device");
	}

	#[test]
	fn disabling_capture_discards_buffered_samples_and_last_beat() {
		let mut beat = SystemBeat::new(None, false);
		// Simulate detection state without opening an audio device in the test.
		beat.enabled = true;
		beat.sample_buffer.push(1.0);
		beat.energy_history.fill(1.0);
		beat.last_beat_scale = 2.0;
		let old_sender = beat.sample_tx.clone();
		old_sender.try_send(vec![1.0; WINDOW_SIZE]).unwrap();
		beat.handle_command(&Command::SetBeatPulseEnabled(false));
		beat.poll();
		assert!(!beat.is_active());
		assert!(!beat.enabled);
		assert!(beat.sample_buffer.is_empty());
		assert!(beat.energy_history.iter().all(|energy| *energy == 0.0));
		assert_eq!(beat.latest_beat().1, 0.0);
		assert!(old_sender.try_send(vec![1.0]).is_err());
	}
}
