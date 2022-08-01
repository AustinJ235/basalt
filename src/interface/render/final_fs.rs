// Fixed in https://github.com/vulkano-rs/vulkano/pull/1940
#![allow(clippy::needless_question_mark)]

shader! {
	ty: "fragment",
	vulkan_version: "1.2",
	spirv_version: "1.5",
	src: "
        #version 450

        layout(location = 0) in vec2 in_coords;
        layout(location = 0) out vec4 out_color;

        layout(set = 0, binding = 0) uniform sampler2D prev_color;
        layout(set = 0, binding = 1) uniform sampler2D prev_alpha;

        void main() {
            vec3 color = texture(prev_color, in_coords).rgb;
            vec3 alpha = texture(prev_alpha, in_coords).rgb;
            out_color = vec4(color, alpha.g);
        }
"
}
