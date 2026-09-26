#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() -> eframe::Result<()> {
	sodglumate_rs::run_native()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
