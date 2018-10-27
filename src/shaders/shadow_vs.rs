vulkano_shader!{
	mod_name: shadow_vs,
	ty: "vertex",
	src: "
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

void main() {
	gl_Position = uniforms.vp * other.model * vec4(position, 1.0);
}
"
}

