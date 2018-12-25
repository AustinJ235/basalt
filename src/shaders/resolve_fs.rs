pub mod resolve_fs {
	shader!{
		ty: "fragment",
		src: "
	#version 450

	layout(location = 0) in vec2 in_coords;
	layout(location = 0) out vec4 out_color;
	layout(set = 0, binding = 0) uniform sampler2D color_tex;

	void main() {
		out_color = texture(color_tex, in_coords);
	}
	"
	}
}

