// RGB-distance surrogate. Work in straight RGB, then restore premultiplication.
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) color: vec4<f32>,
};
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};
@vertex fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(input.position, 0.0, 1.0);
    out.tex_coords = input.tex_coords;
    return out;
}
struct Params {
    red: f32,
    green: f32,
    blue: f32,
    tolerance: f32,
    pedestal: f32,
    transparency: f32,
    _pad0: f32,
    _pad1: f32,
};
@group(0) @binding(0) var content: texture_2d<f32>;
@group(0) @binding(1) var content_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = textureSample(content, content_sampler, input.tex_coords);
    if pixel.a <= 0.000001 { return vec4<f32>(0.0); }
    let rgb = pixel.rgb / pixel.a;
    let key = vec3<f32>(params.red, params.green, params.blue);
    let distance = length(rgb - key) / sqrt(3.0);
    let radius = 0.002 * params.tolerance + 0.001 * params.pedestal;
    let softness = max(0.001, 0.001 * params.transparency);
    let coverage = smoothstep(radius, radius + softness, distance);
    let alpha = pixel.a * coverage;
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * alpha, alpha);
}
