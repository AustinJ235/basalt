use crossbeam::queue::MsQueue;
use std::sync::Arc;

pub struct QShare<T: Clone> {
	queues: Vec<Arc<MsQueue<T>>>,
}

impl<T: Clone> QShare<T> {
	pub fn new() -> Self {
		QShare {
			queues: Vec::new()
		}
	}
	
	pub fn push_all(&self, data: T) {
		for queue in &self.queues {
			queue.push(data.clone());
		}
	}
	
	pub fn new_queue(&mut self) -> Arc<MsQueue<T>> {
		let queue = Arc::new(MsQueue::new());
		self.queues.push(queue.clone());
		queue
	}
}
