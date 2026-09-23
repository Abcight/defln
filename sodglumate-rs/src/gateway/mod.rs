use crate::api::{E621Client, Post};
use crate::reactor::{Command, ComponentResponse, Event, Message};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

/// Message from async tasks back to the component
pub enum GatewayMessage {
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
	Failed(String),
}

pub struct BooruGateway {
	client: Arc<E621Client>,
	sender: mpsc::Sender<GatewayMessage>,
	receiver: mpsc::Receiver<GatewayMessage>,
	current_query: String,
	current_page: u32,
	status: SearchStatus,
	last_request_times: VecDeque<Instant>,
	request_generation: u64,
}

impl BooruGateway {
	pub fn new() -> Self {
		log::info!("Initializing Gateway with rate limiting (2 req/sec)");
		let (sender, receiver) = mpsc::channel(100);
		Self {
			client: Arc::new(E621Client::new()),
			sender,
			receiver,
			current_query: String::new(),
			current_page: 1,
			status: SearchStatus::Idle,
			last_request_times: VecDeque::new(),
			request_generation: 0,
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
					self.status = SearchStatus::Failed(message);
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

		tokio::spawn(async move {
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

	pub fn status(&self) -> &SearchStatus {
		&self.status
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
