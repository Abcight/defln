use super::{LoadWork, MediaMessage, MediaPane};
use eframe::egui;
use tokio::sync::mpsc;

pub(super) async fn load(
	work: LoadWork,
	http_client: &reqwest::Client,
	result_tx: &mpsc::Sender<MediaMessage>,
	ctx: &egui::Context,
) {
	implementation::load(work, http_client, result_tx, ctx).await;
}

#[cfg(not(target_arch = "wasm32"))]
mod implementation {
	use super::*;

	pub(super) async fn load(
		work: LoadWork,
		http_client: &reqwest::Client,
		result_tx: &mpsc::Sender<MediaMessage>,
		ctx: &egui::Context,
	) {
		MediaPane::stream_gif_work(&work.url, http_client, result_tx, ctx).await;
	}
}

#[cfg(target_arch = "wasm32")]
mod implementation {
	use super::*;

	pub(super) async fn load(
		work: LoadWork,
		http_client: &reqwest::Client,
		result_tx: &mpsc::Sender<MediaMessage>,
		ctx: &egui::Context,
	) {
		MediaPane::load_gif_work(work, http_client, result_tx, ctx).await;
	}
}
