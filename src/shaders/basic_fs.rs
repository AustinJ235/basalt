vulkano_shader!{
	mod_name: fs,
	ty: "fragment",
	src: "
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
}"
}
