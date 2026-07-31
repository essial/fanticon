struct DisplayUniform {
    surface_size: vec2<f32>,
    crt_strength: f32,
    time_seconds: f32,
};

@group(0) @binding(0)
var display_texture: texture_2d<f32>;

@group(0) @binding(1)
var display_sampler: sampler;

@group(0) @binding(2)
var<uniform> display: DisplayUniform;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: VertexOutput;
    output.position = vec4<f32>(positions[index], 0.0, 1.0);
    return output;
}

fn source_sample(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(display_texture, display_sampler, uv, 0.0).rgb;
}

fn hash_noise(position: vec2<f32>) -> f32 {
    return fract(sin(dot(position, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn bright_pass(uv: vec2<f32>) -> vec3<f32> {
    let sample_color = source_sample(clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));
    let luminance = dot(sample_color, vec3<f32>(0.2126, 0.7152, 0.0722));
    return sample_color * smoothstep(0.52, 0.92, luminance);
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let source_size = vec2<f32>(320.0, 200.0);
    let fit_scale = min(
        display.surface_size.x / source_size.x,
        display.surface_size.y / source_size.y,
    );
    let scale = fit_scale;
    let content_size = source_size * scale;
    let content_origin = floor((display.surface_size - content_size) * 0.5);
    let screen_position = input.position.xy;

    if any(screen_position < content_origin)
        || any(screen_position >= content_origin + content_size)
    {
        return vec4<f32>(0.003, 0.004, 0.006, 1.0);
    }

    let stable_uv = (screen_position - content_origin) / content_size;
    // Sampling coordinates remain completely stable. Only a faint grain pattern
    // changes from frame to frame.
    let frame = floor(display.time_seconds * 60.0);
    let uv = stable_uv;
    let texel = vec2<f32>(1.0 / 320.0, 1.0 / 200.0);
    let center = source_sample(uv);
    let left = source_sample(clamp(uv - vec2<f32>(texel.x, 0.0), vec2<f32>(0.0), vec2<f32>(1.0)));
    let right = source_sample(clamp(uv + vec2<f32>(texel.x, 0.0), vec2<f32>(0.0), vec2<f32>(1.0)));

    // Small channel-dependent horizontal bleed approximates composite/chroma
    // artifacting without adding another render target or temporal history.
    var color = vec3<f32>(
        center.r * 0.84 + left.r * 0.16,
        center.g * 0.94 + (left.g + right.g) * 0.03,
        center.b * 0.84 + right.b * 0.16,
    );

    let source_position = uv * source_size;
    let scan_wave = 0.76 + 0.24 * sin(3.14159265 * fract(source_position.y));

    // RGB triad modulation is anchored to physical output pixels so it remains
    // stable while the emulated pixels use integer scaling.
    let mask_phase = u32(screen_position.x - content_origin.x) % 3u;
    var phosphor = vec3<f32>(0.88);
    if mask_phase == 0u {
        phosphor.r = 1.08;
    } else if mask_phase == 1u {
        phosphor.g = 1.08;
    } else {
        phosphor.b = 1.08;
    }

    let centered = stable_uv * 2.0 - vec2<f32>(1.0);
    let vignette = clamp(1.08 - dot(centered, centered) * 0.18, 0.72, 1.0);
    let crt = color * scan_wave * phosphor * vignette;
    color = mix(center, crt, display.crt_strength);

    // A compact bright-pass halo approximates phosphor and glass bloom. The
    // asymmetric weights keep the spread predominantly horizontal, as expected
    // from a scanned CRT image, while remaining subtle around ordinary colors.
    let bloom_horizontal = bright_pass(uv - vec2<f32>(texel.x, 0.0))
        + bright_pass(uv + vec2<f32>(texel.x, 0.0));
    let bloom_vertical = bright_pass(uv - vec2<f32>(0.0, texel.y))
        + bright_pass(uv + vec2<f32>(0.0, texel.y));
    let bloom_diagonal = bright_pass(uv + vec2<f32>(-texel.x, -texel.y))
        + bright_pass(uv + vec2<f32>(texel.x, -texel.y))
        + bright_pass(uv + vec2<f32>(-texel.x, texel.y))
        + bright_pass(uv + vec2<f32>(texel.x, texel.y));
    color += bloom_horizontal * 0.026 + bloom_vertical * 0.018 + bloom_diagonal * 0.006;

    let snow = hash_noise(floor(screen_position) + vec2<f32>(frame * 17.0, frame * 23.0));
    color += vec3<f32>((snow - 0.5) * 0.012);
    return vec4<f32>(color, 1.0);
}
