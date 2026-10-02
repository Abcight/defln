use crate::api::Post;
use crate::metadata::PostMetadataStore;
use crate::reactor::{Command, ComponentResponse, Event, Message};
use crate::types::{MediaKind, NavDirection};

const PREFETCH_POSTS: usize = 30;

/// Navigation state for the active search session.
pub struct ContentBrowser {
	posts: Vec<Post>,
	current_index: usize,
	current_page: u32,
	pending_link: Option<(u64, u64)>,
}

impl ContentBrowser {
	pub fn new() -> Self {
		Self {
			posts: Vec::new(),
			current_index: 0,
			current_page: 1,
			pending_link: None,
		}
	}

	pub fn observe(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::SearchCompleted {
				posts,
				page,
				is_new,
			} => {
				let posts: Vec<_> = posts
					.iter()
					.filter(|post| supported_media(post))
					.cloned()
					.collect();
				if *is_new {
					self.pending_link = None;
					self.posts = posts;
					self.current_index = 0;
				} else {
					self.posts.extend(posts);
				}
				self.current_page = *page;
				self.emit_current_post_changed()
			}
			Event::LinkedPostLoaded { post_id, result } => {
				let Some((source_id, target_id)) = self.pending_link else {
					return ComponentResponse::none();
				};
				if *post_id != target_id
					|| self.current_post().map(|post| post.id) != Some(source_id)
				{
					return ComponentResponse::none();
				}
				self.pending_link = None;
				let Ok(post) = result else {
					return ComponentResponse::none();
				};
				if post.id != target_id || !supported_media(post) {
					return ComponentResponse::none();
				}
				self.posts[self.current_index] = (**post).clone();
				let mut response = self.emit_current_post_changed();
				response.messages.push(Message::Event(Event::Navigated));
				response
			}
			_ => ComponentResponse::none(),
		}
	}

	pub fn handle(
		&mut self,
		command: &Command,
		metadata: &PostMetadataStore,
	) -> ComponentResponse {
		match command {
			Command::PrepareLinks { source_id } => {
				if self.current_post().map(|post| post.id) != Some(*source_id) {
					return ComponentResponse::none();
				}
				ComponentResponse::command(Command::EnsureMetadata {
					post_ids: (0..self.posts.len().min(PREFETCH_POSTS + 1))
						.filter_map(|offset| self.get_post_relative(offset as isize))
						.map(|post| post.id)
						.collect(),
				})
			}
			Command::Search { .. } => {
				self.pending_link = None;
				ComponentResponse::none()
			}
			Command::OpenLinkedPost {
				source_id,
				target_id,
			} => {
				let Some(source) = self.current_post() else {
					return ComponentResponse::none();
				};
				if source.id != *source_id || !metadata.can_open(source, *target_id) {
					return ComponentResponse::none();
				}
				self.pending_link = Some((*source_id, *target_id));
				if let Some(post) = metadata.post(*target_id).cloned() {
					return self.observe(&Event::LinkedPostLoaded {
						post_id: *target_id,
						result: Ok(Box::new(post)),
					});
				}
				ComponentResponse::command(Command::FetchLinkedPost {
					post_id: *target_id,
				})
			}
			Command::Navigate(direction) => {
				if self.posts.is_empty() {
					return ComponentResponse::none();
				}
				self.pending_link = None;
				match direction {
					NavDirection::Next => {
						self.current_index =
							(self.current_index + 1) % self.posts.len()
					}
					NavDirection::Prev => {
						self.current_index = self
							.current_index
							.checked_sub(1)
							.unwrap_or(self.posts.len() - 1)
					}
					NavDirection::Skip(count) if *count > 0 => {
						self.current_index = (self.current_index + *count as usize)
							.min(self.posts.len() - 1)
					}
					NavDirection::Skip(count) => {
						self.current_index =
							self.current_index.saturating_sub((-*count) as usize)
					}
				}
				let mut response = self.emit_current_post_changed();
				response.messages.push(Message::Event(Event::Navigated));
				response
			}
			_ => ComponentResponse::none(),
		}
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
	pub fn is_empty(&self) -> bool {
		self.posts.is_empty()
	}
	pub fn get_post_relative(&self, offset: isize) -> Option<&Post> {
		if self.posts.is_empty() {
			return None;
		}
		let len = self.posts.len() as isize;
		self.posts
			.get((self.current_index as isize + offset).rem_euclid(len) as usize)
	}

	pub fn related_posts(&self, metadata: &PostMetadataStore) -> Vec<RelatedPost> {
		let Some(post) = self.current_post() else {
			return Vec::new();
		};
		if let Some((previous, next)) = metadata.pool_neighbors(post) {
			return previous
				.into_iter()
				.chain(next)
				.map(|id| RelatedPost {
					id,
					post: metadata
						.post(id)
						.and_then(|post| supported_media(post).then(|| post.clone())),
				})
				.collect();
		}
		metadata
			.related_posts(post)
			.into_iter()
			.map(|post| RelatedPost {
				id: post.id,
				post: Some(post),
			})
			.collect()
	}
	pub fn has_valid_related_posts(&self, metadata: &PostMetadataStore) -> bool {
		!self.related_posts(metadata).is_empty()
	}
	pub fn pool_navigation_targets(
		&self,
		metadata: &PostMetadataStore,
	) -> Option<(Option<u64>, Option<u64>)> {
		self.current_post()
			.and_then(|post| metadata.pool_neighbors(post))
	}
	pub fn direct_navigation_targets(
		&self,
		metadata: &PostMetadataStore,
	) -> Option<(Option<u64>, Option<u64>)> {
		self.current_post()
			.and_then(|post| metadata.direct_targets(post))
	}
	pub fn links_post(&self, metadata: &PostMetadataStore) -> Option<Post> {
		let mut post = self.current_post()?.clone();
		post.relationships
			.children
			.retain(|id| metadata.post(*id).is_some_and(supported_media));
		Some(post)
	}

	fn emit_current_post_changed(&self) -> ComponentResponse {
		let Some(post) = self.current_post() else {
			return ComponentResponse::none();
		};
		let kind =
			MediaKind::from_extension(&post.file.ext).unwrap_or(MediaKind::Image);
		let sample_url = if post.sample.has {
			post.sample.url.clone().or_else(|| post.preview.url.clone())
		} else {
			post.preview.url.clone()
		};
		let mut messages = vec![Message::Command(Command::PrepareLinks {
			source_id: post.id,
		})];
		if sample_url.is_some() || post.file.url.is_some() {
			messages.push(Message::Command(Command::LoadMedia {
				sample_url,
				full_url: post.file.url.clone(),
				kind,
			}));
		}
		if self.posts.len().saturating_sub(self.current_index + 1) < 5 {
			messages.push(Message::Command(Command::FetchNextPage));
		}
		let urls = (1..=PREFETCH_POSTS)
			.filter_map(|offset| self.get_post_relative(offset as isize))
			.filter_map(|post| {
				Some((
					if post.sample.has {
						post.sample.url.clone().or_else(|| post.preview.url.clone())
					} else {
						post.preview.url.clone()
					},
					post.file.url.clone(),
					MediaKind::from_extension(&post.file.ext)?,
				))
			})
			.collect();
		messages.push(Message::Command(Command::PrefetchMedia { urls }));
		ComponentResponse::messages(messages)
	}
}

impl Default for ContentBrowser {
	fn default() -> Self {
		Self::new()
	}
}

#[derive(Clone, Debug)]
pub struct RelatedPost {
	pub id: u64,
	pub post: Option<Post>,
}

fn supported_media(post: &Post) -> bool {
	!post.flags.deleted
		&& post
			.file
			.url
			.as_deref()
			.is_some_and(|url| !url.trim().is_empty())
		&& MediaKind::from_extension(&post.file.ext).is_some()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::api::Pool;

	fn post(id: u64) -> Post {
		let mut post = Post {
			id,
			..Post::default()
		};
		post.file.ext = "jpg".into();
		post.file.url = Some(format!("https://example.test/{id}.jpg"));
		post
	}

	#[test]
	fn preparation_declares_the_visible_metadata_interest() {
		let event = Event::SearchCompleted {
			posts: vec![post(1), post(2)],
			page: 1,
			is_new: true,
		};
		let mut browser = ContentBrowser::new();
		let mut metadata = PostMetadataStore::new();
		metadata.observe(&event);
		browser.observe(&event);

		let response =
			browser.handle(&Command::PrepareLinks { source_id: 1 }, &metadata);
		assert!(matches!(
			response.messages.as_slice(),
			[Message::Command(Command::EnsureMetadata { post_ids })]
				if post_ids == &[1, 2]
		));
	}

	#[test]
	fn known_pool_neighbors_are_visible_before_their_post_metadata_arrives() {
		let event = Event::SearchCompleted {
			posts: vec![post(2)],
			page: 1,
			is_new: true,
		};
		let mut browser = ContentBrowser::new();
		let mut metadata = PostMetadataStore::new();
		metadata.observe(&event);
		browser.observe(&event);
		metadata.observe(&Event::PoolLoaded {
			pool_id: 99,
			result: Ok(Box::new(Pool {
				post_ids: vec![1, 2, 3],
			})),
		});

		let related = browser.related_posts(&metadata);
		assert_eq!(
			related.iter().map(|post| post.id).collect::<Vec<_>>(),
			[1, 3]
		);
		assert!(related.iter().all(|post| post.post.is_none()));
	}
}
