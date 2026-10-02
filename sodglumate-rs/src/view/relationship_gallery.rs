use super::*;
use crate::browser::RelatedPost;
use crate::types::MediaKind;

pub(super) struct RelationshipGalleryView {
	source_id: Option<u64>,
	selected_post_id: Option<u64>,
}

impl RelationshipGalleryView {
	pub(super) fn new() -> Self {
		Self {
			source_id: None,
			selected_post_id: None,
		}
	}

	pub(super) fn render(
		&mut self,
		ui: &mut Ui,
		browser: &ContentBrowser,
		metadata: &crate::metadata::PostMetadataStore,
		media: &MediaPane,
		keyboard_enabled: bool,
		output: &mut ViewOutput,
	) {
		let Some(source) = browser.current_post() else {
			self.source_id = None;
			self.selected_post_id = None;
			return;
		};
		let source_id = source.id;
		let pool_shortcuts = browser.pool_navigation_targets(metadata);
		let direct_shortcuts = browser.direct_navigation_targets(metadata);
		let related = browser.related_posts(metadata);
		if related.is_empty() {
			self.source_id = Some(source_id);
			self.selected_post_id = None;
			if keyboard_enabled && let Some((previous_id, next_id)) = pool_shortcuts {
				if ui.input(|input| input.key_pressed(egui::Key::Z)) {
					Self::open_direct_target(source_id, previous_id, output);
				}
				if ui.input(|input| input.key_pressed(egui::Key::X)) {
					Self::open_direct_target(source_id, next_id, output);
				}
			}
			return;
		}

		if self.source_id != Some(source_id)
			|| !related
				.iter()
				.any(|post| Some(post.post.id) == self.selected_post_id)
		{
			self.source_id = Some(source_id);
			self.selected_post_id = Some(related[0].post.id);
		}

		output.command(Command::PrefetchRelatedMedia {
			urls: related
				.iter()
				.filter_map(|related| Self::full_media_url(&related.post))
				.collect(),
		});

		if keyboard_enabled {
			if let Some((previous_id, next_id)) = pool_shortcuts {
				if ui.input(|input| input.key_pressed(egui::Key::Z)) {
					Self::open_direct_target(source_id, previous_id, output);
				}
				if ui.input(|input| input.key_pressed(egui::Key::X)) {
					Self::open_direct_target(source_id, next_id, output);
				}
			} else if let Some((parent_id, child_id)) = direct_shortcuts {
				if ui.input(|input| input.key_pressed(egui::Key::Z)) {
					Self::open_target(source_id, parent_id, &related, output);
				}
				if ui.input(|input| input.key_pressed(egui::Key::X)) {
					Self::open_target(source_id, child_id, &related, output);
				}
			} else {
				if ui.input(|input| input.key_pressed(egui::Key::Z)) {
					self.move_selection(&related, -1);
				}
				if ui.input(|input| input.key_pressed(egui::Key::X)) {
					self.move_selection(&related, 1);
				}
				if ui.input(|input| input.key_pressed(egui::Key::C)) {
					self.open_selected(source_id, &related, output);
				}
			}
		}

		let thumbnail_size =
			(ui.ctx().screen_rect().height() * 0.12).clamp(48.0, 96.0);
		egui::Area::new(egui::Id::new("relationship_thumbnails"))
			.anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -12.0))
			.order(egui::Order::Foreground)
			.interactable(false)
			.show(ui.ctx(), |ui| {
				ui.horizontal(|ui| {
					for related_post in &related {
						let (rect, _) = ui.allocate_exact_size(
							egui::vec2(thumbnail_size, thumbnail_size),
							egui::Sense::hover(),
						);
						let painter = ui.painter();
						painter.rect_filled(rect, 0.0, egui::Color32::from_gray(30));
						if let Some(loaded) =
							media.get_full_media_by_post(&related_post.post)
						{
							let texture = loaded.texture();
							painter.image(
								texture.id(),
								rect,
								Self::cover_uv(texture.size_vec2()),
								egui::Color32::WHITE,
							);
						}
						let stroke = if direct_shortcuts.is_none()
							&& self.selected_post_id == Some(related_post.post.id)
						{
							egui::Stroke::new(3.0, egui::Color32::WHITE)
						} else {
							egui::Stroke::new(
								1.0,
								egui::Color32::from_white_alpha(96),
							)
						};
						painter.rect_stroke(rect, 0.0, stroke);
					}
				});
			});
	}

	fn full_media_url(post: &crate::api::Post) -> Option<(String, MediaKind)> {
		let kind = MediaKind::from_extension(&post.file.ext)?;
		Some((post.file.url.clone()?, kind))
	}

	fn move_selection(&mut self, related: &[RelatedPost], delta: isize) {
		let selected = related
			.iter()
			.position(|post| Some(post.post.id) == self.selected_post_id)
			.unwrap_or(0) as isize;
		let next = (selected + delta).clamp(0, related.len() as isize - 1) as usize;
		self.selected_post_id = Some(related[next].post.id);
	}

	fn open_selected(
		&self,
		source_id: u64,
		related: &[RelatedPost],
		output: &mut ViewOutput,
	) {
		Self::open_target(source_id, self.selected_post_id, related, output);
	}

	fn open_target(
		source_id: u64,
		target_id: Option<u64>,
		related: &[RelatedPost],
		output: &mut ViewOutput,
	) {
		let Some(target_id) = target_id else {
			return;
		};
		if related.iter().any(|post| post.post.id == target_id) {
			output.command(Command::OpenLinkedPost {
				source_id,
				target_id,
			});
		}
	}

	fn open_direct_target(
		source_id: u64,
		target_id: Option<u64>,
		output: &mut ViewOutput,
	) {
		if let Some(target_id) = target_id {
			output.command(Command::OpenLinkedPost {
				source_id,
				target_id,
			});
		}
	}

	fn cover_uv(image_size: egui::Vec2) -> egui::Rect {
		let aspect = image_size.x / image_size.y.max(1.0);
		if aspect > 1.0 {
			let margin = (1.0 - aspect.recip()) * 0.5;
			egui::Rect::from_min_max(
				egui::pos2(margin, 0.0),
				egui::pos2(1.0 - margin, 1.0),
			)
		} else {
			let margin = (1.0 - aspect) * 0.5;
			egui::Rect::from_min_max(
				egui::pos2(0.0, margin),
				egui::pos2(1.0, 1.0 - margin),
			)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::api::Post;
	use crate::reactor::Message;

	fn related(id: u64) -> RelatedPost {
		RelatedPost {
			post: Post {
				id,
				..Post::default()
			},
		}
	}

	#[test]
	fn selection_stops_at_each_end_and_confirms_the_focused_post() {
		let related = vec![related(10), related(20), related(30)];
		let mut gallery = RelationshipGalleryView::new();
		gallery.selected_post_id = Some(10);
		gallery.move_selection(&related, -1);
		assert_eq!(gallery.selected_post_id, Some(10));
		gallery.move_selection(&related, 1);
		assert_eq!(gallery.selected_post_id, Some(20));

		let mut output = ViewOutput::default();
		gallery.open_selected(7, &related, &mut output);
		assert!(matches!(
			output.into_messages().next(),
			Some(Message::Command(Command::OpenLinkedPost {
				source_id: 7,
				target_id: 20
			}))
		));
	}

	#[test]
	fn direct_targets_are_opened_without_waiting_for_a_thumbnail() {
		let mut output = ViewOutput::default();
		RelationshipGalleryView::open_direct_target(7, Some(20), &mut output);
		assert!(matches!(
			output.into_messages().next(),
			Some(Message::Command(Command::OpenLinkedPost {
				source_id: 7,
				target_id: 20
			}))
		));
	}
}
