pub mod glyph_post_fs {
	shader!{
		ty: "fragment",
		src: "
			#version 450
			
			layout(location = 0) in vec2 in_coords;
			layout(location = 0) out float color;
			layout(set = 0, binding = 0) uniform sampler2D tex;

			void main() {
				color = texture(tex, in_coords).r;
			}
		"
	}
}
