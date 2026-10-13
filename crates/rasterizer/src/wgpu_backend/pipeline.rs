pub(crate) const SHADER_SRC: &str = r#"
struct Globals {
    resolution: vec2<f32>,
};

struct InstanceData {
    // x = shape type, y = gradient stop count
    kind_data: vec4<u32>,
    // Draw and original shape bounds in local pixels.
    bounds: vec4<f32>,
    shape_bounds: vec4<f32>,
    color: vec4<f32>,
    color2: vec4<f32>,
    brightness: vec4<f32>,
    grayscale: vec4<f32>,
    contrast: vec4<f32>,
    saturation: vec4<f32>,
    // x = offset, y = darkness, z = roundness, w = enabled
    vignette: vec4<f32>,
    // x = invert amount
    invert: vec4<f32>,
    // x = hue rotation in degrees
    hue: vec4<f32>,
    // rgb = tint color in sRGB, w = amount
    tint: vec4<f32>,
    // rgb = duotone endpoints in sRGB, w = enabled on primary
    duotone_primary: vec4<f32>,
    duotone_secondary: vec4<f32>,
    // x = contrast, y = saturation, z = inverse gamma, w = enabled
    grading: vec4<f32>,
    // rgb = tint color in sRGB, w = amount
    grading_tint: vec4<f32>,
    clip_rects: array<vec4<f32>, 4>,
    mask_opacity: vec4<f32>,
    mask_kinds: vec4<u32>,
    mask_shapes: array<vec4<f32>, 4>,
    mask_color0: array<vec4<f32>, 4>,
    mask_color1: array<vec4<f32>, 4>,
    mask_color2: array<vec4<f32>, 4>,
    mask_color3: array<vec4<f32>, 4>,
    mask_stop_positions: array<vec4<f32>, 4>,
    mask_stop_counts: vec4<u32>,
    // corner radius, stroke width, angle, legacy inherited opacity slot
    params: vec4<f32>,
    // inherited opacity kept separate from image/text UV coordinates
    opacity: vec4<f32>,
    // x' = dot(transform_x.xyz, vec3(x, y, 1))
    // y' = dot(transform_y.xyz, vec3(x, y, 1))
    transform_x: vec4<f32>,
    transform_y: vec4<f32>,
    stop_positions: array<vec4<f32>, 16>,
    stop_colors: array<vec4<f32>, 16>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var<storage, read> instances: array<InstanceData>;
@group(2) @binding(0) var image_texture: texture_2d<f32>;
@group(2) @binding(1) var image_sampler: sampler;
@group(3) @binding(0) var path_mask_texture: texture_2d<f32>;
@group(3) @binding(1) var path_mask_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local_position: vec2<f32>,
    @location(1) @interpolate(flat) instance_index: u32,
    @location(2) transformed_position: vec2<f32>,
};

fn to_clip_position(pixel_position: vec2<f32>) -> vec4<f32> {
    let ndc = vec2<f32>(
         pixel_position.x / globals.resolution.x * 2.0 - 1.0,
        -pixel_position.y / globals.resolution.y * 2.0 + 1.0,
    );
    return vec4<f32>(ndc, 0.0, 1.0);
}

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) iid: u32,
) -> VertexOutput {
    let instance = instances[iid];
    let x = instance.bounds.x;
    let y = instance.bounds.y;
    let w = instance.bounds.z;
    let h = instance.bounds.w;

    // Avoid dynamically indexing a local array: some native shader backends
    // only permit constant indexes for this expression class.
    var pixel_pos: vec2<f32>;
    switch vid {
        case 0u: { pixel_pos = vec2<f32>(x,     y + h); }
        case 1u: { pixel_pos = vec2<f32>(x + w, y + h); }
        case 2u: { pixel_pos = vec2<f32>(x,     y    ); }
        case 3u: { pixel_pos = vec2<f32>(x,     y    ); }
        case 4u: { pixel_pos = vec2<f32>(x + w, y + h); }
        default: { pixel_pos = vec2<f32>(x + w, y    ); }
    }
    let value = vec3<f32>(pixel_pos, 1.0);
    let transformed = vec2<f32>(
        dot(instance.transform_x.xyz, value),
        dot(instance.transform_y.xyz, value),
    );
    return VertexOutput(to_clip_position(transformed), pixel_pos, iid, transformed);
}

struct MeshVertexInput {
    @location(0) position: vec2<f32>,
};

@vertex
fn vs_mesh(
    vertex: MeshVertexInput,
    @builtin(instance_index) iid: u32,
) -> VertexOutput {
    let instance = instances[iid];
    let value = vec3<f32>(vertex.position, 1.0);
    let transformed = vec2<f32>(
        dot(instance.transform_x.xyz, value),
        dot(instance.transform_y.xyz, value),
    );
    return VertexOutput(to_clip_position(transformed), vertex.position, iid, transformed);
}

fn gradient_color(instance_idx: u32, t: f32) -> vec4<f32> {
    let count = max(instances[instance_idx].kind_data.y, 1u);
    if t <= instances[instance_idx].stop_positions[0].x {
        return instances[instance_idx].stop_colors[0];
    }

    var previous_position = instances[instance_idx].stop_positions[0].x;
    var previous_color = instances[instance_idx].stop_colors[0];
    for (var index = 1u; index < 16u; index = index + 1u) {
        if index >= count {
            break;
        }
        let next_position = instances[instance_idx].stop_positions[index].x;
        let next_color = instances[instance_idx].stop_colors[index];
        if t <= next_position {
            let span = max(next_position - previous_position, 0.000001);
            let amount = clamp((t - previous_position) / span, 0.0, 1.0);
            return mix(previous_color, next_color, amount);
        }
        previous_position = next_position;
        previous_color = next_color;
    }
    return previous_color;
}

fn linear_to_srgb(channel: f32) -> f32 {
    if channel <= 0.0031308 {
        return channel * 12.92;
    }
    return 1.055 * pow(channel, 1.0 / 2.4) - 0.055;
}

fn srgb_to_linear(channel: f32) -> f32 {
    if channel <= 0.04045 {
        return channel / 12.92;
    }
    return pow((channel + 0.055) / 1.055, 2.4);
}

fn apply_color_filters(color: vec4<f32>, instance: InstanceData) -> vec4<f32> {
    if instance.brightness.x == 1.0
        && instance.grayscale.x == 0.0
        && instance.contrast.x == 1.0
        && instance.saturation.x == 1.0
        && instance.invert.x == 0.0
        && instance.hue.x == 0.0
        && instance.tint.w == 0.0
        && instance.duotone_primary.w == 0.0
        && instance.grading.w == 0.0
    {
        return color;
    }
    // Filter math follows TinySkia's sRGB byte-domain behaviour. All regular
    // GPU inputs (including image/video samples) arrive here in linear light,
    // so convert them to sRGB for the filter operations and convert the
    // result back before storing into the sRGB render target.
    var srgb = vec3<f32>(
        linear_to_srgb(clamp(color.r, 0.0, 1.0)),
        linear_to_srgb(clamp(color.g, 0.0, 1.0)),
        linear_to_srgb(clamp(color.b, 0.0, 1.0)),
    );
    srgb = srgb * instance.brightness.x;
    let luma = dot(srgb, vec3<f32>(0.299, 0.587, 0.114));
    srgb = luma + (srgb - vec3<f32>(luma)) * instance.saturation.x;
    if instance.grayscale.x > 0.0 {
        let gray = dot(srgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        srgb = mix(srgb, vec3<f32>(gray), instance.grayscale.x);
    }
    srgb = (srgb - vec3<f32>(0.5)) * instance.contrast.x + vec3<f32>(0.5);
    srgb = mix(srgb, vec3<f32>(1.0) - srgb, clamp(instance.invert.x, 0.0, 1.0));
    let max_channel = max(srgb.r, max(srgb.g, srgb.b));
    let min_channel = min(srgb.r, min(srgb.g, srgb.b));
    let delta = max_channel - min_channel;
    let value = max_channel;
    let saturation_value = select(0.0, delta / max_channel, max_channel > 0.0);
    var hue_degrees = 0.0;
    if delta > 0.000001 {
        if max_channel == srgb.r {
            hue_degrees = 60.0 * ((srgb.g - srgb.b) / delta);
        } else if max_channel == srgb.g {
            hue_degrees = 60.0 * ((srgb.b - srgb.r) / delta + 2.0);
        } else {
            hue_degrees = 60.0 * ((srgb.r - srgb.g) / delta + 4.0);
        }
    }
    hue_degrees = (hue_degrees + instance.hue.x) % 360.0;
    if hue_degrees < 0.0 {
        hue_degrees = hue_degrees + 360.0;
    }
    let chroma = value * saturation_value;
    let hue_sector = hue_degrees / 60.0;
    let x = chroma * (1.0 - abs((hue_sector % 2.0) - 1.0));
    let match_value = value - chroma;
    var rotated = vec3<f32>(0.0);
    if hue_sector < 1.0 {
        rotated = vec3<f32>(chroma, x, 0.0);
    } else if hue_sector < 2.0 {
        rotated = vec3<f32>(x, chroma, 0.0);
    } else if hue_sector < 3.0 {
        rotated = vec3<f32>(0.0, chroma, x);
    } else if hue_sector < 4.0 {
        rotated = vec3<f32>(0.0, x, chroma);
    } else if hue_sector < 5.0 {
        rotated = vec3<f32>(x, 0.0, chroma);
    } else {
        rotated = vec3<f32>(chroma, 0.0, x);
    }
    srgb = rotated + vec3<f32>(match_value);
    srgb = mix(srgb, instance.tint.rgb, clamp(instance.tint.w, 0.0, 1.0));
    if instance.duotone_primary.w > 0.0 {
        let duo_luma = clamp(dot(srgb, vec3<f32>(0.299, 0.587, 0.114)), 0.0, 1.0);
        srgb = mix(instance.duotone_primary.rgb, instance.duotone_secondary.rgb, duo_luma);
    }
    if instance.grading.w > 0.0 {
        srgb = pow(clamp(srgb, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(instance.grading.z));
        srgb = (srgb - vec3<f32>(0.5)) * instance.grading.x + vec3<f32>(0.5);
        let grading_luma = dot(srgb, vec3<f32>(0.299, 0.587, 0.114));
        srgb = grading_luma + (srgb - vec3<f32>(grading_luma)) * instance.grading.y;
        srgb = mix(srgb, instance.grading_tint.rgb, clamp(instance.grading_tint.w, 0.0, 1.0));
    }
    let linear = vec3<f32>(
        srgb_to_linear(clamp(srgb.r, 0.0, 1.0)),
        srgb_to_linear(clamp(srgb.g, 0.0, 1.0)),
        srgb_to_linear(clamp(srgb.b, 0.0, 1.0)),
    );
    return vec4<f32>(linear, color.a);
}

fn vignette_factor(position: vec2<f32>, bounds: vec4<f32>, settings: vec4<f32>) -> f32 {
    if settings.w < 0.5 {
        return 1.0;
    }
    let local = clamp((position - bounds.xy) / max(bounds.zw, vec2<f32>(0.000001)), vec2<f32>(0.0), vec2<f32>(1.0));
    let px = 2.0 * abs(local.x - 0.5);
    let py = 2.0 * abs(local.y - 0.5);
    let offset = clamp(settings.x, 0.0, 2.0);
    let darkness = clamp(settings.y, 0.0, 1.0);
    let roundness = clamp(settings.z, 0.0, 1.0);
    let max_diag = (1.0 - roundness) + roundness * sqrt(2.0);
    let span = max(max_diag - offset, 0.00001);
    let d = mix(max(px, py), sqrt(px * px + py * py), roundness);
    let t = clamp((d - offset) / span, 0.0, 1.0);
    let smooth_factor = t * t * (3.0 - 2.0 * t);
    return 1.0 - darkness * smooth_factor;
}

fn apply_srgb_vignette(rgb: vec3<f32>, factor: f32) -> vec3<f32> {
    let srgb = vec3<f32>(
        linear_to_srgb(clamp(rgb.r, 0.0, 1.0)),
        linear_to_srgb(clamp(rgb.g, 0.0, 1.0)),
        linear_to_srgb(clamp(rgb.b, 0.0, 1.0)),
    ) * factor;
    return vec3<f32>(
        srgb_to_linear(clamp(srgb.r, 0.0, 1.0)),
        srgb_to_linear(clamp(srgb.g, 0.0, 1.0)),
        srgb_to_linear(clamp(srgb.b, 0.0, 1.0)),
    );
}

fn composited_color(rgb: vec3<f32>, alpha: f32, instance: InstanceData) -> vec4<f32> {
    var output_rgb = rgb;
    // A frame containing fixed-function blend operations must keep every
    // source and destination in the same sRGB domain. The regular renderer
    // remains linear-light for compatibility with its existing path.
    // Image/video/Lottie samples are already stored as normalized sRGB
    // texels; analytic colors are stored in linear space.
    var output_is_srgb = false;
    if (instance.kind_data.w & 2u) != 0u && instance.kind_data.x != 5u {
        output_rgb = vec3<f32>(
            linear_to_srgb(clamp(rgb.r, 0.0, 1.0)),
            linear_to_srgb(clamp(rgb.g, 0.0, 1.0)),
            linear_to_srgb(clamp(rgb.b, 0.0, 1.0)),
        );
        output_is_srgb = true;
    }
    // Multiply/Screen/Darken/Lighten blend factors require premultiplied
    // source RGB.
    if instance.kind_data.z != 0u {
        return vec4<f32>(output_rgb * alpha, alpha);
    }
    if !output_is_srgb {
        output_rgb = vec3<f32>(
            linear_to_srgb(clamp(output_rgb.r, 0.0, 1.0)),
            linear_to_srgb(clamp(output_rgb.g, 0.0, 1.0)),
            linear_to_srgb(clamp(output_rgb.b, 0.0, 1.0)),
        );
    }
    return vec4<f32>(output_rgb, alpha);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let instance = instances[in.instance_index];
    let mask_coverage_value = mask_coverage(in.transformed_position, instance);
    if mask_coverage_value <= 0.0 {
        discard;
    }
    let shape_type = instance.kind_data.x;
    let shape = instance.shape_bounds;
    var col = instance.color;
    var coverage = 1.0;

    if shape_type == 0u {
        let half = shape.zw * 0.5;
        let center = shape.xy + half;
        let corner_r = clamp(instance.params.x, 0.0, min(half.x, half.y));
        let stroke_width = instance.params.y;
        let p = in.local_position - center;
        let q = abs(p) - half + vec2<f32>(corner_r);
        let distance = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - corner_r;
        if corner_r <= 0.0 && stroke_width <= 0.0 {
            // TinySkia's plain axis-aligned rectangles have hard edges. The
            // analytic SDF smoothing is reserved for rounded/stroked shapes;
            // applying it to opaque fills creates an avoidable CPU/GPU fringe.
            coverage = 1.0;
        } else {
            coverage = 1.0 - smoothstep(-0.75, 0.75, distance);
        }
        if stroke_width > 0.0 {
            let stroke_coverage = 1.0 - smoothstep(
                stroke_width * 0.5 - 0.75,
                stroke_width * 0.5 + 0.75,
                abs(distance),
            );
            col = mix(col, instance.color2, stroke_coverage);
            coverage = max(coverage, stroke_coverage);
        }

    } else if shape_type == 1u {
        let center = shape.xy + shape.zw * 0.5;
        let radius = min(shape.z, shape.w) * 0.5;
        let distance = length(in.local_position - center) - radius;
        coverage = 1.0 - smoothstep(-0.75, 0.75, distance);
        let stroke_width = instance.params.y;
        if stroke_width > 0.0 {
            let stroke_coverage = 1.0 - smoothstep(
                stroke_width * 0.5 - 0.75,
                stroke_width * 0.5 + 0.75,
                abs(distance),
            );
            col = mix(col, instance.color2, stroke_coverage);
            coverage = max(coverage, stroke_coverage);
        }

    } else if shape_type == 2u {
        let angle_rad = instance.params.z * 3.14159265 / 180.0;
        let dir = vec2<f32>(sin(angle_rad), cos(angle_rad));
        let center = shape.xy + shape.zw * 0.5;
        let half_diagonal = length(shape.zw) * 0.5;
        let t = dot(in.local_position - center, dir) / max(half_diagonal * 2.0, 0.000001) + 0.5;
        col = gradient_color(in.instance_index, clamp(t, 0.0, 1.0));

    } else if shape_type == 3u {
        let center = shape.xy + shape.zw * 0.5;
        let radius = max(shape.z * 0.5, 0.000001);
        let t = clamp(length(in.local_position - center) / radius, 0.0, 1.0);
        col = gradient_color(in.instance_index, t);
    }

    col = apply_color_filters(col, instance);
    let vignette = vignette_factor(in.local_position, instance.shape_bounds, instance.vignette);
    let alpha = col.a * instance.opacity.x * coverage * mask_coverage_value;
    return composited_color(apply_srgb_vignette(col.rgb, vignette), alpha, instance);
}

@fragment
fn fs_solid(in: VertexOutput) -> @location(0) vec4<f32> {
    let instance = instances[in.instance_index];
    let mask_coverage_value = mask_coverage(in.transformed_position, instance);
    if mask_coverage_value <= 0.0 {
        discard;
    }
    let color = apply_color_filters(instance.color, instance);
    let vignette = vignette_factor(in.local_position, instance.shape_bounds, instance.vignette);
    let alpha = color.a * instance.opacity.x * mask_coverage_value;
    return composited_color(apply_srgb_vignette(color.rgb, vignette), alpha, instance);
}

@fragment
fn fs_image(in: VertexOutput) -> @location(0) vec4<f32> {
    let instance = instances[in.instance_index];
    let mask_coverage_value = mask_coverage(in.transformed_position, instance);
    if mask_coverage_value <= 0.0 {
        discard;
    }
    let bounds = instance.shape_bounds;
    let local = (in.local_position - bounds.xy) / max(bounds.zw, vec2<f32>(0.000001));
    let uv = mix(instance.params.xy, instance.params.zw, clamp(local, vec2<f32>(0.0), vec2<f32>(1.0)));
    let sampled = textureSample(image_texture, image_sampler, uv);
    let sampled_linear = vec3<f32>(
        srgb_to_linear(sampled.r),
        srgb_to_linear(sampled.g),
        srgb_to_linear(sampled.b),
    );
    let color = apply_color_filters(vec4<f32>(sampled_linear, instance.color.a), instance);
    let vignette = vignette_factor(in.local_position, instance.shape_bounds, instance.vignette);
    let alpha = sampled.a * instance.color.a * instance.opacity.x * mask_coverage_value;
    return composited_color(apply_srgb_vignette(color.rgb, vignette), alpha, instance);
}

@fragment
fn fs_text(in: VertexOutput) -> @location(0) vec4<f32> {
    let instance = instances[in.instance_index];
    let mask_coverage_value = mask_coverage(in.transformed_position, instance);
    if mask_coverage_value <= 0.0 {
        discard;
    }
    let bounds = instance.shape_bounds;
    let local = (in.local_position - bounds.xy) / max(bounds.zw, vec2<f32>(0.000001));
    let uv = mix(instance.params.xy, instance.params.zw, clamp(local, vec2<f32>(0.0), vec2<f32>(1.0)));
    let coverage = textureSample(image_texture, image_sampler, uv).r;
    let color = apply_color_filters(instance.color, instance);
    let vignette = vignette_factor(in.local_position, instance.shape_bounds, instance.vignette);
    let alpha = coverage * color.a * instance.opacity.x * mask_coverage_value;
    return composited_color(apply_srgb_vignette(color.rgb, vignette), alpha, instance);
}

fn mask_coverage(position: vec2<f32>, instance: InstanceData) -> f32 {
    var coverage = 0.0;
    var has_mask = false;
    var has_regular_mask = false;
    var clip_coverage = 1.0;
    let rect0 = instance.clip_rects[0];
    let rect1 = instance.clip_rects[1];
    let rect2 = instance.clip_rects[2];
    let rect3 = instance.clip_rects[3];
    let opacity0 = instance.mask_opacity[0];
    let opacity1 = instance.mask_opacity[1];
    let opacity2 = instance.mask_opacity[2];
    let opacity3 = instance.mask_opacity[3];
    if opacity0 >= 0.0 { has_mask = true; let value = mask_shape_coverage(position, rect0, instance.mask_shapes[0], instance.mask_kinds.x, opacity0, instance.mask_color0[0], instance.mask_color1[0], instance.mask_color2[0], instance.mask_color3[0], instance.mask_stop_positions[0], instance.mask_stop_counts.x); if instance.mask_kinds.x == 6u { clip_coverage *= value; } else { has_regular_mask = true; coverage = 1.0 - (1.0 - coverage) * (1.0 - value); } }
    if opacity1 >= 0.0 { has_mask = true; let value = mask_shape_coverage(position, rect1, instance.mask_shapes[1], instance.mask_kinds.y, opacity1, instance.mask_color0[1], instance.mask_color1[1], instance.mask_color2[1], instance.mask_color3[1], instance.mask_stop_positions[1], instance.mask_stop_counts.y); if instance.mask_kinds.y == 6u { clip_coverage *= value; } else { has_regular_mask = true; coverage = 1.0 - (1.0 - coverage) * (1.0 - value); } }
    if opacity2 >= 0.0 { has_mask = true; let value = mask_shape_coverage(position, rect2, instance.mask_shapes[2], instance.mask_kinds.z, opacity2, instance.mask_color0[2], instance.mask_color1[2], instance.mask_color2[2], instance.mask_color3[2], instance.mask_stop_positions[2], instance.mask_stop_counts.z); if instance.mask_kinds.z == 6u { clip_coverage *= value; } else { has_regular_mask = true; coverage = 1.0 - (1.0 - coverage) * (1.0 - value); } }
    if opacity3 >= 0.0 { has_mask = true; let value = mask_shape_coverage(position, rect3, instance.mask_shapes[3], instance.mask_kinds.w, opacity3, instance.mask_color0[3], instance.mask_color1[3], instance.mask_color2[3], instance.mask_color3[3], instance.mask_stop_positions[3], instance.mask_stop_counts.w); if instance.mask_kinds.w == 6u { clip_coverage *= value; } else { has_regular_mask = true; coverage = 1.0 - (1.0 - coverage) * (1.0 - value); } }
    if has_mask {
        var effective_coverage = coverage;
        if !has_regular_mask { effective_coverage = 1.0; }
        var result = effective_coverage * clip_coverage;
        if (instance.kind_data.w & 1u) == 1u {
            let uv = position / globals.resolution;
            result *= textureSample(path_mask_texture, path_mask_sampler, uv).r;
        }
        return result;
    }
    if (instance.kind_data.w & 1u) == 1u {
        let uv = position / globals.resolution;
        return textureSample(path_mask_texture, path_mask_sampler, uv).r;
    }
    return 1.0;
}

fn mask_shape_coverage(position: vec2<f32>, rect: vec4<f32>, shape: vec4<f32>, kind: u32, opacity: f32, color0: vec4<f32>, color1: vec4<f32>, color2: vec4<f32>, color3: vec4<f32>, positions: vec4<f32>, stop_count: u32) -> f32 {
    var inside = position.x >= rect.x && position.y >= rect.y &&
        position.x < rect.x + rect.z && position.y < rect.y + rect.w;
    if kind == 1u {
        let delta = position - shape.xy;
        inside = dot(delta, delta) < shape.z * shape.z;
    }
    if kind == 4u {
        let delta = position - shape.xy;
        inside = dot(delta, delta) < shape.z * shape.z;
    }
    if kind == 5u {
        let half_size = rect.zw * 0.5;
        let radius = min(shape.x, min(half_size.x, half_size.y));
        let delta = abs(position - (rect.xy + half_size)) -
            (half_size - vec2<f32>(radius, radius));
        let distance = length(max(delta, vec2<f32>(0.0))) +
            min(max(delta.x, delta.y), 0.0) - radius;
        inside = distance <= 0.0;
    }
    var shape_opacity = opacity;
    if kind == 2u || kind == 3u || kind == 4u {
        let direction = shape.zw - shape.xy;
        var t = clamp(dot(position - shape.xy, direction) / max(dot(direction, direction), 0.000001), 0.0, 1.0);
        if kind == 4u {
            t = clamp(length(position - shape.xy) / max(shape.z, 0.000001), 0.0, 1.0);
        }
        var color = color0;
        if stop_count >= 2u {
            if t <= positions.y {
                color = mix(color0, color1, clamp((t - positions.x) / max(positions.y - positions.x, 0.000001), 0.0, 1.0));
            } else if stop_count == 2u {
                color = color1;
            } else if t <= positions.z {
                color = mix(color1, color2, clamp((t - positions.y) / max(positions.z - positions.y, 0.000001), 0.0, 1.0));
            } else if stop_count == 3u {
                color = color2;
            } else if t <= positions.w {
                color = mix(color2, color3, clamp((t - positions.z) / max(positions.w - positions.z, 0.000001), 0.0, 1.0));
            } else {
                color = color3;
            }
        }
        shape_opacity = color.a;
        if kind == 3u {
            shape_opacity = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)) * color.a;
        }
    }
    if inside {
        return clamp(shape_opacity, 0.0, 1.0);
    }
    return 0.0;
}
"#;

// ────────────────────────────────────────────────────────────────────────────
// GPU State
// ────────────────────────────────────────────────────────────────────────────

pub(crate) fn shader_opacity_supported(source: &str) -> bool {
    !source.contains("@fragment") && source.contains("return ")
}

pub(crate) fn shader_source_with_opacity(source: &str, opacity: f32) -> String {
    if opacity >= 1.0 || !shader_opacity_supported(source) {
        return source.to_owned();
    }
    let Some(return_start) = source.rfind("return ") else {
        return source.to_owned();
    };
    let expression_start = return_start + "return ".len();
    let Some(semicolon_offset) = source[expression_start..].find(';') else {
        return source.to_owned();
    };
    let semicolon = expression_start + semicolon_offset;
    let expression = &source[expression_start..semicolon];
    format!(
        "{}let rust_motion_color = {}; return vec4<f32>(rust_motion_color.rgb, rust_motion_color.a * {:.9});{}",
        &source[..return_start],
        expression,
        opacity,
        &source[semicolon + 1..]
    )
}
