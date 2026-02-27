#import bevy_ui::ui_vertex_output::UiVertexOutput
#import bevy_ui_render::color_space::hsv_to_linear_rgb
#import editor_feathers::color_picker_common::rounded_box_sdf

struct HsvRectUniforms {
    hue: f32,
    border_radius: f32,
}

@group(1) @binding(0)
var<uniform> uniforms: HsvRectUniforms;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let hue = uniforms.hue / 360.0;
    let saturation = in.uv.x;
    let brightness = 1.0 - in.uv.y;
    let rgb = hsv_to_linear_rgb(vec3(hue, saturation, brightness));

    let pixel_pos = (in.uv - 0.5) * in.size;
    let half_size = in.size * 0.5;
    let d = rounded_box_sdf(pixel_pos, half_size, uniforms.border_radius);
    let alpha = 1.0 - smoothstep(-1.0, 1.0, d);

    return vec4<f32>(rgb, alpha);
}
