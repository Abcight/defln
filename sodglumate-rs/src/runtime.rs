use std::future::Future;

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn(future: impl Future<Output = ()> + Send + 'static) {
	tokio::spawn(future);
}

#[cfg(target_arch = "wasm32")]
pub fn spawn(future: impl Future<Output = ()> + 'static) {
	wasm_bindgen_futures::spawn_local(future);
}
