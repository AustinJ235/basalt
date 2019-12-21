pub mod glyph_base_vs {
	shader!{
		ty: "vertex",
		src: "
			#version 450
			
			layout(location = 0) in vec2 position;

			void main() {
				gl_Position = vec4(position, 0, 1);
			}
		"
	}
}

