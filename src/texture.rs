//use std::fs::File;
use image;
//use image::ImageDecoder;
use std::path::Path;

#[derive(Debug)]
pub struct LoadImageRes {
	pub width: u32,
	pub height: u32,
	pub data: Vec<u8>
}

pub fn load_image<P: AsRef<Path>>(path_: P) -> Result<LoadImageRes, String> {
	let path = path_.as_ref();
	
	if !path.is_file() {
		return Err(format!("Provided path is not a file: {}", path.display()))
	}
	
	let ext = match path.extension() {
		Some(some) => match some.to_str() {
			Some(some) => some,
			None => return Err(format!("Bad extension."))
		}, None => return Err(format!("File doesn't have an extension"))
	}; let convert_to_srgb = match ext {
		"png" => false,
		"jpeg" => true,
		"jpg" => true,
		_ => false
	}; let image = match image::open(path) {
		Ok(ok) => ok.to_rgba(),
		Err(e) => return Err(format!("Failed to open image: {}", e))
	};
	
	Ok(LoadImageRes {
		width: image.width(),
		height: image.height(),
		data: if convert_to_srgb {
			image.into_vec().into_iter().map(|v|
				f32::round(f32::powf(((v as f32 / 255.0) + 0.055) / 1.055, 2.4) * 255.0) as u8
			).collect()
		} else {
			image.into_vec()
		}
	})
}

/*pub fn load_image<P: AsRef<Path>>(path_: P) -> Result<LoadImageRes, String> {
	let path = path_.as_ref();
	
	if !path.is_file() {
		return Err(format!("Provided path is not a file: {}", path.display()))
	}
	
	let ext = match path.extension() {
		Some(some) => match some.to_str() {
			Some(some) => some,
			None => return Err(format!("Bad extension."))
		}, None => return Err(format!("File doesn't have an extension"))
	}; let handle = match File::open(&path) {
		Ok(ok) => ok,
		Err(e) => return Err(format!("Failed to open file: {}", e))
	}; let mut decoder = match ext {
		"png" => image::png::PNGDecoder::new(handle),
		_ => return Err(format!("Unsupported format"))
	}; let color_ty = match decoder.colortype() {
		Ok(ok) => ok,
		Err(e) => return Err(format!("Unable to get color type: {}", e))
	}; let texel_size = match color_ty {
		image::ColorType::Gray(_) => 1,
		image::ColorType::RGB(_) => 3,
		image::ColorType::GrayA(_) => 2,
		image::ColorType::RGBA(_) => 4,
		_ => return Err(format!("Unsupported color type."))
	}; let width = match decoder.row_len() {
		Ok(ok) => ok / texel_size,
		Err(e) => return Err(format!("Failed to get row len: {}", e))
	}; let raw_data = match decoder.read_image() {
		Ok(ok) => match ok {
			image::DecodingResult::U8(data) => data,
			_ => return Err(format!("Unsupported color data."))
		}, Err(e) => return Err(format!("Failed to read image: {}", e))
	};
	
	let height = raw_data.len() / (width * texel_size);
	let mut data: Vec<u8> = Vec::with_capacity(width * height * 4);
	let mut byte_i = 0_usize;
	
	for byte in raw_data {
		match color_ty {
			image::ColorType::Gray(_) => {
				data.push(byte);
				data.push(byte);
				data.push(byte);
				data.push(255);
			}, image::ColorType::RGB(_) => {
				data.push(byte);
				byte_i += 1;
				
				if byte_i >= texel_size {
					byte_i = 0;
					data.push(255);
				}
			}, image::ColorType::GrayA(_) => {
				if byte_i == 0 {
					data.push(byte);
					data.push(byte);
					data.push(byte);
					byte_i = 1;
				} else if byte_i == 1 {
					data.push(byte);
					byte_i = 0;
				}
			}, image::ColorType::RGBA(_) => data.push(byte),
			_ => return Err(format!("Unsupported color type."))
		}
	}

	Ok(LoadImageRes {
		width: width as u32,
		height: height as u32,
		data: data
	})
}*/

/*
use std::fs::read_dir;
use vulkano::device;
use vulkano::image::immutable::ImmutableImage;
use vulkano::image::traits::ImageViewAccess;
use std::sync::Arc;
use vulkano;

pub fn load_2d<P: AsRef<Path>>(path: P, queue: Arc<device::Queue>) -> Result<Arc<ImageViewAccess + Send + Sync>, String> {
	match load_image(path) {
		Ok(res) => match ImmutableImage::from_iter(
			res.data.into_iter(),
			vulkano::image::Dimensions::Dim2d {
				width: res.width,
				height: res.height,
			}, vulkano::format::Format::R8G8B8A8Srgb,
			queue.clone()
		) {
			Ok(ok) => Ok(ok.0 as Arc<ImageViewAccess + Send + Sync>),
			Err(e) => Err(format!("Failed to create immutable image: {}", e))
		}, Err(e) => Err(e)
	}
}

pub fn load_2d_array_from_dir<P: AsRef<Path>>(path_: P, queue: Arc<device::Queue>) -> Result<Arc<ImageViewAccess + Send + Sync>, String> {
	let dir_path = path_.as_ref();
	let mut paths = Vec::new();
	
	for entry_ in read_dir(dir_path).unwrap() {
		let entry = entry_.unwrap();
		let path_buf = entry.path();
		
		if path_buf.as_path().is_file() {
			paths.push(path_buf);
		}
	}
	
	paths.sort();
	load_2d_array(paths, queue)
}

pub fn load_2d_array<P: AsRef<Path>>(paths: Vec<P>, queue: Arc<device::Queue>) -> Result<Arc<ImageViewAccess + Send + Sync>, String> {
	let mut results = Vec::new();

	for path in paths {
		results.push(match load_image(&path) {
			Ok(ok) => ok,
			Err(e) => return Err(format!("Failed to load '{}': {}", path.as_ref().display(), e))
		});
	}
	
	let num_layers = results.len() as u32;
	let mut max_width = 0;
	let mut max_height = 0;
	
	for result in &results {
		if result.width > max_width {
			max_width = result.width;
		} if result.height > max_height {
			max_height = result.height;
		}
	}
	
	let mut data = Vec::with_capacity((num_layers * max_width * max_height) as usize);
	
	for mut result in results {
		if result.width != max_width || result.height != max_height {
			for x in 0..max_width {
				for y in 0..max_height {
					if x >= result.width || y >= result.width {
						for _ in 0..4 {
							data.push(0);
						}
					} else {
						for i in 0..4 {
							data.push(result.data[((y*result.width*4)+(x*4)+i) as usize]);
						}
					}
				}
			}
		} else {
			data.append(&mut result.data);
		}
	}
	
	match ImmutableImage::from_iter(
		data.into_iter(),
		vulkano::image::Dimensions::Dim2dArray {
			width: max_width,
			height: max_height,
			array_layers: num_layers,
		}, vulkano::format::Format::R8G8B8A8Srgb,
		queue.clone()
	) {
		Ok(ok) => Ok(ok.0 as Arc<ImageViewAccess + Send + Sync>),
		Err(e) => Err(format!("Failed to create immutable image: {}", e))
	}
}*/

