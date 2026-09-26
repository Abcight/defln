mod api;
#[cfg(not(target_arch = "wasm32"))]
mod beat;
#[cfg(target_arch = "wasm32")]
#[path = "beat/web.rs"]
mod beat;
mod breathing;
mod browser;
mod config;
mod gateway;
mod media;
mod platform;
mod reactor;
#[cfg(test)]
#[path = "../regressions/mod.rs"]
mod regressions;
mod settings;
mod types;
mod view;

use reactor::Reactor;

#[cfg(not(target_arch = "wasm32"))]
pub fn run_native() -> eframe::Result<()> {
	env_logger::Builder::from_env(
		env_logger::Env::default().default_filter_or("info"),
	)
	.init();

	let native_options = eframe::NativeOptions {
		viewport: eframe::egui::ViewportBuilder::default()
			.with_inner_size([1280.0, 720.0])
			.with_min_inner_size([480.0, 360.0])
			.with_decorations(false)
			.with_drag_and_drop(true),
		..Default::default()
	};

	eframe::run_native(
		"Sodglumate",
		native_options,
		Box::new(|cc| Ok(Box::new(Reactor::new(&cc.egui_ctx)))),
	)
}

#[cfg(target_arch = "wasm32")]
mod web {
	use super::Reactor;
	use wasm_bindgen::prelude::*;

	#[wasm_bindgen]
	pub struct WebHandle {
		runner: eframe::WebRunner,
	}

	#[wasm_bindgen]
	impl WebHandle {
		#[wasm_bindgen(constructor)]
		pub fn new() -> Self {
			eframe::WebLogger::init(log::LevelFilter::Info).ok();
			Self {
				runner: eframe::WebRunner::new(),
			}
		}

		#[wasm_bindgen]
		pub async fn start(
			&self,
			canvas: web_sys::HtmlCanvasElement,
		) -> Result<(), JsValue> {
			self.runner
				.start(
					canvas,
					eframe::WebOptions::default(),
					Box::new(|cc| Ok(Box::new(Reactor::new(&cc.egui_ctx)))),
				)
				.await
		}

		#[wasm_bindgen]
		pub fn destroy(&self) {
			self.runner.destroy();
		}
	}
}

#[cfg(target_arch = "wasm32")]
pub use web::WebHandle;
