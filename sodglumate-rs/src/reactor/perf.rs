//! Live performance observation support.
//!
//! This harness intentionally uses live e621 requests. It is for observing
//! real-time behavior in representative scenarios, not for detecting
//! performance regressions: e621 data and network conditions may vary.

use super::{Command, Message, Reactor};
use crate::types::NavDirection;
use eframe::{App, Frame, egui};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

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
			path.set_extension("toml");
		}
		Some(path)
	}

	pub(crate) fn name(&self) -> &str {
		&self.name
	}

	fn from_file(path: &Path) -> Result<Self, String> {
		let toml =
			std::fs::read_to_string(path).map_err(|error| error.to_string())?;
		Self::from_toml(&toml)
	}

	pub(crate) fn from_toml(toml: &str) -> Result<Self, String> {
		let scenario: Self =
			toml::from_str(toml).map_err(|error| error.to_string())?;
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
			self.next_event += 1;
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
		next_event.unwrap_or(Duration::ZERO).saturating_sub(elapsed)
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
