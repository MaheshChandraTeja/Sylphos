struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) texture_kind: f32,
};

@group(0) @binding(0)
var font_atlas: texture_2d<f32>;

@group(0) @binding(1)
var media_sampler: sampler;

@group(0) @binding(2)
var image_atlas: texture_2d<f32>;

@group(0) @binding(3)
var image_sampler: sampler;

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) texture_kind: f32,
) -> VertexOut {
    var out: VertexOut;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.color = color;
    out.uv = uv;
    out.texture_kind = texture_kind;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    if (in.texture_kind < 0.5) {
        return in.color;
    }

    if (in.texture_kind < 1.5) {
        let alpha = textureSample(font_atlas, media_sampler, in.uv).r;
        return vec4<f32>(in.color.rgb, in.color.a * alpha);
    }

    let pixel = textureSample(image_atlas, image_sampler, in.uv);
    return vec4<f32>(pixel.rgb * in.color.rgb, pixel.a * in.color.a);
}
