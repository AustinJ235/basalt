pub mod interface_fs {
	shader!{
		ty: "fragment",
		src: "
	#version 450

	layout(location = 0) in vec2 coords;
	layout(location = 1) in vec4 color;
	layout(location = 2) in flat int type;

	layout(location = 0) out vec4 out_color;

	layout(set = 0, binding = 0) uniform sampler2D tex;

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
		vec2 texSize = textureSize(tex, 0);
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
		vec4 sample0 = texture(tex, offset.xz);
		vec4 sample1 = texture(tex, offset.yz);
		vec4 sample2 = texture(tex, offset.xw);
		vec4 sample3 = texture(tex, offset.yw);
		float sx = s.x / (s.x + s.y);
		float sy = s.z / (s.z + s.w);
		return mix(mix(sample3, sample2, sx), mix(sample1, sample0, sx), sy);
	}

	void main() {
		if(type == 0) { // Verts with Color
			out_color = color;
		} else if(type == 1) { // Verts with Texture mixed with Color
			out_color = vec4(color.rgb, textureBicubic(coords).a * color.a);
		} else if(type == 2) { // Text Glyph
			float value = texture(tex, coords).r;
			out_color = vec4(vec3(color * value), value);
		} else if(type >= 100 && type <= 199) {
			if(type == 101) { // YUV Image
				vec2 y_coords = vec2(coords.x, (coords.y / 3.0) * 2.0);
				vec2 u_coords = vec2(coords.x / 2.0, (2.0 / 3.0) + (coords.y / 3.0));
				vec2 v_coords = vec2(0.5 + (coords.x / 2.0), (2.0 / 3.0) + (coords.y / 3.0));
				
				vec4 tex_color = vec4(
					textureBicubic(y_coords).r,
					textureBicubic(u_coords).r,
					textureBicubic(v_coords).r,
					1.0
				);
				
				float r = tex_color.r + (1.402 * (tex_color.b - 0.5));
				float g = tex_color.r - (0.344 * (tex_color.g - 0.5)) - (0.714 * (tex_color.b - 0.5));
				float b = tex_color.r + (1.772 * (tex_color.g - 0.5));
				r = pow((r + 0.055) / 1.055, 2.4);
				g = pow((g + 0.055) / 1.055, 2.4);
				b = pow((b + 0.055) / 1.055, 2.4);
				
				out_color = vec4(r, g, b, tex_color.a);
			} else if(type == 102) { // BackColorAdd
				out_color = clamp(textureBicubic(coords) + color, 0.0, 1.0);
			} else if(type == 103) { // BackColorBehind
				vec4 image_color = textureBicubic(coords);
				out_color = clamp(mix(color, image_color, image_color.z), 0.0, 1.0);
			} else if(type == 104) { // BackColorSubtract
				out_color = clamp(textureBicubic(coords) - color, 0.0, 1.0);
			} else if(type == 105) { // BackColorMultiply
				out_color = clamp(textureBicubic(coords) * color, 0.0, 1.0);
			} else if(type == 106) { // BackColorDivide
				out_color = clamp(textureBicubic(coords) / color, 0.0, 1.0);
			} else if(type == 107) { // Invert
				vec4 image_color = textureBicubic(coords);
				out_color = vec4(
					1.0 - image_color.r,
					1.0 - image_color.g,
					1.0 - image_color.b,
					image_color.a
				);
			} else { // Normal Image / Unknown
				out_color = textureBicubic(coords);
			}
		} else { // Unknown
			out_color = color;
		}
	}
	"
	}
}

