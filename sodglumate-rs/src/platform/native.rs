use crate::config::SavedSettings;
use directories::{BaseDirs, ProjectDirs};
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

pub(crate) use std::time::Instant;

pub(super) fn spawn(future: impl Future<Output = ()> + Send + 'static) {
	tokio::spawn(future);
}

pub(super) fn api_client() -> reqwest::Client {
	reqwest::Client::builder()
		.user_agent("Sodglumate/0.1 (by furikeno)")
		.connect_timeout(Duration::from_secs(10))
		.timeout(Duration::from_secs(30))
		.build()
		.expect("Failed to build reqwest client")
}

pub(super) fn e621_posts_url() -> String {
	"https://e621.net/posts.json".into()
}

pub(super) fn e621_post_url(id: u64) -> String {
	format!("https://e621.net/posts/{id}.json")
}

pub(super) fn e621_pool_url(id: u64) -> String {
	format!("https://e621.net/pools/{id}.json")
}

pub(super) fn media_client() -> reqwest::Client {
	reqwest::Client::builder()
		.user_agent("Sodglumate/0.1 (by furikeno)")
		.connect_timeout(Duration::from_secs(10))
		.timeout(Duration::from_secs(60))
		.build()
		.unwrap_or_else(|error| {
			log::error!("Failed to build media HTTP client: {error}");
			reqwest::Client::new()
		})
}

fn config_dir() -> Option<PathBuf> {
	if cfg!(target_os = "windows") {
		ProjectDirs::from("", "", "sodglumate").map(|p| p.config_dir().to_path_buf())
	} else {
		BaseDirs::new().map(|b| b.home_dir().join(".sodglumate"))
	}
}

pub(super) fn load_settings() -> SavedSettings {
	if let Some(dir) = config_dir() {
		let path = dir.join("settings.toml");
		if let Ok(content) = fs::read_to_string(&path) {
			match toml::from_str::<SavedSettings>(&content) {
				Ok(settings) => return settings.normalized(),
				Err(error) => log::warn!("Failed to parse settings.toml: {error}"),
			}
		}
	}
	SavedSettings::default()
}

pub(super) fn save_settings(settings: &SavedSettings) {
	let Some(dir) = config_dir() else { return };
	if let Err(error) = fs::create_dir_all(&dir) {
		log::warn!("Failed to create config directory: {error}");
		return;
	}
	match toml::to_string(settings) {
		Ok(content) => {
			if let Err(error) = fs::write(dir.join("settings.toml"), content) {
				log::warn!("Failed to write settings.toml: {error}");
			}
		}
		Err(error) => log::warn!("Failed to serialize settings: {error}"),
	}
}
