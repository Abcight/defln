//! Immediate-mode presentation layer.
//
//! A view and the operational component whose state it displays are deliberately
//! separate objects, even when they currently have a one-to-one relationship.
//!
//! Views may read application state and emit commands or events, but must not
//! perform networking, decoding, audio capture, background work, or directly
//! mutate components.
//!
//! Keep interaction and rendering state here; keep durable application state
//! and work in components.
//!
//! Do not merge a view into a component as a convenience.

use crate::beat::SystemBeat;
use crate::breathing::BreathingOverlay;
use crate::browser::ContentBrowser;
use crate::gateway::{BooruGateway, SearchStatus};
use crate::media::MediaPane;
use crate::reactor::{Command, Event, ViewOutput};
use crate::settings::SettingsManager;
use crate::types::{BreathingPhase, BreathingStyle, ImageFillMode, NavDirection};
use eframe::egui::{self, LayerId, ScrollArea, Ui};
use egui_extras::{Column, TableBuilder};
use std::time::{Duration, Instant};

pub mod island;
pub mod text_utils;

mod beat_overlay;
mod content_overlay;
mod media;
mod modal;
mod navigation;
mod top_bar;

use beat_overlay::BeatOverlayView;
use content_overlay::ContentOverlayView;
use island::{IslandAction, IslandCtx, IslandWidget, ROOT_ISLAND};
use media::MediaView;
use modal::{ModalContent, ModalView};
use navigation::IslandNavigationView;
use top_bar::TopBarView;

/// Read-only access to operational application state during a UI pass.
///
/// Views may combine data from any number of components, but can only affect
/// them by returning commands or events after the complete UI pass has finished.
pub struct ApplicationState<'a> {
	pub gateway: &'a BooruGateway,
	pub browser: &'a ContentBrowser,
	pub media: &'a MediaPane,
	pub breathing: &'a BreathingOverlay,
	pub settings: &'a SettingsManager,
	pub beat: &'a SystemBeat,
}

pub trait View {
	fn render(
		&mut self,
		ctx: &egui::Context,
		state: &ApplicationState<'_>,
	) -> ViewOutput;
}

/// Sequential compositor for independent visual elements.
pub struct Views {
	top_bar: TopBarView,
	media: MediaView,
	modal: ModalView,
	island_navigation: IslandNavigationView,
	beat_overlay: BeatOverlayView,
	content_overlay: ContentOverlayView,
}

impl Views {
	pub fn new(settings: &SettingsManager) -> Self {
		Self {
			top_bar: TopBarView::new(settings),
			media: MediaView::new(),
			modal: ModalView::new(),
			island_navigation: IslandNavigationView::new(),
			beat_overlay: BeatOverlayView::new(),
			content_overlay: ContentOverlayView,
		}
	}

	fn render_frame(
		&mut self,
		ctx: &egui::Context,
		state: &ApplicationState<'_>,
	) -> ViewOutput {
		use egui::UiBuilder;

		let layer_id = LayerId::background();
		let available_rect = ctx.available_rect();
		let mut ui = Ui::new(
			ctx.clone(),
			layer_id,
			"global_egui_frame".into(),
			UiBuilder::new().max_rect(available_rect),
		);
		ui.set_clip_rect(ctx.screen_rect());

		let panel_frame = egui::Frame::default()
			.fill(ui.style().visuals.window_fill())
			.stroke(ui.style().visuals.widgets.noninteractive.fg_stroke)
			.outer_margin(1.0);

		let view_output = panel_frame
			.show(&mut ui, |ui| {
				let app_rect = ui.max_rect();

				ui.expand_to_include_rect(app_rect);

				let mut content_ui =
					ui.new_child(UiBuilder::new().max_rect(app_rect));
				self.render_frame_inner(&mut content_ui, state)
			})
			.inner;

		view_output
	}

	fn render_frame_inner(
		&mut self,
		mut ui: &mut Ui,
		state: &ApplicationState<'_>,
	) -> ViewOutput {
		let mut output = ViewOutput::default();
		let previous_search_preferences = (
			self.top_bar.search_query.clone(),
			self.top_bar.search_query_presets.clone(),
			self.top_bar.search_page_input.clone(),
		);
		let media_url = state.media.current_url().map(str::to_owned);
		if self.media.last_media_url != media_url {
			self.media.last_media_url = media_url;
			self.media.image_load_time = Instant::now();
			self.media.user_has_panned = false;
			self.media.user_zoom = 1.0;
			self.media.user_pan_offset = egui::Vec2::ZERO;
		}
		let (beat_at, beat_scale) = state.beat.latest_beat();
		if beat_at > self.beat_overlay.last_beat_time && beat_scale > 0.0 {
			self.beat_overlay.last_beat_time = beat_at;
			self.beat_overlay.last_beat_scale = beat_scale;
			self.beat_overlay.beat_intensity = beat_scale;
		}
		let modal_active = !matches!(self.modal.modal, ModalContent::None);

		// Handle input only when no modal is active
		if !modal_active {
			let is_typing = ui.memory(|m| m.focused().is_some());
			if !is_typing {
				self.island_navigation
					.handle_keyboard_input(ui, &mut output);
			}
		}

		// Top panel
		self.top_bar.render(
			&mut ui,
			state,
			&mut self.modal,
			&mut output,
			!modal_active,
			true,
		);

		// Central panel
		let island_active = self.island_navigation.island_ctx.active
			|| self.island_navigation.island_ctx.in_cooldown();
		self.media.render(
			&mut ui,
			state,
			island_active,
			self.beat_overlay.beat_intensity,
			&mut output,
			!modal_active,
		);

		// Overlays
		match state.breathing.style() {
			BreathingStyle::Classic => {
				self.content_overlay
					.render_breathing_overlay(&mut ui, state.breathing);
				self.content_overlay
					.render_breathing_pulse(&mut ui, state.breathing);
			}
			BreathingStyle::Immersive => {
				self.content_overlay
					.render_immersive_breathing_overlay(&mut ui, state.breathing);
			}
		}
		self.content_overlay
			.render_info_overlay(&mut ui, state.browser);

		// Beat debug dot
		self.beat_overlay.render(&mut ui);

		// Island navigation overlay
		self.island_navigation.render(
			&mut ui,
			state.settings,
			&mut self.modal,
			&mut output,
		);

		// Modal popup (on top of everything)
		self.modal.render(&mut ui, &mut output);

		if previous_search_preferences
			!= (
				self.top_bar.search_query.clone(),
				self.top_bar.search_query_presets.clone(),
				self.top_bar.search_page_input.clone(),
			) {
			output.command(Command::SetSearchPreferences {
				query: self.top_bar.search_query.clone(),
				presets: self.top_bar.search_query_presets.clone(),
				page_input: self.top_bar.search_page_input.clone(),
			});
		}

		output
	}
}

impl Default for Views {
	fn default() -> Self {
		Self::new(&SettingsManager::default())
	}
}

impl View for Views {
	fn render(
		&mut self,
		ctx: &egui::Context,
		state: &ApplicationState<'_>,
	) -> ViewOutput {
		self.render_frame(ctx, state)
	}
}
