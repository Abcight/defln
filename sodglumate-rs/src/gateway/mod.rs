use crate::api::Post;
use crate::booru::{BooruClient, BooruSource};
use crate::config::BooruCredentials;
use crate::platform::Instant;
use crate::reactor::{Command, ComponentResponse, Event, Message};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Message from async tasks back to the component
pub enum GatewayMessage {
	LinkCandidate {
		post_id: u64,
		generation: u64,
		source_generation: u64,
		result: Result<Box<Post>, String>,
	},
	LinkedPost {
		post_id: u64,
		source_generation: u64,
		result: Result<Box<Post>, String>,
	},
	SearchComplete {
		posts: Vec<Post>,
		page: u32,
		is_new: bool,
		generation: u64,
	},
	SearchError {
		message: String,
		generation: u64,
	},
}

#[derive(Debug)]
pub enum SearchStatus {
	Idle,
	Loading,
	Ready,
	Failed,
}

pub struct BooruGateway {
	client: Arc<BooruClient>,
	source: BooruSource,
	credentials: BooruCredentials,
	sender: mpsc::Sender<GatewayMessage>,
	receiver: mpsc::Receiver<GatewayMessage>,
	current_query: String,
	current_page: u32,
	status: SearchStatus,
	last_request_times: VecDeque<Instant>,
	request_generation: u64,
	source_generation: u64,
}

impl BooruGateway {
	pub fn new() -> Self {
		Self::with_credentials(BooruCredentials::default())
	}

	pub fn with_credentials(credentials: BooruCredentials) -> Self {
		log::info!("Initializing Gateway with rate limiting (2 req/sec)");
		let (sender, receiver) = mpsc::channel(100);
		Self {
			client: Arc::new(BooruClient::new(BooruSource::E621, &credentials)),
			source: BooruSource::E621,
			credentials,
			sender,
			receiver,
			current_query: String::new(),
			current_page: 1,
			status: SearchStatus::Idle,
			last_request_times: VecDeque::new(),
			request_generation: 0,
			source_generation: 0,
		}
	}

	/// Check if we can make an API request (hard limit: 2 req/sec)
	fn can_request(&self) -> bool {
		if self.last_request_times.len() < 2 {
			return true;
		}
		if let Some(oldest) = self.last_request_times.front() {
			oldest.elapsed().as_secs_f32() >= 1.0
		} else {
			true
		}
	}

	fn record_request(&mut self) {
		self.last_request_times.push_back(Instant::now());
		if self.last_request_times.len() > 2 {
			self.last_request_times.pop_front();
		}
	}

	pub fn poll(&mut self) -> ComponentResponse {
		let mut messages = Vec::new();
		while let Ok(msg) = self.receiver.try_recv() {
			match msg {
				GatewayMessage::LinkCandidate {
					post_id,
					generation,
					source_generation,
					result,
				} => {
					if source_generation != self.source_generation {
						continue;
					}
					messages.push(Message::Event(Event::LinkCandidateLoaded {
						post_id,
						generation,
						result,
					}));
				}
				GatewayMessage::LinkedPost {
					post_id,
					source_generation,
					result,
				} => {
					if source_generation != self.source_generation {
						continue;
					}
					messages.push(Message::Event(Event::LinkedPostLoaded {
						post_id,
						result,
					}));
				}
				GatewayMessage::SearchComplete {
					posts,
					page,
					is_new,
					generation,
				} => {
					if generation != self.request_generation {
						log::debug!(
							"Ignoring stale search response for generation {}",
							generation
						);
						continue;
					}
					log::info!(
						"Search complete: page={}, posts={}, is_new={}",
						page,
						posts.len(),
						is_new
					);
					self.status = SearchStatus::Ready;
					self.current_page = page;
					messages.push(Message::Event(Event::SearchCompleted {
						posts,
						page,
						is_new,
					}));
				}
				GatewayMessage::SearchError {
					message,
					generation,
				} => {
					if generation != self.request_generation {
						log::debug!(
							"Ignoring stale search error for generation {}",
							generation
						);
						continue;
					}
					log::error!("Search error: {}", message);
					self.status = SearchStatus::Failed;
				}
			}
		}

		if messages.is_empty() {
			ComponentResponse::none()
		} else {
			ComponentResponse::messages(messages)
		}
	}

	pub fn handle_command(&mut self, command: &Command) -> ComponentResponse {
		match command {
			Command::SetBooruSource(source) => {
				if *source != self.source {
					log::info!("Switching booru source to {}", source.label());
					self.source = *source;
					self.refresh_client();
				}
			}
			Command::SetDapiCredentials {
				source,
				user_id,
				api_key,
			} => {
				let credentials = crate::config::DapiCredentials {
					user_id: user_id.clone(),
					api_key: api_key.clone(),
				}
				.normalized();
				match source {
					BooruSource::Rule34 => self.credentials.rule34 = credentials,
					BooruSource::Gelbooru => self.credentials.gelbooru = credentials,
					BooruSource::E621 => return ComponentResponse::none(),
				}
				if *source == self.source {
					let query = self.current_query.clone();
					let page = self.current_page;
					self.refresh_client();
					if !query.is_empty() {
						return ComponentResponse::command(Command::Search {
							query,
							page,
						});
					}
				}
			}
			Command::FetchLinkCandidate {
				post_id,
				generation,
			} => {
				if !self.can_request() {
					return ComponentResponse::schedule_command(
						command.clone(),
						std::time::Duration::from_secs(1),
					);
				}
				self.record_request();
				let client = self.client.clone();
				let sender = self.sender.clone();
				let (post_id, generation) = (*post_id, *generation);
				let source_generation = self.source_generation;
				crate::platform::spawn(async move {
					let result = client
						.get_post(post_id)
						.await
						.map(Box::new)
						.map_err(|error| error.to_string());
					let _ = sender
						.send(GatewayMessage::LinkCandidate {
							post_id,
							generation,
							source_generation,
							result,
						})
						.await;
				});
			}
			Command::FetchLinkedPost { post_id } => {
				if !self.can_request() {
					return ComponentResponse::schedule_command(
						command.clone(),
						std::time::Duration::from_secs(1),
					);
				}
				self.record_request();
				let client = self.client.clone();
				let sender = self.sender.clone();
				let post_id = *post_id;
				let source_generation = self.source_generation;
				crate::platform::spawn(async move {
					let result = client
						.get_post(post_id)
						.await
						.map(Box::new)
						.map_err(|error| error.to_string());
					let _ = sender
						.send(GatewayMessage::LinkedPost {
							post_id,
							source_generation,
							result,
						})
						.await;
				});
			}
			Command::Search { query, page } => {
				let limit = 50;
				if !self.can_request() {
					log::warn!("API rate limit exceeded, dropping search request");
					return ComponentResponse::none();
				}
				log::info!(
					"SearchRequest: query='{}', page={}, limit={}",
					query,
					page,
					limit
				);
				self.record_request();
				self.current_query = query.clone();
				self.current_page = *page;
				self.status = SearchStatus::Loading;
				self.request_generation = self.request_generation.wrapping_add(1);
				self.spawn_search(
					query.clone(),
					*page,
					limit,
					true,
					self.request_generation,
				);
			}
			Command::FetchNextPage => {
				if !self.can_request() {
					log::debug!("API rate limit: delaying FetchNextPage");
					return ComponentResponse::none();
				}
				if !self.is_loading() && !self.current_query.is_empty() {
					let next_page = self.current_page + 1;
					log::info!(
						"FetchNextPage: query='{}', page={}",
						self.current_query,
						next_page
					);
					self.record_request();
					self.status = SearchStatus::Loading;
					self.spawn_search(
						self.current_query.clone(),
						next_page,
						50,
						false,
						self.request_generation,
					);
				} else if self.is_loading() {
					log::debug!("FetchNextPage ignored: fetch already pending");
				}
			}
			_ => {}
		}
		ComponentResponse::none()
	}

	fn spawn_search(
		&self,
		mut query: String,
		page: u32,
		limit: u32,
		is_new: bool,
		generation: u64,
	) {
		// TODO: This is a hack
		if !query.contains("-video") {
			query.push_str(" -video");
		}
		log::info!(
			"Spawning API request: query='{}', page={}, limit={}",
			query,
			page,
			limit
		);
		let client = self.client.clone();
		let sender = self.sender.clone();

		crate::platform::spawn(async move {
			log::debug!("API request started: page={}", page);
			match client.search_posts(&query, limit, page).await {
				Ok(posts) => {
					log::info!(
						"API response: page={}, received {} posts",
						page,
						posts.len()
					);
					let _ = sender
						.send(GatewayMessage::SearchComplete {
							posts,
							page,
							is_new,
							generation,
						})
						.await;
				}
				Err(e) => {
					log::error!("API error: page={}, error={}", page, e);
					let _ = sender
						.send(GatewayMessage::SearchError {
							message: e.to_string(),
							generation,
						})
						.await;
				}
			}
		});
	}

	pub fn is_loading(&self) -> bool {
		matches!(self.status, SearchStatus::Loading)
	}

	pub fn source(&self) -> BooruSource {
		self.source
	}

	fn refresh_client(&mut self) {
		self.client = Arc::new(BooruClient::new(self.source, &self.credentials));
		self.status = SearchStatus::Idle;
		self.last_request_times.clear();
		self.request_generation = self.request_generation.wrapping_add(1);
		self.source_generation = self.source_generation.wrapping_add(1);
	}
}

impl Default for BooruGateway {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn throttled_link_requests_are_scheduled_instead_of_dropped() {
		let mut gateway = BooruGateway::new();
		gateway.record_request();
		gateway.record_request();
		let response =
			gateway.handle_command(&Command::FetchLinkedPost { post_id: 42 });
		assert!(matches!(
			response.scheduled.as_slice(),
			[(
				Message::Command(Command::FetchLinkedPost { post_id: 42 }),
				_
			)]
		));
	}

	#[test]
	fn linked_post_results_do_not_change_the_search_or_pagination_state() {
		let mut gateway = BooruGateway::new();
		gateway.current_query = "wolves".into();
		gateway.current_page = 3;
		gateway.status = SearchStatus::Loading;
		gateway
			.sender
			.try_send(GatewayMessage::LinkedPost {
				post_id: 42,
				source_generation: gateway.source_generation,
				result: Err("HTTP 404".into()),
			})
			.unwrap();
		let response = gateway.poll();
		assert!(matches!(
			response.messages.as_slice(),
			[Message::Event(Event::LinkedPostLoaded {
				post_id: 42,
				result: Err(_)
			})]
		));
		assert_eq!(gateway.current_query, "wolves");
		assert_eq!(gateway.current_page, 3);
		assert!(gateway.is_loading());
	}

	#[test]
	fn stale_search_responses_are_ignored() {
		let mut gateway = BooruGateway::new();
		gateway.request_generation = 2;
		gateway.status = SearchStatus::Loading;
		gateway
			.sender
			.try_send(GatewayMessage::SearchComplete {
				posts: Vec::new(),
				page: 1,
				is_new: true,
				generation: 1,
			})
			.expect("test channel should accept the response");

		let response = gateway.poll();

		assert!(response.messages.is_empty());
		assert!(gateway.is_loading());
	}
}
