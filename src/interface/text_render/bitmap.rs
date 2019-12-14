use super::glyph::*;

pub struct GlyphBitmap {
	pub width: u32,
	pub height: u32,
	pub data: Vec<u8>,
}

impl GlyphBitmap {
	pub fn spread_state(
		&self,
		states: &mut Vec<Vec<u16>>,
		state: u16,
		x: usize,
		y: usize)
	-> (bool, Vec<(usize, usize)>) {
	
		let mut boundry_hit = false;
		let mut spread_to = Vec::new();
		
		if states[x][y] == 0 {
			states[x][y] = state;
			
			if x != 0 {
				spread_to.push((x-1, y));
			} else {
				boundry_hit = true;
			}
			
			if x != self.width as usize - 1 {
				spread_to.push((x+1, y));
			} else {
				boundry_hit = true;
			}
			
			if y != 0 {
				spread_to.push((x, y-1));
			} else {
				boundry_hit = true;
			}
			
			if y != self.height as usize - 1 {
				spread_to.push((x, y+1));
			} else {
				boundry_hit = true;
			}
		}
		
		(boundry_hit, spread_to)
	}

	pub fn fill(&mut self) {
		let mut states = Vec::with_capacity(self.width as usize);
		states.resize_with(self.width as usize, || {
			let mut out = Vec::with_capacity(self.height as usize);
			out.resize(self.height as usize, 0_u16);
			out
		});
		
		for x in 0..(self.width as usize) {
			for y in 0..(self.height as usize) {
				if self.data[self.index(x, y)] != 0 {
					states[x][y] = 1;
				}
			}
		}
		
		let mut state_count = 2;
		
		'spreader: loop {
			for x in 0..(self.width as usize) {
				for y in 0..(self.height as usize) {
					if states[x][y] == 0 {
						let cur_state = state_count;
						state_count += 1;
						let mut hit_boundry = false;
						let mut spread_to = Vec::new();
						spread_to.push((x, y));
						
						while let Some((x, y)) = spread_to.pop() {
							let (hit, mut add) = self.spread_state(&mut states, cur_state, x, y);
							
							if hit {
								hit_boundry = true;
							}
							
							spread_to.append(&mut add);
						}
						
						continue 'spreader;
					}
				}
			}
			
			break;
		}
		
		let val_base = (u8::max_value() as f32 / 2.0) / state_count as f32;
		
		for x in 0..(self.width as usize) {
			for y in 0..(self.height as usize) {
				if states[x][y] >= 2 {
					let i = self.index(x, y);
					self.data[i] = ((states[x][y] - 2) as f32 * val_base).floor() as u8 + 64;
				}
			}
		}
		
		println!("regions: {}", state_count - 2);
	}
	
	#[inline]
	fn index(&self, x: usize, y: usize) -> usize {
		(self.width as usize * y) + x
	}
	
	pub fn draw_line(&mut self, glyph: &BasaltGlyph, points: &[f32]) {
		let diff_x = points[2] - points[0];
		let diff_y = points[3] - points[1];
		let steps = (diff_x.powi(2) + diff_y.powi(2)).sqrt().ceil() as usize;
		
		for s in 0..=steps {
			let x = (points[0] + ((diff_x / steps as f32) * s as f32)).floor() - glyph.bounds_min[0] as f32;
			let y = (points[1] + ((diff_y / steps as f32) * s as f32)).ceil() - glyph.bounds_min[1] as f32;
			
			if let Some(v) = self.data.get_mut(((self.width as f32 * (self.height as f32 - y)) + x).trunc() as usize) {
				*v = 255;
			}
		}
	}
	
	pub fn draw_curve(&mut self, glyph: &BasaltGlyph, points: &[f32]) {
		let steps = 10_usize;
		let mut last: Box<[f32; 2]> = Box::new([points[0], points[1]]);
		
		for s in 0..=steps {
			let t = s as f32 / steps as f32;
			let next = Box::new(
				lerp(
					t,
					&lerp(t, &points[0..2], &points[2..4]),
					&lerp(t, &points[2..4], &points[4..6])
				)
			);
			
			self.draw_line(glyph, [*last, *next].concat().as_slice());
			last = next;
		}
	}
}

#[inline]
fn lerp(t: f32, p1: &[f32], p2: &[f32]) -> [f32; 2] {
	[
		p1[0] + ((p2[0] - p1[0]) * t),
		p1[1] + ((p2[1] - p1[1]) * t)
	]
}