pub mod final_fs {
	shader! {
		ty: "fragment",
		src: "
            #version 450

            layout(location = 0) in vec2 in_coords;
            layout(location = 0) out vec4 out_color;

            layout(input_attachment_index = 0, set = 0, binding = 0) uniform subpassInput prev_color;
            layout(input_attachment_index = 1, set = 0, binding = 1) uniform subpassInput prev_alpha;

            void main() {
                vec3 color = subpassLoad(prev_color).rgb;
                vec3 alpha = subpassLoad(prev_alpha).rgb;
                out_color = vec4(color, 1.0);
            }
    "
	}
}
