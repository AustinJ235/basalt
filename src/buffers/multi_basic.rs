use parking_lot::Mutex;
use super::basic::BasicBuf;
use std::collections::BTreeMap;
use super::Vert;
use std::sync::Arc;
use vulkano::device::{self,Device};
use vulkano::buffer::immutable::ImmutableBuffer;
use vulkano::image::traits::ImageViewAccess;
use vulkano::sampler::Sampler;
use shaders::vs;
use super::Buffer;
use atlas::Atlas;

pub struct MultiBasicBuf {
	atlas: Arc<Atlas>,
	inner: Mutex<Inner>,
}

struct Inner {
	current: usize,
	buffers: BTreeMap<usize, Arc<BasicBuf>>,
}

impl MultiBasicBuf {
	pub(crate) fn new(atlas: Arc<Atlas>) -> Self {
		MultiBasicBuf {
			atlas: atlas,
			inner: Mutex::new(Inner {
				current: 0,
				buffers: BTreeMap::new()
			}),
		}
	}
	
	/// Adds a buffer and returns the newly created buffer
	pub fn add(&self, i: usize) -> Arc<BasicBuf> {
		let new_buf = Arc::new(BasicBuf::new(self.atlas.clone()));
		self.inner.lock().buffers.insert(i, new_buf.clone());
		new_buf
	}
	
	/// Get the buffer or else add it
	pub fn get_or_add(&self, i: usize) -> Arc<BasicBuf> {
		match self.get(i) {
			Some(some) => some,
			None => self.add(i)
		}
	}
	
	/// Gets a buffer with an index
	pub fn get(&self, i: usize) -> Option<Arc<BasicBuf>> {
		self.inner.lock().buffers.get(&i).cloned()
	}
	
	/// Removes all buffers
	pub fn clear(&self) {
		self.inner.lock().buffers.clear();
	}
	
	/// Removes just a buffer
	pub fn remove(&self, i: usize) {
		self.inner.lock().buffers.remove(&i);
	}
	
	/// Returns an enumerated list of all the buffers
	pub fn all(&self) -> Vec<(usize, Arc<BasicBuf>)> {
		let mut out = Vec::new();
		for (i, buf) in &self.inner.lock().buffers {
			out.push((i.clone(), buf.clone()));
		} out
	}
	
	/// Switch the rendering buffer
	pub fn switch_to(&self, i: usize) {
		self.inner.lock().current = i;
	}
}

impl Buffer for MultiBasicBuf {
	fn draw(&self, device: Arc<Device>, queue: Arc<device::Queue>) ->
		Option<(
			Arc<ImmutableBuffer<vs::ty::Other>>,
			Vec<(
				Arc<ImageViewAccess + Send + Sync>,
				Arc<Sampler>,
				Arc<ImmutableBuffer<[Vert]>>
			)>, Vec<(
				Arc<ImageViewAccess + Send + Sync>,
				Arc<Sampler>,
				Arc<ImmutableBuffer<[Vert]>>
			)>,
		)>
	{
		let inner = self.inner.lock();
		
		match inner.buffers.get(&inner.current) {
			Some(some) => some.draw(device, queue),
			None => None
		}
	}
	
	fn triangles(&self) -> usize {
		let inner = self.inner.lock();
		
		match inner.buffers.get(&inner.current) {
			Some(some) => some.triangles(),
			None => 0
		}
	}
}
