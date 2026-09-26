use super::*;
pub(super) struct IslandNavigationView {
	pub(super) island_ctx: IslandCtx,
	pub(super) prev_shift_held: bool,
	links_open: bool,
	links_post_id: Option<u64>,
	links_children: Vec<u64>,
}

impl IslandNavigationView {
	pub(super) fn new() -> Self {
		Self {
			island_ctx: IslandCtx::new(),
			prev_shift_held: false,
			links_open: false,
			links_post_id: None,
			links_children: Vec::new(),
		}
	}

	pub(super) fn handle_keyboard_input(&mut self, ui: &Ui, output: &mut ViewOutput) {
		// Detect shift press/release edges for island activation
		let shift_held = ui.input(|i| i.modifiers.shift);
		if shift_held && !self.prev_shift_held {
			self.island_ctx.activate(&ROOT_ISLAND, 3);
			self.links_open = false;
		} else if !shift_held && self.prev_shift_held {
			self.island_ctx.deactivate();
		}
		self.prev_shift_held = shift_held;

		// Island overlay consumes all input when active or just closed
		if self.island_ctx.active || self.island_ctx.in_cooldown() {
			return;
		}

		let space_pressed = ui.input(|i| i.key_pressed(egui::Key::Space));
		let ctrl_pressed = ui.input(|i| i.modifiers.ctrl);
		if space_pressed {
			if ctrl_pressed {
				output.command(Command::Navigate(NavDirection::Skip(10)));
			} else {
				output.command(Command::Navigate(NavDirection::Next));
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::api::Post;
	use crate::reactor::Message;

	#[test]
	fn shift_links_parent_emits_an_in_place_navigation_command() {
		let ctx = egui::Context::default();
		let mut navigation = IslandNavigationView::new();
		let mut browser = ContentBrowser::new();
		let mut post = Post {
			id: 42,
			..Post::default()
		};
		post.file.ext = "jpg".into();
		post.file.url = Some("https://example.test/42.jpg".into());
		post.relationships.parent_id = Some(17);
		browser.observe(&Event::SearchCompleted {
			posts: vec![post],
			page: 1,
			is_new: true,
		});
		let settings = SettingsManager::default();
		let mut modal = ModalView::new();
		modal.modal = ModalContent::None;
		let mut messages = Vec::new();
		for key in [
			None,
			Some(egui::Key::S),
			Some(egui::Key::D),
			Some(egui::Key::Space),
			Some(egui::Key::D),
			Some(egui::Key::Space),
		] {
			let modifiers = egui::Modifiers {
				shift: true,
				..Default::default()
			};
			let events = key
				.into_iter()
				.flat_map(|key| {
					[true, false].map(|pressed| egui::Event::Key {
						key,
						physical_key: None,
						pressed,
						repeat: false,
						modifiers,
					})
				})
				.collect();
			let _ = ctx.run(
				egui::RawInput {
					modifiers,
					events,
					..Default::default()
				},
				|ctx| {
					egui::CentralPanel::default().show(ctx, |ui| {
						let mut output = ViewOutput::default();
						navigation.handle_keyboard_input(ui, &mut output);
						navigation.render(
							ui,
							&settings,
							&browser,
							&mut modal,
							&mut output,
						);
						messages.extend(output.into_messages());
					});
				},
			);
		}
		assert!(matches!(
			messages.as_slice(),
			[
				Message::Command(Command::PrepareLinks { source_id: 42 }),
				Message::Command(Command::OpenLinkedPost {
					source_id: 42,
					target_id: 17
				})
			]
		));
	}
}

impl IslandNavigationView {
	pub(super) fn render(
		&mut self,
		ui: &mut Ui,
		settings: &SettingsManager,
		browser: &ContentBrowser,
		modal: &mut ModalView,
		output: &mut ViewOutput,
	) {
		if !matches!(modal.modal, ModalContent::None) {
			return;
		}

		let post_id = browser.current_post().map(|post| post.id);
		let children = browser.validated_child_ids();
		if self.links_open
			&& self.island_ctx.active
			&& (post_id != self.links_post_id || children != self.links_children)
		{
			let selected = self.island_ctx.selected;
			self.island_ctx
				.replace_current(island::links_island(browser.links_post().as_ref()));
			if post_id == self.links_post_id
				&& children.len() >= self.links_children.len()
			{
				self.island_ctx.selected = selected;
			}
			if post_id != self.links_post_id
				&& let Some(source_id) = post_id
			{
				output.command(Command::PrepareLinks { source_id });
			}
			self.links_post_id = post_id;
			self.links_children = children.clone();
		}
		if let Some(action) = IslandWidget::new(&mut self.island_ctx).show(ui.ctx()) {
			match action {
				IslandAction::Emit(factory) => output.command(factory()),
				IslandAction::Push(island) => self.island_ctx.push(island.clone()),
				IslandAction::Pop => {
					self.island_ctx.pop();
					self.links_open = false;
				}
				IslandAction::Links => {
					self.island_ctx
						.push(island::links_island(browser.links_post().as_ref()));
					self.links_open = true;
					self.links_post_id = post_id;
					self.links_children = children.clone();
					if let Some(source_id) = post_id {
						output.command(Command::PrepareLinks { source_id });
					}
				}
				IslandAction::OpenLinkedPost(target_id) => {
					if let Some(source_id) = post_id {
						output.command(Command::OpenLinkedPost {
							source_id,
							target_id,
						});
					}
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
