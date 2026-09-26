#[cfg(not(target_arch = "wasm32"))]
#[path = "platform/native.rs"]
mod implementation;
#[cfg(target_arch = "wasm32")]
#[path = "platform/web.rs"]
mod implementation;

use crate::config::SavedSettings;
use std::future::Future;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn spawn(future: impl Future<Output = ()> + Send + 'static) {
	implementation::spawn(future);
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn spawn(future: impl Future<Output = ()> + 'static) {
	implementation::spawn(future);
}

pub(crate) fn api_client() -> reqwest::Client {
	implementation::api_client()
}

pub(crate) fn media_client() -> reqwest::Client {
	implementation::media_client()
}

pub(crate) fn load_settings() -> SavedSettings {
	implementation::load_settings()
}

pub(crate) fn save_settings(settings: &SavedSettings) {
	implementation::save_settings(settings);
}
