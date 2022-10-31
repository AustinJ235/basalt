use std::sync::Arc;

use crossbeam::queue::SegQueue;
use vulkano::descriptor_set::allocator::{DescriptorSetAlloc, DescriptorSetAllocator};
use vulkano::descriptor_set::layout::DescriptorSetLayout;
use vulkano::descriptor_set::pool::{
    DescriptorPool, DescriptorPoolAllocError, DescriptorPoolCreateInfo, DescriptorSetAllocateInfo,
};
use vulkano::descriptor_set::sys::UnsafeDescriptorSet;
use vulkano::device::{Device, DeviceOwned};
use vulkano::OomError;

const POOL_SET_COUNT: u32 = 16;

pub struct LayerDescPool {
    device: Arc<Device>,
    layout: Arc<DescriptorSetLayout>,
    pools: SegQueue<DescriptorPool>,
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
            pools: SegQueue::new(),
            reserve: Arc::new(SegQueue::new()),
        }
    }
}

unsafe impl DescriptorSetAllocator for LayerDescPool {
    type Alloc = LayerDescAlloc;

    fn allocate(
        &self,
        _layout: &Arc<DescriptorSetLayout>,
        _variable_descriptor_count: u32,
    ) -> Result<LayerDescAlloc, OomError> {
        // TODO: check layout & variable_descriptor_count ???

        if let Some(existing) = self.reserve.pop() {
            return Ok(LayerDescAlloc {
                inner: Some(existing),
                reserve: self.reserve.clone(),
            });
        }

        let mut pool_sizes = self.layout.descriptor_counts().clone();
        pool_sizes.values_mut().for_each(|v| *v *= POOL_SET_COUNT);

        let pool = DescriptorPool::new(
            self.device.clone(),
            DescriptorPoolCreateInfo {
                max_sets: POOL_SET_COUNT,
                pool_sizes,
                can_free_descriptor_sets: false,
                ..DescriptorPoolCreateInfo::default()
            },
        )?;

        let variable_descriptor_count = self.layout.variable_descriptor_count();
        let mut sets = unsafe {
            match pool.allocate_descriptor_sets((0..POOL_SET_COUNT).map(|_| {
                DescriptorSetAllocateInfo {
                    layout: &self.layout,
                    variable_descriptor_count,
                }
            })) {
                Ok(ok) => Ok(ok),
                Err(DescriptorPoolAllocError::OutOfHostMemory) => Err(OomError::OutOfHostMemory),
                Err(DescriptorPoolAllocError::OutOfDeviceMemory) => {
                    Err(OomError::OutOfDeviceMemory)
                },
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

impl DescriptorSetAlloc for LayerDescAlloc {
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
