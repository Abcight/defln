pub mod event;
pub mod queue;
pub mod scheduler;

pub use event::{
	BeatEvent, BreathingEvent, BrowserEvent, ComponentResponse, Event, GatewayEvent,
	MediaEvent, SettingsEvent, SourceEvent,
};
pub use queue::EventQueue;
pub use scheduler::Scheduler;

use crate::beat::SystemBeat;
use crate::breathing::BreathingOverlay;
use crate::browser::ContentBrowser;
use crate::coach::CoachManager;
use crate::gateway::BooruGateway;
use crate::media::MediaPane;
use crate::settings::SettingsManager;
use crate::view::{ApplicationState, View, Views};
use eframe::egui;

pub struct Reactor {
	queue: EventQueue,
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
		let settings = crate::config::load_settings();

		let mut reactor = Self {
			queue: EventQueue::new(),
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
				std::time::Duration::from_secs_f32(settings.auto_play_delay_secs),
				settings.cap_by_breathing,
			),
			beat: SystemBeat::new(settings.selected_audio_device),
			coach: None,
		};

		if settings.coach_enabled
			&& let (Some(m), Some(p), Some(mdir), Some(pdir)) = (
				&settings.coach_model,
				&settings.coach_preset,
				crate::config::get_models_dir(),
				crate::config::get_presets_dir(),
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
		for e in response.events {
			self.queue.push(e);
		}
		for (e, d) in response.scheduled {
			self.scheduler.schedule(e, d);
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

		// Process event queue until empty
		let mut iterations = 0;
		while let Some(event) = self.queue.pop() {
			log::trace!("Processing event: {:?}", event);
			let response = self.route(&event);
			self.process_response(response);

			iterations += 1;
			if iterations > 1000 {
				log::warn!("Event loop exceeded 1000 iterations, breaking");
				break;
			}
		}

		// Render
		let events = self.views.render(
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

		// Process any events from rendering immediately
		for event in events {
			log::trace!("Processing render event: {:?}", event);
			let response = self.route(&event);
			self.process_response(response);
		}
	}

	fn route(&mut self, event: &Event) -> ComponentResponse {
		let mut response;

		match event {
			Event::Source(e) => response = self.handle_source(e),
			Event::Gateway(_) => response = self.gateway.handle(event),
			Event::Browser(b) => {
				response = self.browser.handle(event);
				if let BrowserEvent::Navigate { direction } = b {
					if let Some(coach) = &self.coach {
						let coach_event = match direction {
							crate::types::NavDirection::Next => {
								crate::coach::CoachEvent::NextImage
							}
							crate::types::NavDirection::Prev => {
								crate::coach::CoachEvent::PrevImage
							}
							crate::types::NavDirection::Skip(s) => {
								if *s > 0 {
									crate::coach::CoachEvent::NextImage
								} else {
									crate::coach::CoachEvent::PrevImage
								}
							}
						};
						coach.send_event(coach_event);
					}
					let settings_res = self.settings.handle(event, &self.breathing);
					response.events.extend(settings_res.events);
					response.scheduled.extend(settings_res.scheduled);
				}
			}
			Event::Media(_) => response = self.media.handle(event),
			Event::Beat(_) => response = self.beat.handle(event),
			Event::Breathing(b) => {
				response = self.breathing.handle(event);
				if let BreathingEvent::PhaseStarted(p) = b {
					if let Some(coach) = &self.coach {
						coach.send_event(crate::coach::CoachEvent::PhaseChange(
							format!("{:?}", p),
						));
					}
					// Route PhaseStarted to settings as well
					let settings_res = self.settings.handle(event, &self.breathing);
					response.events.extend(settings_res.events);
					response.scheduled.extend(settings_res.scheduled);
				}
			}
			Event::Settings(_) => {
				response = self.settings.handle(event, &self.breathing)
			}
		}

		response
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

		let (Some(model), Some(preset), Some(models_dir), Some(presets_dir)) = (
			model,
			preset,
			crate::config::get_models_dir(),
			crate::config::get_presets_dir(),
		) else {
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

	fn handle_source(&mut self, event: &SourceEvent) -> ComponentResponse {
		match event {
			SourceEvent::Search { query, page } => {
				log::info!("Source search: query='{}', page={}", query, page);
				ComponentResponse::emit(Event::Gateway(GatewayEvent::SearchRequest {
					query: query.clone(),
					page: *page,
					limit: 50,
				}))
			}
			SourceEvent::Navigate(direction) => {
				log::debug!("Source navigate: {:?}", direction);
				ComponentResponse::emit(Event::Browser(BrowserEvent::Navigate {
					direction: *direction,
				}))
			}
			SourceEvent::ConfigureCoach {
				enabled,
				model,
				preset,
			} => {
				self.configure_coach(*enabled, model.clone(), preset.clone());
				ComponentResponse::none()
			}
		}
	}
}

impl eframe::App for Reactor {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		self.tick(ctx);
	}

	fn save(&mut self, _storage: &mut dyn eframe::Storage) {
		let saved = crate::config::SavedSettings {
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
		crate::config::save_settings(&saved);
	}
}
