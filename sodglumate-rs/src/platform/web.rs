use crate::config::SavedSettings;
use std::future::Future;

pub(super) fn spawn(future: impl Future<Output = ()> + 'static) {
	wasm_bindgen_futures::spawn_local(future);
}

pub(super) fn api_client() -> reqwest::Client {
	reqwest::Client::builder()
		.user_agent("Sodglumate/0.1 (by furikeno)")
		.build()
		.expect("Failed to build reqwest client")
}

pub(super) fn media_client() -> reqwest::Client {
	reqwest::Client::builder()
		.user_agent("Sodglumate/0.1 (by furikeno)")
		.build()
		.unwrap_or_else(|error| {
			log::error!("Failed to build media HTTP client: {error}");
			reqwest::Client::new()
		})
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
