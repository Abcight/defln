use crate::config::SavedSettings;
use std::cmp::Ordering;
use std::future::Future;
use std::ops::Add;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Instant(f64);

impl Instant {
	pub(crate) fn now() -> Self {
		let performance = web_sys::window()
			.and_then(|window| window.performance())
			.expect("browser performance API is unavailable");
		Self(performance.now())
	}

	pub(crate) fn elapsed(self) -> Duration {
		Duration::from_secs_f64(((Self::now().0 - self.0) / 1_000.0).max(0.0))
	}
}

impl Add<Duration> for Instant {
	type Output = Self;

	fn add(self, duration: Duration) -> Self::Output {
		Self(self.0 + duration.as_secs_f64() * 1_000.0)
	}
}

impl PartialEq for Instant {
	fn eq(&self, other: &Self) -> bool {
		self.0 == other.0
	}
}

impl Eq for Instant {}

impl PartialOrd for Instant {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for Instant {
	fn cmp(&self, other: &Self) -> Ordering {
		self.0.total_cmp(&other.0)
	}
}

pub(super) fn spawn(future: impl Future<Output = ()> + 'static) {
	wasm_bindgen_futures::spawn_local(future);
}

pub(super) fn api_client() -> reqwest::Client {
	reqwest::Client::new()
}

pub(super) fn e621_posts_url() -> String {
	"/api/e621/posts".into()
}

pub(super) fn e621_post_url(id: u64) -> String {
	format!("/api/e621/posts/{id}")
}

pub(super) fn media_client() -> reqwest::Client {
	reqwest::Client::new()
}

pub(super) fn load_settings() -> SavedSettings {
	let Some(storage) =
		web_sys::window().and_then(|window| window.local_storage().ok().flatten())
	else {
		return SavedSettings::default();
	};
	match storage.get_item("sodglumate.settings") {
		Ok(Some(value)) => serde_json::from_str::<SavedSettings>(&value)
			.map(SavedSettings::normalized)
			.unwrap_or_else(|error| {
				log::warn!("Failed to parse saved browser settings: {error}");
				SavedSettings::default()
			}),
		Ok(None) | Err(_) => SavedSettings::default(),
	}
}

pub(super) fn save_settings(settings: &SavedSettings) {
	let Some(storage) =
		web_sys::window().and_then(|window| window.local_storage().ok().flatten())
	else {
		return;
	};
	match serde_json::to_string(settings) {
		Ok(value) => {
			if let Err(error) = storage.set_item("sodglumate.settings", &value) {
				log::warn!("Failed to save browser settings: {error:?}");
			}
		}
		Err(error) => log::warn!("Failed to serialize browser settings: {error}"),
	}
}
