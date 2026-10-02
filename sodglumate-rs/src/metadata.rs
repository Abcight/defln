use crate::api::{Pool, Post};
use crate::reactor::{Command, ComponentResponse, Event};
use crate::types::MediaKind;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
enum MetadataRequest {
	Post(u64),
	Pool(u64),
}

type PoolNeighbor = (u64, Option<u64>, Option<u64>);

pub struct PostMetadataStore {
	posts: HashMap<u64, Post>,
	pools: HashMap<u64, Pool>,
	pools_by_post: HashMap<u64, Vec<u64>>,
	pool_neighbors: HashMap<u64, Vec<PoolNeighbor>>,
	interested_posts: HashSet<u64>,
	queued: VecDeque<MetadataRequest>,
	queued_set: HashSet<MetadataRequest>,
	in_flight: Option<MetadataRequest>,
}

impl PostMetadataStore {
	pub fn new() -> Self {
		Self {
			posts: HashMap::new(),
			pools: HashMap::new(),
			pools_by_post: HashMap::new(),
			pool_neighbors: HashMap::new(),
			interested_posts: HashSet::new(),
			queued: VecDeque::new(),
			queued_set: HashSet::new(),
			in_flight: None,
		}
	}

	pub fn observe(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::SearchCompleted { posts, .. } => {
				for post in posts {
					self.insert_post(post.clone());
				}
				ComponentResponse::none()
			}
			Event::LinkCandidateLoaded {
				post_id, result, ..
			}
			| Event::LinkedPostLoaded { post_id, result } => {
				self.complete(MetadataRequest::Post(*post_id));
				if let Ok(post) = result
					&& post.id == *post_id
				{
					self.insert_post((**post).clone());
				}
				self.next_request()
			}
			Event::PoolLoaded {
				pool_id, result, ..
			} => {
				self.complete(MetadataRequest::Pool(*pool_id));
				if let Ok(pool) = result {
					self.insert_pool(*pool_id, (**pool).clone());
				}
				self.next_request()
			}
			_ => ComponentResponse::none(),
		}
	}

	pub fn handle(&mut self, command: &Command) -> ComponentResponse {
		let Command::EnsureMetadata { post_ids } = command else {
			return ComponentResponse::none();
		};
		for post_id in post_ids {
			self.interested_posts.insert(*post_id);
			self.queue_metadata_for_post(*post_id);
		}
		self.next_request()
	}

	pub fn post(&self, post_id: u64) -> Option<&Post> {
		self.posts.get(&post_id)
	}

	pub fn related_posts(&self, post: &Post) -> Vec<Post> {
		let ids = self.pool_neighbors(post).map_or_else(
			|| {
				post.relationships
					.parent_id
					.into_iter()
					.chain(post.relationships.children.iter().copied())
					.collect::<Vec<_>>()
			},
			|(previous, next)| previous.into_iter().chain(next).collect(),
		);
		ids.into_iter()
			.filter_map(|id| self.posts.get(&id))
			.filter(|post| supported_media(post))
			.cloned()
			.collect()
	}

	pub fn pool_neighbors(&self, post: &Post) -> Option<(Option<u64>, Option<u64>)> {
		let neighbors = self.pool_neighbors.get(&post.id)?;
		for pool_id in self.pool_ids_for_post(post) {
			if let Some((_, previous, next)) =
				neighbors.iter().find(|(id, _, _)| *id == pool_id)
			{
				return Some((*previous, *next));
			}
		}
		None
	}

	pub fn direct_targets(&self, post: &Post) -> Option<(Option<u64>, Option<u64>)> {
		self.pool_neighbors(post).or_else(|| {
			let child = match post.relationships.children.as_slice() {
				[] => None,
				[id] => Some(*id),
				_ => return None,
			};
			Some((post.relationships.parent_id, child))
		})
	}

	pub fn can_open(&self, source: &Post, target_id: u64) -> bool {
		source.relationships.parent_id == Some(target_id)
			|| source.relationships.children.contains(&target_id)
			|| self.pool_neighbors(source).is_some_and(|(previous, next)| {
				previous == Some(target_id) || next == Some(target_id)
			})
	}

	fn insert_post(&mut self, post: Post) {
		self.posts.insert(post.id, post);
	}

	fn insert_pool(&mut self, pool_id: u64, pool: Pool) {
		for (index, post_id) in pool.post_ids.iter().enumerate() {
			let pool_ids = self.pools_by_post.entry(*post_id).or_default();
			if !pool_ids.contains(&pool_id) {
				pool_ids.push(pool_id);
			}
			let previous = index
				.checked_sub(1)
				.and_then(|i| pool.post_ids.get(i))
				.copied();
			let next = pool.post_ids.get(index + 1).copied();
			if previous.is_some() || next.is_some() {
				let entries = self.pool_neighbors.entry(*post_id).or_default();
				if !entries.iter().any(|(id, _, _)| *id == pool_id) {
					entries.push((pool_id, previous, next));
				}
			}
		}
		self.pools.insert(pool_id, pool.clone());
		for post_id in pool.post_ids {
			if self.interested_posts.contains(&post_id) {
				self.queue_metadata_for_post(post_id);
			}
		}
	}

	fn pool_ids_for_post(&self, post: &Post) -> Vec<u64> {
		let mut ids = self
			.pools_by_post
			.get(&post.id)
			.cloned()
			.unwrap_or_default();
		for pool_id in &post.pools {
			if !ids.contains(pool_id) {
				ids.push(*pool_id);
			}
		}
		ids
	}

	fn queue_metadata_for_post(&mut self, post_id: u64) {
		let Some(post) = self.posts.get(&post_id).cloned() else {
			return;
		};
		for related_id in post
			.relationships
			.parent_id
			.into_iter()
			.chain(post.relationships.children.iter().copied())
		{
			self.queue(MetadataRequest::Post(related_id));
		}
		for pool_id in self.pool_ids_for_post(&post) {
			if self.pools.contains_key(&pool_id) {
				if let Some((previous, next)) = self.pool_neighbors(&post) {
					for neighbor in previous.into_iter().chain(next) {
						self.queue(MetadataRequest::Post(neighbor));
					}
				}
			} else {
				self.queue(MetadataRequest::Pool(pool_id));
			}
		}
	}

	fn queue(&mut self, request: MetadataRequest) {
		if self.in_flight == Some(request)
			|| self.queued_set.contains(&request)
			|| matches!(request, MetadataRequest::Post(id) if self.posts.contains_key(&id))
			|| matches!(request, MetadataRequest::Pool(id) if self.pools.contains_key(&id))
		{
			return;
		}
		self.queued.push_back(request);
		self.queued_set.insert(request);
	}

	fn complete(&mut self, request: MetadataRequest) {
		if self.in_flight == Some(request) {
			self.in_flight = None;
		}
	}

	fn next_request(&mut self) -> ComponentResponse {
		if self.in_flight.is_some() {
			return ComponentResponse::none();
		}
		let Some(request) = self.queued.pop_front() else {
			return ComponentResponse::none();
		};
		self.queued_set.remove(&request);
		self.in_flight = Some(request);
		let command = match request {
			MetadataRequest::Post(post_id) => Command::FetchLinkCandidate { post_id },
			MetadataRequest::Pool(pool_id) => Command::FetchPool { pool_id },
		};
		ComponentResponse::command(command)
	}
}

impl Default for PostMetadataStore {
	fn default() -> Self {
		Self::new()
	}
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
	use crate::reactor::Message;

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
	fn pool_load_indexes_every_member_for_immediate_navigation() {
		let mut store = PostMetadataStore::new();
		let middle = post(2);
		store.insert_post(middle.clone());
		store.insert_pool(
			99,
			Pool {
				post_ids: vec![1, 2, 3],
			},
		);

		assert_eq!(store.pool_neighbors(&middle), Some((Some(1), Some(3))));
		assert_eq!(store.direct_targets(&middle), Some((Some(1), Some(3))));
	}

	#[test]
	fn ensuring_metadata_deduplicates_requests() {
		let mut source = post(1);
		source.relationships.children = vec![2];
		let mut store = PostMetadataStore::new();
		store.insert_post(source);

		let first = store.handle(&Command::EnsureMetadata { post_ids: vec![1] });
		assert!(matches!(
			first.messages.as_slice(),
			[Message::Command(Command::FetchLinkCandidate {
				post_id: 2,
				..
			})]
		));
		let repeated = store.handle(&Command::EnsureMetadata { post_ids: vec![1] });
		assert!(repeated.messages.is_empty());
	}
}
