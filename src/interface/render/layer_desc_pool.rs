use crossbeam::queue::SegQueue;
use std::sync::Arc;
use vulkano::descriptor_set::layout::DescriptorSetLayout;
use vulkano::descriptor_set::pool::{
	DescriptorPool, DescriptorPoolAlloc, DescriptorPoolAllocError, DescriptorSetAllocateInfo,
	UnsafeDescriptorPool,
};
use vulkano::descriptor_set::sys::UnsafeDescriptorSet;
use vulkano::device::{Device, DeviceOwned};
use vulkano::OomError;

const POOL_SET_COUNT: u32 = 16;

pub struct LayerDescPool {
	device: Arc<Device>,
	layout: Arc<DescriptorSetLayout>,
	pools: Vec<UnsafeDescriptorPool>,
	reserve: Arc<SegQueue<UnsafeDescriptorSet>>,
}

pub struct LayerDescAlloc {
	inner: Option<UnsafeDescriptorSet>,
	reserve: Arc<SegQueue<UnsafeDescriptorSet>>,
}

impl LayerDescPool {
	pub fn new(device: Arc<Device>, layout: Arc<DescriptorSetLayout>) -> Self {
		Self {
			device,
			layout,
			pools: Vec::new(),
			reserve: Arc::new(SegQueue::new()),
		}
	}
}

unsafe impl DescriptorPool for LayerDescPool {
	type Alloc = LayerDescAlloc;

	fn alloc(
		&mut self,
		_layout: &DescriptorSetLayout,
		_variable_descriptor_count: u32,
	) -> Result<LayerDescAlloc, OomError> {
		// TODO: check layout & variable_descriptor_count ???

		if let Some(existing) = self.reserve.pop() {
			return Ok(LayerDescAlloc {
				inner: Some(existing),
				reserve: self.reserve.clone(),
			});
		}

		let mut pool = UnsafeDescriptorPool::new(
			self.device.clone(),
			&(*self.layout.descriptors_count() * POOL_SET_COUNT),
			POOL_SET_COUNT,
			false,
		)?;

		let variable_descriptor_count = self.layout.variable_descriptor_count();
		let mut sets = unsafe {
			match pool.alloc((0..POOL_SET_COUNT).map(|_| {
				DescriptorSetAllocateInfo {
					layout: &self.layout,
					variable_descriptor_count,
				}
			})) {
				Ok(ok) => Ok(ok),
				Err(DescriptorPoolAllocError::OutOfHostMemory) =>
					Err(OomError::OutOfHostMemory),
				Err(DescriptorPoolAllocError::OutOfDeviceMemory) =>
					Err(OomError::OutOfDeviceMemory),
				_ => unreachable!(),
			}?
		};

		let ret_set = sets.next().unwrap();
		sets.for_each(|set| self.reserve.push(set));
		self.pools.push(pool);

		Ok(LayerDescAlloc {
			inner: Some(ret_set),
			reserve: self.reserve.clone(),
		})
	}
}

unsafe impl DeviceOwned for LayerDescPool {
	fn device(&self) -> &Arc<Device> {
		&self.device
	}
}

impl DescriptorPoolAlloc for LayerDescAlloc {
	fn inner(&self) -> &UnsafeDescriptorSet {
		self.inner.as_ref().unwrap()
	}

	fn inner_mut(&mut self) -> &mut UnsafeDescriptorSet {
		self.inner.as_mut().unwrap()
	}
}

impl Drop for LayerDescAlloc {
	fn drop(&mut self) {
		self.reserve.push(self.inner.take().unwrap());
	}
}
