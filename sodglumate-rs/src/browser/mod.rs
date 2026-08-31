use crate::api::Post;
use crate::reactor::{BrowserEvent, ComponentResponse, Event, GatewayEvent, MediaEvent};
use crate::types::{MediaKind, NavDirection};

pub struct ContentBrowser {
	posts: Vec<Post>,
	current_index: usize,
	current_page: u32,
}

impl ContentBrowser {
	pub fn new() -> Self {
		log::info!("Initializing");
		Self {
			posts: Vec::new(),
			current_index: 0,
			current_page: 1,
		}
	}

	pub fn handle(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::Browser(BrowserEvent::PostsReceived {
				posts,
				page,
				is_new,
			}) => {
				let filtered_posts: Vec<Post> = posts
					.iter()
					.filter(|p| MediaKind::from_extension(&p.file.ext).is_some())
					.cloned()
					.collect();

				if *is_new {
					log::info!(
						"New search results: page={}, posts={}",
						page,
						filtered_posts.len(),
					);
					self.posts = filtered_posts;
					self.current_index = 0;
					self.current_page = *page;
				} else {
					log::info!(
						"Appended results: page={}, new_posts={}",
						page,
						filtered_posts.len(),
					);
					self.posts.extend(filtered_posts);
					self.current_page = *page;
				}

				if !self.posts.is_empty() {
					self.emit_current_post_changed()
				} else {
					log::warn!("Received empty posts");
					ComponentResponse::none()
				}
			}
			Event::Browser(BrowserEvent::Navigate { direction }) => {
				if self.posts.is_empty() {
					log::debug!("Navigate ignored: no posts");
					return ComponentResponse::none();
				}

				let old_index = self.current_index;
				match direction {
					NavDirection::Next => {
						self.current_index = (self.current_index + 1) % self.posts.len();
					}
					NavDirection::Prev => {
						if self.current_index == 0 {
							self.current_index = self.posts.len().saturating_sub(1);
						} else {
							self.current_index -= 1;
						}
					}
					NavDirection::Skip(count) => {
						let count = *count;
						if count > 0 {
							self.current_index = (self.current_index + count as usize)
								.min(self.posts.len().saturating_sub(1));
						} else {
							self.current_index =
								self.current_index.saturating_sub((-count) as usize);
						}
					}
				}
				log::info!(
					"Navigate {:?}: {} -> {} (of {})",
					direction,
					old_index,
					self.current_index,
					self.posts.len()
				);

				self.emit_current_post_changed()
			}
			_ => ComponentResponse::none(),
		}
	}

	fn emit_current_post_changed(&self) -> ComponentResponse {
		let post = self.posts.get(self.current_index).cloned();
		let mut events = Vec::new();

		if let Some(post) = post {
			// Request media load with sample and full URLs
			let kind = MediaKind::from_extension(&post.file.ext).unwrap_or(MediaKind::Image);
			let sample_url = preview_url(&post);
			let full_url = post.file.url.clone();

			if sample_url.is_some() || full_url.is_some() {
				log::debug!(
					"Requesting media load: sample={:?}, full={:?} (kind={:?})",
					sample_url,
					full_url,
					kind
				);
				events.push(Event::Media(MediaEvent::LoadRequest {
					sample_url,
					full_url,
					kind,
				}));
			}

			// Check if near end for prefetching
			let remaining = self.posts.len().saturating_sub(self.current_index + 1);
			if remaining < 5 {
				log::debug!(
					"Near end of results (remaining={}), requesting next page",
					remaining
				);
				events.push(Event::Gateway(GatewayEvent::FetchNextPage));
			}

			// Emit prefetch hints for next 30 posts
			let prefetch_urls: Vec<(Option<String>, Option<String>, MediaKind)> = (1..=30)
				.filter_map(|i| {
					let idx = (self.current_index + i) % self.posts.len();
					self.posts.get(idx).and_then(|p| {
						let kind = MediaKind::from_extension(&p.file.ext)?;
						let sample_url = preview_url(p);
						Some((sample_url, p.file.url.clone(), kind))
					})
				})
				.collect();

			if !prefetch_urls.is_empty() {
				log::debug!("Requesting prefetch for {} URLs", prefetch_urls.len());
				events.push(Event::Media(MediaEvent::Prefetch {
					urls: prefetch_urls,
				}));
			}
		}

		ComponentResponse::emit_many(events)
	}

	pub fn current_post(&self) -> Option<&Post> {
		self.posts.get(self.current_index)
	}

	pub fn current_index(&self) -> usize {
		self.current_index
	}

	pub fn posts_len(&self) -> usize {
		self.posts.len()
	}

	pub fn get_post_relative(&self, offset: isize) -> Option<&Post> {
		if self.posts.is_empty() {
			return None;
		}
		let len = self.posts.len() as isize;
		let idx = (self.current_index as isize + offset).rem_euclid(len) as usize;
		self.posts.get(idx)
	}

	pub fn is_empty(&self) -> bool {
		self.posts.is_empty()
	}
}

impl Default for ContentBrowser {
	fn default() -> Self {
		Self::new()
	}
}

fn preview_url(post: &Post) -> Option<String> {
	if post.sample.has {
		post.sample.url.clone().or_else(|| post.preview.url.clone())
	} else {
		post.preview.url.clone()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn post(id: u64, ext: &str) -> Post {
		let mut post = Post {
			id,
			..Post::default()
		};
		post.file.ext = ext.to_owned();
		post.file.url = Some(format!("https://example.test/{id}.{ext}"));
		post
	}

	fn posts_received(posts: Vec<Post>, is_new: bool) -> Event {
		Event::Browser(BrowserEvent::PostsReceived {
			posts,
			page: 1,
			is_new,
		})
	}

	#[test]
	fn filters_unsupported_posts_and_starts_at_first_media() {
		let mut browser = ContentBrowser::new();
		browser.handle(&posts_received(
			vec![post(1, "jpg"), post(2, "mp4"), post(3, "swf")],
			true,
		));

		assert_eq!(browser.posts_len(), 2);
		assert_eq!(browser.current_post().map(|post| post.id), Some(1));
	}

	#[test]
	fn navigation_wraps_and_skip_stays_within_results() {
		let mut browser = ContentBrowser::new();
		browser.handle(&posts_received(vec![post(1, "jpg"), post(2, "png")], true));

		browser.handle(&Event::Browser(BrowserEvent::Navigate {
			direction: NavDirection::Prev,
		}));
		assert_eq!(browser.current_post().map(|post| post.id), Some(2));

		browser.handle(&Event::Browser(BrowserEvent::Navigate {
			direction: NavDirection::Skip(10),
		}));
		assert_eq!(browser.current_post().map(|post| post.id), Some(2));
	}

	#[test]
	fn accepts_only_decodable_image_formats() {
		assert_eq!(MediaKind::from_extension("JPG"), Some(MediaKind::Image));
		assert_eq!(MediaKind::from_extension("gif"), Some(MediaKind::Playable));
		assert_eq!(MediaKind::from_extension("swf"), None);
		assert_eq!(MediaKind::from_extension("mp4"), Some(MediaKind::Playable));
	}

	#[test]
	fn uses_preview_when_playable_post_has_no_sample() {
		let mut browser = ContentBrowser::new();
		let mut playable = post(1, "gif");
		playable.sample.has = false;
		playable.preview.url = Some("https://example.test/preview.jpg".to_owned());

		let response = browser.handle(&posts_received(vec![playable], true));
		assert!(response.events.iter().any(|event| matches!(
			event,
			Event::Media(MediaEvent::LoadRequest { sample_url, .. })
				if sample_url.as_deref() == Some("https://example.test/preview.jpg")
		)));
	}
}
