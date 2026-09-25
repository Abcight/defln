use super::*;
pub(super) struct ContentOverlayView;

impl ContentOverlayView {
	pub(super) fn render_breathing_overlay(
		&self,
		ui: &mut Ui,
		breathing: &BreathingOverlay,
	) {
		if !breathing.is_visible() {
			return;
		}

		let screen_height = ui.ctx().screen_rect().height();
		let font_size = (screen_height * 0.05).max(16.0);
		let margin_offset = -(screen_height * 0.03).max(10.0);

		egui::Area::new(egui::Id::new("breathing_overlay"))
			.anchor(
				egui::Align2::RIGHT_BOTTOM,
				egui::vec2(margin_offset, margin_offset),
			)
			.interactable(false)
			.order(egui::Order::Foreground)
			.show(ui.ctx(), |ui| {
				ui.with_layout(
					egui::Layout::right_to_left(egui::Align::Center),
					|ui| {
						let state = breathing.state();
						let elapsed = state.start_time.elapsed();
						let remaining =
							state.duration.saturating_sub(elapsed).as_secs() + 1;

						let (text, color) = match state.phase {
							BreathingPhase::Prepare => {
								(format!("PREPARE {}", remaining), egui::Color32::RED)
							}
							BreathingPhase::Inhale => {
								("INHALE".to_string(), egui::Color32::YELLOW)
							}
							BreathingPhase::Hold => {
								("HOLD".to_string(), egui::Color32::YELLOW)
							}
							BreathingPhase::Release => {
								("RELEASE".to_string(), egui::Color32::GREEN)
							}
							BreathingPhase::Idle => {
								("".to_string(), egui::Color32::TRANSPARENT)
							}
						};

						if !text.is_empty() {
							let font_id = egui::FontId::monospace(font_size);
							let stroke_width = (font_size * 0.05).max(1.0);
							Self::draw_outlined_text(
								ui,
								&text,
								font_id,
								color,
								stroke_width,
							);
						}
					},
				);
			});
	}

	pub(super) fn render_breathing_pulse(
		&self,
		ui: &mut Ui,
		breathing: &BreathingOverlay,
	) {
		if !breathing.is_visible() {
			return;
		}

		let state = breathing.state();
		let elapsed = state.start_time.elapsed().as_secs_f32();
		let pulse_duration = 1.5;

		if elapsed < pulse_duration {
			let t = elapsed / pulse_duration;
			let opacity = (t * std::f32::consts::PI).sin();
			let scale = 0.3 + 1.0 * (1.0 - (1.0 - t).powi(4));

			let (text, color) = match state.phase {
				BreathingPhase::Prepare => ("PREPARE", egui::Color32::RED),
				BreathingPhase::Inhale => ("INHALE", egui::Color32::YELLOW),
				BreathingPhase::Hold => ("HOLD", egui::Color32::YELLOW),
				BreathingPhase::Release => ("RELEASE", egui::Color32::GREEN),
				BreathingPhase::Idle => return,
			};

			let screen_rect = ui.ctx().screen_rect();
			let center = screen_rect.center();
			let font_size = (screen_rect.height() * 0.15) * scale;

			egui::Area::new(egui::Id::new("breathing_pulse"))
				.fixed_pos(center)
				.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
				.interactable(false)
				.order(egui::Order::Foreground)
				.show(ui.ctx(), |ui| {
					let font_id = egui::FontId::proportional(font_size);
					let shadow_color = egui::Color32::BLACK.gamma_multiply(opacity);
					let text_color = color.gamma_multiply(opacity);

					let galley = ui.painter().layout_no_wrap(
						text.to_string(),
						font_id.clone(),
						text_color,
					);

					let stroke_width = (font_size * 0.02).max(1.0);
					let offsets = [
						egui::vec2(-stroke_width, -stroke_width),
						egui::vec2(0.0, -stroke_width),
						egui::vec2(stroke_width, -stroke_width),
						egui::vec2(-stroke_width, 0.0),
						egui::vec2(stroke_width, 0.0),
						egui::vec2(-stroke_width, stroke_width),
						egui::vec2(0.0, stroke_width),
						egui::vec2(stroke_width, stroke_width),
					];

					let text_size = galley.size();
					let draw_pos = center - (text_size / 2.0);

					for offset in offsets {
						let shadow_galley = ui.painter().layout_no_wrap(
							text.to_string(),
							font_id.clone(),
							shadow_color,
						);
						ui.painter().galley(
							draw_pos + offset,
							shadow_galley,
							shadow_color,
						);
					}
					ui.painter().galley(draw_pos, galley, text_color);
				});

			ui.ctx().request_repaint();
		}
	}

	pub(super) fn render_immersive_breathing_overlay(
		&self,
		ui: &mut Ui,
		breathing: &BreathingOverlay,
	) {
		if !breathing.is_visible() {
			return;
		}

		let state = breathing.state();
		let elapsed = state.start_time.elapsed().as_secs_f32();
		let duration = state.duration.as_secs_f32();
		let progress = (elapsed / duration).clamp(0.0, 1.0);

		let screen_rect = ui.ctx().screen_rect();
		let screen_width = screen_rect.width();
		let screen_height = screen_rect.height();

		// Calculate visual properties based on phase
		let (text, text_color, bar_fill, bar_bg_alpha, text_alpha) = match state.phase
		{
			BreathingPhase::Prepare => {
				// Text fades in fast, background fades in gradually
				let text_alpha = (progress * 4.0).min(1.0);
				let bg_alpha = progress * 0.4;
				("PREPARE", egui::Color32::RED, 0.0, bg_alpha, text_alpha)
			}
			BreathingPhase::Inhale => {
				// Fill bar from 0% to 100%
				("INHALE", egui::Color32::YELLOW, progress, 0.4, 1.0)
			}
			BreathingPhase::Hold => {
				// Bar stays full
				("HOLD", egui::Color32::YELLOW, 1.0, 0.4, 1.0)
			}
			BreathingPhase::Release => {
				// Empty the bar, fade out background and text
				let fade = 1.0 - progress;
				let bg_alpha = 0.4 * fade;
				("RELEASE", egui::Color32::GREEN, fade, bg_alpha, fade)
			}
			BreathingPhase::Idle => {
				// Fade everything out quickly
				let alpha = (1.0 - progress * 2.0).max(0.0);
				("", egui::Color32::TRANSPARENT, 0.0, 0.0, alpha)
			}
		};

		// Skip rendering if completely transparent
		if text_alpha <= 0.001 && bar_bg_alpha <= 0.001 {
			return;
		}

		ui.ctx().request_repaint();

		// Render semi-transparent background overlay
		egui::Area::new(egui::Id::new("immersive_breathing_bg"))
			.fixed_pos(screen_rect.min)
			.order(egui::Order::Foreground)
			.interactable(false)
			.show(ui.ctx(), |ui| {
				let bg_alpha = (bar_bg_alpha * text_alpha * 180.0) as u8;
				ui.painter().rect_filled(
					screen_rect,
					0.0,
					egui::Color32::from_rgba_unmultiplied(0, 0, 0, bg_alpha),
				);
			});

		// Render progress bar just below the centered text
		let font_size = screen_height * 0.08;
		let bar_height = screen_height * 0.015;
		let text_center_y = screen_height / 2.0;
		let bar_y = text_center_y + (font_size * 0.6); // Small gap below text
		let bar_width = screen_width * 0.4;
		let bar_x = (screen_width - bar_width) / 2.0;
		let bar_rect = egui::Rect::from_min_size(
			egui::pos2(bar_x, bar_y),
			egui::vec2(bar_width, bar_height),
		);

		if bar_bg_alpha > 0.001 {
			egui::Area::new(egui::Id::new("immersive_breathing_bar"))
				.fixed_pos(bar_rect.min)
				.order(egui::Order::Foreground)
				.interactable(false)
				.show(ui.ctx(), |ui| {
					let painter = ui.painter();
					let rounding = bar_height * 0.5;

					// Background track
					let bg_alpha = (text_alpha * 100.0) as u8;
					painter.rect_filled(
						bar_rect,
						rounding,
						egui::Color32::from_rgba_unmultiplied(40, 40, 50, bg_alpha),
					);

					// Filled portion
					if bar_fill > 0.001 {
						let fill_width = bar_rect.width() * bar_fill;
						let fill_rect = egui::Rect::from_min_size(
							bar_rect.min,
							egui::vec2(fill_width, bar_height),
						);
						let fill_color = text_color.gamma_multiply(text_alpha);
						painter.rect_filled(fill_rect, rounding, fill_color);
					}
				});
		}

		// Render centered text
		if !text.is_empty() {
			egui::Area::new(egui::Id::new("immersive_breathing_text"))
				.anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
				.order(egui::Order::Foreground)
				.interactable(false)
				.show(ui.ctx(), |ui| {
					let font_id = egui::FontId::proportional(font_size);
					let display_color = text_color.gamma_multiply(text_alpha);
					let stroke_width = (font_size * 0.03).max(1.0);
					Self::draw_outlined_text(
						ui,
						text,
						font_id,
						display_color,
						stroke_width,
					);
				});
		}
	}

	pub(super) fn render_info_overlay(&self, ui: &mut Ui, browser: &ContentBrowser) {
		if browser.is_empty() {
			return;
		}

		let post = match browser.current_post() {
			Some(p) => p,
			None => return,
		};

		let screen_height = ui.ctx().screen_rect().height();
		let font_size = (screen_height * 0.02).max(12.0);
		let margin = (screen_height * 0.03).max(10.0);
		let stroke_width = (font_size * 0.05).max(1.0);

		egui::Area::new(egui::Id::new("image_info_overlay"))
			.anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(margin, -margin))
			.interactable(false)
			.order(egui::Order::Foreground)
			.show(ui.ctx(), |ui| {
				let text_color = egui::Color32::WHITE;
				let font_id = egui::FontId::proportional(font_size);

				let add_text_line =
					|ui: &mut egui::Ui, label: &str, content: &str| {
						if !content.is_empty() {
							ui.horizontal(|ui| {
								Self::draw_outlined_text(
									ui,
									label,
									font_id.clone(),
									egui::Color32::LIGHT_GRAY,
									stroke_width,
								);
								Self::draw_outlined_text(
									ui,
									" ",
									font_id.clone(),
									egui::Color32::TRANSPARENT,
									0.0,
								);
								Self::draw_outlined_text(
									ui,
									content,
									font_id.clone(),
									text_color,
									stroke_width,
								);
							});
						}
					};

				ui.vertical(|ui| {
					add_text_line(ui, "Post ID:", &post.id.to_string());

					let artist_str = post.tags.artist.join(", ");
					if !artist_str.is_empty() && artist_str != "invalid_artist" {
						add_text_line(ui, "Artist:", &artist_str);
					}

					let copyright_str = post.tags.copyright.join(", ");
					if !copyright_str.is_empty()
						&& copyright_str != "invalid_copyright"
					{
						add_text_line(ui, "Copyright:", &copyright_str);
					}

					if browser.has_valid_related_posts() {
						add_text_line(ui, "Has related posts:", "Yes");
					}
				});
			});
	}

	fn draw_outlined_text(
		ui: &mut egui::Ui,
		text: &str,
		font_id: egui::FontId,
		color: egui::Color32,
		stroke_width: f32,
	) {
		let galley =
			ui.painter()
				.layout_no_wrap(text.to_string(), font_id.clone(), color);
		let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());

		let offsets = [
			egui::vec2(-stroke_width, -stroke_width),
			egui::vec2(0.0, -stroke_width),
			egui::vec2(stroke_width, -stroke_width),
			egui::vec2(-stroke_width, 0.0),
			egui::vec2(stroke_width, 0.0),
			egui::vec2(-stroke_width, stroke_width),
			egui::vec2(0.0, stroke_width),
			egui::vec2(stroke_width, stroke_width),
		];

		let num_passes = offsets.len() as f32;
		let base_alpha = color.a() as f32;
		let per_pass_alpha = (base_alpha / num_passes).max(1.0) as u8;
		let shadow_color =
			egui::Color32::from_rgba_unmultiplied(0, 0, 0, per_pass_alpha);

		for offset in offsets {
			let shadow_galley = ui.painter().layout_no_wrap(
				text.to_string(),
				font_id.clone(),
				shadow_color,
			);
			ui.painter()
				.galley(rect.min + offset, shadow_galley, shadow_color);
		}

		ui.painter().galley(rect.min, galley, color);
	}
}
