pub mod layer_vs {
	shader! {
		ty: "vertex",
		src: "
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
	"
	}
}
