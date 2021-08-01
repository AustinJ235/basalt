pub mod layer_fs {
	shader! {
		ty: "fragment",
		vulkan_version: "1.1",
		spirv_version: "1.5",
		src: "
	#version 450

	layout(location = 0) in vec2 coords;
	layout(location = 1) in vec4 color;
	layout(location = 2) in flat int type;

	layout(location = 0) out vec3 out_color;
    layout(location = 1) out vec3 out_alpha;

    layout(input_attachment_index = 0, set = 0, binding = 0) uniform subpassInput prev_color;
    layout(input_attachment_index = 1, set = 0, binding = 1) uniform subpassInput prev_alpha;
	layout(set = 0, binding = 2) uniform sampler2D tex_linear;
	layout(set = 0, binding = 3) uniform sampler2D tex_nearest;

	void main() {
		if(type == 0) { // Verts with Color

		} else if(type == 1) { // Verts with Texture mixed with Color

		} else if(type == 2) { // Text Glyph

		} else if(type >= 100 && type <= 199) {
			if(type == 101) { // YUV Image
				
			} else if(type == 102) { // BackColorAdd
				
			} else if(type == 103) { // BackColorBehind
				
			} else if(type == 104) { // BackColorSubtract

			} else if(type == 105) { // BackColorMultiply

			} else if(type == 106) { // BackColorDivide

			} else if(type == 107) { // Invert

			} else { // Normal Image / Unknown

			}
		} else { // Unknown
			
		}
	}
	"
	}
}
