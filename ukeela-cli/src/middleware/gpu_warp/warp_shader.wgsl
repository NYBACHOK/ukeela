struct WarpParams {
    input_width: f32,
    input_height: f32,
    output_width: f32,
    output_height: f32,
    transform_a00: f32,
    transform_a01: f32,
    transform_a02: f32,
    transform_a10: f32,
    transform_a11: f32,
    transform_a12: f32,
    padding0: f32,
    padding1: f32,
};

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var output_texture: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> params: WarpParams;

fn reflect_index(index: i32, extent: i32) -> i32 {
    let period = extent * 2;
    let wrapped = ((index % period) + period) % period;
    if wrapped >= extent {
        return period - wrapped - 1;
    }
    return wrapped;
}

fn reflected_load(x: i32, y: i32) -> vec4<f32> {
    let width = i32(params.input_width);
    let height = i32(params.input_height);
    return textureLoad(
        input_texture,
        vec2i(reflect_index(x, width), reflect_index(y, height)),
        0
    );
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = i32(global_id.x);
    let y = i32(global_id.y);
    if x >= i32(params.output_width) || y >= i32(params.output_height) {
        return;
    }

    let dst = vec2f(f32(x), f32(y));
    let src_x = params.transform_a00 * dst.x
        + params.transform_a01 * dst.y
        + params.transform_a02;
    let src_y = params.transform_a10 * dst.x
        + params.transform_a11 * dst.y
        + params.transform_a12;
    let x0 = i32(floor(src_x));
    let y0 = i32(floor(src_y));
    let fraction = vec2f(src_x - floor(src_x), src_y - floor(src_y));

    let top = mix(reflected_load(x0, y0), reflected_load(x0 + 1, y0), fraction.x);
    let bottom = mix(
        reflected_load(x0, y0 + 1),
        reflected_load(x0 + 1, y0 + 1),
        fraction.x
    );
    textureStore(output_texture, vec2i(x, y), mix(top, bottom, fraction.y));
}
