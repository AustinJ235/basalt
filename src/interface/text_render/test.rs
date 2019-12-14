use super::*;

#[test]
fn render_text() {
	use crate::interface::bin::{self,BinStyle,PositionTy};
	
	const FONT_SIZE: f32 = 48.0;
	
	let basalt = crate::Basalt::new(
		crate::Options::default()
			.ignore_dpi(true)
			.window_size(1000, 100)
			.title("Basalt")
	).unwrap();
	basalt.spawn_app_loop();
	
	let glyphs = shape_text("Hello World!", tag::from_string("DFLT").unwrap(), tag::from_string("dflt").unwrap()).unwrap();
	let mut glyph_bins = basalt.interface_ref().new_bins(glyphs.len() + 1);
	let background = glyph_bins.pop().unwrap();
	
	background.style_update(BinStyle {
		position_t: Some(PositionTy::FromWindow),
		pos_from_t: Some(26.0),
		pos_from_b: Some(10.0),
		pos_from_l: Some(50.0),
		pos_from_r: Some(10.0),
		text: String::from("."),
		overflow_y: Some(true),
		.. background.style_copy()
	});
	
	for (i, glyph) in glyphs.iter().enumerate() {
		let mut bitmap = glyph.bitmap().unwrap();
		
		if bitmap.width == 0 || bitmap.height == 0 {
			continue;
		}
		
		bitmap.fill();
	
		let image = crate::atlas::Image::new(
			crate::atlas::ImageType::SMono,
			crate::atlas::ImageDims {
				w: bitmap.width, 
				h: bitmap.height,
			},
			crate::atlas::ImageData::D8(bitmap.data)
		).unwrap();
		
		let coords = basalt.atlas_ref().load_image(crate::atlas::SubImageCacheID::None, image).unwrap();
		let scale = glyph.units_per_pixel / FONT_SIZE;
		
		dbg!(bitmap.height as f32 / scale);
		
		glyph_bins[i].style_update(BinStyle {
			position_t: Some(PositionTy::FromParent),
			pos_from_t: Some((glyph.y as f32 / scale) + 30.0),
			pos_from_l: Some(glyph.x as f32 / scale),
			width: Some(bitmap.width as f32 / scale),
			height: Some(bitmap.height as f32 / scale),
			back_image_atlas: Some(coords),
			.. glyph_bins[i].style_copy()
		});
		
		background.add_child(glyph_bins[i].clone());
	}
	
	basalt.wait_for_exit().unwrap();
}
