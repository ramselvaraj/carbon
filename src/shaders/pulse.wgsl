// Trail plus a shockwave ring that expands from the caret every time it jumps.
fn cursor_fx(p: vec2<f32>) -> vec4<f32> {
    let d = sd_trail(p);
    let body = smoothstep(0.6, -0.4, d);

    let t = fx.view.w;                       // seconds since the caret moved
    let life = clamp(t / 0.45, 0.0, 1.0);
    let c = fx.rect.xy + fx.rect.zw * 0.5;
    let r = length(p - c);
    let radius = 4.0 + life * 34.0;
    let ring = exp(-pow((r - radius) / 2.5, 2.0)) * (1.0 - life) * 0.8;

    let a = clamp(body + ring, 0.0, 1.0) * fx.state.x * fx.color.a;
    return vec4(fx.color.rgb * a, a);
}
