//! Components own application state and work; views own immediate-mode layout
//! and interaction.
//!
//! Components may prepare resources required for presentation, for example,
//! they can manage the downloading and storing of media from the internet.
//!
//! Components must not build UI layout or handle widgets.
//!
//! Views communicate changes only through commands and events, which the
//! reactor applies after rendering.

pub mod message;
pub mod queue;
pub mod scheduler;

pub use message::{Command, ComponentResponse, Event, Message, ViewOutput};
pub use queue::MessageQueue;
pub use scheduler::Scheduler;

use crate::beat::SystemBeat;
use crate::breathing::BreathingOverlay;
use crate::browser::ContentBrowser;
use crate::config::{SavedSettings, load_settings, save_settings};
use crate::gateway::BooruGateway;
use crate::media::MediaPane;
use crate::settings::SettingsManager;
use crate::view::{ApplicationState, View, Views};
use eframe::{App, Frame, egui};

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
}

impl Reactor {
	pub fn new(ctx: &egui::Context) -> Self {
		log::info!("Initializing all components");
		let settings = load_settings();
		let settings_manager = SettingsManager::from_saved(&settings);

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
			views: Views::new(&settings_manager),
			settings: settings_manager,
			beat: SystemBeat::new(
				settings.selected_audio_device,
				settings.beat_pulse_enabled,
			),
		};

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
			},
		);

		// All views observed the same state; only now may their output take effect.
		for message in output.into_messages() {
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
			| Command::ToggleCapByBreathing
			| Command::SetSearchPreferences { .. }
			| Command::SetAutoPanCycleDuration(_)
			| Command::SetBeatPulseScale(_)
			| Command::SetImageFillMode(_) => {
				self.settings.handle_command(command, &self.breathing)
			}
			Command::SetBeatPulseEnabled(_) => {
				self.beat.handle_command(command);
				self.settings.handle_command(command, &self.breathing)
			}
			Command::SetAudioDevice(_) => self.beat.handle_command(command),
		}
	}

	fn publish(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::SearchCompleted { .. } => self.browser.observe(event),
			Event::Navigated | Event::BreathingPhaseStarted(_) => {
				self.settings.observe(event, &self.breathing)
			}
			Event::MediaPainted => self.media.observe(event),
		}
	}
}

impl App for Reactor {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
		self.tick(ctx);
		ctx.request_repaint_after(std::time::Duration::from_secs(1));
	}

	fn save(&mut self, _storage: &mut dyn eframe::Storage) {
		let saved = SavedSettings {
			search_query: self.settings.search_query().to_owned(),
			search_query_presets: self.settings.search_query_presets().to_vec(),
			search_page_input: self.settings.search_page_input().to_owned(),
			auto_play: self.settings.auto_play(),
			auto_play_delay_secs: self.settings.auto_play_delay().as_secs_f32(),
			cap_by_breathing: self.settings.cap_by_breathing(),
			breathing_prepare_multiplier: self.breathing.phase_multipliers().prepare,
			breathing_inhale_multiplier: self.breathing.phase_multipliers().inhale,
			breathing_hold_multiplier: self.breathing.phase_multipliers().hold,
			breathing_release_multiplier: self.breathing.phase_multipliers().release,
			breathing_idle_multiplier: self.breathing.phase_multipliers().idle,
			breathing_style: self.breathing.style(),
			auto_pan_cycle_duration: self.settings.auto_pan_cycle_duration(),
			selected_audio_device: self.beat.selected_device().clone(),
			beat_pulse_enabled: self.settings.beat_pulse_enabled(),
			beat_pulse_scale: self.settings.beat_pulse_scale(),
			image_fill_mode: self.settings.image_fill_mode(),
		};
		save_settings(&saved);
	}
}
