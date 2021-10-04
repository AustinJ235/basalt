pub mod bin;
pub mod checkbox;
pub mod hook;
pub mod on_off_button;
pub mod render;
pub mod scroll_bar;
pub mod slider;

pub use self::render::ItfDrawTarget;

use self::bin::Bin;
use self::hook::HookManager;
use self::render::composer::{Composer, ComposerEv};
use self::render::ItfRenderer;
use crate::image_view::BstImageView;
use crate::Basalt;
use ilmenite::{Ilmenite, ImtFillQuality, ImtFont, ImtRasterOpts, ImtSampleQuality, ImtWeight};
use parking_lot::{Mutex, RwLock};
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};
use vulkano::command_buffer::{AutoCommandBufferBuilder, PrimaryAutoCommandBuffer};

impl_vertex!(ItfVertInfo, position, coords, color, ty, tex_i);
#[derive(Clone, Debug)]
#[repr(C)]
pub(crate) struct ItfVertInfo {
	pub position: (f32, f32, f32),
	pub coords: (f32, f32),
	pub color: (f32, f32, f32, f32),
	pub ty: i32,
	pub tex_i: u32,
}

impl Default for ItfVertInfo {
	fn default() -> Self {
		ItfVertInfo {
			position: (0.0, 0.0, 0.0),
			coords: (0.0, 0.0),
			color: (0.0, 0.0, 0.0, 0.0),
			ty: 0,
			tex_i: 0,
		}
	}
}

pub(crate) fn scale_verts(win_size: &[f32; 2], scale: f32, verts: &mut Vec<ItfVertInfo>) {
	for vert in verts {
		vert.position.0 *= scale;
		vert.position.1 *= scale;
		vert.position.0 += win_size[0] / -2.0;
		vert.position.0 /= win_size[0] / 2.0;
		vert.position.1 += win_size[1] / -2.0;
		vert.position.1 /= win_size[1] / 2.0;
	}
}

#[derive(Clone, Copy)]
struct Scale {
	pub win: f32,
	pub itf: f32,
}

impl Scale {
	fn effective(&self, ignore_win: bool) -> f32 {
		if ignore_win {
			self.itf
		} else {
			self.itf * self.win
		}
	}
}

pub struct Interface {
	basalt: Arc<Basalt>,
	bin_i: Mutex<u64>,
	bin_map: RwLock<BTreeMap<u64, Weak<Bin>>>,
	pub(crate) ilmenite: Arc<Ilmenite>,
	pub(crate) hook_manager: Arc<HookManager>,
	renderer: Mutex<ItfRenderer>,
	composer: Arc<Composer>,
	scale: Mutex<Scale>,
}

impl Interface {
	pub(crate) fn new(basalt: Arc<Basalt>) -> Arc<Self> {
		let bin_map: RwLock<BTreeMap<u64, Weak<Bin>>> = RwLock::new(BTreeMap::new());
		let ilmenite = Arc::new(Ilmenite::new());
		let imt_fill_quality_op = basalt.options_ref().imt_fill_quality.clone();
		let imt_sample_quality_op = basalt.options_ref().imt_sample_quality.clone();

		if basalt.options_ref().imt_gpu_accelerated {
			ilmenite.add_font(
				ImtFont::from_bytes_gpu(
					"ABeeZee",
					ImtWeight::Normal,
					ImtRasterOpts {
						fill_quality: imt_fill_quality_op.unwrap_or(ImtFillQuality::Normal),
						sample_quality: imt_sample_quality_op
							.unwrap_or(ImtSampleQuality::Normal),
						raster_image_format: basalt.formats_in_use().atlas,
						..ImtRasterOpts::default()
					},
					basalt.device(),
					basalt.compute_queue(),
					include_bytes!("ABeeZee-Regular.ttf").to_vec(),
				)
				.unwrap(),
			);
		} else {
			ilmenite.add_font(
				ImtFont::from_bytes_cpu(
					"ABeeZee",
					ImtWeight::Normal,
					ImtRasterOpts {
						fill_quality: imt_fill_quality_op.unwrap_or(ImtFillQuality::Normal),
						sample_quality: imt_sample_quality_op
							.unwrap_or(ImtSampleQuality::Normal),
						..ImtRasterOpts::default()
					},
					include_bytes!("ABeeZee-Regular.ttf").to_vec(),
				)
				.unwrap(),
			);
		}

		Arc::new(Interface {
			bin_i: Mutex::new(0),
			bin_map,
			scale: Mutex::new(Scale {
				win: basalt.window().scale_factor(),
				itf: basalt.options_ref().scale,
			}),
			hook_manager: HookManager::new(basalt.clone()),
			ilmenite,
			renderer: Mutex::new(ItfRenderer::new(basalt.clone())),
			composer: Composer::new(basalt.clone()),
			basalt,
		})
	}

	/// The current scale without taking into account dpi based window scaling.
	pub fn current_scale(&self) -> f32 {
		self.scale.lock().itf
	}

	/// The current scale taking into account dpi based window scaling.
	pub fn current_effective_scale(&self) -> f32 {
		let ignore_dpi = self.basalt.options_ref().ignore_dpi;
		self.scale.lock().effective(ignore_dpi)
	}

	/// Set the current scale. Doesn't account for dpi based window scaling.
	pub fn set_scale(&self, set_scale: f32) {
		let ignore_dpi = self.basalt.options_ref().ignore_dpi;
		let mut scale = self.scale.lock();
		scale.itf = set_scale;
		self.composer.send_event(ComposerEv::Scale(scale.effective(ignore_dpi)));
	}

	pub(crate) fn set_window_scale(&self, set_scale: f32) {
		let ignore_dpi = self.basalt.options_ref().ignore_dpi;
		let mut scale = self.scale.lock();
		scale.win = set_scale;
		self.composer.send_event(ComposerEv::Scale(scale.effective(ignore_dpi)));
	}

	/// Set the current scale taking into account dpi based window scaling.
	pub fn set_effective_scale(&self, set_scale: f32) {
		let ignore_dpi = self.basalt.options_ref().ignore_dpi;
		let mut scale = self.scale.lock();

		if ignore_dpi {
			scale.itf = set_scale;
		} else {
			scale.itf = set_scale / scale.win;
		};

		self.composer.send_event(ComposerEv::Scale(scale.effective(ignore_dpi)));
	}

	/// Get the current MSAA level.
	pub fn current_msaa(&self) -> BstMSAALevel {
		let mut renderer = self.renderer.lock();
		*renderer.msaa_mut_ref()
	}

	/// Set the MSAA Level.
	pub fn set_msaa(&self, set_msaa: BstMSAALevel) {
		let mut renderer = self.renderer.lock();
		*renderer.msaa_mut_ref() = set_msaa;
	}

	/// Increase MSAA to the next step.
	pub fn increase_msaa(&self) -> BstMSAALevel {
		let mut renderer = self.renderer.lock();
		renderer.msaa_mut_ref().increase();
		*renderer.msaa_mut_ref()
	}

	/// Decrease MSAA to the next step.
	pub fn decrease_msaa(&self) -> BstMSAALevel {
		let mut renderer = self.renderer.lock();
		renderer.msaa_mut_ref().decrease();
		*renderer.msaa_mut_ref()
	}

	pub(crate) fn composer_ref(&self) -> &Arc<Composer> {
		&self.composer
	}

	pub fn get_bin_id_atop(&self, mut x: f32, mut y: f32) -> Option<u64> {
		let scale = self.current_effective_scale();
		x /= scale;
		y /= scale;

		let bins: Vec<Arc<Bin>> =
			self.bin_map.read().iter().filter_map(|(_, b)| b.upgrade()).collect();
		let mut inside = Vec::new();

		for bin in bins {
			if bin.mouse_inside(x, y) {
				if !bin.style_copy().pass_events.unwrap_or(false) {
					let z = bin.post_update().z_index;
					inside.push((z, bin));
				}
			}
		}

		inside.sort_by_key(|&(z, _)| z);
		inside.pop().map(|v| v.1.id())
	}

	pub fn get_bin_atop(&self, mut x: f32, mut y: f32) -> Option<Arc<Bin>> {
		let scale = self.current_effective_scale();
		x /= scale;
		y /= scale;

		let bins: Vec<Arc<Bin>> =
			self.bin_map.read().iter().filter_map(|(_, b)| b.upgrade()).collect();
		let mut inside = Vec::new();

		for bin in bins {
			if bin.mouse_inside(x, y) {
				if !bin.style_copy().pass_events.unwrap_or(false) {
					let z = bin.post_update().z_index;
					inside.push((z, bin));
				}
			}
		}

		inside.sort_by_key(|&(z, _)| z);
		inside.pop().map(|v| v.1)
	}

	/// Returns a list of all bins that have a strong reference. Note keeping this
	/// list will keep all bins returned alive and prevent them from being dropped.
	/// This list should be dropped asap to prevent issues with bins being dropped.
	pub fn bins(&self) -> Vec<Arc<Bin>> {
		self.bin_map.read().iter().filter_map(|(_, b)| b.upgrade()).collect()
	}

	pub fn new_bins(&self, amt: usize) -> Vec<Arc<Bin>> {
		let mut out = Vec::with_capacity(amt);
		let mut bin_i = self.bin_i.lock();
		let mut bin_map = self.bin_map.write();

		for _ in 0..amt {
			let id = *bin_i;
			*bin_i += 1;
			let bin = Bin::new(id.clone(), self.basalt.clone());
			bin_map.insert(id, Arc::downgrade(&bin));
			out.push(bin);
		}

		out
	}

	pub fn new_bin(&self) -> Arc<Bin> {
		self.new_bins(1).pop().unwrap()
	}

	pub fn get_bin(&self, id: u64) -> Option<Arc<Bin>> {
		match self.bin_map.read().get(&id) {
			Some(some) => some.upgrade(),
			None => None,
		}
	}

	pub fn mouse_inside(&self, mut mouse_x: f32, mut mouse_y: f32) -> bool {
		let scale = self.current_effective_scale();
		mouse_x /= scale;
		mouse_y /= scale;

		for bin in self.bins() {
			if bin.mouse_inside(mouse_x, mouse_y) {
				return true;
			}
		}
		false
	}

	pub fn draw<S: Send + Sync + 'static>(
		&self,
		cmd: AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
		target: ItfDrawTarget<S>,
	) -> (AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>, Option<Arc<BstImageView>>) {
		self.renderer.lock().draw(cmd, target)
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BstMSAALevel {
	One,
	Two,
	Four,
	Eight,
}

impl PartialOrd for BstMSAALevel {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for BstMSAALevel {
	fn cmp(&self, other: &Self) -> std::cmp::Ordering {
		self.as_u32().cmp(&other.as_u32())
	}
}

impl BstMSAALevel {
	pub(crate) fn as_u32(&self) -> u32 {
		match self {
			Self::One => 1,
			Self::Two => 2,
			Self::Four => 4,
			Self::Eight => 8,
		}
	}

	pub(crate) fn as_vulkano(&self) -> vulkano::image::SampleCount {
		match self {
			Self::One => vulkano::image::SampleCount::Sample1,
			Self::Two => vulkano::image::SampleCount::Sample2,
			Self::Four => vulkano::image::SampleCount::Sample4,
			Self::Eight => vulkano::image::SampleCount::Sample8,
		}
	}

	pub fn increase(&mut self) {
		*self = match self {
			Self::One => Self::Two,
			Self::Two => Self::Four,
			Self::Four => Self::Eight,
			Self::Eight => Self::Eight,
		};
	}

	pub fn decrease(&mut self) {
		*self = match self {
			Self::One => Self::One,
			Self::Two => Self::One,
			Self::Four => Self::Two,
			Self::Eight => Self::Four,
		};
	}
}
