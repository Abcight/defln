//! Live performance observation support.
//!
//! This harness intentionally uses live e621 requests. It is for observing
//! real-time behavior in representative scenarios, not for detecting
//! performance regressions: e621 data and network conditions may vary.

use super::{Command, Message, Reactor};
use crate::types::NavDirection;
use eframe::{App, Frame, egui};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::Command as ProcessCommand;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(1);

pub fn run_native() -> eframe::Result<()> {
	let live_perf = match LivePerfConfig::from_args(std::env::args().skip(1)) {
		Ok(config) => config,
		Err(message) => {
			eprintln!("{message}");
			return Ok(());
		}
	};

	env_logger::Builder::from_env(
		env_logger::Env::default().default_filter_or("info"),
	)
	.init();

	let native_options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default()
			.with_inner_size([1280.0, 720.0])
			.with_min_inner_size([480.0, 360.0])
			.with_decorations(false)
			.with_drag_and_drop(true),
		..Default::default()
	};

	eframe::run_native(
		"Sodglumate",
		native_options,
		Box::new(move |cc| {
			let app: Box<dyn App> = match live_perf {
				Some(config) => Box::new(LivePerfApp::new(&cc.egui_ctx, config)),
				None => Box::new(Reactor::new(&cc.egui_ctx)),
			};
			Ok(app)
		}),
	)
}

pub struct LivePerfConfig {
	scenario: Scenario,
}

impl LivePerfConfig {
	fn from_args(args: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
		let mut scenario_path = None;
		let args: Vec<_> = args.collect();
		let mut index = 0;
		while index < args.len() {
			match args[index].as_str() {
				"--live-perf" => {
					index += 1;
					let Some(path) = args.get(index) else {
						return Err(Self::usage(
							"missing scenario path after --live-perf",
						));
					};
					scenario_path = Some(PathBuf::from(path));
				}
				"--help" | "-h" => return Err(Self::usage("")),
				argument => {
					return Err(Self::usage(&format!(
						"unknown argument: {argument}"
					)));
				}
			}
			index += 1;
		}

		let Some(scenario_path) = scenario_path else {
			return Ok(None);
		};
		let scenario = Scenario::from_reference(&scenario_path).map_err(|error| {
			Self::usage(&format!(
				"invalid scenario {}: {error}",
				scenario_path.display()
			))
		})?;
		Ok(Some(Self { scenario }))
	}

	fn usage(error: &str) -> String {
		let prefix = (!error.is_empty())
			.then(|| format!("{error}\n\n"))
			.unwrap_or_default();
		format!(
			"{prefix}Usage: sodglumate-rs --live-perf <scenario-name-or-path>\n\nThe live harness is for observing real-time performance, not regression measurement."
		)
	}
}

#[derive(Debug, Deserialize)]
pub(crate) struct Scenario {
	name: String,
	events: Vec<ScenarioEvent>,
}

impl Scenario {
	pub(crate) fn from_reference(reference: &Path) -> Result<Self, String> {
		if let Some(named_path) = Self::named_path(reference)
			&& named_path.is_file()
		{
			return Self::from_file(&named_path);
		}
		Self::from_file(reference)
	}

	fn named_path(reference: &Path) -> Option<PathBuf> {
		let mut components = reference.components();
		let Component::Normal(name) = components.next()? else {
			return None;
		};
		if components.next().is_some() {
			return None;
		}
		let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
		path.push("src/testing/performance");
		path.push(name);
		if path.extension().is_none() {
			path.set_extension("json");
		}
		Some(path)
	}

	pub(crate) fn name(&self) -> &str {
		&self.name
	}

	fn from_file(path: &Path) -> Result<Self, String> {
		let json =
			std::fs::read_to_string(path).map_err(|error| error.to_string())?;
		Self::from_json(&json)
	}

	pub(crate) fn from_json(json: &str) -> Result<Self, String> {
		let scenario: Self =
			serde_json::from_str(json).map_err(|error| error.to_string())?;
		scenario.validate()?;
		Ok(scenario)
	}

	fn validate(&self) -> Result<(), String> {
		if self.name.trim().is_empty() {
			return Err("scenario name must not be empty".into());
		}
		let Some(last_event) = self.events.last() else {
			return Err("scenario must contain events".into());
		};
		for window in self.events.windows(2) {
			if window[0].at_ms > window[1].at_ms {
				return Err("events must be ordered by at_ms".into());
			}
		}
		if !matches!(last_event.command, ScenarioCommand::Finish) {
			return Err("the final event must be a finish command".into());
		}
		for event in &self.events[..self.events.len() - 1] {
			event.validate()?;
		}
		Ok(())
	}
}

#[derive(Debug, Deserialize)]
struct ScenarioEvent {
	at_ms: u64,
	command: ScenarioCommand,
}

impl ScenarioEvent {
	fn validate(&self) -> Result<(), String> {
		match &self.command {
			ScenarioCommand::Search { query, page } if query.trim().is_empty() => {
				Err("search query must not be empty".into())
			}
			ScenarioCommand::Search { page, .. } if *page == 0 => {
				Err("search page must be at least one".into())
			}
			ScenarioCommand::Skip { count } if *count == 0 => {
				Err("skip count must not be zero".into())
			}
			ScenarioCommand::Finish => Err("finish must be the final event".into()),
			_ => Ok(()),
		}
	}

	fn to_command(&self) -> Option<Command> {
		match &self.command {
			ScenarioCommand::Search { query, page } => Some(Command::Search {
				query: query.clone(),
				page: *page,
			}),
			ScenarioCommand::Next => Some(Command::Navigate(NavDirection::Next)),
			ScenarioCommand::Previous => Some(Command::Navigate(NavDirection::Prev)),
			ScenarioCommand::Skip { count } => {
				Some(Command::Navigate(NavDirection::Skip(*count)))
			}
			ScenarioCommand::Finish => None,
		}
	}

	fn name(&self) -> &'static str {
		match &self.command {
			ScenarioCommand::Search { .. } => "search",
			ScenarioCommand::Next => "next",
			ScenarioCommand::Previous => "previous",
			ScenarioCommand::Skip { .. } => "skip",
			ScenarioCommand::Finish => "finish",
		}
	}
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ScenarioCommand {
	Search {
		query: String,
		#[serde(default = "default_page")]
		page: u32,
	},
	Next,
	Previous,
	Skip {
		count: i32,
	},
	Finish,
}

fn default_page() -> u32 {
	1
}

pub struct LivePerfHarness {
	scenario: Scenario,
	started_at: Instant,
	next_event: usize,
	next_snapshot_at: Duration,
	last_cpu_sample: Option<(Instant, u64)>,
	clock_ticks_per_second: Option<u64>,
}

#[derive(Serialize)]
struct ObservationRecord<'a> {
	kind: &'static str,
	scenario: &'a str,
	wall_time_unix_ms: u128,
	elapsed_ms: u128,
	event: Option<&'static str>,
	post_count: usize,
	current_post_id: Option<u64>,
	gateway_loading: bool,
	media_loading: bool,
	rss_kb: Option<u64>,
	process_cpu_ms: Option<u64>,
	interval_cpu_percent: Option<f64>,
}

impl LivePerfHarness {
	fn new(config: LivePerfConfig) -> Self {
		log::info!(
			"Starting live performance scenario '{}'; results are observational and not regression measurements",
			config.scenario.name()
		);
		Self {
			scenario: config.scenario,
			started_at: Instant::now(),
			next_event: 0,
			next_snapshot_at: Duration::ZERO,
			last_cpu_sample: None,
			clock_ticks_per_second: clock_ticks_per_second(),
		}
	}

	fn poll(&mut self, reactor: &mut Reactor) -> bool {
		let elapsed = self.started_at.elapsed();
		let mut finished = false;
		while let Some(event) = self.scenario.events.get(self.next_event) {
			if Duration::from_millis(event.at_ms) > elapsed {
				break;
			}
			if let Some(command) = event.to_command() {
				reactor.queue.push(Message::Command(command));
			} else {
				finished = true;
			}
			self.write_record("event", Some(event.name()), elapsed, reactor);
			self.next_event += 1;
		}
		if elapsed >= self.next_snapshot_at {
			self.write_record("snapshot", None, elapsed, reactor);
			self.next_snapshot_at = elapsed + SNAPSHOT_INTERVAL;
		}
		finished
	}

	fn next_wakeup(&self) -> Duration {
		let elapsed = self.started_at.elapsed();
		let next_event = self
			.scenario
			.events
			.get(self.next_event)
			.map(|event| Duration::from_millis(event.at_ms));
		let next = next_event
			.map(|event| event.min(self.next_snapshot_at))
			.unwrap_or(self.next_snapshot_at);
		next.saturating_sub(elapsed).min(SNAPSHOT_INTERVAL)
	}

	fn write_record(
		&mut self,
		kind: &'static str,
		event: Option<&'static str>,
		elapsed: Duration,
		reactor: &Reactor,
	) {
		let (rss_kb, process_cpu_ms) = process_metrics(self.clock_ticks_per_second);
		let interval_cpu_percent = process_cpu_ms.and_then(|cpu_ms| {
			let now = Instant::now();
			let previous = self.last_cpu_sample.replace((now, cpu_ms))?;
			let wall_ms = now.duration_since(previous.0).as_secs_f64() * 1000.0;
			(wall_ms > 0.0)
				.then(|| (cpu_ms.saturating_sub(previous.1) as f64 / wall_ms) * 100.0)
		});
		let record = ObservationRecord {
			kind,
			scenario: self.scenario.name(),
			wall_time_unix_ms: SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.unwrap_or_default()
				.as_millis(),
			elapsed_ms: elapsed.as_millis(),
			event,
			post_count: reactor.browser.posts_len(),
			current_post_id: reactor.browser.current_post().map(|post| post.id),
			gateway_loading: reactor.gateway.is_loading(),
			media_loading: reactor.media.is_loading(),
			rss_kb,
			process_cpu_ms,
			interval_cpu_percent,
		};
		let Ok(json) = serde_json::to_string(&record) else {
			return;
		};
		log::info!("PERF {json}");
	}
}

struct LivePerfApp {
	reactor: Reactor,
	harness: LivePerfHarness,
}

impl LivePerfApp {
	fn new(ctx: &egui::Context, config: LivePerfConfig) -> Self {
		Self {
			reactor: Reactor::new(ctx),
			harness: LivePerfHarness::new(config),
		}
	}
}

impl App for LivePerfApp {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
		let finished = self.harness.poll(&mut self.reactor);
		self.reactor.tick(ctx);
		if finished {
			log::info!(
				"Live performance scenario '{}' finished",
				self.harness.scenario.name()
			);
			ctx.send_viewport_cmd(egui::ViewportCommand::Close);
		} else {
			ctx.request_repaint_after(self.harness.next_wakeup());
		}
	}

	fn save(&mut self, storage: &mut dyn eframe::Storage) {
		self.reactor.save(storage);
	}
}

#[cfg(target_os = "linux")]
fn clock_ticks_per_second() -> Option<u64> {
	ProcessCommand::new("getconf")
		.arg("CLK_TCK")
		.output()
		.ok()
		.filter(|output| output.status.success())
		.and_then(|output| String::from_utf8(output.stdout).ok())
		.and_then(|output| output.trim().parse().ok())
}

#[cfg(not(target_os = "linux"))]
fn clock_ticks_per_second() -> Option<u64> {
	None
}

#[cfg(target_os = "linux")]
fn process_metrics(
	clock_ticks_per_second: Option<u64>,
) -> (Option<u64>, Option<u64>) {
	let rss_kb =
		std::fs::read_to_string("/proc/self/status")
			.ok()
			.and_then(|status| {
				status.lines().find_map(|line| {
					line.strip_prefix("VmRSS:")
						.and_then(|value| value.split_whitespace().next())
						.and_then(|value| value.parse().ok())
				})
			});
	let cpu_ticks =
		std::fs::read_to_string("/proc/self/stat")
			.ok()
			.and_then(|stat| {
				let (_, fields) = stat.rsplit_once(')')?;
				let fields: Vec<_> = fields.split_whitespace().collect();
				Some(
					fields.get(11)?.parse::<u64>().ok()?
						+ fields.get(12)?.parse::<u64>().ok()?,
				)
			});
	let cpu_ms = clock_ticks_per_second
		.zip(cpu_ticks)
		.map(|(ticks_per_second, ticks)| ticks * 1_000 / ticks_per_second);
	(rss_kb, cpu_ms)
}

#[cfg(not(target_os = "linux"))]
fn process_metrics(
	_clock_ticks_per_second: Option<u64>,
) -> (Option<u64>, Option<u64>) {
	(None, None)
}
