// Crisp caret, no effects. Still runs on the GPU.
fn cursor_fx(p: vec2<f32>) -> vec4<f32> {
    let d = sd_cursor(p);
    let a = smoothstep(0.5, -0.5, d) * fx.state.x * fx.color.a;
    return vec4(fx.color.rgb * a, a);
}
