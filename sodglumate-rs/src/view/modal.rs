use super::*;
#[derive(Clone)]
pub(super) enum ModalContent {
	None,
	Hello,
	BreathingDisclaimer,
}

pub(super) struct ModalView {
	pub(super) user_is_adult: bool,
	pub(super) user_accepted_tos: bool,
	pub(super) modal: ModalContent,
	pub(super) breathing_disclaimer_accepted: bool,
	pub(super) breathing_disclaimer_checked: bool,
}

impl ModalView {
	pub(super) fn new() -> Self {
		Self {
			user_is_adult: false,
			user_accepted_tos: false,
			modal: ModalContent::Hello,
			breathing_disclaimer_accepted: false,
			breathing_disclaimer_checked: false,
		}
	}

	pub(super) fn render(&mut self, ctx: &egui::Context, output: &mut ViewOutput) {
		if matches!(self.modal, ModalContent::None) {
			return;
		}

		let screen_rect = ctx.screen_rect();

		// Draw semi-transparent dark overlay
		egui::Area::new(egui::Id::new("modal_backdrop"))
			.fixed_pos(screen_rect.min)
			.order(egui::Order::Foreground)
			.show(ctx, |ui| {
				let painter = ui.painter();
				painter.rect_filled(
					screen_rect,
					0.0,
					egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180),
				);
			});

		// Draw centered popup window
		egui::Window::new("popup_modal")
			.title_bar(false)
			.resizable(false)
			.collapsible(false)
			.anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
			.order(egui::Order::Foreground)
			.show(ctx, |ui| {
				ui.set_width(450.0);
				ui.vertical_centered(|ui| match &self.modal.clone() {
					ModalContent::Hello => {
						ui.add_space(10.0);
						ui.heading("Welcome! Please read the Terms of Use.");
						ui.label("Make sure you are of legal age to view this content.");
						ui.add_space(10.0);

						// Framed ScrollArea for legal text
						egui::Frame::none()
							.fill(egui::Color32::from_gray(40))
							.inner_margin(12.0)
							.rounding(4.0)
							.show(ui, |ui| {
								ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
									ui.with_layout(
										egui::Layout::top_down(egui::Align::LEFT),
										|ui| {
											text_utils::render_rich_text(ui, include_str!("resources/legal.txt"));
										},
									);
								});
							});

						ui.add_space(10.0);
						ui.label("If you do not meet these requirements or do not agree to these terms, you must not access or use the Application.");
						ui.add_space(10.0);

						ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
							ui.checkbox(&mut self.user_is_adult, "I am 18 years of age or older.");
							ui.checkbox(
								&mut self.user_accepted_tos,
								"I have read and accept the Terms of Use.",
							);
						});

						ui.add_space(10.0);

						ui.horizontal(|ui| {
							if ui.button("   Decline   ").clicked() {
								std::process::exit(0);
							}
							ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
								if !self.user_accepted_tos || !self.user_is_adult {
									ui.disable();
								}
								if ui.button("   Enter   ").clicked() {
									self.modal = ModalContent::None;
								}
							});
						});
					}
					ModalContent::BreathingDisclaimer => {
						ui.add_space(10.0);
						ui.heading("Breathing Disclaimer");
						ui.label("Please read the disclaimer below before using this functionality.");
						ui.add_space(10.0);

						egui::Frame::none()
							.fill(egui::Color32::from_gray(40))
							.inner_margin(12.0)
							.rounding(4.0)
							.show(ui, |ui| {
								ScrollArea::vertical()
									.scroll_bar_visibility(
										egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
									)
									.max_height(200.0)
									.show(ui, |ui| {
										ui.set_min_width(ui.available_width());
										ui.with_layout(
											egui::Layout::top_down(egui::Align::LEFT),
											|ui| {
												text_utils::render_rich_text(
													ui,
													include_str!("resources/breathing.txt"),
												);
											},
										);
									});
							});

						ui.add_space(10.0);
						ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
							ui.checkbox(
								&mut self.breathing_disclaimer_checked,
								"I understand the above disclaimer and proceed at my own risk.",
							);
						});
						ui.add_space(10.0);

						ui.horizontal(|ui| {
							if ui.button("   Decline   ").clicked() {
								self.modal = ModalContent::None;
								self.breathing_disclaimer_checked = false;
							}
							ui.with_layout(
								egui::Layout::right_to_left(egui::Align::Center),
								|ui| {
									if !self.breathing_disclaimer_checked {
										ui.disable();
									}
									if ui.button("   Accept   ").clicked() {
										self.breathing_disclaimer_accepted = true;
										self.modal = ModalContent::None;
										output.command(Command::ToggleBreathing);
									}
								},
							);
						});
					},
					ModalContent::None => {}
				});
			});
	}
}
