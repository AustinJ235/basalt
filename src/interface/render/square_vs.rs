pub mod square_vs {
	shader! {
		ty: "vertex",
		src: "
	#version 450
	layout(location = 0) in vec2 position;
	layout(location = 0) out vec2 out_coords;

	void main() {
		out_coords = vec2(position.x / 2.0, position.y / 2.0) + vec2(0.5);
		gl_Position = vec4(position, 0, 1);
	}
	"
	}
}
