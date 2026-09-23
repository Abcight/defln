use super::message::Message;
use std::collections::VecDeque;

/// FIFO message queue. Message order is part of the reactor contract.
pub struct MessageQueue {
	queue: VecDeque<Message>,
}

impl MessageQueue {
	pub fn new() -> Self {
		Self {
			queue: VecDeque::new(),
		}
	}

	/// Push an event to the appropriate priority queue
	pub fn push(&mut self, message: Message) {
		self.queue.push_back(message);
	}

	/// Pop the highest priority event available
	pub fn pop(&mut self) -> Option<Message> {
		self.queue.pop_front()
	}
}

impl Default for MessageQueue {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::reactor::{Command, Event};
	use crate::types::NavDirection;

	#[test]
	fn preserves_message_order_across_kinds() {
		let mut queue = MessageQueue::new();
		queue.push(Message::Event(Event::MediaPainted));
		queue.push(Message::Command(Command::Navigate(NavDirection::Next)));

		assert!(matches!(
			queue.pop(),
			Some(Message::Event(Event::MediaPainted))
		));
		assert!(matches!(
			queue.pop(),
			Some(Message::Command(Command::Navigate(NavDirection::Next)))
		));
	}
}
