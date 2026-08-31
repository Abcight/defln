mod post_2270206;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

const MAX_REQUESTS_PER_SECOND: usize = 2;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(1);

pub(crate) struct LiveE621Client {
	client: reqwest::Client,
	request_times: VecDeque<Instant>,
}

impl LiveE621Client {
	fn new() -> Self {
		Self {
			client: reqwest::Client::builder()
				.user_agent("Sodglumate regression test/0.1")
				.connect_timeout(Duration::from_secs(10))
				.timeout(Duration::from_secs(30))
				.build()
				.expect("build regression-test HTTP client"),
			request_times: VecDeque::new(),
		}
	}

	async fn get(&mut self, url: &str) -> anyhow::Result<reqwest::Response> {
		self.wait_for_rate_limit().await;
		self.request_times.push_back(Instant::now());
		Ok(self.client.get(url).send().await?.error_for_status()?)
	}

	async fn wait_for_rate_limit(&mut self) {
		loop {
			while self
				.request_times
				.front()
				.is_some_and(|request| request.elapsed() >= RATE_LIMIT_WINDOW)
			{
				self.request_times.pop_front();
			}

			if self.request_times.len() < MAX_REQUESTS_PER_SECOND {
				return;
			}

			if let Some(oldest) = self.request_times.front() {
				tokio::time::sleep(RATE_LIMIT_WINDOW.saturating_sub(oldest.elapsed())).await;
			}
		}
	}
}

#[test]
fn e621_regressions_run_sequentially_under_one_rate_limit() {
	let runtime = tokio::runtime::Runtime::new().expect("create regression-test runtime");
	runtime.block_on(async {
		let mut client = LiveE621Client::new();
		post_2270206::run(&mut client)
			.await
			.expect("post 2270206 regression failed");
	});
}
