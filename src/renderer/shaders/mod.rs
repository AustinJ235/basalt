use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use vulkano::descriptor_set::layout::{
    DescriptorBindingFlags, DescriptorSetLayoutBinding, DescriptorSetLayoutCreateFlags,
    DescriptorSetLayoutCreateInfo, DescriptorType,
};
use vulkano::device::Device;
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
