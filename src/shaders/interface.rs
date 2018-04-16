#[allow(dead_code)]
pub mod interface_vs {
	#[derive(VulkanoShader)]
	#[ty = "vertex"]
	#[src = "
		#version 450
		layout(location = 0) in vec3 position;
		layout(location = 1) in vec2 coords;
		layout(location = 2) in vec4 color;
		layout(location = 3) in int ty;
		
		layout(location = 0) out vec2 out_coords;
		layout(location = 1) out vec4 out_color;
		layout(location = 2) out int out_type;
		
		void main() {
			out_coords = coords;
			out_color = color;
			out_type = ty;
			gl_Position = vec4(position, 1);
		}
	"]
	struct Dummy;
}

#[allow(dead_code)]
pub mod interface_fs {
	#[derive(VulkanoShader)]
	#[ty = "fragment"]
	#[src = "
	#version 450
	
	layout(location = 0) in vec2 coords;
	layout(location = 1) in vec4 color;
	layout(location = 2) in flat int type;
	
	layout(location = 0) out vec4 out_color;
	
	layout(set = 0, binding = 0) uniform sampler2D tex;
	
	void main() {
		if(type == 0) { // Verts with Color
			out_color = color;
		} else if(type == 1) { // Verts with Texture mixed with Color
			out_color = vec4(color.rgb, texture(tex, coords).a * color.a);
		} else if(type == 2) { // Verts with Texture
			vec4 tex_color = texture(tex, coords);
			out_color = vec4(mix(tex_color.rgb, color.rgb, color.a), tex_color.a);
		} else if(type == 3) { // YUV & SRGB
			vec4 full_res_tex = texture(tex, coords);
			vec4 half_res_tex = texture(tex, coords/2);
			vec4 tex_color = vec4(full_res_tex.r, half_res_tex.g, half_res_tex.b, full_res_tex.a);
			
			float r = tex_color.r + (1.402 * (tex_color.b - 0.5));
			float g = tex_color.r - (0.344 * (tex_color.g - 0.5)) - (0.714 * (tex_color.b - 0.5));
			float b = tex_color.r + (1.772 * (tex_color.g - 0.5));
			r = pow((r + 0.055) / 1.055, 2.4);
			g = pow((g + 0.055) / 1.055, 2.4);
			b = pow((b + 0.055) / 1.055, 2.4);
			
			out_color = vec4(r, g, b, tex_color.a);
		} else { // Unknown
			out_color = color;
		}
	}
	"]
	struct Dummy;
}

