pub mod message;
pub mod queue;
pub mod scheduler;

pub use message::{Command, ComponentResponse, Event, Message, ViewOutput};
pub use queue::MessageQueue;
pub use scheduler::Scheduler;

use crate::beat::SystemBeat;
use crate::breathing::BreathingOverlay;
use crate::browser::ContentBrowser;
use crate::coach::{CoachEvent, CoachManager};
use crate::config::{
	SavedSettings, get_models_dir, get_presets_dir, load_settings, save_settings,
};
use crate::gateway::BooruGateway;
use crate::media::MediaPane;
use crate::settings::SettingsManager;
use crate::types::NavDirection;
use crate::view::{ApplicationState, View, Views};
use eframe::{App, Frame, egui};
use std::time::Duration;

pub struct Reactor {
	queue: MessageQueue,
	scheduler: Scheduler,

	pub gateway: BooruGateway,
	pub browser: ContentBrowser,
	pub media: MediaPane,
	pub breathing: BreathingOverlay,
	pub views: Views,
	pub settings: SettingsManager,
	pub beat: SystemBeat,
	pub coach: Option<CoachManager>,
}

impl Reactor {
	pub fn new(ctx: &egui::Context) -> Self {
		log::info!("Initializing all components");
		let settings = load_settings();

		let mut reactor = Self {
			queue: MessageQueue::new(),
			scheduler: Scheduler::new(),
			gateway: BooruGateway::new(),
			browser: ContentBrowser::new(),
			media: MediaPane::new(ctx),
			breathing: BreathingOverlay::new(
				false, // Breathing always starts off
				settings.breathing_phase_multipliers(),
				settings.breathing_style,
			),
			views: Views::new(
				settings.search_query,
				settings.search_query_presets,
				settings.search_page_input,
				settings.auto_pan_cycle_duration,
				settings.beat_pulse_enabled,
				settings.beat_pulse_scale,
				settings.image_fill_mode,
				settings.coach_enabled,
				settings.coach_model.clone(),
				settings.coach_preset.clone(),
			),
			settings: SettingsManager::new(
				settings.auto_play,
				Duration::from_secs_f32(settings.auto_play_delay_secs),
				settings.cap_by_breathing,
			),
			beat: SystemBeat::new(settings.selected_audio_device),
			coach: None,
		};

		if settings.coach_enabled
			&& let (Some(m), Some(p), Some(mdir), Some(pdir)) = (
				&settings.coach_model,
				&settings.coach_preset,
				get_models_dir(),
				get_presets_dir(),
			) {
			let m_path = mdir.join(m);
			let p_path = pdir.join(p);
			if m_path.exists() && p_path.exists() {
				reactor.coach = Some(CoachManager::new(m_path, p_path));
			}
		}

		// Initialize all components
		reactor.process_response(reactor.breathing.init());
		log::info!("Initialization complete");

		reactor
	}

	fn process_response(&mut self, response: ComponentResponse) {
		for message in response.messages {
			self.queue.push(message);
		}
		for (message, delay) in response.scheduled {
			self.scheduler.schedule(message, delay);
		}
	}

	pub fn tick(&mut self, ctx: &egui::Context) {
		// Drain scheduled events
		self.scheduler.tick(&mut self.queue);

		// Poll async components
		let gateway_response = self.gateway.poll();
		let media_response = self.media.poll();
		let beat_response = self.beat.poll();
		self.process_response(gateway_response);
		self.process_response(media_response);
		self.process_response(beat_response);

		if let Some(coach) = &mut self.coach {
			coach.poll();
		}

		self.drain_queue();

		// Render
		let output = self.views.render(
			ctx,
			&ApplicationState {
				gateway: &self.gateway,
				browser: &self.browser,
				media: &self.media,
				breathing: &self.breathing,
				settings: &self.settings,
				beat: &self.beat,
				coach: self.coach.as_ref(),
			},
		);

		// All views observed the same state; only now may their output take effect.
		for message in output.messages {
			self.queue.push(message);
		}
		self.drain_queue();
	}

	fn drain_queue(&mut self) {
		let mut iterations = 0;
		while let Some(message) = self.queue.pop() {
			log::trace!("Processing message: {:?}", message);
			let response = self.process_message(&message);
			self.process_response(response);

			iterations += 1;
			if iterations > 1000 {
				log::warn!("Event loop exceeded 1000 iterations, breaking");
				break;
			}
		}
	}

	fn process_message(&mut self, message: &Message) -> ComponentResponse {
		match message {
			Message::Command(command) => self.dispatch(command),
			Message::Event(event) => self.publish(event),
		}
	}

	fn dispatch(&mut self, command: &Command) -> ComponentResponse {
		match command {
			Command::Search { .. } | Command::FetchNextPage => {
				self.gateway.handle_command(command)
			}
			Command::Navigate(_) => self.browser.handle(command),
			Command::LoadMedia { .. } | Command::PrefetchMedia { .. } => {
				self.media.handle_command(command)
			}
			Command::ToggleBreathing
			| Command::CompleteBreathingPhase
			| Command::SetBreathingPhaseMultiplier { .. }
			| Command::SetBreathingStyle(_) => self.breathing.handle_command(command),
			Command::ToggleAutoPlay
			| Command::SetAutoPlayDelay(_)
			| Command::AdjustAutoPlayDelay(_)
			| Command::AdvanceSlideshow
			| Command::ToggleCapByBreathing => {
				self.settings.handle_command(command, &self.breathing)
			}
			Command::SetAudioDevice(_) => self.beat.handle_command(command),
			Command::ConfigureCoach {
				enabled,
				model,
				preset,
			} => {
				self.configure_coach(*enabled, model.clone(), preset.clone());
				ComponentResponse::none()
			}
		}
	}

	fn publish(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::SearchCompleted { .. } => self.browser.observe(event),
			Event::Navigated(direction) => {
				if let Some(coach) = &self.coach {
					let event = match direction {
						NavDirection::Next => CoachEvent::NextImage,
						NavDirection::Prev => CoachEvent::PrevImage,
						NavDirection::Skip(skip) if *skip < 0 => {
							CoachEvent::PrevImage
						}
						NavDirection::Skip(_) => CoachEvent::NextImage,
					};
					coach.send_event(event);
				}
				self.settings.observe(event, &self.breathing)
			}
			Event::BreathingPhaseStarted(phase) => {
				if let Some(coach) = &self.coach {
					coach.send_event(CoachEvent::PhaseChange(format!("{:?}", phase)));
				}
				self.settings.observe(event, &self.breathing)
			}
			Event::MediaPainted => self.media.observe(event),
		}
	}

	fn configure_coach(
		&mut self,
		enabled: bool,
		model: Option<String>,
		preset: Option<String>,
	) {
		self.coach = None;
		if !enabled {
			return;
		}

		let (Some(model), Some(preset), Some(models_dir), Some(presets_dir)) =
			(model, preset, get_models_dir(), get_presets_dir())
		else {
			return;
		};

		let model_path = models_dir.join(model);
		let preset_path = presets_dir.join(preset);
		if model_path.is_file() && preset_path.is_file() {
			self.coach = Some(CoachManager::new(model_path, preset_path));
		} else {
			log::warn!("Coach model or preset is unavailable");
		}
	}
}

impl App for Reactor {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
		self.tick(ctx);
	}

	fn save(&mut self, _storage: &mut dyn eframe::Storage) {
		let saved = SavedSettings {
			search_query: self.views.search_query.clone(),
			search_query_presets: self.views.search_query_presets.clone(),
			search_page_input: self.views.search_page_input.clone(),
			auto_play: self.settings.auto_play(),
			auto_play_delay_secs: self.settings.auto_play_delay().as_secs_f32(),
			cap_by_breathing: self.settings.cap_by_breathing(),
			breathing_prepare_multiplier: self.breathing.phase_multipliers().prepare,
			breathing_inhale_multiplier: self.breathing.phase_multipliers().inhale,
			breathing_hold_multiplier: self.breathing.phase_multipliers().hold,
			breathing_release_multiplier: self.breathing.phase_multipliers().release,
			breathing_idle_multiplier: self.breathing.phase_multipliers().idle,
			breathing_style: self.breathing.style(),
			auto_pan_cycle_duration: self.views.auto_pan_cycle_duration,
			selected_audio_device: self.beat.selected_device().clone(),
			beat_pulse_enabled: self.views.beat_pulse_enabled,
			beat_pulse_scale: self.views.beat_pulse_scale,
			image_fill_mode: self.views.image_fill_mode,
			coach_enabled: self.views.coach_enabled,
			coach_model: self.views.coach_model.clone(),
			coach_preset: self.views.coach_preset.clone(),
		};
		save_settings(&saved);
	}
}
