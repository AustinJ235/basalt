#[allow(dead_code)]
pub mod water_vs {
	#[derive(VulkanoShader)]
	#[ty = "vertex"]
	#[src = "
		#version 450
		layout(location = 0) in vec3 position;
		layout(location = 1) in vec3 normal;
		layout(location = 2) in vec4 color;
		layout(location = 3) in vec4 tex_info;
		layout(location = 4) in int ty;
		
		layout(set = 0, binding = 0) uniform Data {
			mat4 vp;
		} uniforms;
		
		layout(set = 0, binding = 1) uniform Other {
			mat4 model;
		} other;
		
		layout(location = 0) out vec4 out_color;
		layout(location = 1) out vec4 out_tex_info;
		layout(location = 2) out int out_type;
		
		void main() {
			out_color = color;
			out_type = ty;
			out_tex_info = tex_info;
			gl_Position = uniforms.vp * other.model * vec4(position, 1.0);
		}
	"]
	struct Dummy;
}

#[allow(dead_code)]
pub mod water_fs {
	#[derive(VulkanoShader)]
	#[ty = "fragment"]
	#[src = "
	#version 450
	
	layout(location = 0) in vec4 in_color;
	layout(location = 1) in flat vec4 tex_info;
	layout(location = 2) in flat int in_type;
	
	layout(location = 0) out vec4 out_color;
	
	layout(set = 0, binding = 2) uniform sampler2D terrain_tex;
	
	void main() {
		if(in_type == 0) {
			out_color = in_color;
		} else {
			vec2 coords = vec2(
				(mod(in_color.x, 1.0) * tex_info.z) + tex_info.x,
				(mod(in_color.y, 1.0) * tex_info.w) + tex_info.y
			);
			
			out_color = texture(terrain_tex, coords);
		}
	}
	"]
	struct Dummy;
}

