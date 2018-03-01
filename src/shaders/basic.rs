#[allow(dead_code)]
pub mod vs {
	#[derive(VulkanoShader)]
	#[ty = "vertex"]
	#[src = "
		#version 450
		layout(location = 0) in vec3 position;
		layout(location = 1) in vec3 normal;
		layout(location = 2) in vec4 color;
		layout(location = 3) in vec4 tex_info;
		layout(location = 4) in int ty;
		
		layout(location = 0) out vec3 out_norm;
		layout(location = 1) out vec4 out_color;
		layout(location = 2) out vec4 out_tex_info;
		layout(location = 3) out int out_type;
		
		layout(set = 0, binding = 0) uniform Data {
			mat4 view;
			mat4 proj;
		} uniforms;
		
		layout(set = 0, binding = 1) uniform Other {
			mat4 model;
		} other;
		
		void main() {
			mat4 worldview = uniforms.view * other.model;
			out_norm =  normal;
			out_color = color;
			out_type = ty;
			out_tex_info = tex_info;
			gl_Position = uniforms.proj * worldview * vec4(position, 1.0);
		}
	"]
	struct Dummy;
}

#[allow(dead_code)]
pub mod fs {
	#[derive(VulkanoShader)]
	#[ty = "fragment"]
	#[src = "
	#version 450
	
	layout(location = 0) in vec3 in_norm;
	layout(location = 1) in vec4 in_color;
	layout(location = 2) in flat vec4 tex_info;
	layout(location = 3) in flat int in_type;
	
	layout(location = 0) out vec4 out_color;
	layout(location = 1) out vec4 out_normal;
	
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
		
		out_normal = vec4(normalize(in_norm), 1.0);
	}
	"]
	struct Dummy;
}

