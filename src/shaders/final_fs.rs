pub mod final_fs {
	shader!{
		ty: "fragment",
		src: "
	#version 450

	layout(location = 0) in vec2 in_coords;
	layout(location = 0) out vec4 out_color;

	layout(set = 0, binding = 0) uniform sampler2D deferred_tex;
	layout(set = 0, binding = 1) uniform sampler2D interface_tex;

	void main() {
		vec4 color = texture(deferred_tex, in_coords);
		vec4 itf = texture(interface_tex, in_coords);
		out_color = vec4(mix(color.rgb, itf.rgb, itf.a), 1);
	}
	"
	}
}

