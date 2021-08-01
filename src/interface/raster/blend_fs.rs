pub mod blend_fs {
	shader! {
		ty: "fragment",
		src: "
            #version 450

            layout(location = 0) in vec2 in_coords;

            layout(location = 0) out vec3 out_color;
            layout(location = 1) out vec3 out_alpha;

            layout(input_attachment_index = 0, set = 0, binding = 0) uniform subpassInput src_color;
            layout(input_attachment_index = 1, set = 0, binding = 1) uniform subpassInput src_alpha;
            layout(input_attachment_index = 2, set = 0, binding = 2) uniform subpassInput prev_color;
            layout(input_attachment_index = 3, set = 0, binding = 3) uniform subpassInput prev_alpha;

            void main() {
                vec3 src_color = subpassLoad(src_color).rgb;
                vec3 src_alpha = subpassLoad(src_alpha).rgb;
                vec3 prev_color = subpassLoad(prev_color).rgb;
                vec3 prev_alpha = subpassLoad(prev_alpha).rgb;

                out_color = src_color;
                out_alpha = src_alpha;
            }
    "
	}
}
