// Neon glow around a stationary caret (no trail).
fn cursor_fx(p: vec2<f32>) -> vec4<f32> {
    let d = sd_cursor(p);
    let body = smoothstep(0.6, -0.4, d);
    let halo = 0.55 * exp(-max(d, 0.0) / 7.0);
    let core = vec3(1.0) * body * 0.35;
    let a = clamp(body + halo, 0.0, 1.0) * fx.state.x * fx.color.a;
    return vec4(fx.color.rgb * a + core * fx.state.x, a);
}
