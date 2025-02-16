mod vko {
    pub use vulkano::format::Format;
}

use crate::image::{ImageData, ImageFormat};

pub fn image_data_to_vulkan_format(
    image_format: ImageFormat,
    image_data: &ImageData,
    vulkan_format: vko::Format,
) -> Vec<u8> {
    match vulkan_format {
        vko::Format::R8G8B8A8_UINT | vko::Format::R8G8B8A8_UNORM => {
            match image_data {
                ImageData::D8(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => image_data.clone(),
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| [chunk[0], chunk[1], chunk[2], 255])
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| [*value, *value, *value, 255])
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| [chunk[0], chunk[0], chunk[0], chunk[1]])
                                .collect()
                        },
                        ImageFormat::SRGBA => {
                            image_data
                                .iter()
                                .map(|value| f32u8(stl(u8f32(*value))))
                                .collect()
                        },
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(stl(u8f32(chunk[0]))),
                                        f32u8(stl(u8f32(chunk[1]))),
                                        f32u8(stl(u8f32(chunk[2]))),
                                        255,
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(stl(u8f32(*value)));
                                    [value, value, value, 255]
                                })
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(stl(u8f32(chunk[0])));
                                    [value, value, value, f32u8(stl(u8f32(chunk[1])))]
                                })
                                .collect()
                        },
                    }
                },
                ImageData::D16(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => {
                            image_data
                                .iter()
                                .map(|value| f32u8(u16f32(*value)))
                                .collect()
                        },
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(u16f32(chunk[0])),
                                        f32u8(u16f32(chunk[1])),
                                        f32u8(u16f32(chunk[2])),
                                        255,
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(u16f32(*value));
                                    [value, value, value, 255]
                                })
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(u16f32(chunk[0]));
                                    [value, value, value, f32u8(u16f32(chunk[1]))]
                                })
                                .collect()
                        },
                        ImageFormat::SRGBA => {
                            image_data
                                .iter()
                                .map(|value| f32u8(stl(u16f32(*value))))
                                .collect()
                        },
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(stl(u16f32(chunk[0]))),
                                        f32u8(stl(u16f32(chunk[1]))),
                                        f32u8(stl(u16f32(chunk[2]))),
                                        255,
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(stl(u16f32(*value)));
                                    [value, value, value, 255]
                                })
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(stl(u16f32(chunk[0])));
                                    [value, value, value, f32u8(stl(u16f32(chunk[1])))]
                                })
                                .collect()
                        },
                    }
                },
            }
        },
        vko::Format::B8G8R8A8_UINT | vko::Format::B8G8R8A8_UNORM => {
            match image_data {
                ImageData::D8(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| [chunk[2], chunk[1], chunk[0], chunk[3]])
                                .collect()
                        },
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| [chunk[2], chunk[1], chunk[0], 255])
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| [*value, *value, *value, 255])
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| [chunk[0], chunk[0], chunk[0], chunk[1]])
                                .collect()
                        },
                        ImageFormat::SRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(stl(u8f32(chunk[2]))),
                                        f32u8(stl(u8f32(chunk[1]))),
                                        f32u8(stl(u8f32(chunk[0]))),
                                        f32u8(stl(u8f32(chunk[3]))),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(stl(u8f32(chunk[2]))),
                                        f32u8(stl(u8f32(chunk[1]))),
                                        f32u8(stl(u8f32(chunk[0]))),
                                        255,
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(stl(u8f32(*value)));
                                    [value, value, value, 255]
                                })
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(stl(u8f32(chunk[0])));
                                    [value, value, value, f32u8(stl(u8f32(chunk[1])))]
                                })
                                .collect()
                        },
                    }
                },
                ImageData::D16(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(u16f32(chunk[2])),
                                        f32u8(u16f32(chunk[1])),
                                        f32u8(u16f32(chunk[0])),
                                        f32u8(u16f32(chunk[3])),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(u16f32(chunk[2])),
                                        f32u8(u16f32(chunk[1])),
                                        f32u8(u16f32(chunk[0])),
                                        255,
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(u16f32(*value));
                                    [value, value, value, 255]
                                })
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(u16f32(chunk[0]));
                                    [value, value, value, f32u8(u16f32(chunk[1]))]
                                })
                                .collect()
                        },
                        ImageFormat::SRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(stl(u16f32(chunk[2]))),
                                        f32u8(stl(u16f32(chunk[1]))),
                                        f32u8(stl(u16f32(chunk[0]))),
                                        f32u8(stl(u16f32(chunk[3]))),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(stl(u16f32(chunk[2]))),
                                        f32u8(stl(u16f32(chunk[1]))),
                                        f32u8(stl(u16f32(chunk[0]))),
                                        255,
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(stl(u16f32(*value)));
                                    [value, value, value, 255]
                                })
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(stl(u16f32(chunk[0])));
                                    [value, value, value, f32u8(stl(u16f32(chunk[1])))]
                                })
                                .collect()
                        },
                    }
                },
            }
        },
        vko::Format::A8B8G8R8_UINT_PACK32 | vko::Format::A8B8G8R8_UNORM_PACK32 => {
            match image_data {
                ImageData::D8(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| [chunk[3], chunk[2], chunk[1], chunk[0]])
                                .collect()
                        },
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| [255, chunk[2], chunk[1], chunk[0]])
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| [255, *value, *value, *value])
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| [chunk[1], chunk[0], chunk[0], chunk[0]])
                                .collect()
                        },
                        ImageFormat::SRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(stl(u8f32(chunk[3]))),
                                        f32u8(stl(u8f32(chunk[2]))),
                                        f32u8(stl(u8f32(chunk[1]))),
                                        f32u8(stl(u8f32(chunk[0]))),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        255,
                                        f32u8(stl(u8f32(chunk[2]))),
                                        f32u8(stl(u8f32(chunk[1]))),
                                        f32u8(stl(u8f32(chunk[0]))),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(stl(u8f32(*value)));
                                    [255, value, value, value]
                                })
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(stl(u8f32(chunk[0])));
                                    [f32u8(stl(u8f32(chunk[1]))), value, value, value]
                                })
                                .collect()
                        },
                    }
                },
                ImageData::D16(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(u16f32(chunk[3])),
                                        f32u8(u16f32(chunk[2])),
                                        f32u8(u16f32(chunk[1])),
                                        f32u8(u16f32(chunk[0])),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        255,
                                        f32u8(u16f32(chunk[2])),
                                        f32u8(u16f32(chunk[1])),
                                        f32u8(u16f32(chunk[0])),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(u16f32(*value));
                                    [255, value, value, value]
                                })
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(u16f32(chunk[0]));
                                    [f32u8(u16f32(chunk[1])), value, value, value]
                                })
                                .collect()
                        },
                        ImageFormat::SRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(stl(u16f32(chunk[3]))),
                                        f32u8(stl(u16f32(chunk[2]))),
                                        f32u8(stl(u16f32(chunk[1]))),
                                        f32u8(stl(u16f32(chunk[0]))),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        255,
                                        f32u8(stl(u16f32(chunk[2]))),
                                        f32u8(stl(u16f32(chunk[1]))),
                                        f32u8(stl(u16f32(chunk[0]))),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(stl(u16f32(*value)));
                                    [255, value, value, value]
                                })
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(stl(u16f32(chunk[0])));
                                    [f32u8(stl(u16f32(chunk[1]))), value, value, value]
                                })
                                .collect()
                        },
                    }
                },
            }
        },
        vko::Format::R8G8B8A8_SRGB => {
            match image_data {
                ImageData::D8(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => {
                            image_data
                                .iter()
                                .map(|value| f32u8(lts(u8f32(*value))))
                                .collect()
                        },
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(lts(u8f32(chunk[0]))),
                                        f32u8(lts(u8f32(chunk[1]))),
                                        f32u8(lts(u8f32(chunk[2]))),
                                        255,
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(lts(u8f32(*value)));
                                    [value, value, value, 255]
                                })
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(lts(u8f32(chunk[0])));
                                    [value, value, value, f32u8(lts(u8f32(chunk[1])))]
                                })
                                .collect()
                        },
                        ImageFormat::SRGBA => image_data.clone(),
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| [chunk[0], chunk[1], chunk[2], 255])
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| [*value, *value, *value, 255])
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| [chunk[0], chunk[0], chunk[0], chunk[1]])
                                .collect()
                        },
                    }
                },
                ImageData::D16(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => {
                            image_data
                                .iter()
                                .map(|value| f32u8(lts(u16f32(*value))))
                                .collect()
                        },
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(lts(u16f32(chunk[0]))),
                                        f32u8(lts(u16f32(chunk[1]))),
                                        f32u8(lts(u16f32(chunk[2]))),
                                        255,
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(lts(u16f32(*value)));
                                    [value, value, value, 255]
                                })
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(lts(u16f32(chunk[0])));
                                    [value, value, value, f32u8(lts(u16f32(chunk[1])))]
                                })
                                .collect()
                        },
                        ImageFormat::SRGBA => {
                            image_data
                                .iter()
                                .map(|value| f32u8(u16f32(*value)))
                                .collect()
                        },
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(u16f32(chunk[0])),
                                        f32u8(u16f32(chunk[1])),
                                        f32u8(u16f32(chunk[2])),
                                        255,
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(u16f32(*value));
                                    [value, value, value, 255]
                                })
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(u16f32(chunk[0]));
                                    [value, value, value, f32u8(u16f32(chunk[1]))]
                                })
                                .collect()
                        },
                    }
                },
            }
        },
        vko::Format::B8G8R8A8_SRGB => {
            match image_data {
                ImageData::D8(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(lts(u8f32(chunk[2]))),
                                        f32u8(lts(u8f32(chunk[1]))),
                                        f32u8(lts(u8f32(chunk[0]))),
                                        f32u8(lts(u8f32(chunk[3]))),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(lts(u8f32(chunk[2]))),
                                        f32u8(lts(u8f32(chunk[1]))),
                                        f32u8(lts(u8f32(chunk[0]))),
                                        255,
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(lts(u8f32(*value)));
                                    [value, value, value, 255]
                                })
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(lts(u8f32(chunk[0])));
                                    [value, value, value, f32u8(lts(u8f32(chunk[1])))]
                                })
                                .collect()
                        },
                        ImageFormat::SRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| [chunk[2], chunk[1], chunk[0], chunk[3]])
                                .collect()
                        },
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| [chunk[2], chunk[1], chunk[0], 255])
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| [*value, *value, *value, 255])
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| [chunk[0], chunk[0], chunk[0], chunk[1]])
                                .collect()
                        },
                    }
                },
                ImageData::D16(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(lts(u16f32(chunk[2]))),
                                        f32u8(lts(u16f32(chunk[1]))),
                                        f32u8(lts(u16f32(chunk[0]))),
                                        f32u8(lts(u16f32(chunk[3]))),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(lts(u16f32(chunk[2]))),
                                        f32u8(lts(u16f32(chunk[1]))),
                                        f32u8(lts(u16f32(chunk[0]))),
                                        255,
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(lts(u16f32(*value)));
                                    [value, value, value, 255]
                                })
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(lts(u16f32(chunk[0])));
                                    [value, value, value, f32u8(lts(u16f32(chunk[1])))]
                                })
                                .collect()
                        },
                        ImageFormat::SRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(u16f32(chunk[2])),
                                        f32u8(u16f32(chunk[1])),
                                        f32u8(u16f32(chunk[0])),
                                        f32u8(u16f32(chunk[3])),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(u16f32(chunk[2])),
                                        f32u8(u16f32(chunk[1])),
                                        f32u8(u16f32(chunk[0])),
                                        255,
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(u16f32(*value));
                                    [value, value, value, 255]
                                })
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(u16f32(chunk[0]));
                                    [value, value, value, f32u8(u16f32(chunk[1]))]
                                })
                                .collect()
                        },
                    }
                },
            }
        },
        vko::Format::A8B8G8R8_SRGB_PACK32 => {
            match image_data {
                ImageData::D8(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(lts(u8f32(chunk[3]))),
                                        f32u8(lts(u8f32(chunk[2]))),
                                        f32u8(lts(u8f32(chunk[1]))),
                                        f32u8(lts(u8f32(chunk[0]))),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        255,
                                        f32u8(lts(u8f32(chunk[2]))),
                                        f32u8(lts(u8f32(chunk[1]))),
                                        f32u8(lts(u8f32(chunk[0]))),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(lts(u8f32(*value)));
                                    [255, value, value, value]
                                })
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(lts(u8f32(chunk[0])));
                                    [f32u8(lts(u8f32(chunk[1]))), value, value, value]
                                })
                                .collect()
                        },
                        ImageFormat::SRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| [chunk[3], chunk[2], chunk[1], chunk[0]])
                                .collect()
                        },
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| [255, chunk[2], chunk[1], chunk[0]])
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| [255, *value, *value, *value])
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| [chunk[1], chunk[0], chunk[0], chunk[0]])
                                .collect()
                        },
                    }
                },
                ImageData::D16(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(lts(u16f32(chunk[3]))),
                                        f32u8(lts(u16f32(chunk[2]))),
                                        f32u8(lts(u16f32(chunk[1]))),
                                        f32u8(lts(u16f32(chunk[0]))),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        255,
                                        f32u8(lts(u16f32(chunk[2]))),
                                        f32u8(lts(u16f32(chunk[1]))),
                                        f32u8(lts(u16f32(chunk[0]))),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(lts(u16f32(*value)));
                                    [255, value, value, value]
                                })
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(lts(u16f32(chunk[0])));
                                    [f32u8(lts(u16f32(chunk[1]))), value, value, value]
                                })
                                .collect()
                        },
                        ImageFormat::SRGBA => {
                            image_data
                                .chunks_exact(4)
                                .flat_map(|chunk| {
                                    [
                                        f32u8(u16f32(chunk[3])),
                                        f32u8(u16f32(chunk[2])),
                                        f32u8(u16f32(chunk[1])),
                                        f32u8(u16f32(chunk[0])),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        255,
                                        f32u8(u16f32(chunk[2])),
                                        f32u8(u16f32(chunk[1])),
                                        f32u8(u16f32(chunk[0])),
                                    ]
                                })
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u8(u16f32(*value));
                                    [255, value, value, value]
                                })
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u8(u16f32(chunk[0]));
                                    [f32u8(u16f32(chunk[1])), value, value, value]
                                })
                                .collect()
                        },
                    }
                },
            }
        },
        vko::Format::R16G16B16A16_UINT | vko::Format::R16G16B16A16_UNORM => {
            match image_data {
                ImageData::D8(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => {
                            image_data
                                .iter()
                                .flat_map(|value| f32u16(u8f32(*value)).to_ne_bytes())
                                .collect()
                        },
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u16(u8f32(chunk[0])).to_ne_bytes(),
                                        f32u16(u8f32(chunk[1])).to_ne_bytes(),
                                        f32u16(u8f32(chunk[2])).to_ne_bytes(),
                                        65535_u16.to_ne_bytes(),
                                    ]
                                    .into_iter()
                                    .flatten()
                                })
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u16(u8f32(*value)).to_ne_bytes();
                                    [value, value, value, 65535_u16.to_ne_bytes()]
                                        .into_iter()
                                        .flatten()
                                })
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u16(u8f32(chunk[0])).to_ne_bytes();
                                    [value, value, value, f32u16(u8f32(chunk[1])).to_ne_bytes()]
                                        .into_iter()
                                        .flatten()
                                })
                                .collect()
                        },
                        ImageFormat::SRGBA => {
                            image_data
                                .iter()
                                .flat_map(|value| f32u16(stl(u8f32(*value))).to_ne_bytes())
                                .collect()
                        },
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u16(stl(u8f32(chunk[0]))).to_ne_bytes(),
                                        f32u16(stl(u8f32(chunk[1]))).to_ne_bytes(),
                                        f32u16(stl(u8f32(chunk[2]))).to_ne_bytes(),
                                        65535_u16.to_ne_bytes(),
                                    ]
                                    .into_iter()
                                    .flatten()
                                })
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u16(stl(u8f32(*value))).to_ne_bytes();
                                    [value, value, value, 65535_u16.to_ne_bytes()]
                                        .into_iter()
                                        .flatten()
                                })
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u16(stl(u8f32(chunk[0]))).to_ne_bytes();
                                    [
                                        value,
                                        value,
                                        value,
                                        f32u16(stl(u8f32(chunk[1]))).to_ne_bytes(),
                                    ]
                                    .into_iter()
                                    .flatten()
                                })
                                .collect()
                        },
                    }
                },
                ImageData::D16(image_data) => {
                    match image_format {
                        ImageFormat::LRGBA => {
                            image_data
                                .iter()
                                .flat_map(|value| value.to_ne_bytes())
                                .collect()
                        },
                        ImageFormat::LRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        chunk[0].to_ne_bytes(),
                                        chunk[1].to_ne_bytes(),
                                        chunk[2].to_ne_bytes(),
                                        65535_u16.to_ne_bytes(),
                                    ]
                                    .into_iter()
                                    .flatten()
                                })
                                .collect()
                        },
                        ImageFormat::LMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = value.to_ne_bytes();
                                    [value, value, value, 65535_u16.to_ne_bytes()]
                                        .into_iter()
                                        .flatten()
                                })
                                .collect()
                        },
                        ImageFormat::LMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = chunk[0].to_ne_bytes();
                                    [value, value, value, chunk[1].to_ne_bytes()]
                                        .into_iter()
                                        .flatten()
                                })
                                .collect()
                        },
                        ImageFormat::SRGBA => {
                            image_data
                                .iter()
                                .flat_map(|value| f32u16(stl(u16f32(*value))).to_ne_bytes())
                                .collect()
                        },
                        ImageFormat::SRGB => {
                            image_data
                                .chunks_exact(3)
                                .flat_map(|chunk| {
                                    [
                                        f32u16(stl(u16f32(chunk[0]))).to_ne_bytes(),
                                        f32u16(stl(u16f32(chunk[1]))).to_ne_bytes(),
                                        f32u16(stl(u16f32(chunk[2]))).to_ne_bytes(),
                                        65535_u16.to_ne_bytes(),
                                    ]
                                    .into_iter()
                                    .flatten()
                                })
                                .collect()
                        },
                        ImageFormat::SMono => {
                            image_data
                                .iter()
                                .flat_map(|value| {
                                    let value = f32u16(stl(u16f32(*value))).to_ne_bytes();
                                    [value, value, value, 65535_u16.to_ne_bytes()]
                                        .into_iter()
                                        .flatten()
                                })
                                .collect()
                        },
                        ImageFormat::SMonoA => {
                            image_data
                                .chunks_exact(2)
                                .flat_map(|chunk| {
                                    let value = f32u16(stl(u16f32(chunk[0]))).to_ne_bytes();
                                    [
                                        value,
                                        value,
                                        value,
                                        f32u16(stl(u16f32(chunk[1]))).to_ne_bytes(),
                                    ]
                                    .into_iter()
                                    .flatten()
                                })
                                .collect()
                        },
                    }
                },
            }
        },
        _ => unreachable!(),
    }
}

#[inline(always)]
pub(crate) fn u8f32(v: u8) -> f32 {
    v as f32 / u8::MAX as f32
}

#[inline(always)]
pub(crate) fn f32u8(v: f32) -> u8 {
    (v * u8::MAX as f32).clamp(0.0, u8::MAX as f32).trunc() as u8
}

#[inline(always)]
pub(crate) fn u16f32(v: u16) -> f32 {
    v as f32 / u16::MAX as f32
}

#[inline(always)]
pub(crate) fn f32u16(v: f32) -> u16 {
    (v * u16::MAX as f32).clamp(0.0, u16::MAX as f32).trunc() as u16
}

#[inline(always)]
pub(crate) fn lts(v: f32) -> f32 {
    (v.powf(1.0 / 2.4) * 1.005) - 0.055
}

#[inline(always)]
pub(crate) fn stl(v: f32) -> f32 {
    if v < 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
