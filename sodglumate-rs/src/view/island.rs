use crate::platform::Instant;
use crate::reactor::Command;
use crate::types::{BreathingPhase, BreathingStyle, NavDirection};
use eframe::egui;
use std::sync::LazyLock;
use std::time::Duration;

/// Action to perform when an island entry is selected
#[derive(Clone, Copy)]
pub enum IslandAction {
	/// Fire an event (via factory function)
	Emit(fn() -> Command),
	/// Push a subcategory island onto the stack
	Push(&'static Island),
	/// Pop back to the parent island
	Pop,
	Links,
	OpenLinkedPost(u64),
	RequestBreathingToggle,
	ToggleImageFillMode,
}

/// A single entry in an island grid
#[derive(Clone, Copy)]
pub struct IslandEntry {
	pub label: &'static str,
	pub action: IslandAction,
}

/// An island is a 2D grid of entries
#[derive(Clone)]
pub struct Island {
	pub rows: Vec<Vec<IslandEntry>>,
}

impl Island {
	/// Get the entry at (row, col), if it exists
	pub fn get(&self, row: usize, col: usize) -> Option<&IslandEntry> {
		self.rows.get(row).and_then(|r| r.get(col))
	}

	/// Get the number of rows
	pub fn row_count(&self) -> usize {
		self.rows.len()
	}

	/// Get the number of columns in a specific row
	pub fn col_count(&self, row: usize) -> usize {
		self.rows.get(row).map(|r| r.len()).unwrap_or(0)
	}

	/// Convert a flat index to (row, col)
	pub fn index_to_pos(&self, index: usize) -> (usize, usize) {
		let mut remaining = index;
		for (row_idx, row) in self.rows.iter().enumerate() {
			if remaining < row.len() {
				return (row_idx, remaining);
			}
			remaining -= row.len();
		}
		// Fallback to last valid position
		let last_row = self.rows.len().saturating_sub(1);
		let last_col = self.col_count(last_row).saturating_sub(1);
		(last_row, last_col)
	}

	/// Convert (row, col) to a flat index
	pub fn pos_to_index(&self, row: usize, col: usize) -> usize {
		let mut index = 0;
		for (r, row_entries) in self.rows.iter().enumerate() {
			if r == row {
				return index + col.min(row_entries.len().saturating_sub(1));
			}
			index += row_entries.len();
		}
		index
	}
}

/// Mutable state for the island navigation system
pub struct IslandCtx {
	/// Stack of (island reference, selected index when we left it)
	stack: Vec<(Island, usize)>,
	/// Currently selected index in the topmost island
	pub selected: usize,
	/// Whether the island overlay is currently active
	pub active: bool,
	/// Cached row widths from previous frame
	pub row_widths: Vec<f32>,
	/// Max row width from previous frame
	pub max_row_width: f32,
	/// Cooldown until which input should be ignored after island close
	pub cooldown_until: Option<Instant>,
}

impl Default for IslandCtx {
	fn default() -> Self {
		Self::new()
	}
}

impl IslandCtx {
	pub fn new() -> Self {
		Self {
			stack: Vec::new(),
			selected: 0,
			active: false,
			row_widths: Vec::new(),
			max_row_width: 0.0,
			cooldown_until: None,
		}
	}

	/// Get the currently displayed island (topmost on stack)
	pub fn current_island(&self) -> Option<&Island> {
		self.stack.last().map(|(island, _)| island)
	}

	/// Activate the island overlay with the given root island and default selection
	pub fn activate(&mut self, root: &'static Island, default_selected: usize) {
		self.stack.clear();
		self.stack.push((root.clone(), 0));
		self.selected = default_selected;
		self.active = true;
		self.cooldown_until = None;
	}

	/// Deactivate the island overlay entirely
	pub fn deactivate(&mut self) {
		self.stack.clear();
		self.selected = 0;
		self.active = false;
		self.cooldown_until = Some(Instant::now() + Duration::from_millis(280));
	}

	/// Check if we're in the cooldown period after closing
	pub fn in_cooldown(&self) -> bool {
		self.cooldown_until
			.map(|t| Instant::now() < t)
			.unwrap_or(false)
	}

	/// Push a subcategory island onto the stack
	pub fn push(&mut self, island: Island) {
		let prev_selected = self.selected;
		if let Some((_, selected)) = self.stack.last_mut() {
			*selected = prev_selected;
		}
		self.stack.push((island, 0));
		self.selected = 0;
	}

	pub fn replace_current(&mut self, island: Island) {
		if let Some((current, _)) = self.stack.last_mut() {
			*current = island;
		}
		self.selected = 0;
		self.row_widths.clear();
		self.max_row_width = 0.0;
	}

	/// Pop back to the parent island, returns false if already at root
	pub fn pop(&mut self) -> bool {
		if self.stack.len() > 1 {
			self.stack.pop();
			if let Some((_, prev_selected)) = self.stack.last() {
				self.selected = *prev_selected;
			}
			true
		} else {
			false
		}
	}

	/// Navigate in a direction within the current island
	pub fn navigate(&mut self, direction: GridDirection) {
		let Some(island) = self.current_island() else {
			return;
		};

		let (row, col) = island.index_to_pos(self.selected);
		let (new_row, new_col) = match direction {
			GridDirection::Up => (row.saturating_sub(1), col),
			GridDirection::Down => {
				((row + 1).min(island.row_count().saturating_sub(1)), col)
			}
			GridDirection::Left => (row, col.saturating_sub(1)),
			GridDirection::Right => {
				(row, (col + 1).min(island.col_count(row).saturating_sub(1)))
			}
		};

		// Clamp column to valid range for new row
		let clamped_col = new_col.min(island.col_count(new_row).saturating_sub(1));
		self.selected = island.pos_to_index(new_row, clamped_col);
	}

	/// Get the currently selected entry
	pub fn selected_entry(&self) -> Option<&IslandEntry> {
		let island = self.current_island()?;
		let (row, col) = island.index_to_pos(self.selected);
		island.get(row, col)
	}
}

/// Grid navigation direction
pub enum GridDirection {
	Up,
	Down,
	Left,
	Right,
}

/// Helper to create an emit entry
const fn emit(label: &'static str, factory: fn() -> Command) -> IslandEntry {
	IslandEntry {
		label,
		action: IslandAction::Emit(factory),
	}
}

/// Helper to create a push entry
const fn push(label: &'static str, island: &'static Island) -> IslandEntry {
	IslandEntry {
		label,
		action: IslandAction::Push(island),
	}
}

/// Back entry for subcategories
const BACK_ENTRY: IslandEntry = IslandEntry {
	label: "Back",
	action: IslandAction::Pop,
};

pub static AUTOPLAY_ISLAND: LazyLock<Island> = LazyLock::new(|| Island {
	rows: vec![
		vec![emit("Toggle", || Command::ToggleAutoPlay)],
		vec![
			emit("-1s", || Command::AdjustAutoPlayDelay(-1)),
			emit("+1s", || Command::AdjustAutoPlayDelay(1)),
		],
		vec![BACK_ENTRY],
	],
});

pub static BREATHING_ISLAND: LazyLock<Island> = LazyLock::new(|| Island {
	rows: vec![
		vec![
			IslandEntry {
				label: "Toggle",
				action: IslandAction::RequestBreathingToggle,
			},
			emit("Classic", || {
				Command::SetBreathingStyle(BreathingStyle::Classic)
			}),
			emit("Immersive", || {
				Command::SetBreathingStyle(BreathingStyle::Immersive)
			}),
		],
		vec![
			emit("Low", || Command::SetBreathingPhaseMultiplier {
				phase: BreathingPhase::Idle,
				value: 1.8,
			}),
			emit("Medium", || Command::SetBreathingPhaseMultiplier {
				phase: BreathingPhase::Idle,
				value: 1.0,
			}),
			emit("High", || Command::SetBreathingPhaseMultiplier {
				phase: BreathingPhase::Idle,
				value: 0.67,
			}),
		],
		vec![BACK_ENTRY],
	],
});

/// The root island shown when shift is pressed
pub static ROOT_ISLAND: LazyLock<Island> = LazyLock::new(|| Island {
	rows: vec![
		vec![
			push("Autoplay", &AUTOPLAY_ISLAND),
			push("Breathing", &BREATHING_ISLAND),
			IslandEntry {
				label: "View",
				action: IslandAction::ToggleImageFillMode,
			},
		],
		vec![
			emit("Previous image", || Command::Navigate(NavDirection::Prev)),
			emit("Next image", || Command::Navigate(NavDirection::Next)),
		],
		vec![
			emit("Rewind 10", || Command::Navigate(NavDirection::Skip(-10))),
			IslandEntry {
				label: "Links",
				action: IslandAction::Links,
			},
			emit("Skip 10", || Command::Navigate(NavDirection::Skip(10))),
		],
	],
});

pub fn links_island(post: Option<&crate::api::Post>) -> Island {
	let mut entries = vec![BACK_ENTRY];
	if let Some(post) = post {
		if let Some(id) = post.relationships.parent_id {
			entries.push(IslandEntry {
				label: "Parent",
				action: IslandAction::OpenLinkedPost(id),
			});
		}
		const LABELS: [&str; 8] = [
			"Child1", "Child2", "Child3", "Child4", "Child5", "Child6", "Child7",
			"Child8",
		];
		let capacity = 9 - entries.len();
		for (&id, label) in post
			.relationships
			.children
			.iter()
			.zip(LABELS)
			.take(capacity)
		{
			entries.push(IslandEntry {
				label,
				action: IslandAction::OpenLinkedPost(id),
			});
		}
	}
	Island {
		rows: entries.chunks(3).map(|row| row.to_vec()).collect(),
	}
}

/// A custom egui widget for displaying and interacting with islands
pub struct IslandWidget<'a> {
	ctx: &'a mut IslandCtx,
}

impl<'a> IslandWidget<'a> {
	pub fn new(ctx: &'a mut IslandCtx) -> Self {
		Self { ctx }
	}

	/// Show the island overlay. Returns the action if one was confirmed.
	pub fn show(&mut self, egui_ctx: &egui::Context) -> Option<IslandAction> {
		if !self.ctx.active {
			return None;
		}

		let island = self.ctx.current_island()?.clone();

		// Handle input first
		let action = self.handle_input(egui_ctx);

		// Render overlay and update width cache
		self.render(egui_ctx, &island);

		action
	}

	fn handle_input(&mut self, ctx: &egui::Context) -> Option<IslandAction> {
		let mut confirmed_action = None;

		ctx.input(|i| {
			// WASD navigation
			if i.key_pressed(egui::Key::W) {
				self.ctx.navigate(GridDirection::Up);
			}
			if i.key_pressed(egui::Key::S) {
				self.ctx.navigate(GridDirection::Down);
			}
			if i.key_pressed(egui::Key::A) {
				self.ctx.navigate(GridDirection::Left);
			}
			if i.key_pressed(egui::Key::D) {
				self.ctx.navigate(GridDirection::Right);
			}

			// Space to confirm
			if i.key_pressed(egui::Key::Space)
				&& let Some(entry) = self.ctx.selected_entry()
			{
				confirmed_action = Some(entry.action);
			}
		});

		confirmed_action
	}

	fn render(&mut self, ctx: &egui::Context, island: &Island) {
		let screen_rect = ctx.screen_rect();

		let offset_x = screen_rect.width() * 0.15;
		let offset_y = -screen_rect.height() * 0.2;

		egui::Area::new(egui::Id::new("island_overlay"))
			.anchor(egui::Align2::LEFT_BOTTOM, [offset_x, offset_y])
			.show(ctx, |ui| {
				egui::Frame::none().show(ui, |ui| {
					Self::render_grid_impl(self.ctx, ui, island);
				});
			});
	}

	fn render_grid_impl(
		island_ctx: &mut IslandCtx,
		ui: &mut egui::Ui,
		island: &Island,
	) {
		let screen_height = ui.ctx().screen_rect().height();
		let scale = (screen_height / 800.0).max(0.5);

		let selected_pos = island.index_to_pos(island_ctx.selected);
		let cached_widths = &island_ctx.row_widths;
		let max_width = island_ctx.max_row_width;

		ui.spacing_mut().item_spacing = egui::vec2(8.0 * scale, 8.0 * scale);

		let mut new_widths = Vec::with_capacity(island.rows.len());

		for (row_idx, row) in island.rows.iter().enumerate() {
			// Get cached width for this row (0 on first frame)
			let row_width = cached_widths.get(row_idx).copied().unwrap_or(0.0);
			// Calculate padding to center this row
			let padding = ((max_width - row_width) / 2.0).max(0.0);

			let response = ui.horizontal(|ui| {
				// Add left padding to center
				if padding > 0.0 {
					ui.add_space(padding);
				}
				for (col_idx, entry) in row.iter().enumerate() {
					let is_selected = (row_idx, col_idx) == selected_pos;
					Self::render_entry_static(ui, entry, is_selected, scale);
				}
			});

			// Store actual width
			new_widths.push(response.response.rect.width() - padding);
		}

		// Update cached widths
		let new_max = new_widths.iter().cloned().fold(0.0f32, f32::max);
		island_ctx.row_widths = new_widths;
		island_ctx.max_row_width = new_max;
	}

	fn render_entry_static(
		ui: &mut egui::Ui,
		entry: &IslandEntry,
		is_selected: bool,
		scale: f32,
	) {
		let font_size = (16.0 * scale).max(12.0);
		let h_margin = 16.0 * scale;
		let v_margin = 10.0 * scale;
		let rounding = 6.0 * scale;
		let stroke_width = if is_selected {
			2.0 * scale
		} else {
			1.0 * scale
		};

		let (bg_color, text_color, stroke_color) = if is_selected {
			(
				egui::Color32::from_rgb(70, 130, 200),
				egui::Color32::WHITE,
				egui::Color32::from_rgb(100, 170, 255),
			)
		} else {
			(
				egui::Color32::from_rgb(50, 50, 60),
				egui::Color32::from_gray(200),
				egui::Color32::from_rgb(70, 70, 80),
			)
		};

		let label = entry.label.to_string();

		egui::Frame::none()
			.fill(bg_color)
			.rounding(rounding)
			.inner_margin(egui::Margin::symmetric(h_margin, v_margin))
			.stroke(egui::Stroke::new(stroke_width, stroke_color))
			.show(ui, |ui| {
				ui.label(
					egui::RichText::new(label)
						.color(text_color)
						.size(font_size)
						.strong(),
				);
			});
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::api::Post;

	#[test]
	fn links_have_back_parent_and_children_with_a_total_limit_of_nine() {
		for parent in [None, Some(99)] {
			let mut post = Post::default();
			post.relationships.parent_id = parent;
			post.relationships.children = (1..=20).collect();
			let island = links_island(Some(&post));
			let entries: Vec<_> = island.rows.iter().flatten().collect();
			assert_eq!(island.row_count(), 3);
			assert!(island.rows.iter().all(|row| row.len() == 3));
			assert_eq!(entries.len(), 9);
			assert_eq!(entries[0].label, "Back");
			assert!(matches!(entries[0].action, IslandAction::Pop));
			let children_start = if parent.is_some() {
				assert_eq!(entries[1].label, "Parent");
				assert!(matches!(
					entries[1].action,
					IslandAction::OpenLinkedPost(99)
				));
				2
			} else {
				1
			};
			for (index, entry) in entries[children_start..].iter().enumerate() {
				assert_eq!(entry.label, format!("Child{}", index + 1));
				assert!(
					matches!(entry.action, IslandAction::OpenLinkedPost(id) if id == (index + 1) as u64)
				);
			}
		}
	}

	#[test]
	fn empty_links_still_allow_returning_to_root() {
		for post in [None, Some(Post::default())] {
			let island = links_island(post.as_ref());
			assert_eq!(island.rows.len(), 1);
			assert_eq!(island.rows[0].len(), 1);
			let mut ctx = IslandCtx::new();
			ctx.activate(&ROOT_ISLAND, 6);
			ctx.push(island);
			ctx.navigate(GridDirection::Right);
			ctx.navigate(GridDirection::Down);
			assert!(matches!(
				ctx.selected_entry().unwrap().action,
				IslandAction::Pop
			));
			assert!(ctx.pop());
			assert_eq!(ctx.selected, 6);
			assert!(matches!(
				ctx.selected_entry().unwrap().action,
				IslandAction::Links
			));
		}
	}

	#[test]
	fn replacing_links_resets_selection_to_back() {
		let mut post = Post::default();
		post.relationships.children = vec![1, 2, 3, 4];
		let mut ctx = IslandCtx::new();
		ctx.activate(&ROOT_ISLAND, 6);
		ctx.push(links_island(Some(&post)));
		ctx.selected = 4;
		ctx.replace_current(links_island(None));
		assert_eq!(ctx.selected, 0);
		assert_eq!(ctx.selected_entry().unwrap().label, "Back");
		assert!(ctx.pop());
		assert_eq!(ctx.selected, 6);
	}
}
