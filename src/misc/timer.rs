use std::time::Instant;

pub struct Timer {
	times: Vec<(String, f64)>,
	current_instant: Option<Instant>,
	current_name: Option<String>,
}

impl Timer {
	pub fn new() -> Self {
		Timer {
			times: Vec::new(),
			current_instant: None,
			current_name: None,
		}
	}

	pub fn start<S: Into<String>>(&mut self, name: S) {
		if let Some(instant) = self.current_instant.take() {
			if let Some(name) = self.current_name.take() {
				let elapsed = instant.elapsed();
				let ms = (elapsed.as_secs() as f64 * 1000.0)
					+ (elapsed.subsec_nanos() as f64 / 1000000.0);
				self.times.push((name, ms));
			}
		}

		self.current_name = Some(name.into());
		self.current_instant = Some(Instant::now());
	}

	pub fn stop(&mut self) {
		if let Some(instant) = self.current_instant.take() {
			if let Some(name) = self.current_name.take() {
				let elapsed = instant.elapsed();
				let ms = (elapsed.as_secs() as f64 * 1000.0)
					+ (elapsed.subsec_nanos() as f64 / 1000000.0);
				self.times.push((name, ms));
			}
		}
	}

	pub fn display(&self) -> String {
		let mut out = String::new();

		for &(ref name, ref ms) in &self.times {
			out.push_str(format!("{}: {:.3} ms, ", name, ms).as_str());
		}

		out.pop();
		out.pop();
		out
	}

	pub fn display_micros(&self) -> String {
		let mut out = String::new();

		for &(ref name, ref ms) in &self.times {
			out.push_str(format!("{}: {:.3} µs, ", name, ms * 1000.0).as_str());
		}

		out.pop();
		out.pop();
		out
	}
}
