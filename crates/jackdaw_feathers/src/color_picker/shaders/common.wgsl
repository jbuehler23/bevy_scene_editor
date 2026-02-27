#define_import_path editor_feathers::color_picker_common

fn checkerboard(uv: vec2<f32>, cell_count: f32) -> f32 {
    let checker = floor(uv * cell_count);
    return (checker.x + checker.y) % 2.0;
}

fn rounded_box_sdf(center: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(center) - half_size + radius;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}
