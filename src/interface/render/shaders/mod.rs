pub mod ui_fs {
    vulkano_shaders::shader! {
        ty: "fragment",
        vulkan_version: "1.2",
        spirv_version: "1.5",
        path: "./src/interface/render/shaders/ui.fs"
    }
}

pub mod ui_vs {
    vulkano_shaders::shader! {
        ty: "vertex",
        vulkan_version: "1.2",
        spirv_version: "1.5",
        path: "./src/interface/render/shaders/ui.vs"
    }
}
