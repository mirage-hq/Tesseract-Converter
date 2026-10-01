// Bounded After Effects Keylight 906 (Keylight 1.2) approximation for the
// supported static profile: View Final Result, Soft Colour replacement with a
// neutral Replace Colour, Source Alpha Normal and Clip Black 0.
//
// The equations are implementation hypotheses from Keylight's documented
// behaviour, not its unpublished algorithm. For straight colour C = P / A of
// the premultiplied input (P, A) and Screen Colour S:
//   p      the strictly dominant channel of S; lo and hi the two other
//          channels, ordered by their values in S (a tie keeps RGB order)
//   D(x)   x[p] - (b * x[lo] + (1 - b) * x[hi]), b = Screen Balance / 100
//   k0     clamp(1 - Screen Gain / 100 * D(C) / D(S), 0, 1)   raw screen matte
//   k      clamp(k0 / (Clip White / 100), 0, 1)                linear clip
//   Q0     clamp(C - (1 - k0) * S, 0, k0)    screen subtraction, already
//                                            associated with k0
//   Q      clamp(Q0 + (k - k0) * luma(C), 0, k)   neutral Soft Colour
//   out    (A * Q, A * k)                         Source Alpha Normal
// A pixel whose channel p is not strictly dominant is foreground and stays
// unchanged. Degenerate runtime parameters (no unique dominant screen channel,
// D(S) <= 1e-6 or Clip White <= 0) pass every pixel through unchanged.
// Transparent input stays transparent black. Values are the renderer's
// display-referred SDR working values; HDR or colour-managed input is outside
// this approximation.

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

// One f32 per declared parameter, in declaration order, padded to 4 lanes.
struct Params {
  screen_r: f32,
  screen_g: f32,
  screen_b: f32,
  screen_gain: f32,
  screen_balance: f32,
  clip_white: f32,
  _pad0: f32,
  _pad1: f32,
};

@group(0) @binding(0) var t_texture: texture_2d<f32>;
@group(0) @binding(1) var s_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;

// Rec. 601 weights, the repository convention; Keylight's are unpublished.
const LUMA: vec3<f32> = vec3<f32>(0.299, 0.587, 0.114);
// The converter rejects a Screen Colour at or below the same limit.
const MIN_SCREEN_DIFFERENCE: f32 = 1e-6;

// One-hot selector of the strictly dominant channel, or zero on a tie.
fn dominant(color: vec3<f32>) -> vec3<f32> {
  return vec3<f32>(
    select(0.0, 1.0, color.r > color.g && color.r > color.b),
    select(0.0, 1.0, color.g > color.r && color.g > color.b),
    select(0.0, 1.0, color.b > color.r && color.b > color.g),
  );
}

// Weights of D: +1 on the dominant channel, -b on the smaller and -(1 - b) on
// the larger remaining screen channel.
fn difference_weights(screen: vec3<f32>, primary: vec3<f32>, balance: f32) -> vec3<f32> {
  var low: vec3<f32>;
  if (primary.r > 0.5) {
    low = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 1.0, 0.0), screen.g <= screen.b);
  } else if (primary.g > 0.5) {
    low = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), screen.r <= screen.b);
  } else {
    low = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), screen.r <= screen.g);
  }
  let high = vec3<f32>(1.0) - primary - low;
  return primary - balance * low - (1.0 - balance) * high;
}

fn keylight(source: vec4<f32>) -> vec4<f32> {
  if (source.a <= 0.0) {
    return vec4<f32>(0.0);
  }
  let screen = clamp(
    vec3<f32>(params.screen_r, params.screen_g, params.screen_b),
    vec3<f32>(0.0),
    vec3<f32>(1.0),
  );
  let primary = dominant(screen);
  let balance = clamp(params.screen_balance / 100.0, 0.0, 1.0);
  let weights = difference_weights(screen, primary, balance);
  let screen_difference = dot(weights, screen);
  let white = min(params.clip_white / 100.0, 1.0);
  if (dot(primary, primary) < 0.5 || screen_difference <= MIN_SCREEN_DIFFERENCE || white <= 0.0) {
    return source;
  }
  let color = clamp(source.rgb / source.a, vec3<f32>(0.0), vec3<f32>(1.0));
  if (dot(dominant(color), primary) < 0.5) {
    return source;
  }
  let gain = max(params.screen_gain, 0.0) / 100.0;
  let raw = clamp(1.0 - gain * dot(weights, color) / screen_difference, 0.0, 1.0);
  let clipped = clamp(raw / white, 0.0, 1.0);
  let subtracted = clamp(color - (1.0 - raw) * screen, vec3<f32>(0.0), vec3<f32>(raw));
  let replaced = subtracted + vec3<f32>((clipped - raw) * dot(color, LUMA));
  let keyed = clamp(replaced, vec3<f32>(0.0), vec3<f32>(clipped));
  return vec4<f32>(source.a * keyed, source.a * clipped);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
  let keyed = keylight(textureSample(t_texture, s_sampler, input.tex_coords));
  // Premultiplied composite contract: RGB never exceeds alpha.
  return vec4<f32>(clamp(keyed.rgb, vec3<f32>(0.0), vec3<f32>(keyed.a)), keyed.a);
}
