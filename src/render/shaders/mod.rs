use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

mod vko {
    pub use vulkano::descriptor_set::layout::{
        DescriptorBindingFlags, DescriptorSetLayoutBinding, DescriptorSetLayoutCreateFlags,
        DescriptorSetLayoutCreateInfo, DescriptorType,
    };
    pub use vulkano::device::Device;
    pub use vulkano::pipeline::layout::{
        PipelineDescriptorSetLayoutCreateInfo, PipelineLayoutCreateFlags,
    };
    pub use vulkano::shader::{ShaderModule, ShaderStages};
}

static UI_VS_MODULE: OnceLock<Arc<vko::ShaderModule>> = OnceLock::new();

pub fn ui_vs_sm(device: Arc<vko::Device>) -> Arc<vko::ShaderModule> {
    UI_VS_MODULE
        .get_or_init(move || ui_vs::load(device).unwrap())
        .clone()
}

pub mod ui_vs {
    vulkano_shaders::shader! {
        ty: "vertex",
        vulkan_version: "1.2",
        spirv_version: "1.5",
        path: "./src/render/shaders/ui.vs"
    }
}

static UI_FS_MODULE: OnceLock<Arc<vko::ShaderModule>> = OnceLock::new();

pub fn ui_fs_sm(device: Arc<vko::Device>) -> Arc<vko::ShaderModule> {
    UI_FS_MODULE
        .get_or_init(move || ui_fs::load(device).unwrap())
        .clone()
}

pub mod ui_fs {
    vulkano_shaders::shader! {
        ty: "fragment",
        vulkan_version: "1.2",
        spirv_version: "1.5",
        path: "./src/render/shaders/ui.fs"
    }
}

pub fn pipeline_descriptor_set_layout_create_info(
    image_capacity: u32,
) -> vko::PipelineDescriptorSetLayoutCreateInfo {
    vko::PipelineDescriptorSetLayoutCreateInfo {
        flags: vko::PipelineLayoutCreateFlags::empty(),
        set_layouts: vec![vko::DescriptorSetLayoutCreateInfo {
            flags: vko::DescriptorSetLayoutCreateFlags::empty(),
            bindings: BTreeMap::from([
                (
                    0,
                    vko::DescriptorSetLayoutBinding {
                        binding_flags: vko::DescriptorBindingFlags::empty(),
                        descriptor_count: 1,
                        stages: vko::ShaderStages::FRAGMENT,
                        immutable_samplers: Vec::new(),
                        ..vko::DescriptorSetLayoutBinding::descriptor_type(
                            vko::DescriptorType::Sampler,
                        )
                    },
                ),
                (
                    1,
                    vko::DescriptorSetLayoutBinding {
                        binding_flags: vko::DescriptorBindingFlags::VARIABLE_DESCRIPTOR_COUNT,
                        descriptor_count: image_capacity,
                        stages: vko::ShaderStages::FRAGMENT,
                        immutable_samplers: Vec::new(),
                        ..vko::DescriptorSetLayoutBinding::descriptor_type(
                            vko::DescriptorType::SampledImage,
                        )
                    },
                ),
            ]),
            ..Default::default()
        }],
        push_constant_ranges: Vec::new(),
    }
}

static FINAL_VS_MODULE: OnceLock<Arc<vko::ShaderModule>> = OnceLock::new();

pub fn final_vs_sm(device: Arc<vko::Device>) -> Arc<vko::ShaderModule> {
    FINAL_VS_MODULE
        .get_or_init(move || final_vs::load(device).unwrap())
        .clone()
}

pub mod final_vs {
    vulkano_shaders::shader! {
        ty: "vertex",
        vulkan_version: "1.2",
        spirv_version: "1.5",
        path: "./src/render/shaders/final.vs"
    }
}

static FINAL_FS_MODULE: OnceLock<Arc<vko::ShaderModule>> = OnceLock::new();

pub fn final_fs_sm(device: Arc<vko::Device>) -> Arc<vko::ShaderModule> {
    FINAL_FS_MODULE
        .get_or_init(move || final_fs::load(device).unwrap())
        .clone()
}

pub mod final_fs {
    vulkano_shaders::shader! {
        ty: "fragment",
        vulkan_version: "1.2",
        spirv_version: "1.5",
        path: "./src/render/shaders/final.fs"
    }
}
