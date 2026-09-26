use crate::api::Post;
use crate::platform::Instant;
use crate::reactor::{Command, ComponentResponse, Event};
use crate::types::{AnimatedFrame, LoadedMedia, MediaKind};
use eframe::egui;
#[cfg(feature = "video")]
use egui_player_rs::VideoPainter;

use indexmap::IndexMap;
use std::collections::{HashSet, VecDeque};
use std::io::Cursor;
#[cfg(not(target_arch = "wasm32"))]
use std::io::{self, BufRead, Read, Seek, SeekFrom};
use std::time::Duration;
use tokio::sync::mpsc;

mod gif_loader;

/// Number of background workers for general loading
const NUM_WORKERS: usize = 4;
const MAX_DOWNLOAD_BYTES: u64 = 25 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 50_000_000;
const MAX_ANIMATION_FRAMES: usize = 256;
const MAX_PREFETCH_GIFS: usize = 2;

pub(crate) enum DecodedMedia {
	Image(egui::ColorImage),
	Animated(Vec<(egui::ColorImage, Duration)>),
}

pub enum MediaMessage {
	WorkFinished {
		url: String,
		priority: bool,
	},
	ImageLoaded {
		url: String,
		is_sample: bool,
		full_url: String, // Key for cache lookup
		result: Result<DecodedMedia, String>,
	},
	PlayableLoaded {
		url: String,
		result: Result<Vec<u8>, String>,
	},
	#[cfg(not(target_arch = "wasm32"))]
	GifFrame {
		url: String,
		frame: Result<Option<(egui::ColorImage, Duration)>, String>,
		finished: bool,
	},
}

#[derive(Clone, Copy)]
enum LoadKind {
	Image,
	Playable,
}

#[cfg(feature = "video")]
type VideoDebugState = (Option<String>, bool, Option<(u32, u32)>);
#[cfg(not(target_arch = "wasm32"))]
type GifStreamFrame = (Option<(egui::ColorImage, Duration)>, bool);
#[cfg(not(target_arch = "wasm32"))]
type GifStreamResult = Result<GifStreamFrame, String>;

struct PlaybackTiming {
	url: String,
	attempted_at: Instant,
	decoder_ready_logged: bool,
	painted_logged: bool,
}

/// A unit of work sent to a loading worker
pub(super) struct LoadWork {
	url: String,
	is_sample: bool,
	cache_key: String,
	kind: LoadKind,
	priority: bool,
}

#[cfg(not(target_arch = "wasm32"))]
struct StreamingGifReader {
	receiver: mpsc::Receiver<Result<Vec<u8>, String>>,
	buffer: Vec<u8>,
	position: usize,
	finished: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl StreamingGifReader {
	fn new(receiver: mpsc::Receiver<Result<Vec<u8>, String>>) -> Self {
		Self {
			receiver,
			buffer: Vec::new(),
			position: 0,
			finished: false,
		}
	}

	fn receive_chunk(&mut self) -> io::Result<bool> {
		loop {
			match self.receiver.blocking_recv() {
				Some(Ok(chunk)) if !chunk.is_empty() => {
					self.buffer = chunk;
					self.position = 0;
					return Ok(true);
				}
				Some(Ok(_)) => {}
				Some(Err(error)) => {
					self.finished = true;
					return Err(io::Error::other(error));
				}
				None => {
					self.finished = true;
					return Ok(false);
				}
			}
		}
	}
}

#[cfg(not(target_arch = "wasm32"))]
impl Read for StreamingGifReader {
	fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
		if buffer.is_empty() {
			return Ok(0);
		}
		let available = self.fill_buf()?;
		if available.is_empty() {
			return Ok(0);
		}
		let count = available.len().min(buffer.len());
		buffer[..count].copy_from_slice(&available[..count]);
		self.consume(count);
		Ok(count)
	}
}

#[cfg(not(target_arch = "wasm32"))]
impl BufRead for StreamingGifReader {
	fn fill_buf(&mut self) -> io::Result<&[u8]> {
		while self.position >= self.buffer.len() && !self.finished {
			self.receive_chunk()?;
		}
		Ok(&self.buffer[self.position..])
	}

	fn consume(&mut self, amount: usize) {
		self.position = self.position.saturating_add(amount).min(self.buffer.len());
	}
}

#[cfg(not(target_arch = "wasm32"))]
impl Seek for StreamingGifReader {
	fn seek(&mut self, _position: SeekFrom) -> io::Result<u64> {
		Err(io::Error::new(
			io::ErrorKind::Unsupported,
			"streaming GIF input is not seekable",
		))
	}
}

/// Represents a media item's loading state
#[derive(Clone, Debug)]
pub struct MediaItem {
	pub sample_url: Option<String>,
	pub full_url: Option<String>,
	pub kind: MediaKind,
}

/// State of an item in the cache
#[derive(Clone, Debug)]
pub enum CacheState {
	SampleOnly,
	Full,
}

pub struct MediaPane {
	// Cache keyed by full_url (or sample_url if no full)
	cache: IndexMap<String, (LoadedMedia, CacheState)>,
	loading_set: HashSet<String>,
	failures: IndexMap<String, String>,
	pending_set: HashSet<String>,

	// Current item being displayed
	current_item: Option<MediaItem>,
	#[cfg(feature = "video")]
	video_player: Option<VideoPainter>,
	#[cfg(feature = "video")]
	video_url: Option<String>,
	#[cfg(feature = "video")]
	last_video_debug_state: Option<VideoDebugState>,
	playback_timing: Option<PlaybackTiming>,

	// Pending queues for tiered loading
	pending_samples: VecDeque<MediaItem>, // Breadth-first samples
	pending_full: VecDeque<MediaItem>,    // Depth-first full versions
	gif_prefetch_budget: usize,

	// Keep waiting work here so navigation can promote or replace it.
	queued_work: IndexMap<String, LoadWork>,
	general_active: usize,
	priority_active: bool,
	http_client: reqwest::Client,
	result_tx: mpsc::Sender<MediaMessage>,

	// Result channel
	receiver: mpsc::Receiver<MediaMessage>,

	egui_ctx: egui::Context,
}

impl MediaPane {
	pub fn new(ctx: &egui::Context) -> Self {
		log::info!(
			"Initializing MediaCache with {} workers + 1 priority worker",
			NUM_WORKERS
		);

		let (result_tx, result_rx) = mpsc::channel(100);
		let http_client = crate::platform::media_client();

		Self {
			cache: IndexMap::new(),
			loading_set: HashSet::new(),
			failures: IndexMap::new(),
			pending_set: HashSet::new(),
			current_item: None,
			#[cfg(feature = "video")]
			video_player: None,
			#[cfg(feature = "video")]
			video_url: None,
			#[cfg(feature = "video")]
			last_video_debug_state: None,
			playback_timing: None,
			pending_samples: VecDeque::new(),
			pending_full: VecDeque::new(),
			gif_prefetch_budget: 0,
			queued_work: IndexMap::new(),
			general_active: 0,
			priority_active: false,
			http_client,
			result_tx,
			receiver: result_rx,
			egui_ctx: ctx.clone(),
		}
	}

	fn dispatch_loads(&mut self) {
		loop {
			let priority_index = self
				.queued_work
				.values()
				.position(|work| work.priority && !work.is_sample)
				.or_else(|| self.queued_work.values().position(|work| work.priority));
			let (index, priority) = if let Some(index) =
				priority_index.filter(|_| !self.priority_active)
			{
				(index, true)
			} else if self.general_active < NUM_WORKERS
				&& !self.queued_work.is_empty()
			{
				(priority_index.unwrap_or(0), false)
			} else {
				break;
			};
			let (_, work) = self.queued_work.shift_remove_index(index).unwrap();
			self.loading_set.insert(work.url.clone());
			if priority {
				self.priority_active = true;
			} else {
				self.general_active += 1;
			}
			let client = self.http_client.clone();
			let sender = self.result_tx.clone();
			let ctx = self.egui_ctx.clone();
			crate::platform::spawn(async move {
				let url = work.url.clone();
				Self::process_work(work, &client, &sender, &ctx).await;
				let _ = sender
					.send(MediaMessage::WorkFinished { url, priority })
					.await;
				ctx.request_repaint();
			});
		}
	}

	async fn process_work(
		work: LoadWork,
		http_client: &reqwest::Client,
		result_tx: &mpsc::Sender<MediaMessage>,
		ctx: &egui::Context,
	) {
		if matches!(work.kind, LoadKind::Playable) && Self::is_gif_url(&work.url) {
			gif_loader::load(work, http_client, result_tx, ctx).await;
			return;
		}

		let result = Self::load_work(http_client, &work).await;
		Self::emit_load_result(work, result, result_tx, ctx).await;
	}

	#[cfg(target_arch = "wasm32")]
	pub(super) async fn load_gif_work(
		work: LoadWork,
		http_client: &reqwest::Client,
		result_tx: &mpsc::Sender<MediaMessage>,
		ctx: &egui::Context,
	) {
		let result = Self::load_work(http_client, &work)
			.await
			.and_then(|bytes| Self::decode_media(&bytes))
			.map_err(|error| error.to_string());
		let _ = result_tx
			.send(MediaMessage::ImageLoaded {
				url: work.url,
				is_sample: work.is_sample,
				full_url: work.cache_key,
				result,
			})
			.await;
		ctx.request_repaint();
	}

	async fn emit_load_result(
		work: LoadWork,
		result: Result<Vec<u8>, anyhow::Error>,
		result_tx: &mpsc::Sender<MediaMessage>,
		ctx: &egui::Context,
	) {
		match work.kind {
			LoadKind::Image => {
				let result = match result {
					Ok(bytes) => {
						Self::decode_media(&bytes).map_err(|e| e.to_string())
					}
					Err(error) => Err(error.to_string()),
				};
				let _ = result_tx
					.send(MediaMessage::ImageLoaded {
						url: work.url,
						is_sample: work.is_sample,
						full_url: work.cache_key,
						result,
					})
					.await;
				ctx.request_repaint();
			}
			LoadKind::Playable => {
				let _ = result_tx
					.send(MediaMessage::PlayableLoaded {
						url: work.url,
						result: result.map_err(|error| error.to_string()),
					})
					.await;
				ctx.request_repaint();
			}
		}
	}

	#[cfg(not(target_arch = "wasm32"))]
	fn spawn_gif_decoder(
		url: &str,
	) -> (
		mpsc::Sender<Result<Vec<u8>, String>>,
		mpsc::Receiver<GifStreamResult>,
	) {
		use image::AnimationDecoder;
		let (chunk_tx, chunk_rx) = mpsc::channel(4);
		let (frame_tx, frame_rx) = mpsc::channel(1);
		let decode_url = url.to_owned();
		std::thread::spawn(move || {
			let decode_started_at = Instant::now();
			log::info!("GIF timing: decode started url={}", decode_url);
			let send_error = |error: String| {
				let _ = frame_tx.blocking_send(Err(error));
			};
			let decoder = match image::codecs::gif::GifDecoder::new(
				StreamingGifReader::new(chunk_rx),
			) {
				Ok(decoder) => decoder,
				Err(error) => {
					send_error(error.to_string());
					return;
				}
			};
			let mut frame_count = 0;
			for frame in decoder.into_frames() {
				if frame_count == MAX_ANIMATION_FRAMES {
					send_error(format!(
						"Animation contains more than {} frames",
						MAX_ANIMATION_FRAMES
					));
					return;
				}
				let frame = match frame {
					Ok(frame) => frame,
					Err(error) => {
						send_error(error.to_string());
						return;
					}
				};
				let duration = Self::frame_duration(frame.delay());
				let color_image =
					match Self::color_image_from_rgba(frame.into_buffer()) {
						Ok(image) => image,
						Err(error) => {
							send_error(error.to_string());
							return;
						}
					};
				let first_frame = frame_count == 0;
				frame_count += 1;
				if first_frame {
					log::info!(
						"GIF timing: first frame decoded url={} elapsed_ms={:.1}",
						decode_url,
						decode_started_at.elapsed().as_secs_f64() * 1000.0
					);
				}
				if frame_tx
					.blocking_send(Ok((Some((color_image, duration)), false)))
					.is_err()
				{
					return;
				}
			}
			let result = if frame_count == 0 {
				Err("GIF contains no frames".to_owned())
			} else {
				log::info!(
					"GIF timing: decode complete url={} frames={} elapsed_ms={:.1}",
					decode_url,
					frame_count,
					decode_started_at.elapsed().as_secs_f64() * 1000.0
				);
				Ok((None, true))
			};
			let _ = frame_tx.blocking_send(result);
		});
		(chunk_tx, frame_rx)
	}

	#[cfg(not(target_arch = "wasm32"))]
	async fn forward_gif_frames(
		url: String,
		mut frame_rx: mpsc::Receiver<GifStreamResult>,
		result_tx: mpsc::Sender<MediaMessage>,
		ctx: egui::Context,
	) {
		while let Some(frame) = frame_rx.recv().await {
			let finished = frame
				.as_ref()
				.map(|(_, finished)| *finished)
				.unwrap_or(true);
			let frame = frame.map(|(frame, _)| frame);
			let _ = result_tx
				.send(MediaMessage::GifFrame {
					url: url.to_owned(),
					frame,
					finished,
				})
				.await;
			ctx.request_repaint();
			if finished {
				break;
			}
		}
	}

	#[cfg(not(target_arch = "wasm32"))]
	pub(super) async fn stream_gif_work(
		url: &str,
		http_client: &reqwest::Client,
		result_tx: &mpsc::Sender<MediaMessage>,
		ctx: &egui::Context,
	) {
		let download_started_at = Instant::now();
		let response = match http_client.get(url).send().await {
			Ok(response) => response,
			Err(error) => {
				Self::send_gif_error(url, error.to_string(), result_tx, ctx).await;
				return;
			}
		};
		log::info!(
			"GIF timing: response received url={} elapsed_ms={:.1}",
			url,
			download_started_at.elapsed().as_secs_f64() * 1000.0
		);
		if !response.status().is_success() {
			Self::send_gif_error(
				url,
				format!("HTTP Status: {}", response.status()),
				result_tx,
				ctx,
			)
			.await;
			return;
		}
		if response
			.content_length()
			.is_some_and(|length| length > MAX_DOWNLOAD_BYTES)
		{
			Self::send_gif_error(
				url,
				format!("Media exceeds {} MiB", MAX_DOWNLOAD_BYTES / (1024 * 1024)),
				result_tx,
				ctx,
			)
			.await;
			return;
		}

		let (chunk_tx, frame_rx) = Self::spawn_gif_decoder(url);
		let frame_task = tokio::spawn(Self::forward_gif_frames(
			url.to_owned(),
			frame_rx,
			result_tx.clone(),
			ctx.clone(),
		));
		let mut response = response;
		let mut downloaded_bytes = 0_u64;
		let stream_result = loop {
			let chunk = match response.chunk().await {
				Ok(Some(chunk)) => chunk,
				Ok(None) => break Ok(()),
				Err(error) => break Err(error.to_string()),
			};
			downloaded_bytes += chunk.len() as u64;
			if downloaded_bytes > MAX_DOWNLOAD_BYTES {
				break Err(format!(
					"Media exceeds {} MiB",
					MAX_DOWNLOAD_BYTES / (1024 * 1024)
				));
			}
			if chunk_tx.send(Ok(chunk.to_vec())).await.is_err() {
				break Err("GIF decoder stopped receiving data".to_owned());
			}
		};

		if let Err(error) = &stream_result {
			let _ = chunk_tx.send(Err(error.clone())).await;
		}
		drop(chunk_tx);
		match stream_result {
			Ok(()) => log::info!(
				"GIF timing: download complete url={} bytes={} elapsed_ms={:.1}",
				url,
				downloaded_bytes,
				download_started_at.elapsed().as_secs_f64() * 1000.0
			),
			Err(error) => log::error!(
				"GIF download failed: {} - {} bytes={} elapsed_ms={:.1}",
				url,
				error,
				downloaded_bytes,
				download_started_at.elapsed().as_secs_f64() * 1000.0
			),
		}
		let _ = frame_task.await;
	}

	#[cfg(not(target_arch = "wasm32"))]
	async fn send_gif_error(
		url: &str,
		error: String,
		result_tx: &mpsc::Sender<MediaMessage>,
		ctx: &egui::Context,
	) {
		let _ = result_tx
			.send(MediaMessage::GifFrame {
				url: url.to_owned(),
				frame: Err(error),
				finished: true,
			})
			.await;
		ctx.request_repaint();
	}

	/// Shared bounded media download used by all workers.
	async fn load_work(
		http_client: &reqwest::Client,
		work: &LoadWork,
	) -> Result<Vec<u8>, anyhow::Error> {
		let download_started_at = Instant::now();
		let is_gif = Self::is_gif_url(&work.url);
		let resp = http_client.get(&work.url).send().await?;
		if is_gif {
			log::info!(
				"GIF timing: response received url={} elapsed_ms={:.1}",
				work.url,
				download_started_at.elapsed().as_secs_f64() * 1000.0
			);
		}
		if !resp.status().is_success() {
			anyhow::bail!("HTTP Status: {}", resp.status());
		}
		if resp
			.content_length()
			.is_some_and(|length| length > MAX_DOWNLOAD_BYTES)
		{
			anyhow::bail!("Media exceeds {} MiB", MAX_DOWNLOAD_BYTES / (1024 * 1024));
		}
		let bytes = resp.bytes().await?;
		if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
			anyhow::bail!("Media exceeds {} MiB", MAX_DOWNLOAD_BYTES / (1024 * 1024));
		}
		if is_gif {
			log::info!(
				"GIF timing: download complete url={} bytes={} elapsed_ms={:.1}",
				work.url,
				bytes.len(),
				download_started_at.elapsed().as_secs_f64() * 1000.0
			);
		}
		Ok(bytes.to_vec())
	}

	pub(crate) fn decode_media(bytes: &[u8]) -> Result<DecodedMedia, anyhow::Error> {
		if image::guess_format(bytes)? == image::ImageFormat::Gif {
			use image::AnimationDecoder;

			let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))?;
			let mut frames = Vec::new();
			for frame in decoder.into_frames() {
				if frames.len() == MAX_ANIMATION_FRAMES {
					anyhow::bail!(
						"Animation contains more than {} frames",
						MAX_ANIMATION_FRAMES
					);
				}
				frames.push(frame?);
			}
			if frames.len() > 1 {
				let frames = frames
					.into_iter()
					.map(|frame| {
						let (numerator, denominator) = frame.delay().numer_denom_ms();
						let duration = if numerator == 0 {
							Duration::from_millis(100)
						} else {
							Duration::from_secs_f64(
								f64::from(numerator)
									/ f64::from(denominator) / 1000.0,
							)
						};
						Self::color_image_from_rgba(frame.into_buffer())
							.map(|image| (image, duration))
					})
					.collect::<Result<Vec<_>, _>>()?;
				return Ok(DecodedMedia::Animated(frames));
			}
		}

		let img = image::load_from_memory(bytes)?;
		Ok(DecodedMedia::Image(Self::color_image_from_rgba(
			img.to_rgba8(),
		)?))
	}

	fn color_image_from_rgba(
		img: image::RgbaImage,
	) -> Result<egui::ColorImage, anyhow::Error> {
		let pixels = u64::from(img.width()) * u64::from(img.height());
		if pixels > MAX_IMAGE_PIXELS {
			anyhow::bail!("Image exceeds {} pixels", MAX_IMAGE_PIXELS);
		}
		let size = [img.width() as usize, img.height() as usize];
		let pixels = img.as_flat_samples();
		Ok(egui::ColorImage::from_rgba_unmultiplied(
			size,
			pixels.as_slice(),
		))
	}

	#[cfg(not(target_arch = "wasm32"))]
	fn frame_duration(delay: image::Delay) -> Duration {
		let (numerator, denominator) = delay.numer_denom_ms();
		if numerator == 0 {
			Duration::from_millis(100)
		} else {
			Duration::from_secs_f64(
				f64::from(numerator) / f64::from(denominator) / 1000.0,
			)
		}
	}

	pub fn poll(&mut self) -> ComponentResponse {
		#[cfg(feature = "video")]
		self.poll_video_state();

		// Process completed loads
		while let Ok(msg) = self.receiver.try_recv() {
			match msg {
				MediaMessage::WorkFinished { url, priority } => {
					self.loading_set.remove(&url);
					if priority {
						self.priority_active = false;
					} else {
						self.general_active -= 1;
					}
				}
				MediaMessage::ImageLoaded {
					url,
					is_sample,
					full_url,
					result,
				} => {
					match result {
						Ok(decoded_media) => {
							// A late preview must not replace full content or a streaming GIF.
							if is_sample
								&& self.cache.get(&full_url).is_some_and(
									|(media, state)| {
										matches!(state, CacheState::Full)
											|| matches!(
												media,
												LoadedMedia::AnimatedImage { .. }
											)
									},
								) {
								continue;
							}

							log::info!(
								"Image loaded: {} (sample={})",
								url,
								is_sample
							);
							let loaded_media = match decoded_media {
								DecodedMedia::Image(color_image) => {
									LoadedMedia::Image {
										texture: self.egui_ctx.load_texture(
											&url,
											color_image,
											egui::TextureOptions::LINEAR,
										),
									}
								}
								DecodedMedia::Animated(frames) => {
									LoadedMedia::AnimatedImage {
										frames: frames
											.into_iter()
											.enumerate()
											.map(
												|(index, (color_image, duration))| {
													AnimatedFrame {
											texture: self.egui_ctx.load_texture(
												format!("{url}#frame-{index}"),
												color_image,
												egui::TextureOptions::LINEAR,
											),
											duration,
										}
												},
											)
											.collect(),
										started_at: Instant::now(),
										complete: true,
									}
								}
							};
							let state = if is_sample {
								CacheState::SampleOnly
							} else {
								CacheState::Full
							};
							self.cache
								.insert(full_url.clone(), (loaded_media, state));

							let is_initial_load =
								if let Some(ref current) = self.current_item {
									if is_sample {
										true // Sample is always initial
									} else {
										// Full is initial only if there's no sample
										current.sample_url.is_none()
									}
								} else {
									false
								};

							if is_initial_load
								&& let Some(ref current) = self.current_item
								&& (current.full_url.as_ref() == Some(&full_url)
									|| current.sample_url.as_ref() == Some(&full_url))
							{
								self.failures.shift_remove(&url);
							}
						}
						Err(error) => {
							log::error!("Image load failed: {} - {}", url, error);
							self.record_failure(url.clone(), error.clone());
							let current_item_matches =
								self.current_item.as_ref().is_some_and(|item| {
									item.sample_url.as_deref() == Some(url.as_str())
										|| item.full_url.as_deref()
											== Some(url.as_str())
								});
							if current_item_matches
								&& self.get_current_media().is_none()
							{}
						}
					}
				}
				#[cfg(not(target_arch = "wasm32"))]
				MediaMessage::GifFrame {
					url,
					frame,
					finished,
				} => match frame {
					Ok(Some((color_image, duration))) => {
						self.note_decoder_ready(&url, "gif");
						let frame_index = self
							.cache
							.get(&url)
							.and_then(|(media, _)| match media {
								LoadedMedia::AnimatedImage { frames, .. } => {
									Some(frames.len())
								}
								LoadedMedia::Image { .. } => None,
							})
							.unwrap_or(0);
						let texture = self.egui_ctx.load_texture(
							format!("{url}#frame-{frame_index}"),
							color_image,
							egui::TextureOptions::LINEAR,
						);
						let entry =
							self.cache.entry(url.clone()).or_insert_with(|| {
								(
									LoadedMedia::AnimatedImage {
										frames: Vec::new(),
										started_at: Instant::now(),
										complete: false,
									},
									CacheState::SampleOnly,
								)
							});
						if !matches!(entry.0, LoadedMedia::AnimatedImage { .. }) {
							entry.0 = LoadedMedia::AnimatedImage {
								frames: Vec::new(),
								started_at: Instant::now(),
								complete: false,
							};
						}
						if let LoadedMedia::AnimatedImage { frames, .. } =
							&mut entry.0
						{
							frames.push(AnimatedFrame { texture, duration });
						}
						if finished {
							if let LoadedMedia::AnimatedImage { complete, .. } =
								&mut entry.0
							{
								*complete = true;
							}
							entry.1 = CacheState::Full;
						}
					}
					Ok(None) => {
						if finished
							&& let Some((media, state)) = self.cache.get_mut(&url)
						{
							if let LoadedMedia::AnimatedImage { complete, .. } = media
							{
								*complete = true;
							}
							*state = CacheState::Full;
						}
					}
					Err(error) => {
						log::error!("GIF load failed: {} - {}", url, error);
						self.record_failure(url.clone(), error.clone());
						if self.current_item.as_ref().is_some_and(|item| {
							item.full_url.as_deref() == Some(url.as_str())
						}) && self.get_current_media().is_none()
						{}
					}
				},
				MediaMessage::PlayableLoaded { url, result } => match result {
					Ok(bytes) => {
						log::info!(
							"Playable media downloaded but not handled by a specialized backend: url={} bytes={}",
							url,
							bytes.len()
						);
					}
					Err(error) => {
						log::error!(
							"Playable media load failed: {} - {}",
							url,
							error
						);
						self.record_failure(url.clone(), error.clone());
						let current_item_matches =
							self.current_item.as_ref().is_some_and(|item| {
								item.full_url.as_deref() == Some(url.as_str())
							});
						if current_item_matches {}
					}
				},
			}
		}

		// Process loading queue with priority logic
		self.process_loading_queue();
		self.dispatch_loads();

		self.prune_cache();

		ComponentResponse::none()
	}

	fn process_loading_queue(&mut self) {
		// Always try to load both sample and full for the currently displayed item
		if let Some(ref current) = self.current_item.clone() {
			let cache_key = self.get_cache_key(current);
			let (has_sample, has_full) = self
				.cache
				.get(&cache_key)
				.map(|(_, state)| {
					(
						true,
						matches!(state, CacheState::Full), // Full implies sample content too
					)
				})
				.unwrap_or((false, false));

			let sample_loading = current
				.sample_url
				.as_ref()
				.map(|u| self.loading_set.contains(u))
				.unwrap_or(false);
			let full_loading = current
				.full_url
				.as_ref()
				.map(|u| self.loading_set.contains(u))
				.unwrap_or(false);

			// Prioritize the current preview as well as its full-resolution media.
			if !has_sample {
				if let Some(ref sample_url) = current.sample_url {
					if !sample_loading {
						self.enqueue_load(
							sample_url.clone(),
							true,
							cache_key.clone(),
							true,
						);
					}
				} else if !current.kind.is_playable()
					&& let Some(ref full_url) = current.full_url
				{
					// No sample available; treat full as the first-tier load
					if !full_loading {
						self.enqueue_load(
							full_url.clone(),
							false,
							cache_key.clone(),
							true,
						);
					}
				}
			}

			// Playable media is rendered by the active VideoPainter; only stills
			// need their full-resolution bytes in the image cache.
			if !current.kind.is_playable() {
				if !has_full
					&& let Some(ref full_url) = current.full_url
					&& !full_loading
				{
					self.enqueue_load(
						full_url.clone(),
						false,
						cache_key.clone(),
						true,
					);
				}
			} else if let Some(ref full_url) = current.full_url
				&& Self::is_gif_url(full_url)
				&& !has_full && !full_loading
			{
				self.enqueue_playable(full_url.clone(), true);
			}
		}

		// Drain pending samples into general workers
		while let Some(item) = self.pending_samples.pop_front() {
			let cache_key = self.get_cache_key(&item);
			if let Some((_, state)) = self.cache.get(&cache_key) {
				if matches!(state, CacheState::SampleOnly) {
					self.pending_full.push_back(item);
				}
				continue;
			}

			if let Some(ref sample_url) = item.sample_url {
				self.enqueue_load(sample_url.clone(), true, cache_key, false);
				self.pending_full.push_back(item);
			} else if let Some(ref full_url) = item.full_url
				&& !self.loading_set.contains(full_url)
			{
				if item.kind.is_playable() {
					if Self::is_gif_url(full_url) && self.gif_prefetch_budget > 0 {
						self.gif_prefetch_budget -= 1;
						self.enqueue_playable(full_url.clone(), false);
					}
				} else {
					self.enqueue_load(full_url.clone(), false, cache_key, false);
				}
			}
		}

		// Drain pending full versions into general workers
		while let Some(item) = self.pending_full.pop_front() {
			let cache_key = self.get_cache_key(&item);
			let has_full = self
				.cache
				.get(&cache_key)
				.map(|(_, state)| matches!(state, CacheState::Full))
				.unwrap_or(false);
			if has_full {
				continue;
			}
			if let Some(ref full_url) = item.full_url
				&& !self.loading_set.contains(full_url)
			{
				if item.kind.is_playable() {
					if Self::is_gif_url(full_url) && self.gif_prefetch_budget > 0 {
						self.gif_prefetch_budget -= 1;
						self.enqueue_playable(full_url.clone(), false);
					}
				} else {
					self.enqueue_load(full_url.clone(), false, cache_key, false);
				}
			}
		}
	}

	fn get_cache_key(&self, item: &MediaItem) -> String {
		item.full_url
			.clone()
			.or_else(|| item.sample_url.clone())
			.unwrap_or_default()
	}

	#[cfg(feature = "video")]
	fn start_video(&mut self, url: &str) {
		if self.video_url.as_deref() == Some(url) && self.video_player.is_some() {
			if let Some(player) = &self.video_player {
				player.set_playback(true);
			}
			return;
		}

		#[cfg(feature = "video")]
		self.stop_video();
		log::info!("Starting streaming player: url={}", url);
		let player = VideoPainter::new();
		player.set_video_source_url(url.to_owned());
		player.set_playback(true);
		self.video_player = Some(player);
		self.video_url = Some(url.to_owned());
	}

	#[cfg(feature = "video")]
	fn stop_video(&mut self) {
		if let Some(player) = &self.video_player {
			player.deactivate();
		}
		self.video_player = None;
		self.video_url = None;
		self.last_video_debug_state = None;
	}

	fn note_decoder_ready(&mut self, url: &str, backend: &str) {
		if let Some(timing) = &mut self.playback_timing
			&& timing.url == url
			&& !timing.decoder_ready_logged
		{
			let elapsed_ms = timing.attempted_at.elapsed().as_secs_f64() * 1000.0;
			log::info!(
				"Playback timing: first frame ready url={} backend={} elapsed_ms={:.1}",
				url,
				backend,
				elapsed_ms
			);
			timing.decoder_ready_logged = true;
		}
	}

	fn note_painted(&mut self, url: &str, backend: &str) {
		if let Some(timing) = &mut self.playback_timing
			&& timing.url == url
			&& !timing.painted_logged
		{
			let elapsed_ms = timing.attempted_at.elapsed().as_secs_f64() * 1000.0;
			log::info!(
				"Playback timing: first frame painted url={} backend={} elapsed_ms={:.1}",
				url,
				backend,
				elapsed_ms
			);
			timing.painted_logged = true;
		}
	}

	fn note_current_painted(&mut self) {
		let Some(item) = self.current_item.as_ref() else {
			return;
		};
		let Some(url) = item.full_url.clone() else {
			return;
		};
		if Self::is_gif_url(&url) {
			if self
				.get_current_media()
				.is_some_and(LoadedMedia::is_animated)
			{
				self.note_decoder_ready(&url, "gif");
				self.note_painted(&url, "gif");
			}
		}
		#[cfg(feature = "video")]
		if !Self::is_gif_url(&url)
			&& self
				.video_player
				.as_ref()
				.is_some_and(VideoPainter::has_decoded_frames)
		{
			self.note_decoder_ready(&url, "video");
			self.note_painted(&url, "video");
		}
	}

	pub fn current_is_playable(&self) -> bool {
		self.current_item.as_ref().is_some_and(|item| {
			item.kind.is_playable()
				&& !item.full_url.as_deref().is_some_and(Self::is_gif_url)
		})
	}

	#[cfg(feature = "video")]
	pub fn current_video_player(&self) -> Option<&VideoPainter> {
		self.video_player.as_ref()
	}

	pub fn needs_painted_notification(&self) -> bool {
		let Some(timing) = &self.playback_timing else {
			return false;
		};
		if timing.painted_logged {
			return false;
		}
		if Self::is_gif_url(&timing.url) {
			self.get_current_media()
				.is_some_and(LoadedMedia::is_animated)
		} else {
			#[cfg(feature = "video")]
			{
				self.video_player
					.as_ref()
					.is_some_and(VideoPainter::has_decoded_frames)
			}
			#[cfg(not(feature = "video"))]
			{
				false
			}
		}
	}

	#[cfg(feature = "video")]
	fn poll_video_state(&mut self) {
		let Some(player) = &self.video_player else {
			return;
		};
		let observation = (
			player.status_message(),
			player.has_decoded_frames(),
			player.decoded_frame_size(),
			player.playback(),
			player.current_time_seconds(),
		);
		self.log_video_debug_state(
			observation.0,
			observation.1,
			observation.2,
			observation.3,
			observation.4,
		);
	}

	#[cfg(feature = "video")]
	fn log_video_debug_state(
		&mut self,
		status: Option<String>,
		has_frames: bool,
		frame_size: Option<(u32, u32)>,
		playback: bool,
		time: f64,
	) {
		let state = (status, has_frames, frame_size);
		if self.last_video_debug_state.as_ref() == Some(&state) {
			return;
		}
		log::info!(
			"Video player state: url={:?} status={:?} has_frames={} frame_size={:?} playback={} time={:.3}",
			self.video_url,
			state.0,
			state.1,
			state.2,
			playback,
			time
		);
		self.last_video_debug_state = Some(state);
	}

	/// Queue a load, keeping selected media ahead of speculative downloads.
	fn enqueue_load(
		&mut self,
		url: String,
		is_sample: bool,
		cache_key: String,
		priority: bool,
	) {
		self.enqueue_work(url, is_sample, cache_key, LoadKind::Image, priority);
	}

	fn enqueue_playable(&mut self, url: String, priority: bool) {
		self.enqueue_work(url, false, String::new(), LoadKind::Playable, priority);
	}

	fn enqueue_work(
		&mut self,
		url: String,
		is_sample: bool,
		cache_key: String,
		kind: LoadKind,
		priority: bool,
	) {
		if self.loading_set.contains(&url) || self.failures.contains_key(&url) {
			return;
		}
		if let Some(work) = self.queued_work.get_mut(&url) {
			work.priority |= priority;
			return;
		}
		self.queued_work.insert(
			url.clone(),
			LoadWork {
				url,
				is_sample,
				cache_key,
				kind,
				priority,
			},
		);
	}

	pub fn handle_command(&mut self, command: &Command) -> ComponentResponse {
		match command {
			Command::LoadMedia {
				sample_url,
				full_url,
				kind,
			} => {
				log::info!(
					"LoadRequest: sample={:?}, full={:?} (kind={:?})",
					sample_url,
					full_url,
					kind
				);
				let item = MediaItem {
					sample_url: sample_url.clone(),
					full_url: full_url.clone(),
					kind: *kind,
				};
				for work in self.queued_work.values_mut() {
					work.priority = item.full_url.as_ref() == Some(&work.url)
						|| item.sample_url.as_ref() == Some(&work.url);
				}
				self.current_item = Some(item.clone());
				if item.kind.is_playable() {
					if let Some(url) = &item.full_url {
						self.playback_timing = Some(PlaybackTiming {
							url: url.clone(),
							attempted_at: Instant::now(),
							decoder_ready_logged: false,
							painted_logged: false,
						});
						log::info!("Playback timing: attempted start url={}", url);
						if Self::is_gif_url(url) {
							// GIFs are decoded while their response is still downloading;
							// the preview remains visible until the first full frame arrives.
							#[cfg(feature = "video")]
							self.stop_video();
						} else {
							#[cfg(feature = "video")]
							self.start_video(url);
						}
					}
				} else {
					#[cfg(feature = "video")]
					self.stop_video();
					self.playback_timing = None;
				}
				if let Some(url) = &item.sample_url {
					self.failures.shift_remove(url);
				}
				if let Some(url) = &item.full_url {
					self.failures.shift_remove(url);
				}
				if let Some(error) = self.current_error() {
					log::warn!("Cannot display selected media: {error}");
				}
			}
			Command::PrefetchMedia { urls } => {
				log::debug!("Prefetch requested for {} items", urls.len());

				// Active downloads finish; obsolete waiting downloads are discarded.
				self.queued_work.clear();

				// Clear old pending items and reset
				self.pending_samples.clear();
				self.pending_full.clear();
				self.pending_set.clear();
				self.gif_prefetch_budget = MAX_PREFETCH_GIFS;

				for (sample_url, full_url, kind) in urls {
					let item = MediaItem {
						sample_url: sample_url.clone(),
						full_url: full_url.clone(),
						kind: *kind,
					};
					let cache_key = self.get_cache_key(&item);

					if !self.pending_set.contains(&cache_key) {
						self.pending_set.insert(cache_key);
						self.pending_samples.push_back(item);
					}
				}
			}
			_ => {}
		}

		ComponentResponse::none()
	}

	pub fn observe(&mut self, event: &Event) -> ComponentResponse {
		if matches!(event, Event::MediaPainted) {
			self.note_current_painted();
		}
		ComponentResponse::none()
	}

	fn prune_cache(&mut self) {
		const MAX_CACHE_SIZE: usize = 100;
		if self.cache.len() > MAX_CACHE_SIZE {
			let current_key =
				self.current_item.as_ref().map(|i| self.get_cache_key(i));
			let to_remove: Vec<String> = self
				.cache
				.keys()
				.filter(|k| Some(*k) != current_key.as_ref())
				.take(self.cache.len() - MAX_CACHE_SIZE)
				.cloned()
				.collect();

			if !to_remove.is_empty() {
				log::debug!("Pruning {} items from cache", to_remove.len());
			}

			for key in to_remove {
				self.cache.shift_remove(&key);
			}
		}
	}

	fn record_failure(&mut self, url: String, error: String) {
		const MAX_FAILURES: usize = 256;
		self.failures.shift_remove(&url);
		self.failures.insert(url, error);
		while self.failures.len() > MAX_FAILURES {
			self.failures.shift_remove_index(0);
		}
	}

	fn is_gif_url(url: &str) -> bool {
		url.split('?')
			.next()
			.and_then(|path| path.rsplit('/').next())
			.is_some_and(|name| name.to_ascii_lowercase().ends_with(".gif"))
	}

	/// Get the best available media for the current item
	pub fn get_current_media(&self) -> Option<&LoadedMedia> {
		let item = self.current_item.as_ref()?;
		self.media_for_urls(item.full_url.as_deref(), item.sample_url.as_deref())
	}

	pub fn get_media_by_post(&self, post: &Post) -> Option<&LoadedMedia> {
		let full_url = post.file.url.as_deref();
		let sample_url = if post.sample.has {
			post.sample.url.as_deref()
		} else {
			None
		};
		self.media_for_urls(full_url, sample_url)
	}

	fn media_for_urls(
		&self,
		full_url: Option<&str>,
		sample_url: Option<&str>,
	) -> Option<&LoadedMedia> {
		full_url
			.and_then(|url| self.cache.get(url))
			.or_else(|| sample_url.and_then(|url| self.cache.get(url)))
			.map(|(media, _)| media)
	}

	pub fn current_url(&self) -> Option<&str> {
		self.current_item
			.as_ref()
			.and_then(|i| i.full_url.as_deref().or(i.sample_url.as_deref()))
	}

	#[cfg(feature = "video")]
	pub fn current_video_size(&self) -> Option<(u32, u32)> {
		self.video_player
			.as_ref()
			.and_then(VideoPainter::decoded_frame_size)
	}

	pub fn is_loading(&self) -> bool {
		!self.loading_set.is_empty()
	}

	pub fn current_error(&self) -> Option<&str> {
		if self.get_current_media().is_some() {
			return None;
		}
		#[cfg(not(feature = "video"))]
		if self.current_is_playable() {
			return Some("Video playback is disabled in this build.");
		}
		let item = self.current_item.as_ref()?;
		item.full_url
			.as_ref()
			.and_then(|url| self.failures.get(url))
			.or_else(|| {
				item.sample_url
					.as_ref()
					.and_then(|url| self.failures.get(url))
			})
			.map(String::as_str)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;

	fn image_result(is_sample: bool) -> MediaMessage {
		MediaMessage::ImageLoaded {
			url: if is_sample { "preview.jpg" } else { "full.jpg" }.into(),
			is_sample,
			full_url: "full.jpg".into(),
			result: Ok(DecodedMedia::Image(egui::ColorImage::new(
				if is_sample { [1, 1] } else { [2, 2] },
				egui::Color32::WHITE,
			))),
		}
	}

	#[test]
	fn full_image_wins_regardless_of_download_completion_order() {
		for order in [[false, true], [true, false]] {
			let mut media = MediaPane::new(&egui::Context::default());
			for is_sample in order {
				media.result_tx.try_send(image_result(is_sample)).unwrap();
				media.poll();
			}
			let (loaded, state) = media.cache.get("full.jpg").unwrap();
			assert!(matches!(state, CacheState::Full));
			assert_eq!(loaded.texture().size(), [2, 2]);
		}
	}

	#[test]
	fn late_preview_preserves_an_incomplete_streaming_animation() {
		let mut media = MediaPane::new(&egui::Context::default());
		media
			.result_tx
			.try_send(MediaMessage::GifFrame {
				url: "full.jpg".into(),
				frame: Ok(Some((
					egui::ColorImage::new([2, 2], egui::Color32::WHITE),
					Duration::from_millis(100),
				))),
				finished: false,
			})
			.unwrap();
		media.result_tx.try_send(image_result(true)).unwrap();
		media.poll();
		assert!(matches!(
			&media.cache["full.jpg"].0,
			LoadedMedia::AnimatedImage {
				complete: false,
				..
			}
		));
	}

	fn prefetch(media: &mut MediaPane, names: &[&str]) {
		media.handle_command(&Command::PrefetchMedia {
			urls: names
				.iter()
				.map(|name| (None, Some((*name).into()), MediaKind::Image))
				.collect(),
		});
		media.process_loading_queue();
	}

	fn select(media: &mut MediaPane, url: &str) {
		media.handle_command(&Command::LoadMedia {
			sample_url: None,
			full_url: Some(url.into()),
			kind: MediaKind::Image,
		});
		media.process_loading_queue();
	}

	#[tokio::test]
	async fn selected_prefetch_uses_reserved_capacity_without_duplicate_work() {
		let mut media = MediaPane::new(&egui::Context::default());
		prefetch(&mut media, &["a", "b", "c", "d", "e", "selected"]);
		media.dispatch_loads();
		assert_eq!(media.general_active, NUM_WORKERS);
		assert!(!media.priority_active);
		assert!(!media.loading_set.contains("selected"));

		select(&mut media, "selected");
		media.dispatch_loads();
		assert!(media.loading_set.contains("selected"));
		assert!(media.priority_active);
		assert!(media.queued_work.contains_key("e"));
		assert!(!media.queued_work.contains_key("selected"));

		// Selecting an active URL does not enqueue a second download.
		select(&mut media, "selected");
		media.dispatch_loads();
		assert_eq!(media.loading_set.len(), NUM_WORKERS + 1);
		assert!(!media.queued_work.contains_key("selected"));
	}

	#[tokio::test]
	async fn failed_downloads_release_capacity_and_drain_waiting_work() {
		let mut media = MediaPane::new(&egui::Context::default());
		// Invalid URLs fail before any network access, exercising real task completion.
		prefetch(&mut media, &["a", "b", "c", "d", "e", "f"]);
		select(&mut media, "current");
		media.dispatch_loads();
		assert_eq!(media.loading_set.len(), NUM_WORKERS + 1);
		tokio::time::timeout(Duration::from_secs(1), async {
			while !media.loading_set.is_empty() || !media.queued_work.is_empty() {
				tokio::task::yield_now().await;
				media.poll();
			}
		})
		.await
		.expect("failed work must release all download slots");
		assert_eq!(media.failures.len(), 7);
		assert_eq!(media.general_active, 0);
		assert!(!media.priority_active);
	}

	#[test]
	fn new_prefetch_discards_obsolete_waiting_work_and_keeps_current_selection() {
		let mut media = MediaPane::new(&egui::Context::default());
		prefetch(&mut media, &["obsolete", "current"]);
		select(&mut media, "current");
		prefetch(&mut media, &["next"]);
		assert!(!media.queued_work.contains_key("obsolete"));
		assert!(media.queued_work["current"].priority);
		assert!(media.queued_work.contains_key("next"));
	}

	#[test]
	fn replacing_prefetch_preserves_full_load_after_preview_arrives() {
		let mut media = MediaPane::new(&egui::Context::default());
		media.result_tx.try_send(image_result(true)).unwrap();
		media.poll();
		for _ in 0..2 {
			media.handle_command(&Command::PrefetchMedia {
				urls: vec![(
					Some("preview.jpg".into()),
					Some("full.jpg".into()),
					MediaKind::Image,
				)],
			});
			media.process_loading_queue();
			assert!(media.queued_work.contains_key("full.jpg"));
			assert!(!media.queued_work.contains_key("preview.jpg"));
		}
	}

	#[test]
	fn selecting_another_item_demotes_the_previous_priority() {
		let mut media = MediaPane::new(&egui::Context::default());
		select(&mut media, "previous");
		select(&mut media, "current");
		assert!(!media.queued_work["previous"].priority);
		assert!(media.queued_work["current"].priority);
	}

	#[cfg(not(feature = "video"))]
	#[tokio::test]
	async fn disabled_video_reports_unavailable_but_gifs_remain_supported() {
		let mut media = MediaPane::new(&eframe::egui::Context::default());
		for (url, expected_error) in [
			(
				"https://cdn.example/post.mp4",
				Some("Video playback is disabled in this build."),
			),
			("https://cdn.example/post.gif", None),
		] {
			media.handle_command(&crate::reactor::Command::LoadMedia {
				sample_url: None,
				full_url: Some(url.to_owned()),
				kind: crate::types::MediaKind::Playable,
			});
			assert_eq!(media.current_error(), expected_error);
			assert!(!media.needs_painted_notification());
		}
	}

	#[test]
	fn detects_gif_urls_without_confusing_other_media() {
		assert!(MediaPane::is_gif_url(
			"https://cdn.example/post.GIF?download=1"
		));
		assert!(!MediaPane::is_gif_url("https://cdn.example/post.mp4"));
		assert!(!MediaPane::is_gif_url("https://cdn.example/gif-preview"));
	}

	#[tokio::test]
	async fn decodes_first_gif_frame_before_stream_finishes() {
		let gif = [
			b'G', b'I', b'F', b'8', b'9', b'a', 1, 0, 1, 0, 128, 0, 0, 0, 0, 0, 255,
			255, 255, 33, 249, 4, 1, 0, 0, 0, 0, 44, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2,
			68, 1, 0, 59,
		];
		let (chunk_tx, mut frame_rx) =
			MediaPane::spawn_gif_decoder("https://cdn.example/stream.gif");

		// The trailer is deliberately withheld. A streaming decoder must still
		// make the first frame available from the image data already received.
		chunk_tx
			.send(Ok(gif[..gif.len() - 1].to_vec()))
			.await
			.expect("decoder should still be receiving data");
		let first_frame =
			tokio::time::timeout(Duration::from_secs(1), frame_rx.recv())
				.await
				.expect("first frame should not wait for the trailer")
				.expect("decoder should emit a frame")
				.expect("GIF should decode successfully");
		assert!(first_frame.0.is_some());
		assert!(!first_frame.1);

		drop(chunk_tx);
	}
}
