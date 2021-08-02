pub mod layer_fs {
	shader! {
		ty: "fragment",
		vulkan_version: "1.1",
		spirv_version: "1.5",
		src: "
	#version 450

	layout(constant_id = 0) const uint layer_i = 0;

	layout(location = 0) in vec2 coords;
	layout(location = 1) in vec4 color;
	layout(location = 2) in flat int type;

	layout(location = 0) out vec4 out_c;
    layout(location = 1) out vec4 out_a;

    layout(input_attachment_index = 0, set = 0, binding = 0) uniform subpassInput prev_c;
    layout(input_attachment_index = 1, set = 0, binding = 1) uniform subpassInput prev_a;

	layout(set = 0, binding = 2) uniform sampler2D tex_linear;
	layout(set = 0, binding = 3) uniform sampler2D tex_nearest;

	void main() {
		// out_c = src_c + ((vec3(1.0) - src_a) * dst_c);
        // out_a = src_a + ((vec3(1.0) - src_a) * dst_a);

		vec4 base_c;
		vec4 base_a;

		// The first layer the prev color/alpha will be garbage
		if(layer_i == 0) {
			base_c = vec4(0.0, 0.0, 0.0, 1.0);
			base_a = vec4(1.0, 1.0, 1.0, 1.0);
		} else {
			base_c = subpassLoad(prev_c);
			base_a = subpassLoad(prev_a);
		}

		// First vertexes in layer will be of type -1, this used to write to all fragments.
		if(type == -1) {
			out_c = base_c;
			out_a = base_a;
		}
		
		// Blended with Color
		else if(type == 0) { 
			out_c = vec4(color.rgb, 1.0) + (vec4(1.0 - color.a) * base_c);
			out_a = vec4(vec3(color.a), 1.0) + (vec4(1.0 - color.a) * base_a);
		}
		
		// Texture mixed with Color
		else if(type == 1) { 
			// Not Implemented Yet!
			out_c = base_c;
			out_a = base_a;
		}
		
		// Glyph
		else if(type == 2) { 
			// https://github.com/servo/webrender/blob/master/webrender/doc/text-rendering.md#component-alpha
			vec4 mask = vec4(texture(tex_nearest, coords).rgb, 1.0);
			out_c.r = color.r * mask.r + (1.0 - color.a * mask.r) * base_c.r;
			out_c.g = color.g * mask.g + (1.0 - color.a * mask.g) * base_c.g;
			out_c.b = color.b * mask.b + (1.0 - color.a * mask.b) * base_c.b;
			out_a.r = color.a * mask.r + (1.0 - color.a * mask.r) * base_a.r;
			out_a.g = color.a * mask.g + (1.0 - color.a * mask.g) * base_a.g;
			out_a.b = color.a * mask.b + (1.0 - color.a * mask.b) * base_a.b;
		}
		
		// Image Filters/Effects
		else if(type >= 100 && type <= 199) {

			// YUV
			if(type == 101) {
				// Not Implemented Yet!
				out_c = base_c;
				out_a = base_a;
			}
			
			// BackColorAdd
			else if(type == 102) { 
				// Not Implemented Yet!
				out_c = base_c;
				out_a = base_a;
			}
			
			// BackColorBehind
			else if(type == 103) {
				// Not Implemented Yet!
				out_c = base_c;
				out_a = base_a;
			}
			
			// BackColorSubtract
			else if(type == 104) { 
				// Not Implemented Yet!
				out_c = base_c;
				out_a = base_a;
			}
			
			// BackColorMultiply
			else if(type == 105) {
				// Not Implemented Yet!
				out_c = base_c;
				out_a = base_a;
			}
			
			// BackColorDivide
			else if(type == 106) { 
				// Not Implemented Yet!
				out_c = base_c;
				out_a = base_a;
			}
			
			// Invert
			else if(type == 107) { 
				// Not Implemented Yet!
				out_c = base_c;
				out_a = base_a;
			}
			
			// Unknown - Do Nothing
			else {
				out_c = base_c;
				out_a = base_a;
			}
		}
		
		// Unknown - Do Nothing
		else {
			out_c = base_c;
			out_a = base_a;
		}
	}
	"
	}
}
