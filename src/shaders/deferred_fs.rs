pub mod deferred_fs {
	shader!{
		ty: "fragment",
		src: "
	#version 450

	#define NEAR_PLANE 0.1
	#define FAR_PLANE 1000.0

	#define SAMPLE_SCALE 1.008
	#define DEPTH_MIN_CLAMP -0.0421
	#define DEPTH_MAX_CLAMP  0.0421
	#define OCCLUSION_DIVIDER 0.0675

	layout(location = 0) in vec2 in_coords;
	layout(location = 0) out vec4 out_color;

	layout(set = 0, binding = 0) uniform sampler2D color_tex;
	layout(set = 0, binding = 1) uniform sampler2D normal_tex;
	layout(set = 0, binding = 2) uniform sampler2D depth_tex;
	layout(set = 0, binding = 3) uniform sampler2D trans_color_tex;
	layout(set = 0, binding = 4) uniform sampler2D trans_normal_tex;
	layout(set = 0, binding = 5) uniform sampler2D trans_depth_tex;
	layout(set = 0, binding = 6) uniform sampler2D shadow_tex;
	layout(set = 0, binding = 7) uniform samplerCube sky_box;

	layout(set = 0, binding = 8) uniform Data {
		vec4 samples[512];
	} uni;

	layout(set = 1, binding = 0) uniform Other {
		int win_size_x;
		int win_size_y;
		float aspect_ratio;
		mat4 view;
		mat4 inverse_view;
		mat4 projection;
		mat4 inverse_projection;
		mat4 shadow_vp;
		int shadow_state;
		vec4 sun_direction;
		int samples;
		float strength;
		float sample_scale;
		float range;
	} other;

	float linear_depth(float in_depth) {
		return 2.0 * NEAR_PLANE * FAR_PLANE / (FAR_PLANE + NEAR_PLANE - (2.0 * in_depth - 1.0) * (FAR_PLANE - NEAR_PLANE));
	}

	vec3 world_pos(vec2 in_coords) {
		vec4 position = vec4(in_coords * 2.0 - 1.0, texture(depth_tex, in_coords).r, 1.0);
		position = other.inverse_projection * position;
		position /= position.w;
		position = other.inverse_view * position;
		position /= position.w;
		return position.xyz;
	}

	vec3 shadow_coords(vec3 world_pos) {
		vec4 position = vec4(world_pos, 1.0);
		position = other.shadow_vp * position;
		position /= position.w;
		return vec3(position.xy / 2.0 + 0.5, position.z);
	}

	float shadow_mult(vec2 in_coords) {
		float shadow_value = 0.0;

		for(int i = 0; i < 16; i++) {
			vec3 shadow_coords = shadow_coords(world_pos(in_coords));
			float shadow_depth = texture(shadow_tex, shadow_coords.xy + uni.samples[i+16].xy * 0.001).r;
			
			if(shadow_depth < shadow_coords.z) {
				shadow_value += 1.0;
			}
		}
		
		shadow_value /= 16;
		return exp(clamp(exp(1.1 - shadow_value) / exp(1.0) + 0.42, 0.0, 1.0)) / exp(1.0);
	}

	vec4 apply_crosshair(vec2 in_coords, vec4 color) {
		int center_x = other.win_size_x / 2;
		int center_y = other.win_size_y / 2;
		int half_width = 1;
		int half_height = 15;
		int x = int(in_coords.x * other.win_size_x);
		int y = int(in_coords.y * other.win_size_y);
		
		if(x >= center_x - half_width && x <= center_x + half_width &&
			y >= center_y - half_height && y <= center_y + half_height) {
				color.rgb += vec3(0.5);
				
				if(color.r > 1.0) {
					color.r -= 1.0;
				} if(color.g > 1.0) {
					color.g -= 1.0;
				} if(color.b > 1.0) {
					color.b -= 1.0;
				}
		} else if(y >= center_y - half_width && y <= center_y + half_width &&
			x >= center_x - half_height && x <= center_x + half_height) {
				color.rgb += vec3(0.5);
				
				if(color.r > 1.0) {
					color.r -= 1.0;
				} if(color.g > 1.0) {
					color.g -= 1.0;
				} if(color.b > 1.0) {
					color.b -= 1.0;
				}
		}
		
		return color;
	}

	vec4 apply_fog(vec2 in_coords, vec4 color) {
		float depth = texture(depth_tex, in_coords).r;
		float fog_min = 0.997;
		float clamp_max = 1.0 - fog_min;
		float clamp_mult = 1.0 / clamp_max;
		float amt = clamp(depth-fog_min, 0.0, clamp_max) * clamp_mult * 0.5;
		return mix(color, vec4(0.8, 0.7, 1.0, 1.0), amt);
	}

	// https://math.stackexchange.com/questions/1905533/
	float line_point_dist(vec3 a, vec3 b, vec3 c) {
		vec3 d = (c - b) / distance(c, b);
		vec3 v = a - b;
		float t = dot(v, d);
		vec3 p = b + t * d;
		return distance(p, a);
	}

	void main() {
		float depth = linear_depth(texture(depth_tex, in_coords).r);
		float trans_depth = linear_depth(texture(trans_depth_tex, in_coords).r);
		vec3 normal = texture(normal_tex, in_coords).xyz;	
		
		if(other.shadow_state >= 2) {
			if(in_coords.x > 0.75 && in_coords.y < 0.25) {
				vec2 new_coords = in_coords;
				new_coords.x -= 0.75;
				new_coords.x *= 4.0;
				new_coords.y *= 4.0;
				out_color = vec4(vec3(linear_depth(texture(shadow_tex, new_coords).r)), 1.0);
				return;
			}
		}
		
		if(depth >= FAR_PLANE && trans_depth >= FAR_PLANE) {
			float mult = pow(clamp(world_pos(in_coords).y / 4000.0, -1.0, 1.0) / 2.0, 2.0);
			const vec3 base_color = vec3(0.0 / 255.0, 0.0 / 255.0, 255.0 / 255.0);
			vec4 sky_color = vec4((base_color * 0.50) + (base_color * mult), 1.0);
			
			const float sun_size = 0.05;
			vec4 zero = other.inverse_view * vec4(0.0, 0.0, 0.0, 1.0);
			zero /= zero.w;
			vec3 w_pos = world_pos(in_coords);
			
			vec3 dir = normalize(zero.xyz - w_pos);
			float dist_from_sun = line_point_dist(zero.xyz, zero.xyz-other.sun_direction.xyz, w_pos);
			
			if(w_pos.y > -sun_size && dist_from_sun < sun_size) {
				float sun_mult = clamp(sun_size - dist_from_sun, 0, sun_size) / sun_size;
				sky_color = vec4(1.0, 1.0, 1.0, 1.0);
			} else {
				dir = vec3(dir.z, dir.y, dir.x);
				sky_color = texture(sky_box, dir);
			}
			
			out_color = apply_crosshair(in_coords, sky_color);
			return;
		}

		float occlusion = 0.0;
		float sample_scale = SAMPLE_SCALE * other.sample_scale * (1.0 - (depth / 1000.0));
		vec3 world_pos = world_pos(in_coords);

		for(int i = 0; i < other.samples; i++) {
			vec3 sample_pos = world_pos + (uni.samples[i].xyz * sample_scale);
			vec4 sample_proj = other.projection * other.view * vec4(sample_pos, 1.0);
			sample_proj /= sample_proj.w;
			sample_proj.xy /= 2.0;
			sample_proj.xy += vec2(0.5);
			sample_proj.z = linear_depth(texture(depth_tex, sample_proj.xy).r);
			occlusion += clamp(depth - sample_proj.z, DEPTH_MIN_CLAMP * other.range, DEPTH_MAX_CLAMP * other.range);
		}
		
		occlusion /= float(other.samples) * OCCLUSION_DIVIDER;
		occlusion *= other.strength;
		
		vec4 post_ssao_color = vec4(1.0);
		float diffuse = (1.0 - max(dot(normal, other.sun_direction.xyz), 0.0)) * 0.5 + 0.5;
		
		if(other.shadow_state != 0) {
			post_ssao_color = vec4(vec3(1.0 - occlusion) * texture(color_tex, in_coords).rgb * shadow_mult(in_coords) * diffuse, 1.0);
		} else {
			post_ssao_color = vec4(vec3(1.0 - occlusion) * texture(color_tex, in_coords).rgb * diffuse, 1.0);
		}
		
		if(trans_depth < depth) {
			vec4 trans_color = texture(trans_color_tex, in_coords);
			post_ssao_color = mix(post_ssao_color, trans_color, trans_color.a);
		}
		
		out_color = apply_fog(in_coords, apply_crosshair(in_coords, post_ssao_color));
	}
	"
	}
}

