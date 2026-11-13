struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) border_color: vec4<f32>,
    @location(4) parameters: vec4<f32>,
}

@vertex
fn vertex(
    @location(0) position: vec2<f32>,
    @location(1) local: vec2<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) border_color: vec4<f32>,
    @location(5) parameters: vec4<f32>,
) -> VertexOutput {
    return VertexOutput(vec4<f32>(position, 0.0, 1.0), local, half_size, color, border_color, parameters);
}

fn distance_to_round_rect(point: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let delta = abs(point) - half_size + vec2<f32>(radius);
    return length(max(delta, vec2<f32>(0.0))) + min(max(delta.x, delta.y), 0.0) - radius;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let radius = input.parameters.x;
    let border = input.parameters.y;
    let blur = input.parameters.z;
    let distance = distance_to_round_rect(input.local, input.half_size, radius);
    let aa = max(fwidth(distance), 0.0001) * 0.5;
    let feather = max(blur, aa);
    let outer = 1.0 - smoothstep(-feather, feather, distance);
    let inner_size = max(input.half_size - vec2<f32>(border), vec2<f32>(0.0));
    let inner_distance = distance_to_round_rect(input.local, inner_size, max(radius - border, 0.0));
    var inner = 1.0 - smoothstep(-aa, aa, inner_distance);
    if border == 0.0 { inner = outer; }
    if min(inner_size.x, inner_size.y) <= 0.0 { inner = 0.0; }
    let fill_alpha = input.color.a * inner;
    let border_alpha = input.border_color.a * max(outer - inner, 0.0);
    let alpha = fill_alpha + border_alpha;
    let premultiplied = input.color.rgb * fill_alpha + input.border_color.rgb * border_alpha;
    return vec4<f32>(premultiplied / max(alpha, 0.00001), alpha);
}
