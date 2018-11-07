use harfbuzz_sys::*;
use freetype::freetype::*;
use interface::interface::ItfVertInfo;
use std::ptr;
use std::ffi::CString;
use std::sync::Arc;
use Engine;
use atlas;
use interface::text::WrapTy;
use std::collections::BTreeMap;

pub(crate) fn render_text<T: AsRef<str>, F: AsRef<str>>(engine: &Arc<Engine>, text: T, _family: F, size: f32, color: (f32, f32, f32, f32), _wrap_ty: WrapTy) -> Result<BTreeMap<usize, Vec<ItfVertInfo>>, String> {
	unsafe {
		let size = (size).ceil() as u32;
		
		let mut ft_library = ptr::null_mut();

		match FT_Init_FreeType(&mut ft_library) {
			0 => (),
			e => return Err(format!("FT_Init_FreeType: error {}", e))
		}
		
		let mut ft_face = ptr::null_mut();
		let bytes = include_bytes!("/usr/share/fonts/TTF/Amiri-Regular.ttf");
		
		match FT_New_Memory_Face(ft_library, bytes.as_ptr(), (bytes.len() as i32).into(), 0, &mut ft_face) {
			0 => (),
			e => return Err(format!("FT_New_Memory_Face: error {}", e))
		}
		
		match FT_Set_Pixel_Sizes(ft_face, 0, size.into()) {
			0 => (),
			e => return Err(format!("FT_Set_Pixel_Sizes: error {}", e))
		}
		
		let hb_font = hb_ft_font_create_referenced(ft_face);
		let hb_buffer = hb_buffer_create();
		let ctext = CString::new(text.as_ref()).unwrap();
		
		hb_buffer_add_utf8(hb_buffer, ctext.as_ptr(), -1, 0, -1);
		hb_buffer_guess_segment_properties (hb_buffer);
		hb_shape(hb_font, hb_buffer, ptr::null_mut(), 0);
		
		let len = hb_buffer_get_length(hb_buffer) as usize;
		let info = Vec::from_raw_parts(hb_buffer_get_glyph_infos(hb_buffer, ptr::null_mut()), len, len);
		let pos = Vec::from_raw_parts(hb_buffer_get_glyph_positions(hb_buffer, ptr::null_mut()), len, len);
		let max_ht = (*ft_face).height as f32 / 96.0 + (size as f32 / 2.0);
		
		let mut current_x = 0.0;
		let mut current_y = 0.0;
		let mut vert_map = BTreeMap::new();
		
		for i in 0..len {
			match FT_Load_Glyph(ft_face, info[i].codepoint.into(), FT_LOAD_DEFAULT as i32) {
				0 => (),
				e => return Err(format!("FT_Load_Glyph: error {}", e))
			}
			
			match FT_Render_Glyph((*ft_face).glyph, FT_Render_Mode::FT_RENDER_MODE_NORMAL) {
				0 => (),
				e => return Err(format!("FT_Render_Glyph: error {}", e))
			}
			
			let glyph = *(*ft_face).glyph;
			let bitmap = glyph.bitmap;
			let w = bitmap.width as usize;
			let h = bitmap.rows as usize;
			let mut image_data = Vec::with_capacity(w * h * 4);
			
			for i in 0..((w*h) as isize) {
				image_data.push(0);
				image_data.push(0);
				image_data.push(0);
				image_data.push(*bitmap.buffer.offset(i));
			}
			
			let coords = match engine.atlas_ref().load_raw_with_key(
				&atlas::ImageKey::Glyph(size, info[i].codepoint as u64),
				image_data, w as u32, h as u32
			) {
				Ok(ok) => ok,
				Err(e) => return Err(format!("Atlas::load_raw_with_key: Error {}", e))
			};
			
			let tl = (
				current_x + (pos[i].x_offset as f32 / 64.0) - (glyph.metrics.horiBearingX as f32 / 64.0),
				current_y + (pos[i].y_offset as f32 / 64.0) + (max_ht / 2.0) - (glyph.metrics.horiBearingY as f32 / 64.0),
				0.0
			);
			
			let tr = (tl.0 + (glyph.metrics.width as f32 / 64.0), tl.1, 0.0);
			let bl = (tl.0, tl.1 + (glyph.metrics.height as f32 / 64.0), 0.0);
			let br = (tr.0, bl.1, 0.0);
			
			let ctl = coords.f32_top_left();
			let ctr = coords.f32_top_right();
			let cbl = coords.f32_bottom_left();
			let cbr = coords.f32_bottom_right();
			
			let verts = vert_map.entry(coords.atlas_i).or_insert(Vec::new());
			verts.push(ItfVertInfo { position: tr, coords: ctr, color: color, ty: 1 });
			verts.push(ItfVertInfo { position: tl, coords: ctl, color: color, ty: 1 });
			verts.push(ItfVertInfo { position: bl, coords: cbl, color: color, ty: 1 });
			verts.push(ItfVertInfo { position: tr, coords: ctr, color: color, ty: 1 });
			verts.push(ItfVertInfo { position: bl, coords: cbl, color: color, ty: 1 });
			verts.push(ItfVertInfo { position: br, coords: cbr, color: color, ty: 1 });
			
			current_x += pos[i].x_advance as f32 / 64.0;
			current_y += pos[i].y_advance as f32 / 64.0;
		}
		
		Ok(vert_map)
	}
}
