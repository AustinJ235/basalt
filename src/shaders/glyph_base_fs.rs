pub mod glyph_base_fs {
	shader!{
		ty: "fragment",
		src: "
			#version 450
			
			layout(location = 0) out vec4 color;

			void main() {
				color = vec4(1.0);
			}
		"
	}
}
