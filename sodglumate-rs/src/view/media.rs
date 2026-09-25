use super::*;
pub(super) struct MediaView {
	gallery_transforms: std::collections::HashMap<u64, (f32, egui::Vec2)>,
	pub(super) image_load_time: Instant,
	pub(super) user_has_panned: bool,
	pub(super) last_media_url: Option<String>,
	pub(super) gallery_anim_start_offset: f32,
	pub(super) gallery_anim_offset: f32,
	pub(super) gallery_anim_time: f32,
	pub(super) last_gallery_index: usize,
	pub(super) user_zoom: f32,
	pub(super) user_pan_offset: egui::Vec2,
}

impl MediaView {
	pub(super) fn new() -> Self {
		Self {
			gallery_transforms: Default::default(),
			image_load_time: Instant::now(),
			user_has_panned: false,
			last_media_url: None,
			gallery_anim_start_offset: 0.0,
			gallery_anim_offset: 0.0,
			gallery_anim_time: 0.0,
			last_gallery_index: 0,
			user_zoom: 1.0,
			user_pan_offset: egui::Vec2::ZERO,
		}
	}

	fn paint_gallery_overlap(
		painter: &egui::Painter,
		texture: egui::TextureId,
		rect: egui::Rect,
		uv: egui::Rect,
		back_rect: egui::Rect,
		blend: f32,
	) {
		let visible = rect.intersect(painter.clip_rect());
		let overlap = visible.intersect(back_rect);
		if !overlap.is_positive() {
			painter.image(texture, rect, uv, egui::Color32::WHITE);
			return;
		}

		// Only the shared pixels crossfade; exposed parts remain fully opaque.
		for clip in [
			egui::Rect::from_min_max(
				visible.min,
				egui::pos2(visible.right(), overlap.top()),
			),
			egui::Rect::from_min_max(
				egui::pos2(visible.left(), overlap.bottom()),
				visible.max,
			),
			egui::Rect::from_min_max(
				egui::pos2(visible.left(), overlap.top()),
				overlap.left_bottom(),
			),
			egui::Rect::from_min_max(
				overlap.right_top(),
				egui::pos2(visible.right(), overlap.bottom()),
			),
		] {
			if clip.is_positive() {
				painter.with_clip_rect(clip).image(
					texture,
					rect,
					uv,
					egui::Color32::WHITE,
				);
			}
		}
		painter.with_clip_rect(overlap).image(
			texture,
			rect,
			uv,
			egui::Color32::from_white_alpha(
				(blend.clamp(0.0, 1.0) * 255.0).round() as u8
			),
		);
	}

	pub(super) fn render(
		&mut self,
		ui: &mut Ui,
		state: &ApplicationState<'_>,
		island_active: bool,
		beat_intensity: f32,
		output: &mut ViewOutput,
		enabled: bool,
	) {
		let browser = state.browser;
		let media = state.media;
		let gateway = state.gateway;
		egui::CentralPanel::default().show_inside(ui, |ui| {
			if !enabled {
				ui.disable();
			}
			if gateway.is_loading() && browser.is_empty() {
				ui.centered_and_justified(|ui| {
					ui.spinner();
				});
			} else if let Some(_url) = media.current_url() {
				self.render_media(ui, state, island_active, beat_intensity, output);
			} else {
				ui.centered_and_justified(|ui| {
					ui.label("Enter a query and search to start.");
				});
			}
		});
	}

	fn render_media(
		&mut self,
		ui: &mut egui::Ui,
		state: &ApplicationState<'_>,
		island_active: bool,
		beat_intensity: f32,
		output: &mut ViewOutput,
	) {
		let media = state.media;
		let browser = state.browser;
		let settings = state.settings;
		let pan_cycle = settings.auto_pan_cycle_duration();
		let image_fill_mode = settings.image_fill_mode();
		let load_time = self.image_load_time;
		let mut user_panned = self.user_has_panned;
		if media.needs_painted_notification() {
			output.event(Event::MediaPainted);
		}

		let handle_scroll_input = |ui: &mut egui::Ui, input_active: &mut bool| {
			// Don't process scroll input when island overlay is active or just closed
			if island_active {
				return;
			}

			let mut scroll_delta = egui::Vec2::ZERO;
			let speed = 20.0;

			if ui.input(|i| {
				i.key_down(egui::Key::ArrowRight) || i.key_down(egui::Key::D)
			}) {
				scroll_delta.x -= speed;
				*input_active = true;
			}
			if ui.input(|i| {
				i.key_down(egui::Key::ArrowLeft) || i.key_down(egui::Key::A)
			}) {
				scroll_delta.x += speed;
				*input_active = true;
			}
			if ui.input(|i| {
				i.key_down(egui::Key::ArrowDown) || i.key_down(egui::Key::S)
			}) {
				scroll_delta.y -= speed;
				*input_active = true;
			}
			if ui
				.input(|i| i.key_down(egui::Key::ArrowUp) || i.key_down(egui::Key::W))
			{
				scroll_delta.y += speed;
				*input_active = true;
			}

			if scroll_delta != egui::Vec2::ZERO {
				ui.scroll_with_delta(scroll_delta);
			}
		};

		#[cfg(feature = "video")]
		if media.current_is_playable()
			&& !matches!(image_fill_mode, ImageFillMode::FitToGallery)
		{
			Self::render_current_video(ui, media, None);
			self.user_has_panned = user_panned;
			return;
		}

		let gallery_fallback_media =
			if matches!(image_fill_mode, ImageFillMode::FitToGallery)
				&& media.get_current_media().is_none()
			{
				(1..browser.posts_len() as isize)
					.chain((1..browser.posts_len() as isize).map(|offset| -offset))
					.find_map(|offset| {
						browser
							.get_post_relative(offset)
							.and_then(|post| media.get_media_by_post(post))
					})
			} else {
				None
			};

		if let Some(loaded_media) =
			media.get_current_media().or(gallery_fallback_media)
		{
			if loaded_media.is_animated() {
				ui.ctx().request_repaint();
			}
			{
				let available_size = ui.available_size();
				let texture = loaded_media.texture();
				let img_size = texture.size_vec2();

				if matches!(
					image_fill_mode,
					ImageFillMode::Fit | ImageFillMode::FitToGallery
				) {
					if !island_active {
						let dt = ui.input(|i| i.stable_dt);

						if ui.input(|i| i.key_down(egui::Key::E)) {
							self.user_zoom = (self.user_zoom + dt * 4.0).min(5.0);
							ui.ctx().request_repaint();
						}
						if ui.input(|i| i.key_down(egui::Key::Q)) {
							self.user_zoom = (self.user_zoom - dt * 4.0).max(1.0);
							ui.ctx().request_repaint();
						}

						if self.user_zoom > 1.0 {
							let speed = 1600.0 * dt;
							if ui.input(|i| {
								i.key_down(egui::Key::ArrowRight)
									|| i.key_down(egui::Key::D)
							}) {
								self.user_pan_offset.x -= speed;
								ui.ctx().request_repaint();
							}
							if ui.input(|i| {
								i.key_down(egui::Key::ArrowLeft)
									|| i.key_down(egui::Key::A)
							}) {
								self.user_pan_offset.x += speed;
								ui.ctx().request_repaint();
							}
							if ui.input(|i| {
								i.key_down(egui::Key::ArrowDown)
									|| i.key_down(egui::Key::S)
							}) {
								self.user_pan_offset.y -= speed;
								ui.ctx().request_repaint();
							}
							if ui.input(|i| {
								i.key_down(egui::Key::ArrowUp)
									|| i.key_down(egui::Key::W)
							}) {
								self.user_pan_offset.y += speed;
								ui.ctx().request_repaint();
							}
						} else {
							self.user_pan_offset = egui::Vec2::ZERO;
						}
					}

					let fit_scale = (available_size.x / img_size.x)
						.min(available_size.y / img_size.y);
					let fit_size = img_size * fit_scale * self.user_zoom;
					let pan_limit =
						((fit_size - available_size) * 0.5).max(egui::Vec2::ZERO);
					self.user_pan_offset.x =
						self.user_pan_offset.x.clamp(-pan_limit.x, pan_limit.x);
					self.user_pan_offset.y =
						self.user_pan_offset.y.clamp(-pan_limit.y, pan_limit.y);
				} else {
					self.user_zoom = 1.0;
					self.user_pan_offset = egui::Vec2::ZERO;
				}

				// Apply beat pulse if enabled
				let pulse = if settings.beat_pulse_enabled() && beat_intensity > 0.01
				{
					ui.ctx().request_repaint();
					1.0 + beat_intensity * settings.beat_pulse_scale()
				} else {
					1.0
				};

				match image_fill_mode {
					ImageFillMode::Cover => {
						let width_ratio = available_size.x / img_size.x;
						let height_ratio = available_size.y / img_size.y;
						let scale = width_ratio.max(height_ratio);
						let base_display_size = img_size * scale;

						let mut scroll_area = egui::ScrollArea::both()
							.scroll_bar_visibility(
								egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
							);

						// Auto-pan
						if !user_panned {
							let elapsed = load_time.elapsed().as_secs_f32();
							let cycle =
								(elapsed * 2.0 * std::f32::consts::PI) / pan_cycle;
							let factor = (1.0 - cycle.cos()) * 0.5;

							let overflow = base_display_size - available_size;
							if overflow.x > 0.0 {
								scroll_area = scroll_area
									.horizontal_scroll_offset(overflow.x * factor);
							}
							if overflow.y > 0.0 {
								scroll_area = scroll_area
									.vertical_scroll_offset(overflow.y * factor);
							}
							ui.ctx().request_repaint();
						}

						scroll_area.show(ui, |ui| {
							handle_scroll_input(ui, &mut user_panned);

							let (rect, _response) = ui.allocate_exact_size(
								base_display_size,
								egui::Sense::hover(),
							);

							let center = rect.center();
							let pulsed_size = base_display_size * pulse;
							let pulsed_rect =
								egui::Rect::from_center_size(center, pulsed_size);
							let uv = egui::Rect::from_min_max(
								egui::pos2(0.0, 0.0),
								egui::pos2(1.0, 1.0),
							);

							ui.painter().image(
								texture.id(),
								pulsed_rect,
								uv,
								egui::Color32::WHITE,
							);
						});
					}
					ImageFillMode::Fit => {
						let width_ratio = available_size.x / img_size.x;
						let height_ratio = available_size.y / img_size.y;
						let scale = width_ratio.min(height_ratio) * self.user_zoom;
						let base_display_size = img_size * scale;

						ui.centered_and_justified(|ui| {
							let (rect, _response) = ui.allocate_exact_size(
								available_size,
								egui::Sense::hover(),
							);

							let center = rect.center() + self.user_pan_offset;
							let pulsed_size = base_display_size * pulse;
							let pulsed_rect =
								egui::Rect::from_center_size(center, pulsed_size);
							let uv = egui::Rect::from_min_max(
								egui::pos2(0.0, 0.0),
								egui::pos2(1.0, 1.0),
							);

							ui.painter().image(
								texture.id(),
								pulsed_rect,
								uv,
								egui::Color32::WHITE,
							);
						});
					}
					ImageFillMode::FitToGallery => {
						// Keep the outgoing image's fitted geometry after navigation
						// resets the controls for the newly selected image.
						self.gallery_transforms.retain(|id, _| {
							(-2..=2).any(|offset| {
								browser
									.get_post_relative(offset)
									.is_some_and(|post| post.id == *id)
							})
						});
						if let Some(post) = browser.current_post() {
							self.gallery_transforms.insert(
								post.id,
								(self.user_zoom, self.user_pan_offset),
							);
						}
						let len = browser.posts_len();
						if len > 0 {
							let new_idx = browser.current_index();
							if new_idx != self.last_gallery_index {
								let mut delta = new_idx as isize
									- self.last_gallery_index as isize;
								let ilen = len as isize;
								if delta > ilen / 2 {
									delta -= ilen;
								} else if delta < -ilen / 2 {
									delta += ilen;
								}

								let visual_delta = delta.signum() as f32;

								self.gallery_anim_start_offset =
									self.gallery_anim_offset + visual_delta;
								self.gallery_anim_offset =
									self.gallery_anim_start_offset;
								self.gallery_anim_time = 0.0;
								self.last_gallery_index = new_idx;
							}
						}

						let anim_duration = 0.4;
						if self.gallery_anim_time < anim_duration {
							let dt = ui.input(|i| i.stable_dt);
							self.gallery_anim_time =
								(self.gallery_anim_time + dt).min(anim_duration);
							let t = self.gallery_anim_time / anim_duration;
							let ease = if t < 0.5 {
								4.0 * t * t * t
							} else {
								1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
							};
							self.gallery_anim_offset =
								self.gallery_anim_start_offset * (1.0 - ease);
							ui.ctx().request_repaint();
						} else {
							self.gallery_anim_offset = 0.0;
						}
						#[cfg(feature = "video")]
						let gallery_settled = self.gallery_anim_time >= anim_duration
							&& self.gallery_anim_offset.abs() < 0.001;

						ui.centered_and_justified(|ui| {
							let (rect, _response) = ui.allocate_exact_size(
								available_size,
								egui::Sense::hover(),
							);

							let center_rect =
								egui::Rect::from_min_size(rect.min, available_size);

							let get_fitted_width = |offset: isize| -> f32 {
								if let Some(post) = browser.get_post_relative(offset)
									&& let Some(media) = media.get_media_by_post(post)
								{
									let size = media.texture().size_vec2();
									let scale = (available_size.x / size.x)
										.min(available_size.y / size.y);
									return size.x * scale;
								}
								available_size.x
							};

							let virtual_center = -self.gallery_anim_offset;
							let vc_floor = virtual_center.floor() as isize;
							let vc_ceil = virtual_center.ceil() as isize;
							let vc_fract = virtual_center - vc_floor as f32;

							let w1 = get_fitted_width(vc_floor);
							let w2 = get_fitted_width(vc_ceil);
							let main_w = w1 + (w2 - w1) * vc_fract;

							let gutter_w =
								((available_size.x - main_w) / 2.0).max(0.0);
							let left_gutter = egui::Rect::from_min_size(
								rect.min,
								egui::vec2(gutter_w, available_size.y),
							);
							let right_gutter = egui::Rect::from_min_size(
								rect.min
									+ egui::vec2(available_size.x - gutter_w, 0.0),
								egui::vec2(gutter_w, available_size.y),
							);

							let off_left = left_gutter
								.translate(egui::vec2(-gutter_w - 100.0, 0.0));
							let off_right = right_gutter
								.translate(egui::vec2(gutter_w + 100.0, 0.0));

							let fit_rect = |img_size: egui::Vec2,
							                space: egui::Rect,
							                zoom: f32,
							                pan: egui::Vec2|
							 -> egui::Rect {
								if space.width() <= 0.01 || space.height() <= 0.01 {
									return egui::Rect::from_center_size(
										space.center(),
										egui::Vec2::ZERO,
									);
								}
								let width_ratio = space.width() / img_size.x;
								let height_ratio = space.height() / img_size.y;
								let scale = width_ratio.min(height_ratio) * zoom;
								let size = img_size * scale;
								egui::Rect::from_center_size(
									space.center() + pan,
									size,
								)
							};

							let cover_rect = |img_size: egui::Vec2,
							                  space: egui::Rect|
							 -> egui::Rect {
								if space.width() <= 0.01 || space.height() <= 0.01 {
									return egui::Rect::from_center_size(
										space.center(),
										egui::Vec2::ZERO,
									);
								}
								let width_ratio = space.width() / img_size.x;
								let height_ratio = space.height() / img_size.y;
								let scale = width_ratio.max(height_ratio);
								let size = img_size * scale;
								egui::Rect::from_center_size(space.center(), size)
							};

							let get_rect_at = |slot: isize,
							                   size: egui::Vec2,
							                   zoom: f32,
							                   pan: egui::Vec2|
							 -> egui::Rect {
								match slot {
									..=-2 => cover_rect(size, off_left),
									-1 => cover_rect(size, left_gutter),
									0 => fit_rect(size, center_rect, zoom, pan),
									1 => cover_rect(size, right_gutter),
									2.. => cover_rect(size, off_right),
								}
							};

							let get_clip_at = |slot: isize| -> egui::Rect {
								match slot {
									..=-2 => off_left,
									-1 => left_gutter,
									0 => center_rect,
									1 => right_gutter,
									2.. => off_right,
								}
							};

							let mut gallery_images = Vec::new();
							for offset in [-2, -1, 1, 2, 0] {
								let v = offset as f32 + self.gallery_anim_offset;

								// Only draw if within visible slots roughly
								if !(-2.5..=2.5).contains(&v) {
									continue;
								}

								if let Some(post) = browser.get_post_relative(offset)
									&& let Some(media) = media.get_media_by_post(post)
								{
									let off_texture = media.texture();
									let img_size = off_texture.size_vec2();

									let v_floor = v.floor();
									let v_ceil = v.ceil();
									let fract = v - v_floor;

									let (zoom, pan) = self
										.gallery_transforms
										.get(&post.id)
										.copied()
										.unwrap_or((1.0, egui::Vec2::ZERO));
									let r1 = get_rect_at(
										v_floor as isize,
										img_size,
										zoom,
										pan,
									);
									let r2 = get_rect_at(
										v_ceil as isize,
										img_size,
										zoom,
										pan,
									);

									let interpolated_center = r1.center()
										+ (r2.center() - r1.center()) * fract;
									let interpolated_size =
										r1.size() + (r2.size() - r1.size()) * fract;

									let c1 = get_clip_at(v_floor as isize);
									let c2 = get_clip_at(v_ceil as isize);

									let clip_min = c1.min + (c2.min - c1.min) * fract;
									let clip_max = c1.max + (c2.max - c1.max) * fract;
									let clip_rect =
										egui::Rect::from_min_max(clip_min, clip_max);

									// apply pulse to the current focus
									let dist_from_center = v.abs().min(1.0);
									let current_pulse = 1.0
										+ (pulse - 1.0)
											* (1.0 - 0.5 * dist_from_center);
									let final_size =
										interpolated_size * current_pulse;

									let final_rect = egui::Rect::from_center_size(
										interpolated_center,
										final_size,
									);
									let uv = egui::Rect::from_min_max(
										egui::pos2(0.0, 0.0),
										egui::pos2(1.0, 1.0),
									);

									if final_rect.width() > 0.1
										&& final_rect.height() > 0.1
									{
										gallery_images.push((
											offset,
											off_texture.id(),
											final_rect,
											clip_rect.intersect(ui.clip_rect()),
											uv,
										));
									}
								}
							}

							// Keep the two images around the virtual focus above the
							// side previews, and blend their overlap continuously.
							gallery_images.sort_by_key(|(offset, ..)| {
								if *offset == vc_ceil {
									2
								} else if *offset == vc_floor {
									1
								} else {
									0
								}
							});
							let back_rect = gallery_images
								.iter()
								.find(|(offset, ..)| *offset == vc_floor)
								.map(|(_, _, rect, clip, _)| rect.intersect(*clip));
							for (offset, texture, image_rect, clip, uv) in
								gallery_images
							{
								let mut painter = ui.painter().clone();
								painter.set_clip_rect(clip);
								if offset == vc_ceil
									&& vc_floor != vc_ceil && let Some(back_rect) =
									back_rect
								{
									let blend =
										vc_fract * vc_fract * (3.0 - 2.0 * vc_fract);
									Self::paint_gallery_overlap(
										&painter, texture, image_rect, uv, back_rect,
										blend,
									);
								} else {
									painter.image(
										texture,
										image_rect,
										uv,
										egui::Color32::WHITE,
									);
								}
							}

							#[cfg(feature = "video")]
							if media.current_is_playable() && gallery_settled {
								let post_aspect =
									browser.current_post().and_then(|post| {
										(post.file.width > 0 && post.file.height > 0)
											.then_some(
												post.file.width as f32
													/ post.file.height as f32,
											)
									});
								let video_rect = media
									.get_current_media()
									.map(|preview| {
										let size = preview.texture().size_vec2();
										Self::contained_aspect_rect(
											center_rect,
											size.x / size.y.max(1.0),
										)
									})
									.or_else(|| {
										post_aspect.map(|aspect| {
											Self::contained_aspect_rect(
												center_rect,
												aspect,
											)
										})
									})
									.or_else(|| {
										media.current_video_size().map(
											|(width, height)| {
												Self::contained_aspect_rect(
													center_rect,
													width as f32
														/ height.max(1) as f32,
												)
											},
										)
									})
									.unwrap_or(center_rect);
								Self::render_current_video(
									ui,
									media,
									Some(video_rect),
								);
							}
						});
					}
				}
			}
		} else if media.current_is_playable() {
			#[cfg(feature = "video")]
			Self::render_current_video(ui, media, None);
		} else if media.is_loading() {
			ui.centered_and_justified(|ui| {
				ui.spinner();
			});
		}

		self.user_has_panned = user_panned;
	}

	#[cfg(feature = "video")]
	fn render_current_video(
		ui: &mut egui::Ui,
		media: &MediaPane,
		target_rect: Option<egui::Rect>,
	) {
		let available_rect =
			target_rect.unwrap_or_else(|| ui.available_rect_before_wrap());
		if let Some(player) = media.current_video_player() {
			let rect = target_rect.unwrap_or_else(|| {
				let (width, height) = player.decoded_frame_size().unwrap_or((16, 9));
				Self::contained_aspect_rect(
					available_rect,
					width as f32 / height.max(1) as f32,
				)
			});
			ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
				player.in_sized(
					ui,
					rect.width().max(16.0),
					Some(rect.height().max(16.0)),
				);
			});
		}
	}

	#[cfg(feature = "video")]
	fn contained_aspect_rect(space: egui::Rect, aspect: f32) -> egui::Rect {
		let aspect = aspect.max(0.001);
		let space_aspect = space.width() / space.height().max(1.0);
		let size = if space_aspect > aspect {
			eframe::egui::vec2(space.height() * aspect, space.height())
		} else {
			eframe::egui::vec2(space.width(), space.width() / aspect)
		};
		eframe::egui::Rect::from_center_size(space.center(), size)
	}
}
