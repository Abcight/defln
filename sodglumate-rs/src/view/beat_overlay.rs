use super::*;
pub(super) struct BeatOverlayView {
	pub(super) beat_intensity: f32,
	pub(super) last_beat_time: Instant,
	pub(super) last_beat_scale: f32,
}

impl BeatOverlayView {
	pub(super) fn new() -> Self {
		Self {
			beat_intensity: 0.0,
			last_beat_time: Instant::now(),
			last_beat_scale: 1.0,
		}
	}

	pub(super) fn render(&mut self, ui: &mut Ui) {
		let elapsed = self.last_beat_time.elapsed().as_secs_f32();
		let decay_rate = 4.6;
		self.beat_intensity = self.last_beat_scale * (-decay_rate * elapsed).exp();

		if self.beat_intensity < 0.01 {
			return;
		}

		ui.ctx().request_repaint();

		let screen_rect = ui.ctx().screen_rect();
		let margin = 20.0;
		let base_radius = 6.0;
		let bounce = 10.0;
		let radius = base_radius + self.beat_intensity * bounce;

		let center = egui::pos2(
			screen_rect.right() - margin - base_radius,
			screen_rect.bottom() - margin - base_radius,
		);

		let alpha = (self.beat_intensity * 255.0) as u8;
		let color = egui::Color32::from_rgba_unmultiplied(0, 220, 255, alpha);

		egui::Area::new(egui::Id::new("beat_debug_dot"))
			.fixed_pos(center)
			.order(egui::Order::Foreground)
			.interactable(false)
			.show(ui.ctx(), |ui| {
				ui.painter().circle_filled(center, radius, color);
				// Outer glow ring
				let glow_alpha = (self.beat_intensity * 100.0) as u8;
				let glow_color =
					egui::Color32::from_rgba_unmultiplied(0, 220, 255, glow_alpha);
				ui.painter().circle_stroke(
					center,
					radius + 3.0,
					egui::Stroke::new(2.0, glow_color),
				);
			});
	}
}
