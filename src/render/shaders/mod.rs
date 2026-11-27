use std::sync::{Arc, OnceLock};

mod vko {
    pub use vulkano::descriptor_set::layout::{
        DescriptorBindingFlags, DescriptorSetLayout, DescriptorSetLayoutBinding,
        DescriptorSetLayoutCreateInfo, DescriptorType,
    };
    pub use vulkano::device::Device;
    pub use vulkano::shader::{ShaderModule, ShaderStages};
    pub use vulkano::{Validated, VulkanError};
}

static UI_VS_MODULE: OnceLock<Arc<vko::ShaderModule>> = OnceLock::new();

pub fn ui_vs_sm(device: Arc<vko::Device>) -> Arc<vko::ShaderModule> {
    UI_VS_MODULE
        .get_or_init(move || ui_vs::load(&device).unwrap())
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
        .get_or_init(move || ui_fs::load(&device).unwrap())
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

pub fn create_descriptor_set_layout(
    device: &Arc<vko::Device>,
    image_capacity: u32,
) -> Result<Arc<vko::DescriptorSetLayout>, vko::Validated<vko::VulkanError>> {
    vko::DescriptorSetLayout::new(
        device,
        &vko::DescriptorSetLayoutCreateInfo {
            bindings: &[
                vko::DescriptorSetLayoutBinding {
                    binding: 0,
                    descriptor_count: 1,
                    stages: vko::ShaderStages::FRAGMENT,
                    ..vko::DescriptorSetLayoutBinding::new(vko::DescriptorType::Sampler)
                },
                vko::DescriptorSetLayoutBinding {
                    binding: 1,
                    binding_flags: vko::DescriptorBindingFlags::VARIABLE_DESCRIPTOR_COUNT,
                    descriptor_count: image_capacity,
                    stages: vko::ShaderStages::FRAGMENT,
                    ..vko::DescriptorSetLayoutBinding::new(vko::DescriptorType::SampledImage)
                },
            ],
            ..Default::default()
        },
    )
}

static FINAL_VS_MODULE: OnceLock<Arc<vko::ShaderModule>> = OnceLock::new();

pub fn final_vs_sm(device: Arc<vko::Device>) -> Arc<vko::ShaderModule> {
    FINAL_VS_MODULE
        .get_or_init(move || final_vs::load(&device).unwrap())
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
        .get_or_init(move || final_fs::load(&device).unwrap())
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
