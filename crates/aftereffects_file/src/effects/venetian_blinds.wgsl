// Venetian Blinds approximation. Completion removes the leading fraction
// of every period; 0 degrees gives vertical bands. RGB and alpha are attenuated
// together, preserving the renderer's premultiplied input convention.
struct VertexInput {
  @location(0) position: vec2<f32>,
  @location(1) tex_coords: vec2<f32>,
  @location(2) color: vec4<f32>,
};
struct VertexOutput {
  @builtin(position) clip_position: vec4<f32>,
  @location(0) tex_coords: vec2<f32>,
};
@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
  var out: VertexOutput;
  out.clip_position = vec4<f32>(input.position, 0.0, 1.0);
  out.tex_coords = input.tex_coords;
  return out;
}
struct Params {
  completion: f32,
  direction: f32,
  width: f32,
  plane_width: f32,
  plane_height: f32,
  pixel_center_offset: f32,
  feather: f32,
  _pad2: f32,
};
@group(0) @binding(0) var t_texture: texture_2d<f32>;
@group(0) @binding(1) var s_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;

// Integral of the periodic unit-coverage band, also valid at negative positions.
fn integral(position: f32, period: f32, edge: f32) -> f32 {
  let turn = floor(position / period);
  let phase = position - turn * period;
  return turn * (period - edge) + max(phase - edge, 0.0);
}
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
  let source = textureSample(t_texture, s_sampler, input.tex_coords);
  let angle = radians(params.direction);
  let plane = input.tex_coords * vec2<f32>(params.plane_width, params.plane_height)
    + vec2<f32>(params.pixel_center_offset);
  let position = dot(plane, vec2<f32>(cos(angle), sin(angle)));
  // Evaluate derivatives outside divergent branches; edits retain stable edges.
  let footprint = max(max(fwidth(position), 0.0001), 2.0 * max(params.feather, 0.0));
  let period = max(params.width, 1.0);
  let edge = clamp(params.completion / 100.0, 0.0, 1.0) * period;
  let coverage = clamp(
    (integral(position + footprint * 0.5, period, edge)
      - integral(position - footprint * 0.5, period, edge)) / footprint,
    0.0, 1.0,
  );
  return source * coverage;
}
