// Carbon cursor shader prelude. Your shader only has to define:
//
//     fn cursor_fx(p: vec2<f32>) -> vec4<f32>
//
// `p` is the pixel position in points, relative to the top-left of the editor viewport.
// Return a *premultiplied* RGBA colour. Available helpers / data:
//
//   fx.rect     vec4  x, y, w, h of the real caret
//   fx.color    vec4  caret colour (straight alpha)
//   fx.view     vec4  viewport w, h, time (s), seconds since the caret last moved
//   fx.state    vec4  blink (0/1), shape (0 bar, 1 block, 2 underline), points->px scale, travel distance
//   sd_cursor(p)  signed distance (points) to the caret rect
//   sd_trail(p)   signed distance to the animated trail quad (the caret smeared between old and new position)
//   fx.c01 / fx.c23  the four animated trail corners (TL, TR, BR, BL)

struct Fx {
    rect: vec4<f32>,
    c01: vec4<f32>,
    c23: vec4<f32>,
    color: vec4<f32>,
    view: vec4<f32>,
    state: vec4<f32>,
};
@group(0) @binding(0) var<uniform> fx: Fx;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) p: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VOut {
    var q = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let u = q[i];
    var o: VOut;
    o.pos = vec4(u.x * 2.0 - 1.0, 1.0 - u.y * 2.0, 0.0, 1.0);
    o.p = u * fx.view.xy;
    return o;
}

fn sd_cursor(p: vec2<f32>) -> f32 {
    let c = fx.rect.xy + fx.rect.zw * 0.5;
    let q = abs(p - c) - fx.rect.zw * 0.5;
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0);
}

fn sd_trail(p: vec2<f32>) -> f32 {
    var v = array<vec2<f32>, 4>(fx.c01.xy, fx.c01.zw, fx.c23.xy, fx.c23.zw);
    var d = dot(p - v[0], p - v[0]);
    var s = 1.0;
    for (var i = 0; i < 4; i = i + 1) {
        let j = (i + 3) % 4;
        let e = v[j] - v[i];
        let w = p - v[i];
        let b = w - e * clamp(dot(w, e) / max(dot(e, e), 1e-6), 0.0, 1.0);
        d = min(d, dot(b, b));
        let c1 = p.y >= v[i].y;
        let c2 = p.y < v[j].y;
        let c3 = e.x * w.y > e.y * w.x;
        if ((c1 && c2 && c3) || (!c1 && !c2 && !c3)) { s = -s; }
    }
    return s * sqrt(d);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return cursor_fx(in.p);
}

// ---- end of prelude ----
