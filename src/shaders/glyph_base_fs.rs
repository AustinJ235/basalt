pub mod glyph_base_fs {
	shader!{
		ty: "fragment",
		src: "
			#version 450
			
			layout(location = 0) out float color;
			layout(location = 0) in vec2 in_coords;
			
			layout(set = 0, binding = 0) uniform LineData {
				vec4 lines[256];
				vec4 ray_dirs[8];
				uint count;
				float width;
				float height;
			} line_data;
			
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
			
			float dist_to_line(vec2 pt1, vec2 pt2, vec2 testPt) {
				vec2 lineDir = pt2 - pt1;
				vec2 perpDir = vec2(lineDir.y, -lineDir.x);
				vec2 dirToPt1 = pt1 - testPt;
				return abs(dot(normalize(perpDir), dirToPt1));
			}
			
			float aastep(float threshold, float value) {
				float afwidth = length(vec2(dFdx(value), dFdy(value))) * 0.70710678118654757;
				return smoothstep(threshold-afwidth, threshold+afwidth, value);
			}

			void main() {
				vec2 ray_src = in_coords * vec2(line_data.width, line_data.height);
				float ray_len = length(vec2(line_data.width, line_data.height));
				int least_hits = -1;
				float color_val = 1.0;
				
				for(uint ray_dir_i = 0; ray_dir_i < 8; ray_dir_i++) {
					vec2 ray_dest = ray_src + (line_data.ray_dirs[ray_dir_i].xy * ray_len);
					int hits = 0;
					float ray_color_val = 0;
					
					for(uint line_i = 0; line_i < line_data.count; line_i ++) {
						if(intersect(ray_src, ray_dest, line_data.lines[line_i].xy, line_data.lines[line_i].zw)) {
							float d = dist_to_line(line_data.lines[line_i].xy, line_data.lines[line_i].zw, ray_src);
							ray_color_val += clamp(d, 0.0, 1.0);
							hits++;
						}
					}
					
					if(least_hits == -1 || hits < least_hits) {
						least_hits = hits;
					}
					
					color_val = ray_color_val / float(hits);
				}
				
				if(least_hits % 2 == 0) {
					color = 0.0;
				} else {
					color = color_val;
				}
			}
		"
	}
}
