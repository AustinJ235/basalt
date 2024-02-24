#version 450

#extension GL_EXT_nonuniform_qualifier : enable

layout(location = 0) in vec2 coords;
layout(location = 1) in vec4 color;
layout(location = 2) in flat int type;
layout(location = 3) in vec2 position;
layout(location = 4) in flat uint tex_i;

layout(location = 0) out vec4 out_color;

layout(set = 0, binding = 0) uniform sampler image_sampler;
layout(set = 0, binding = 1) uniform texture2D images[];

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
    vec2 texSize = textureSize(nonuniformEXT(sampler2D(images[tex_i], image_sampler)), 0);
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
    vec4 sample0 = textureLod(nonuniformEXT(sampler2D(images[tex_i], image_sampler)), offset.xz, 0);
    vec4 sample1 = textureLod(nonuniformEXT(sampler2D(images[tex_i], image_sampler)), offset.yz, 0);
    vec4 sample2 = textureLod(nonuniformEXT(sampler2D(images[tex_i], image_sampler)), offset.xw, 0);
    vec4 sample3 = textureLod(nonuniformEXT(sampler2D(images[tex_i], image_sampler)), offset.yw, 0);
    float sx = s.x / (s.x + s.y);
    float sy = s.z / (s.z + s.w);
    return mix(mix(sample3, sample2, sx), mix(sample1, sample0, sx), sy);
}

void main() {
    if(type == 0) { // Blended with Color
        out_color = color;
    }
    else if(type == 100) { // Plain Image
        out_color = textureBicubic(coords);
    }
    else if(type == 102) { // BackColorAdd
        out_color = clamp(textureBicubic(coords) + color, 0.0, 1.0);
    }
    else if(type == 103) { // BackColorBehind
        vec4 image = textureBicubic(coords);
        out_color = clamp(mix(color, image, image.a), 0.0, 1.0);
    }
    else if(type == 104) { // BackColorSubtract
        out_color = clamp(textureBicubic(coords) - color, 0.0, 1.0);
    }
    else if(type == 105) { // BackColorMultiply
        out_color = clamp(textureBicubic(coords) * color, 0.0, 1.0);
    }
    else if(type == 106) { // BackColorDivide
        out_color = clamp(textureBicubic(coords) / color, 0.0, 1.0);
    }
    else if(type == 107) { // Invert
        vec4 image = textureBicubic(coords);
        out_color = vec4(vec3(1.0) - image.rgb, image.a);
    }
    else if(type == 108 || type == 2) { // GlyphWithColor
        out_color = vec4(
            color.rgb,
            textureLod(nonuniformEXT(sampler2D(images[tex_i], image_sampler)), coords, 0).r
        );
    }
}