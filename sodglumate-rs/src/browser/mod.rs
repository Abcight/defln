use crate::api::Post;
use crate::reactor::{Command, ComponentResponse, Event, Message};
use crate::types::{MediaKind, NavDirection};
use std::collections::VecDeque;

pub struct ContentBrowser {
	posts: Vec<Post>,
	current_index: usize,
	current_page: u32,
	link_generation: u64,
	pending_link: Option<u64>,
	children_source: Option<u64>,
	children_generation: u64,
	children_pending: VecDeque<u64>,
	validated_children: Vec<Post>,
}

impl ContentBrowser {
	pub fn new() -> Self {
		log::info!("Initializing");
		Self {
			posts: Vec::new(),
			current_index: 0,
			current_page: 1,
			link_generation: 0,
			pending_link: None,
			children_source: None,
			children_generation: 0,
			children_pending: VecDeque::new(),
			validated_children: Vec::new(),
		}
	}

	pub fn observe(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::LinkCandidateLoaded {
				post_id,
				generation,
				result,
			} => {
				if !self.child_request_is_current(*post_id, *generation) {
					return ComponentResponse::none();
				}
				self.children_pending.pop_front();
				match result {
					Ok(post) if post.id == *post_id && supported_link_media(post) => {
						self.validated_children.push((**post).clone())
					}
					Ok(_) => log::debug!(
						"Omitting child post {post_id}: no supported, accessible media"
					),
					Err(error) => {
						log::warn!(
							"Could not validate child post {post_id}: {error}"
						);
					}
				}
				let capacity = 8 - usize::from(
					self.current_post()
						.is_some_and(|post| post.relationships.parent_id.is_some()),
				);
				if self.validated_children.len() >= capacity {
					self.children_pending.clear();
				}
				self.next_child_request()
			}
			Event::LinkedPostLoaded { generation, result } => {
				if !self.link_request_is_current(*generation) {
					return ComponentResponse::none();
				}
				let target_id = self.pending_link.take().unwrap();
				let post = match result {
					Ok(post) => post,
					Err(error) => {
						log::warn!("Could not open post {target_id}: {error}");
						return ComponentResponse::none();
					}
				};
				if post.id != target_id || !supported_link_media(post) {
					log::warn!(
						"Post {target_id} has no supported, accessible media in this build."
					);
					return ComponentResponse::none();
				}
				self.clear_children();
				self.posts[self.current_index] = (**post).clone();
				let mut response = self.emit_current_post_changed();
				response.messages.push(Message::Event(Event::Navigated));
				response
			}
			Event::SearchCompleted {
				posts,
				page,
				is_new,
			} => {
				let filtered_posts: Vec<Post> = posts
					.iter()
					.filter(|p| MediaKind::from_extension(&p.file.ext).is_some())
					.cloned()
					.collect();

				if *is_new {
					self.cancel_link();
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
			_ => ComponentResponse::none(),
		}
	}

	pub fn handle(&mut self, command: &Command) -> ComponentResponse {
		match command {
			Command::PrepareLinks { source_id } => {
				let Some(post) = self.current_post() else {
					return ComponentResponse::none();
				};
				if post.id != *source_id || self.children_source == Some(*source_id) {
					return ComponentResponse::none();
				}
				let children = post.relationships.children.clone();
				log::info!(
					"Checking {} child posts for post {source_id}",
					children.len()
				);
				self.clear_children();
				self.children_source = Some(*source_id);
				for id in children {
					if !self.children_pending.contains(&id) {
						self.children_pending.push_back(id);
					}
				}
				self.next_child_request()
			}
			Command::Search { .. } => {
				self.cancel_link();
				ComponentResponse::none()
			}
			Command::OpenLinkedPost {
				source_id,
				target_id,
			} => {
				let Some(post) = self.current_post() else {
					return ComponentResponse::none();
				};
				if post.id != *source_id
					|| (post.relationships.parent_id != Some(*target_id)
						&& !post.relationships.children.contains(target_id))
				{
					return ComponentResponse::none();
				}
				let validated = self
					.validated_children
					.iter()
					.find(|post| post.id == *target_id)
					.cloned();
				self.cancel_link();
				self.pending_link = Some(*target_id);
				log::info!("Opening linked post {target_id} from post {source_id}");
				if let Some(post) = validated {
					return self.observe(&Event::LinkedPostLoaded {
						generation: self.link_generation,
						result: Ok(Box::new(post)),
					});
				}
				ComponentResponse::command(Command::FetchLinkedPost {
					post_id: *target_id,
					generation: self.link_generation,
				})
			}
			Command::Navigate(direction) => {
				self.cancel_link();
				if self.posts.is_empty() {
					log::debug!("Navigate ignored: no posts");
					return ComponentResponse::none();
				}

				let old_index = self.current_index;
				match direction {
					NavDirection::Next => {
						self.current_index =
							(self.current_index + 1) % self.posts.len();
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
							self.current_index = (self.current_index
								+ count as usize)
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

				let mut response = self.emit_current_post_changed();
				response.messages.push(Message::Event(Event::Navigated));
				response
			}
			_ => ComponentResponse::none(),
		}
	}

	fn clear_children(&mut self) {
		self.children_generation = self.children_generation.wrapping_add(1);
		self.children_source = None;
		self.children_pending.clear();
		self.validated_children.clear();
	}

	fn next_child_request(&self) -> ComponentResponse {
		match self.children_pending.front() {
			Some(&post_id) => {
				ComponentResponse::command(Command::FetchLinkCandidate {
					post_id,
					generation: self.children_generation,
				})
			}
			None => ComponentResponse::none(),
		}
	}

	pub fn child_request_is_current(&self, post_id: u64, generation: u64) -> bool {
		self.children_generation == generation
			&& self.children_pending.front() == Some(&post_id)
	}

	pub fn links_post(&self) -> Option<Post> {
		let mut post = self.current_post()?.clone();
		post.relationships.children = self.validated_child_ids();
		Some(post)
	}

	pub fn validated_child_ids(&self) -> Vec<u64> {
		self.validated_children.iter().map(|post| post.id).collect()
	}
	#[cfg(test)]
	fn checking_children(&self) -> bool {
		!self.children_pending.is_empty()
	}

	fn cancel_link(&mut self) {
		self.clear_children();
		self.link_generation = self.link_generation.wrapping_add(1);
		self.pending_link = None;
	}

	pub fn link_request_is_current(&self, generation: u64) -> bool {
		self.pending_link.is_some() && self.link_generation == generation
	}

	#[cfg(test)]
	fn link_loading(&self) -> bool {
		self.pending_link.is_some()
	}

	fn emit_current_post_changed(&self) -> ComponentResponse {
		let post = self.posts.get(self.current_index).cloned();
		let mut messages = Vec::new();

		if let Some(post) = post {
			// Request media load with sample and full URLs
			let kind =
				MediaKind::from_extension(&post.file.ext).unwrap_or(MediaKind::Image);
			let sample_url = preview_url(&post);
			let full_url = post.file.url.clone();

			if sample_url.is_some() || full_url.is_some() {
				log::debug!(
					"Requesting media load: sample={:?}, full={:?} (kind={:?})",
					sample_url,
					full_url,
					kind
				);
				messages.push(Message::Command(Command::LoadMedia {
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
				messages.push(Message::Command(Command::FetchNextPage));
			}

			// Emit prefetch hints for next 30 posts
			let prefetch_urls: Vec<(Option<String>, Option<String>, MediaKind)> = (1
				..=30)
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
				messages.push(Message::Command(Command::PrefetchMedia {
					urls: prefetch_urls,
				}));
			}
		}

		ComponentResponse::messages(messages)
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

fn supported_link_media(post: &Post) -> bool {
	let kind = MediaKind::from_extension(&post.file.ext);
	!post.flags.deleted
		&& post
			.file
			.url
			.as_deref()
			.is_some_and(|url| !url.trim().is_empty())
		&& kind.is_some()
		&& (cfg!(feature = "video")
			|| kind == Some(MediaKind::Image)
			|| post.file.ext.eq_ignore_ascii_case("gif"))
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
		Event::SearchCompleted {
			posts,
			page: 1,
			is_new,
		}
	}

	fn linked_browser() -> ContentBrowser {
		let mut browser = ContentBrowser::new();
		let mut source = post(2, "jpg");
		source.relationships.parent_id = Some(10);
		source.relationships.children = vec![20, 30];
		browser.observe(&posts_received(
			vec![post(1, "jpg"), source, post(3, "png")],
			true,
		));
		browser.handle(&Command::Navigate(NavDirection::Next));
		browser
	}

	fn open_link(browser: &mut ContentBrowser, target_id: u64) -> u64 {
		let source_id = browser.current_post().unwrap().id;
		let response = browser.handle(&Command::OpenLinkedPost {
			source_id,
			target_id,
		});
		match response.messages.as_slice() {
			[
				Message::Command(Command::FetchLinkedPost {
					post_id,
					generation,
				}),
			] => {
				assert_eq!(*post_id, target_id);
				*generation
			}
			_ => panic!("expected one linked-post request"),
		}
	}

	fn child_response(
		browser: &mut ContentBrowser,
		post_id: u64,
		result: Result<Box<Post>, String>,
	) -> ComponentResponse {
		browser.observe(&Event::LinkCandidateLoaded {
			post_id,
			generation: browser.children_generation,
			result,
		})
	}

	#[test]
	fn child_tiles_wait_for_validation_and_skip_inaccessible_media() {
		let mut browser = linked_browser();
		browser.posts[1].relationships.children = vec![20, 21, 22, 23, 24, 25, 26];
		assert!(
			browser
				.links_post()
				.unwrap()
				.relationships
				.children
				.is_empty()
		);
		let response = browser.handle(&Command::PrepareLinks { source_id: 2 });
		assert!(matches!(
			response.messages.as_slice(),
			[Message::Command(Command::FetchLinkCandidate {
				post_id: 20,
				..
			})]
		));
		assert!(
			browser
				.handle(&Command::PrepareLinks { source_id: 2 })
				.messages
				.is_empty()
		);
		let mut missing = post(20, "jpg");
		missing.file.url = None;
		let mut deleted = post(21, "jpg");
		deleted.flags.deleted = true;
		let mut blank = post(22, "jpg");
		blank.file.url = Some(" ".into());
		for item in [missing, deleted, blank, post(23, "swf")] {
			child_response(&mut browser, item.id, Ok(Box::new(item)));
			assert!(browser.validated_child_ids().is_empty());
		}
		child_response(&mut browser, 24, Err("HTTP 404".into()));
		child_response(&mut browser, 25, Ok(Box::new(post(25, "gif"))));
		child_response(&mut browser, 26, Ok(Box::new(post(26, "png"))));
		assert_eq!(browser.validated_child_ids(), [25, 26]);
		assert_eq!(
			browser.links_post().unwrap().relationships.children,
			[25, 26]
		);
		assert!(!browser.checking_children());
		assert_eq!(browser.current_post().unwrap().id, 2);
	}

	#[test]
	fn child_limit_counts_valid_posts_instead_of_unchecked_ids() {
		for parent in [None, Some(10)] {
			let mut browser = linked_browser();
			browser.posts[1].relationships.parent_id = parent;
			browser.posts[1].relationships.children = (20..40).collect();
			browser.handle(&Command::PrepareLinks { source_id: 2 });
			for id in 20..23 {
				child_response(&mut browser, id, Ok(Box::new(post(id, "swf"))));
			}
			let capacity = if parent.is_some() { 7 } else { 8 };
			for id in 23..23 + capacity {
				child_response(&mut browser, id, Ok(Box::new(post(id, "jpg"))));
			}
			assert_eq!(browser.validated_children.len(), capacity as usize);
			assert!(!browser.checking_children());
			assert_eq!(
				browser.validated_child_ids(),
				(23..23 + capacity).collect::<Vec<_>>()
			);
		}
	}

	#[test]
	fn stale_child_validation_cannot_populate_another_posts_menu() {
		let mut browser = linked_browser();
		browser.handle(&Command::PrepareLinks { source_id: 2 });
		let generation = browser.children_generation;
		browser.handle(&Command::Navigate(NavDirection::Next));
		assert!(!browser.child_request_is_current(20, generation));
		browser.observe(&Event::LinkCandidateLoaded {
			post_id: 20,
			generation,
			result: Ok(Box::new(post(20, "jpg"))),
		});
		assert!(browser.validated_child_ids().is_empty());
		assert!(!browser.checking_children());
	}

	#[test]
	fn validated_child_opens_in_place_without_a_second_api_request() {
		let mut browser = linked_browser();
		browser.handle(&Command::PrepareLinks { source_id: 2 });
		child_response(&mut browser, 20, Ok(Box::new(post(20, "jpg"))));
		let response = browser.handle(&Command::OpenLinkedPost {
			source_id: 2,
			target_id: 20,
		});
		assert_eq!(browser.current_post().unwrap().id, 20);
		assert_eq!(browser.current_index(), 1);
		assert_eq!(browser.posts_len(), 3);
		assert!(!response.messages.iter().any(|message| matches!(
			message,
			Message::Command(Command::FetchLinkedPost { .. })
		)));
		assert!(browser.validated_child_ids().is_empty());
	}

	#[test]
	fn child_video_support_follows_the_build_feature() {
		assert_eq!(
			supported_link_media(&post(20, "mp4")),
			cfg!(feature = "video")
		);
		assert!(supported_link_media(&post(20, "gif")));
	}

	#[test]
	fn linked_post_replaces_only_the_focused_slot_and_can_be_followed_again() {
		let mut browser = linked_browser();
		let generation = open_link(&mut browser, 10);
		assert!(browser.link_loading());
		assert_eq!(browser.current_post().unwrap().id, 2);
		let mut parent = post(10, "jpg");
		parent.relationships.children = vec![2];
		let response = browser.observe(&Event::LinkedPostLoaded {
			generation,
			result: Ok(Box::new(parent)),
		});
		assert_eq!(
			browser.posts.iter().map(|post| post.id).collect::<Vec<_>>(),
			[1, 10, 3]
		);
		assert_eq!(browser.current_index(), 1);
		assert_eq!(browser.current_page, 1);
		assert!(!browser.link_loading());
		assert!(response.messages.iter().any(|message| matches!(message, Message::Command(Command::LoadMedia { full_url: Some(url), .. }) if url.ends_with("10.jpg"))));
		assert!(
			response
				.messages
				.iter()
				.any(|message| matches!(message, Message::Event(Event::Navigated)))
		);
		let generation = open_link(&mut browser, 2);
		browser.observe(&Event::LinkedPostLoaded {
			generation,
			result: Ok(Box::new(post(2, "jpg"))),
		});
		assert_eq!(browser.current_post().unwrap().id, 2);
		browser.handle(&Command::Navigate(NavDirection::Next));
		assert_eq!(browser.current_post().unwrap().id, 3);
	}

	#[test]
	fn navigation_and_search_invalidate_pending_link_responses() {
		for command in [
			Command::Navigate(NavDirection::Next),
			Command::Search {
				query: "new".into(),
				page: 1,
			},
		] {
			let mut browser = linked_browser();
			let generation = open_link(&mut browser, 20);
			browser.handle(&command);
			assert!(!browser.link_request_is_current(generation));
			let posts = browser.posts.clone();
			for result in [Ok(Box::new(post(20, "jpg"))), Err("late failure".into())]
			{
				assert!(
					browser
						.observe(&Event::LinkedPostLoaded { generation, result })
						.messages
						.is_empty()
				);
				assert_eq!(browser.posts, posts);
			}
		}
	}

	#[test]
	fn latest_link_wins_and_pagination_does_not_cancel_it() {
		let mut browser = linked_browser();
		let old = open_link(&mut browser, 20);
		let generation = open_link(&mut browser, 30);
		browser.observe(&posts_received(vec![post(4, "jpg")], false));
		browser.observe(&Event::LinkedPostLoaded {
			generation: old,
			result: Ok(Box::new(post(20, "jpg"))),
		});
		assert_eq!(browser.current_post().unwrap().id, 2);
		assert!(browser.link_loading());
		browser.observe(&Event::LinkedPostLoaded {
			generation,
			result: Ok(Box::new(post(30, "jpg"))),
		});
		assert_eq!(
			browser.posts.iter().map(|post| post.id).collect::<Vec<_>>(),
			[1, 30, 3, 4]
		);
	}

	#[test]
	fn unavailable_links_leave_the_current_post_intact_and_can_be_retried() {
		let mut inaccessible = post(20, "jpg");
		inaccessible.file.url = None;
		for result in [
			Err("HTTP 404".into()),
			Ok(Box::new(inaccessible)),
			Ok(Box::new(post(20, "swf"))),
		] {
			let mut browser = linked_browser();
			let generation = open_link(&mut browser, 20);
			browser.observe(&Event::LinkedPostLoaded { generation, result });
			assert_eq!(browser.current_post().unwrap().id, 2);
			assert!(!browser.link_loading());
			open_link(&mut browser, 20);
		}
	}

	#[test]
	fn ignores_links_from_an_old_menu_or_unrelated_post() {
		let mut browser = linked_browser();
		for (source_id, target_id) in [(1, 20), (2, 99)] {
			assert!(
				browser
					.handle(&Command::OpenLinkedPost {
						source_id,
						target_id
					})
					.messages
					.is_empty()
			);
			assert!(!browser.link_loading());
		}
	}

	#[test]
	fn filters_unsupported_posts_and_starts_at_first_media() {
		let mut browser = ContentBrowser::new();
		browser.observe(&posts_received(
			vec![post(1, "jpg"), post(2, "mp4"), post(3, "swf")],
			true,
		));

		assert_eq!(browser.posts_len(), 2);
		assert_eq!(browser.current_post().map(|post| post.id), Some(1));
	}

	#[test]
	fn navigation_wraps_and_skip_stays_within_results() {
		let mut browser = ContentBrowser::new();
		browser.observe(&posts_received(vec![post(1, "jpg"), post(2, "png")], true));

		browser.handle(&Command::Navigate(NavDirection::Prev));
		assert_eq!(browser.current_post().map(|post| post.id), Some(2));

		browser.handle(&Command::Navigate(NavDirection::Skip(10)));
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

		let response = browser.observe(&posts_received(vec![playable], true));
		assert!(response.messages.iter().any(|message| matches!(
			message,
			Message::Command(Command::LoadMedia { sample_url, .. })
				if sample_url.as_deref() == Some("https://example.test/preview.jpg")
		)));
	}
}
