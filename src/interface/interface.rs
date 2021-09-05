use crate::interface::bin::Bin;
use crate::interface::hook::HookManager;
use crate::interface::odb::OrderedDualBuffer;
use crate::interface::widget::theme::WidgetTheme;
use crate::interface::widget::{Widget, WidgetID};
use crate::{Basalt, BstEvent, BstItfEv, BstMSAALevel};
use ilmenite::{Ilmenite, ImtFillQuality, ImtFont, ImtRasterOpts, ImtSampleQuality, ImtWeight};
use parking_lot::{Mutex, RwLock};
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

impl_vertex!(ItfVertInfo, position, coords, color, ty);
#[derive(Clone, Debug)]
#[repr(C)]
pub(crate) struct ItfVertInfo {
	pub position: (f32, f32, f32),
	pub coords: (f32, f32),
	pub color: (f32, f32, f32, f32),
	pub ty: i32,
}

impl Default for ItfVertInfo {
	fn default() -> Self {
		ItfVertInfo {
			position: (0.0, 0.0, 0.0),
			coords: (0.0, 0.0),
			color: (0.0, 0.0, 0.0, 0.0),
			ty: 0,
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

#[allow(dead_code)]
struct BinBufferData {
	atlas_i: usize,
	pos: usize,
	len: usize,
}

pub struct Interface {
	basalt: Arc<Basalt>,
	bin_i: Mutex<u64>,
	bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	widget_i: Mutex<WidgetID>,
	widget_map: RwLock<BTreeMap<WidgetID, Weak<dyn Widget + Send + Sync>>>,
	widget_theme: Mutex<WidgetTheme>,
	scale: Mutex<f32>,
	msaa: Mutex<BstMSAALevel>,
	pub(crate) ilmenite: Arc<Ilmenite>,
	pub(crate) odb: Arc<OrderedDualBuffer>,
	pub(crate) hook_manager: Arc<HookManager>,
}

impl Interface {
	pub(crate) fn scale(&self) -> f32 {
		*self.scale.lock()
	}

	pub(crate) fn set_scale(&self, to: f32) {
		*self.scale.lock() = to;
		self.basalt.send_event(BstEvent::BstItfEv(BstItfEv::ScaleChanged));
	}

	pub fn msaa(&self) -> BstMSAALevel {
		*self.msaa.lock()
	}

	pub fn set_msaa(&self, amt: BstMSAALevel) {
		*self.msaa.lock() = amt;
		self.basalt.send_event(BstEvent::BstItfEv(BstItfEv::MSAAChanged));
	}

	pub fn increase_msaa(&self) {
		self.msaa.lock().increase();
		self.basalt.send_event(BstEvent::BstItfEv(BstItfEv::MSAAChanged));
	}

	pub fn decrease_msaa(&self) {
		self.msaa.lock().decrease();
		self.basalt.send_event(BstEvent::BstItfEv(BstItfEv::MSAAChanged));
	}

	pub(crate) fn new(basalt: Arc<Basalt>) -> Arc<Self> {
		let bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>> =
			Arc::new(RwLock::new(BTreeMap::new()));
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
			odb: OrderedDualBuffer::new(basalt.clone(), bin_map.clone()),
			bin_i: Mutex::new(0),
			bin_map,
			widget_i: Mutex::new(0),
			widget_map: RwLock::new(BTreeMap::new()),
			widget_theme: Mutex::new(WidgetTheme::default()),
			scale: Mutex::new(basalt.options_ref().scale),
			msaa: Mutex::new(basalt.options_ref().msaa),
			hook_manager: HookManager::new(basalt.clone()),
			ilmenite,
			basalt,
		})
	}

	pub(crate) fn next_widget_id(&self) -> WidgetID {
		let mut widget_i = self.widget_i.lock();
		let out = *widget_i;
		*widget_i += 1;
		out
	}

	pub(crate) fn register_widget(&self, widget: Arc<dyn Widget + Send + Sync>) {
		let mut widget_map = self.widget_map.write();
		let id = widget.id();
		widget_map.insert(id, Arc::downgrade(&widget));
	}

	pub fn set_widget_theme(&self, theme: WidgetTheme) {
		*self.widget_theme.lock() = theme.clone();
		self.widget_map
			.read()
			.values()
			.filter_map(|wk| wk.upgrade())
			.for_each(|w| w.set_theme(theme.clone()));
	}

	pub fn current_widget_theme(&self) -> WidgetTheme {
		self.widget_theme.lock().clone()
	}

	pub fn get_bin_id_atop(&self, mut x: f32, mut y: f32) -> Option<u64> {
		let scale = self.scale();
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
		let scale = self.scale();
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
		let scale = self.scale();
		mouse_x /= scale;
		mouse_y /= scale;

		for bin in self.bins() {
			if bin.mouse_inside(mouse_x, mouse_y) {
				return true;
			}
		}
		false
	}
}
