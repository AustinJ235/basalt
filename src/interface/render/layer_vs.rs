pub mod layer_vs {
	shader! {
		ty: "vertex",
		vulkan_version: "1.2",
		spirv_version: "1.5",
		src: "
	#version 450

	layout(location = 0) in vec3 position;
	layout(location = 1) in vec2 coords;
	layout(location = 2) in vec4 color;
	layout(location = 3) in int ty;
	layout(location = 4) in uint tex_i;

	layout(location = 0) out vec2 out_coords;
	layout(location = 1) out vec4 out_color;
	layout(location = 2) out flat int out_type;
	layout(location = 3) out vec2 out_position;
	layout(location = 4) out flat uint out_tex_i;

	void main() {
		out_coords = coords;
		out_color = color;
		out_type = ty;
		out_position = position.xy;
		out_tex_i = tex_i;
		gl_Position = vec4(position, 1);
	}
	"
	}
}
