// Fixed in https://github.com/vulkano-rs/vulkano/pull/1940
#![allow(clippy::needless_question_mark)]

shader! {
    ty: "fragment",
    vulkan_version: "1.2",
    spirv_version: "1.5",
    src: "
		#version 450

		#extension GL_EXT_nonuniform_qualifier : enable

		layout(location = 0) in vec2 coords;
		layout(location = 1) in vec4 color;
		layout(location = 2) in flat int type;
		layout(location = 3) in vec2 position;
		layout(location = 4) in flat uint tex_i;

		layout(location = 0) out vec4 out_c;
		layout(location = 1) out vec4 out_a;

		layout(set = 0, binding = 0) uniform sampler2D prev_c;
		layout(set = 0, binding = 1) uniform sampler2D prev_a;
		layout(set = 0, binding = 2) uniform sampler2D tex_nearest[];

		const float epsilon = 0.0001;
		const float oneminus_epsilon = 1.0 - epsilon;

		vec4 cubic(float v) {
			vec4 n = vec4(1.0, 2.0, 3.0, 4.0) - v;
			vec4 s = n * n * n;
			float x = s.x;
			float y = s.y - 4.0 * s.x;
			float z = s.z - 4.0 * s.y + 6.0 * s.x;
			float w = 6.0 - x - y - z;
			return vec4(x, y, z, w) * (1.0/6.0);
		}

		vec4 textureBicubic(vec2 texCoords) {
			vec2 texSize = textureSize(nonuniformEXT(tex_nearest[tex_i]), 0);
			vec2 invTexSize = 1.0 / texSize;
			texCoords = texCoords * texSize - 0.5;
			vec2 fxy = fract(texCoords);
			texCoords -= fxy;
			vec4 xcubic = cubic(fxy.x);
			vec4 ycubic = cubic(fxy.y);
			vec4 c = texCoords.xxyy + vec2 (-0.5, +1.5).xyxy;
			vec4 s = vec4(xcubic.xz + xcubic.yw, ycubic.xz + ycubic.yw);
			vec4 offset = c + vec4 (xcubic.yw, ycubic.yw) / s;
			offset *= invTexSize.xxyy;
			vec4 sample0 = textureLod(nonuniformEXT(tex_nearest[tex_i]), offset.xz, 0);
			vec4 sample1 = textureLod(nonuniformEXT(tex_nearest[tex_i]), offset.yz, 0);
			vec4 sample2 = textureLod(nonuniformEXT(tex_nearest[tex_i]), offset.xw, 0);
			vec4 sample3 = textureLod(nonuniformEXT(tex_nearest[tex_i]), offset.yw, 0);
			float sx = s.x / (s.x + s.y);
			float sy = s.z / (s.z + s.w);
			return mix(mix(sample3, sample2, sx), mix(sample1, sample0, sx), sy);
		}

		void out_std_rgba(vec4 color) {
			if(color.a <= epsilon) {
				discard; // Handled by Clear
			} else if(color.a >= oneminus_epsilon) {
				out_c = vec4(color.rgb, 1.0);
				out_a = vec4(vec3(color.a), 1.0);
			} else {
				vec2 prev_coords = vec2(position.x / 2.0, position.y / 2.0) + vec2(0.5);
				vec3 base_c = texture(prev_c, prev_coords).rgb;
				vec3 base_a = texture(prev_a, prev_coords).rgb;

				out_c.r = (color.r * color.a) + (1.0 - (color.a * color.a)) * base_c.r;
				out_c.g = (color.g * color.a) + (1.0 - (color.a * color.a)) * base_c.g;
				out_c.b = (color.b * color.a) + (1.0 - (color.a * color.a)) * base_c.b;
				out_a.r = (color.a * color.a) + (1.0 - (color.a * color.a)) * base_a.r;
				out_a.g = (color.a * color.a) + (1.0 - (color.a * color.a)) * base_a.g;
				out_a.b = (color.a * color.a) + (1.0 - (color.a * color.a)) * base_a.b;
			}
		}

		void main() {
			if(type == -1) { // Clear
				vec2 prev_coords = vec2(position.x / 2.0, position.y / 2.0) + vec2(0.5);
				out_c = vec4(texture(prev_c, prev_coords).rgb, 1.0);
				out_a = vec4(texture(prev_a, prev_coords).rgb, 1.0);
			}
			else if(type == 0) { // Blended with Color
				out_std_rgba(color);
			}
			else if(type == 100) { // Plain Image
				out_std_rgba(textureBicubic(coords));
			}
			else if(type == 102) { // BackColorAdd
				out_std_rgba(clamp(textureBicubic(coords) + color, 0.0, 1.0));
			}
			else if(type == 103) { // BackColorBehind
				vec4 image = textureBicubic(coords);
				out_std_rgba(clamp(mix(color, image, image.a), 0.0, 1.0));
			}
			else if(type == 104) { // BackColorSubtract
				out_std_rgba(clamp(textureBicubic(coords) - color, 0.0, 1.0));
			}
			else if(type == 105) { // BackColorMultiply
				out_std_rgba(clamp(textureBicubic(coords) * color, 0.0, 1.0));
			}
			else if(type == 106) { // BackColorDivide
				out_std_rgba(clamp(textureBicubic(coords) / color, 0.0, 1.0));
			}
			else if(type == 107) { // Invert
				vec4 image = textureBicubic(coords);
				out_std_rgba(vec4(vec3(1.0) - image.rgb, image.a));
			}
			else if(type == 108 || type == 2) { // GlyphWithColor
				vec3 thisPixel = textureLod(nonuniformEXT(tex_nearest[tex_i]), coords, 0).rgb;
				vec3 rightPixel = textureLod(nonuniformEXT(tex_nearest[tex_i]), coords + vec2(1.0, 0.0), 0).rgb;
				float subPixel = fract(coords.x) * 3.0;
				float subPixelFract = fract(subPixel);
				vec3 mask = vec3(0.0);

				if(subPixel < 1.0) {
					mask = vec3(
						mix(thisPixel.r, thisPixel.g, subPixelFract),
						mix(thisPixel.g, thisPixel.b, subPixelFract),
						mix(thisPixel.b, rightPixel.r, subPixelFract)
					);
				} else if(subPixel < 2.0) {
					mask = vec3(
						mix(thisPixel.g, thisPixel.b, subPixelFract),
						mix(thisPixel.b, rightPixel.r, subPixelFract),
						mix(rightPixel.r, rightPixel.g, subPixelFract)
					);
				} else {
					mask = vec3(
						mix(thisPixel.b, rightPixel.r, subPixelFract),
						mix(rightPixel.r, rightPixel.g, subPixelFract),
						mix(rightPixel.g, rightPixel.b, subPixelFract)
					);
				}

				if(mask.r <= epsilon && mask.g <= epsilon && mask.b <= epsilon) {
					discard;
				} else {
					vec2 prev_coords = vec2(position.x / 2.0, position.y / 2.0) + vec2(0.5);
					vec3 base_c = texture(prev_c, prev_coords).rgb;
					vec3 base_a = texture(prev_a, prev_coords).rgb;
					out_c.r = color.r * mask.r + (1.0 - color.a * mask.r) * base_c.r;
					out_c.g = color.g * mask.g + (1.0 - color.a * mask.g) * base_c.g;
					out_c.b = color.b * mask.b + (1.0 - color.a * mask.b) * base_c.b;
					out_a.r = color.a * mask.r + (1.0 - color.a * mask.r) * base_a.r;
					out_a.g = color.a * mask.g + (1.0 - color.a * mask.g) * base_a.g;
					out_a.b = color.a * mask.b + (1.0 - color.a * mask.b) * base_a.b;
				}
			}
			else { // Unknown - Do Nothing
				discard;
			}
		}
"
}
