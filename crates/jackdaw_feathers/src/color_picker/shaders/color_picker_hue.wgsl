#import bevy_ui::ui_vertex_output::UiVertexOutput
#import bevy_ui_render::color_space::hsv_to_linear_rgb
#import editor_feathers::color_picker_common::rounded_box_sdf

@group(1) @binding(0)
var<uniform> border_radius: f32;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let hue = in.uv.x;
    let rgb = hsv_to_linear_rgb(vec3(hue, 1.0, 1.0));

    let pixel_pos = (in.uv - 0.5) * in.size;
    let half_size = in.size * 0.5;
    let d = rounded_box_sdf(pixel_pos, half_size, border_radius);
    let alpha = 1.0 - smoothstep(-1.0, 1.0, d);

    return vec4<f32>(rgb, alpha);
}
