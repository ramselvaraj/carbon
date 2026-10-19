// Smooth smeared trail with a soft halo.
fn cursor_fx(p: vec2<f32>) -> vec4<f32> {
    let d = sd_trail(p);
    let body = smoothstep(0.6, -0.4, d);
    let halo = 0.22 * exp(-max(d, 0.0) / 5.0);
    let a = clamp(body + halo, 0.0, 1.0) * fx.state.x * fx.color.a;
    return vec4(fx.color.rgb * a, a);
}
