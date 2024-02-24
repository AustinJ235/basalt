use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::layout::{
    DescriptorBindingFlags, DescriptorSetLayout, DescriptorSetLayoutBinding,
    DescriptorSetLayoutCreateFlags, DescriptorSetLayoutCreateInfo, DescriptorType,
};
use vulkano::descriptor_set::persistent::PersistentDescriptorSet;
use vulkano::descriptor_set::WriteDescriptorSet;
use vulkano::device::Device;
use vulkano::image::sampler::{Sampler, SamplerAddressMode, SamplerCreateInfo};
use vulkano::image::view::ImageView;
use vulkano::image::Image;
use vulkano::pipeline::layout::{PipelineDescriptorSetLayoutCreateInfo, PipelineLayoutCreateFlags};
use vulkano::shader::{ShaderModule, ShaderStages};

static UI_VS_MODULE: OnceLock<Arc<ShaderModule>> = OnceLock::new();

pub fn ui_vs_sm(device: Arc<Device>) -> Arc<ShaderModule> {
    UI_VS_MODULE
        .get_or_init(move || ui_vs::load(device).unwrap())
        .clone()
}

pub mod ui_vs {
    vulkano_shaders::shader! {
        ty: "vertex",
        vulkan_version: "1.2",
        spirv_version: "1.5",
        path: "./src/renderer/shaders/ui.vs"
    }
}

static UI_FS_MODULE: OnceLock<Arc<ShaderModule>> = OnceLock::new();

pub fn ui_fs_sm(device: Arc<Device>) -> Arc<ShaderModule> {
    UI_FS_MODULE
        .get_or_init(move || ui_fs::load(device).unwrap())
        .clone()
}

pub mod ui_fs {
    vulkano_shaders::shader! {
        ty: "fragment",
        vulkan_version: "1.2",
        spirv_version: "1.5",
        path: "./src/renderer/shaders/ui.fs"
    }
}

pub fn pipeline_descriptor_set_layout_create_info(
    image_capacity: u32,
) -> PipelineDescriptorSetLayoutCreateInfo {
    PipelineDescriptorSetLayoutCreateInfo {
        flags: PipelineLayoutCreateFlags::empty(),
        set_layouts: vec![DescriptorSetLayoutCreateInfo {
            flags: DescriptorSetLayoutCreateFlags::empty(),
            bindings: BTreeMap::from([
                (
                    0,
                    DescriptorSetLayoutBinding {
                        binding_flags: DescriptorBindingFlags::empty(),
                        descriptor_count: 1,
                        stages: ShaderStages::FRAGMENT,
                        immutable_samplers: Vec::new(),
                        ..DescriptorSetLayoutBinding::descriptor_type(DescriptorType::Sampler)
                    },
                ),
                (
                    1,
                    DescriptorSetLayoutBinding {
                        binding_flags: DescriptorBindingFlags::VARIABLE_DESCRIPTOR_COUNT,
                        descriptor_count: image_capacity,
                        stages: ShaderStages::FRAGMENT,
                        immutable_samplers: Vec::new(),
                        ..DescriptorSetLayoutBinding::descriptor_type(DescriptorType::SampledImage)
                    },
                ),
            ]),
            ..DescriptorSetLayoutCreateInfo::default()
        }],
        push_constant_ranges: Vec::new(),
    }
}

static SAMPLER: OnceLock<Arc<Sampler>> = OnceLock::new();

pub fn create_desc_set(
    device: Arc<Device>,
    desc_alloc: &StandardDescriptorSetAllocator,
    image_capacity: u32,
    images: Vec<Arc<Image>>,
    default_image: Arc<ImageView>,
) -> Arc<PersistentDescriptorSet> {
    assert!(images.len() <= image_capacity as usize);
    let device_sampler = device.clone();

    let sampler = SAMPLER
        .get_or_init(move || {
            Sampler::new(
                device_sampler,
                SamplerCreateInfo {
                    address_mode: [SamplerAddressMode::ClampToBorder; 3],
                    unnormalized_coordinates: true,
                    ..SamplerCreateInfo::default()
                },
            )
            .unwrap()
        })
        .clone();

    let layout = DescriptorSetLayout::new(
        device.clone(),
        pipeline_descriptor_set_layout_create_info(image_capacity).set_layouts[0].clone(),
    )
    .unwrap();

    let num_default_images = image_capacity as usize - images.len();

    PersistentDescriptorSet::new_variable(
        desc_alloc,
        layout,
        image_capacity,
        [
            WriteDescriptorSet::sampler(0, sampler),
            WriteDescriptorSet::image_view_array(
                1,
                0,
                images
                    .into_iter()
                    .map(|image| ImageView::new_default(image).unwrap())
                    .chain((0..num_default_images).map(|_| default_image.clone())),
            ),
        ],
        [],
    )
    .unwrap()
}
