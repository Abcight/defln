use eframe::egui::{Button, Margin, PointerButton, Rect, Sense, ViewportCommand};

use super::*;
pub(super) struct TopBarView {
	pub(super) search_query: String,
	pub(super) search_query_presets: Vec<String>,
	pub(super) search_page_input: String,
	pub(super) selected_search_query_preset: Option<String>,
	pub(super) query_selector_open: bool,
	last_rect: Rect,
}

impl TopBarView {
	pub(super) fn new(settings: &SettingsManager) -> Self {
		let search_query = settings.search_query().to_owned();
		let search_query_presets =
			normalize_search_query_presets(settings.search_query_presets().to_vec());
		let selected_search_query_preset = search_query_presets
			.iter()
			.find(|preset| **preset == search_query)
			.cloned();
		Self {
			search_query,
			search_query_presets,
			search_page_input: settings.search_page_input().to_owned(),
			selected_search_query_preset,
			query_selector_open: false,
			last_rect: Rect::ZERO,
		}
	}

	pub(super) fn render(
		&mut self,
		ui: &mut Ui,
		state: &ApplicationState<'_>,
		modal: &mut ModalView,
		output: &mut ViewOutput,
		enabled: bool,
		embedded_decorations: bool,
	) {
		egui::TopBottomPanel::top("top_panel").show_inside(ui, |ui| {
			if embedded_decorations {
				let bar_response = ui.interact(
					self.last_rect,
					"topbar_interact".into(),
					Sense::click_and_drag(),
				);

				if bar_response.double_clicked() {
					let is_maximized =
						ui.input(|i| i.viewport().maximized.unwrap_or(false));
					ui.ctx()
						.send_viewport_cmd(ViewportCommand::Maximized(!is_maximized));
				}

				if bar_response.drag_started_by(PointerButton::Primary) {
					ui.ctx().stop_dragging();
					ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
					ui.ctx()
						.input_mut(|input| input.pointer = Default::default());
				}

				self.last_rect = egui::Frame::none()
					.inner_margin(Margin {
						left: 2.0,
						right: 2.0,
						top: 8.0,
						bottom: 8.0,
					})
					.show(ui, |ui| {
						ui.with_layout(
							egui::Layout::right_to_left(egui::Align::Center),
							|ui| {
								ui.horizontal(|ui| {
									let maximized = ui.input(|i| {
										i.viewport().maximized.unwrap_or(false)
									});
									for (label, tooltip, command) in [
										("×", "Close", ViewportCommand::Close),
										(
											"□",
											"Resize",
											ViewportCommand::Maximized(!maximized),
										),
										(
											"_",
											"Minimize",
											ViewportCommand::Minimized(true),
										),
									] {
										if ui
											.add(
												Button::new(label)
													.min_size([20.0, 20.0].into()),
											)
											.on_hover_text(tooltip)
											.clicked()
										{
											ui.ctx().send_viewport_cmd(command);
										}
									}
								});

								ui.with_layout(
									egui::Layout::left_to_right(egui::Align::Center),
									|ui| {
										ui.set_clip_rect(
											ui.available_rect_before_wrap(),
										);
										self.render_inner(
											ui, state, modal, output, enabled,
										);
									},
								);
							},
						)
					})
					.response
					.rect;
			} else {
				self.render_inner(ui, state, modal, output, enabled);
			}
		});
	}

	pub(super) fn render_inner(
		&mut self,
		ui: &mut Ui,
		state: &ApplicationState<'_>,
		modal: &mut ModalView,
		output: &mut ViewOutput,
		enabled: bool,
	) {
		let settings = state.settings;
		let breathing = state.breathing;
		let beat = state.beat;

		if !enabled {
			ui.disable();
		}

		ui.horizontal_wrapped(|ui| {
			ui.label("Query:");
			let query_response = self.render_query_selector(ui);
			if self.selected_search_query_preset.is_none() {
				let query = self.search_query.trim();
				let can_save = !query.is_empty()
					&& !self
						.search_query_presets
						.iter()
						.any(|preset| preset == query);

				if ui
					.add_enabled(can_save, egui::Button::new("Save preset"))
					.clicked()
				{
					let preset = query.to_owned();
					self.search_query_presets.push(preset.clone());
					self.selected_search_query_preset = Some(preset);
				}
			}

			ui.label("Page:");
			let page_response = ui.add(
				egui::TextEdit::singleline(&mut self.search_page_input)
					.desired_width(40.0),
			);

			if ui.button("Search").clicked()
				|| (query_response.lost_focus()
					&& ui.input(|i| i.key_pressed(egui::Key::Enter)))
				|| (page_response.lost_focus()
					&& ui.input(|i| i.key_pressed(egui::Key::Enter)))
			{
				let page = self.search_page_input.parse::<u32>().unwrap_or(1).max(1);
				output.command(Command::Search {
					query: self.search_query.clone(),
					page,
				});
			}
			ui.separator();

			ui.label("Quick settings:");

			let mut auto_play = settings.auto_play();
			if ui.checkbox(&mut auto_play, "Auto-play").changed() {
				output.command(Command::ToggleAutoPlay);
			}

			let mut cap_by_breathing = settings.cap_by_breathing();
			if ui
				.checkbox(&mut cap_by_breathing, "Sync with Breathing")
				.changed()
			{
				output.command(Command::ToggleCapByBreathing);
			}

			if settings.auto_play() {
				let mut seconds = settings.auto_play_delay().as_secs_f32();
				ui.label("Interval (s)");
				if ui
					.add(
						egui::DragValue::new(&mut seconds)
							.range(1.0..=60.0)
							.speed(1.0),
					)
					.changed()
				{
					output.command(Command::SetAutoPlayDelay(
						Duration::from_secs_f32(seconds),
					));
				}
			}

			ui.separator();

			let mut breathing_enabled = breathing.is_visible();

			if ui.checkbox(&mut breathing_enabled, "Breathing").clicked() {
				if breathing_enabled && !modal.breathing_disclaimer_accepted {
					modal.modal = ModalContent::BreathingDisclaimer;
				} else {
					output.command(Command::ToggleBreathing);
				}
			}

			if breathing_enabled {
				let multipliers = breathing.phase_multipliers();
				for (label, phase, mut multiplier) in [
					("Prep", BreathingPhase::Prepare, multipliers.prepare),
					("Inhale", BreathingPhase::Inhale, multipliers.inhale),
					("Hold", BreathingPhase::Hold, multipliers.hold),
					("Release", BreathingPhase::Release, multipliers.release),
					("Idle", BreathingPhase::Idle, multipliers.idle),
				] {
					ui.label(label);
					if ui
						.add(
							egui::DragValue::new(&mut multiplier)
								.range(0.5..=3.0)
								.speed(0.1),
						)
						.changed()
					{
						output.command(Command::SetBreathingPhaseMultiplier {
							phase,
							value: multiplier,
						});
					}
				}

				let current_style = breathing.style();
				let style_label = match current_style {
					BreathingStyle::Classic => "Classic",
					BreathingStyle::Immersive => "Immersive",
				};
				egui::ComboBox::from_id_salt("breathing_style")
					.selected_text(style_label)
					.show_ui(ui, |ui| {
						if ui
							.selectable_label(
								current_style == BreathingStyle::Classic,
								"Classic",
							)
							.clicked()
						{
							output.command(Command::SetBreathingStyle(
								BreathingStyle::Classic,
							));
						}
						if ui
							.selectable_label(
								current_style == BreathingStyle::Immersive,
								"Immersive",
							)
							.clicked()
						{
							output.command(Command::SetBreathingStyle(
								BreathingStyle::Immersive,
							));
						}
					});
			}

			ui.separator();

			let mut pan_speed = settings.auto_pan_cycle_duration();
			ui.label("Pan Speed (s)");
			if ui
				.add(
					egui::DragValue::new(&mut pan_speed)
						.range(10.0..=120.0)
						.speed(1.0),
				)
				.changed()
			{
				output.command(Command::SetAutoPanCycleDuration(pan_speed));
			}
			ui.separator();

			let current_fill = settings.image_fill_mode();
			let fill_label = match current_fill {
				ImageFillMode::Cover => "Cover",
				ImageFillMode::Fit => "Fit",
				ImageFillMode::FitToGallery => "Fit to Gallery",
			};
			egui::ComboBox::from_id_salt("image_fill_mode")
				.selected_text(fill_label)
				.show_ui(ui, |ui| {
					if ui
						.selectable_label(
							current_fill == ImageFillMode::Cover,
							"Cover",
						)
						.clicked()
					{
						output
							.command(Command::SetImageFillMode(ImageFillMode::Cover));
					}
					if ui
						.selectable_label(current_fill == ImageFillMode::Fit, "Fit")
						.clicked()
					{
						output.command(Command::SetImageFillMode(ImageFillMode::Fit));
					}
					if ui
						.selectable_label(
							current_fill == ImageFillMode::FitToGallery,
							"Fit to Gallery",
						)
						.clicked()
					{
						output.command(Command::SetImageFillMode(
							ImageFillMode::FitToGallery,
						));
					}
				});

			ui.separator();

			ui.label("Audio:");
			let selected_label = beat.selected_device_label();
			egui::ComboBox::from_id_salt("audio_device")
				.selected_text(selected_label)
				.show_ui(ui, |ui| {
					if ui
						.selectable_label(beat.selected_device().is_none(), "Default")
						.clicked()
					{
						output.command(Command::SetAudioDevice(None));
					}
					for device_name in beat.device_names() {
						let is_selected = beat.selected_device().as_deref()
							== Some(device_name.as_str());
						if ui.selectable_label(is_selected, device_name).clicked() {
							output.command(Command::SetAudioDevice(Some(
								device_name.clone(),
							)));
						}
					}
				});
			let (audio_color, audio_status) = if !settings.beat_pulse_enabled() {
				(
					egui::Color32::GRAY,
					"Audio capture is off. Enable Pulse to discover input devices.",
				)
			} else if beat.is_active() {
				(egui::Color32::GREEN, "Audio capture is active.")
			} else {
				(egui::Color32::RED, "Audio input is unavailable.")
			};
			ui.label(egui::RichText::new("*").color(audio_color).size(10.0))
				.on_hover_text(audio_status);

			let mut beat_pulse_enabled = settings.beat_pulse_enabled();
			if ui.checkbox(&mut beat_pulse_enabled, "Pulse").changed() {
				output.command(Command::SetBeatPulseEnabled(beat_pulse_enabled));
			}
			if beat_pulse_enabled {
				ui.label("Scale");
				let mut beat_pulse_scale = settings.beat_pulse_scale();
				if ui
					.add(
						egui::DragValue::new(&mut beat_pulse_scale)
							.range(0.01..=0.15)
							.speed(0.01),
					)
					.changed()
				{
					output.command(Command::SetBeatPulseScale(beat_pulse_scale));
				}
			}
		});
	}

	fn render_query_selector(&mut self, ui: &mut egui::Ui) -> egui::Response {
		if self
			.selected_search_query_preset
			.as_ref()
			.is_some_and(|preset| {
				preset != &self.search_query
					|| !self.search_query_presets.contains(preset)
			}) {
			self.selected_search_query_preset = None;
		}

		let query_response = ui.add(
			egui::TextEdit::singleline(&mut self.search_query)
				.desired_width(320.0)
				.hint_text("Enter a query"),
		);
		if query_response.changed() {
			self.selected_search_query_preset = None;
		}
		if query_response.has_focus() {
			self.query_selector_open = true;
		}
		let control_rect = query_response.rect;

		if !self.query_selector_open {
			return query_response;
		}

		let mut preset_to_select = None;
		let mut preset_to_delete = None;
		let mut preset_to_move = None;
		let presets = self.search_query_presets.clone();
		let popup = egui::Area::new(egui::Id::new("query_selector_popup"))
			.order(egui::Order::Foreground)
			.fixed_pos(control_rect.left_bottom() + egui::vec2(0.0, 4.0))
			.show(ui.ctx(), |ui| {
				egui::Frame::popup(ui.style())
					.inner_margin(egui::Margin::same(8.0))
					.show(ui, |ui| {
						ui.set_width(control_rect.width().max(360.0));
						TableBuilder::new(ui)
							.id_salt("query_presets_table")
							.striped(true)
							.cell_layout(
								egui::Layout::left_to_right(egui::Align::Center)
									.with_main_align(egui::Align::Min),
							)
							.column(Column::remainder())
							.column(Column::exact(32.0))
							.column(Column::exact(32.0))
							.column(Column::exact(32.0))
							.min_scrolled_height(0.0)
							.max_scroll_height(240.0)
							.body(|mut body| {
								for (index, preset) in presets.iter().enumerate() {
									body.row(32.0, |mut row| {
										row.col(|ui| {
											let selected =
												self.selected_search_query_preset
													.as_deref() == Some(preset.as_str());
											let button_size = egui::vec2(
												ui.available_width(),
												28.0,
											);
											let response = ui
												.allocate_ui_with_layout(
													button_size,
													egui::Layout::left_to_right(
														egui::Align::Center,
													)
													.with_main_align(
														egui::Align::Min,
													),
													|ui| {
														ui.add(
															egui::Button::new(preset)
																.selected(selected)
																.min_size(
																	button_size,
																),
														)
													},
												)
												.inner;
											if response.clicked() {
												preset_to_select =
													Some(preset.clone());
											}
										});
										row.col(|ui| {
											if ui
												.add_enabled_ui(index > 0, |ui| {
													ui.add_sized(
														[ui.available_width(), 28.0],
														egui::Button::new("⬆"),
													)
												})
												.inner
												.on_hover_text("Move preset up")
												.clicked()
											{
												preset_to_move =
													Some((index, index - 1));
											}
										});
										row.col(|ui| {
											if ui
												.add_enabled_ui(
													index + 1 < presets.len(),
													|ui| {
														ui.add_sized(
															[
																ui.available_width(),
																28.0,
															],
															egui::Button::new("⬇"),
														)
													},
												)
												.inner
												.on_hover_text("Move preset down")
												.clicked()
											{
												preset_to_move =
													Some((index, index + 1));
											}
										});
										row.col(|ui| {
											if ui
												.add_sized(
													[ui.available_width(), 28.0],
													egui::Button::new("🗑"),
												)
												.on_hover_text("Delete preset")
												.clicked()
											{
												preset_to_delete = Some(index);
											}
										});
									});
								}
							});
					});
			});

		if ui.ctx().input(|input| input.pointer.any_pressed())
			&& ui.ctx().input(|input| {
				input.pointer.interact_pos().is_some_and(|position| {
					!control_rect.contains(position)
						&& !popup.response.rect.contains(position)
				})
			}) {
			self.query_selector_open = false;
		}

		if let Some(index) = preset_to_delete {
			let deleted_preset = self.search_query_presets.remove(index);
			if self.selected_search_query_preset.as_deref()
				== Some(deleted_preset.as_str())
			{
				self.selected_search_query_preset = None;
			}
		}
		if let Some((from, to)) = preset_to_move {
			self.search_query_presets.swap(from, to);
		}

		if let Some(preset) = preset_to_select {
			self.search_query = preset.clone();
			self.selected_search_query_preset = Some(preset);
			query_response.request_focus();
		}

		query_response
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
