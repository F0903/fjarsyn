struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct Uniforms {
    ndc_min: vec2<f32>,
    ndc_max: vec2<f32>,
}

@group(0) @binding(2) var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var pos = array<vec2<f32>, 4>(
        vec2<f32>(uniforms.ndc_min.x, uniforms.ndc_min.y),
        vec2<f32>(uniforms.ndc_max.x, uniforms.ndc_min.y),
        vec2<f32>(uniforms.ndc_min.x, uniforms.ndc_max.y),
        vec2<f32>(uniforms.ndc_max.x, uniforms.ndc_max.y)
    );
    var uv = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0)
    );

    var output: VertexOutput;
    output.position = vec4<f32>(pos[vertex_index], 0.0, 1.0);
    output.uv = uv[vertex_index];
    return output;
}

@group(0) @binding(0) var t_diffuse: texture_2d<f32>;
@group(0) @binding(1) var s_diffuse: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_diffuse, s_diffuse, in.uv);
}
