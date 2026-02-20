#import bevy_ui::ui_vertex_output::UiVertexOutput
#import bevy_ui_render::color_space::srgb_to_linear_rgb
#import editor_feathers::color_picker_common::{checkerboard, rounded_box_sdf}

struct AlphaSliderUniforms {
    color: vec4<f32>,
    checkerboard_size: f32,
    border_radius: f32,
}

@group(1) @binding(0)
var<uniform> uniforms: AlphaSliderUniforms;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let checker_color_light = vec3<f32>(1.0, 1.0, 1.0);
    let checker_color_dark = srgb_to_linear_rgb(vec3<f32>(0.8, 0.8, 0.8));

    let cell_count = in.size.y / uniforms.checkerboard_size;
    let checker = checkerboard(in.uv * vec2(in.size.x / in.size.y, 1.0), cell_count);
    let checker_rgb = mix(checker_color_dark, checker_color_light, checker);

    let alpha = in.uv.x;
    let color_rgb = srgb_to_linear_rgb(uniforms.color.rgb);
    let final_rgb = mix(checker_rgb, color_rgb, alpha);

    let pixel_pos = (in.uv - 0.5) * in.size;
    let half_size = in.size * 0.5;
    let d = rounded_box_sdf(pixel_pos, half_size, uniforms.border_radius);
    let mask_alpha = 1.0 - smoothstep(-1.0, 1.0, d);

    return vec4<f32>(final_rgb, mask_alpha);
}
