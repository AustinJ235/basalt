pub mod glyph_base_fs {
	shader!{
		ty: "fragment",
		src: "
			#version 450
			
			layout(location = 0) out vec4 color;
			layout(location = 0) in vec2 in_coords;
			
			layout(set = 0, binding = 0) uniform LineData {
				vec4 lines[256];
				uint count;
				float width;
				float height;
			} line_data;
			
float intersectTriangle(vec3 orig, vec3 dir, vec3 vertices[3], float lastHitT)
{
    const float INFINITY = 1e10;
    vec3 u, v, n; // triangle vectors
    vec3 w0, w;  // ray vectors
    float r, a, b; // params to calc ray-plane intersect

    // get triangle edge vectors and plane normal
    u = vertices[1] - vertices[0];
    v = vertices[2] - vertices[0];
    n = cross(u, v);

    w0 = orig - vertices[0];
    a = -dot(n, w0);
    b = dot(n, dir);
    if (abs(b) < 1e-5)
    {
        // ray is parallel to triangle plane, and thus can never intersect.
        return INFINITY;
    }

    // get intersect point of ray with triangle plane
    r = a / b;
    if (r < 0.0)
        return INFINITY; // ray goes away from triangle.

    vec3 I = orig + r * dir;
    float uu, uv, vv, wu, wv, D;
    uu = dot(u, u);
    uv = dot(u, v);
    vv = dot(v, v);
    w = I - vertices[0];
    wu = dot(w, u);
    wv = dot(w, v);
    D = uv * uv - uu * vv;

    // get and test parametric coords
    float s, t;
    s = (uv * wv - vv * wu) / D;
    if (s < 0.0 || s > 1.0)
        return INFINITY;
    t = (uv * wu - uu * wv) / D;
    if (t < 0.0 || (s + t) > 1.0)
        return INFINITY;

    return (r > 1e-5) ? r : INFINITY;
}

			void main() {
				vec2 test_vectors[8] = vec2[8](
					vec2(1.0, 0.0),
					vec2(-1.0, 0.0),
					vec2(0.0, 1.0),
					vec2(0.0, -1.0),
					vec2(-1.0, -1.0),
					vec2(1.0, -1.0),
					vec2(-1.0, 1.0),
					vec2(1.0, 1.0)
				);
				
				
				vec2 src = vec2(
					in_coords.x * line_data.width,
					in_coords.y * line_data.height
				);
				
				float collisions = 0.0;
				float val = 1.0;
				float collisions_total = 0.0;
				
				for(uint t = 0; t < 8; t++) {
					bool hit = false;
				
					for(uint i = 0; i < line_data.count; i++) {
						float r = intersectTriangle(
							vec3(src, 0.0),
							vec3(test_vectors[t], 0.0),
							vec3[3](
								vec3(line_data.lines[i].x, line_data.lines[i].y, 0.0),
								vec3(line_data.lines[i].z, line_data.lines[i].w, 0.0),
								vec3(line_data.lines[i].x, line_data.lines[i].y, -1.0)
							),
							0.0
						);
						
						if(r != 1e10) {
							val *= clamp(r, 0.0, 1.0);
							collisions_total += 1.0;
							hit = true;
						}
					}
					
					if(hit) {
						collisions += 1.0;
					}
				}
				
				if(collisions > 7.5) {
					color = vec4(val);
				} else {
					color = vec4(0.0);
				}
			}
		"
	}
}
