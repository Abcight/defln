use super::*;
pub(super) struct IslandNavigationView {
	pub(super) island_ctx: IslandCtx,
	pub(super) prev_shift_held: bool,
}

impl IslandNavigationView {
	pub(super) fn new() -> Self {
		Self {
			island_ctx: IslandCtx::new(),
			prev_shift_held: false,
		}
	}

	pub(super) fn handle_keyboard_input(
		&mut self,
		ctx: &egui::Context,
		output: &mut ViewOutput,
	) {
		// Detect shift press/release edges for island activation
		let shift_held = ctx.input(|i| i.modifiers.shift);
		if shift_held && !self.prev_shift_held {
			self.island_ctx.activate(&ROOT_ISLAND, 3);
		} else if !shift_held && self.prev_shift_held {
			self.island_ctx.deactivate();
		}
		self.prev_shift_held = shift_held;

		// Island overlay consumes all input when active or just closed
		if self.island_ctx.active || self.island_ctx.in_cooldown() {
			return;
		}

		let space_pressed = ctx.input(|i| i.key_pressed(egui::Key::Space));
		let ctrl_pressed = ctx.input(|i| i.modifiers.ctrl);
		let c_pressed = ctx.input(|i| i.key_pressed(egui::Key::C));

		if c_pressed {
			output.command(Command::ToggleAutoPlay);
		}

		if space_pressed {
			if ctrl_pressed {
				output.command(Command::Navigate(NavDirection::Skip(10)));
			} else {
				output.command(Command::Navigate(NavDirection::Next));
			}
		}
	}
}

impl IslandNavigationView {
	pub(super) fn render(
		&mut self,
		ctx: &egui::Context,
		settings: &SettingsManager,
		modal: &mut ModalView,
		output: &mut ViewOutput,
	) {
		if !matches!(modal.modal, ModalContent::None) {
			return;
		}

		if let Some(action) = IslandWidget::new(&mut self.island_ctx).show(ctx) {
			match action {
				IslandAction::Emit(factory) => output.command(factory()),
				IslandAction::Push(island) => self.island_ctx.push(island),
				IslandAction::Pop => {
					self.island_ctx.pop();
				}
				IslandAction::RequestBreathingToggle => {
					if modal.breathing_disclaimer_accepted {
						output.command(Command::ToggleBreathing);
					} else {
						modal.modal = ModalContent::BreathingDisclaimer;
					}
				}
				IslandAction::ToggleImageFillMode => {
					let mode = match settings.image_fill_mode() {
						ImageFillMode::Cover => ImageFillMode::Fit,
						ImageFillMode::Fit => ImageFillMode::FitToGallery,
						ImageFillMode::FitToGallery => ImageFillMode::Cover,
					};
					output.command(Command::SetImageFillMode(mode));
				}
			}
		}
	}
}
