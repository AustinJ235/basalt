#version 450

layout(location = 0) out vec4 out_color;

layout(input_attachment_index = 0, set = 0, binding = 0) uniform subpassInput user_color;
layout(input_attachment_index = 1, set = 0, binding = 1) uniform subpassInput ui_color;

void main() {
    vec4 user_color = subpassLoad(user_color);
    vec4 ui_color = subpassLoad(ui_color);
    out_color = vec4(mix(user_color.rgb, ui_color.rgb, ui_color.a), user_color.a);
}
