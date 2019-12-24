pub mod glyph_base_fs {
	shader!{
		ty: "fragment",
		src: "
			#version 450
			
			layout(location = 0) out float color;
			layout(location = 0) in vec2 in_coords;
			
			layout(set = 0, binding = 0) uniform LineData {
				vec4 lines[1024];
				uint count;
				float width;
				float height;
			} line_data;
			
			layout(set = 0, binding = 1) uniform SampleData {
				vec4 offsets[16];
				uint samples;
			} sample_data;
			
			layout(set = 0, binding = 2) uniform RayData {
				vec4 dir[4];
				uint count;
			} ray_data;
			
			int ccw(vec2 p0, vec2 p1, vec2 p2) {
				float dx1 = p1.x - p0.x;
				float dy1 = p1.y - p0.y;
				float dx2 = p2.x - p0.x;
				float dy2 = p2.y - p0.y;
				
				if(dx1 * dy2 > dy1 * dx2) {
					return +1;
				}
				
				if(dx1 * dy2 < dy1 * dx2) {
					return -1;
				}
				
				if(dx1 * dx2 < 0 || dy1 * dy2 < 0) {
					return -1;
				}
				
				if((dx1 * dx1) + (dy1 * dy1) < (dx2 * dx2) + (dy2 * dy2)) {
					return +1;
				}
				
				return 0;
			}
			
			bool intersect(vec2 l1p1, vec2 l1p2, vec2 l2p1, vec2 l2p2) {
				return ccw(l1p1, l1p2, l2p1) * ccw(l1p1, l1p2, l2p2) <= 0
						&& ccw(l2p1, l2p2, l1p1) * ccw(l2p1, l2p2, l1p2) <= 0;
			}
			
			bool is_filled(vec2 ray_src) {
				float ray_len = length(vec2(line_data.width, line_data.height));
				int least_hits = -1;
				
				for(uint ray_dir_i = 0; ray_dir_i < ray_data.count; ray_dir_i++) {
					vec2 ray_dest = ray_src + (ray_data.dir[ray_dir_i].xy * ray_len);
					int hits = 0;
					
					for(uint line_i = 0; line_i < line_data.count; line_i ++) {
						if(intersect(ray_src, ray_dest, line_data.lines[line_i].xy, line_data.lines[line_i].zw)) {
							hits++;
						}
					}
					
					if(least_hits == -1 || hits < least_hits) {
						least_hits = hits;
					}
				}
				
				return least_hits % 2 != 0;
			}

			void main() {
				vec2 ray_src = in_coords * vec2(line_data.width, line_data.height);
				uint filled = 0;
				
				for(uint i = 0; i < sample_data.samples; i++) {
					if(is_filled(ray_src + sample_data.offsets[i].xy)) {
						filled++;
					}
				}
				
				
				color = float(filled) / float(sample_data.samples);
			}
		"
	}
}
