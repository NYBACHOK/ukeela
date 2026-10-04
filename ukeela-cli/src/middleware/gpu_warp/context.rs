use std::sync::mpsc;

use anyhow::{Context, ensure};
use tracing::{info, warn};

#[derive(Debug)]
/// wgpu resources shared by affine frame-warp dispatches.
pub struct GpuWarpContext {
    _instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
}

#[derive(Debug)]
pub struct GpuCapabilities {
    pub backend: wgpu::Backend,
    pub name: String,
    pub vendor: u32,
    pub device_type: wgpu::DeviceType,
    pub supports_compute: bool,
    pub max_texture_size: u32,
}

impl GpuWarpContext {
    /// Creates a compute context, preferring Vulkan and retrying other backends if needed.
    ///
    /// # Errors
    ///
    /// Returns an error when no adapter or usable device can be created.
    pub async fn new(prefer_vulkan: bool) -> anyhow::Result<Self> {
        let backends = if prefer_vulkan {
            wgpu::Backends::VULKAN
        } else {
            wgpu::Backends::all()
        };
        let mut instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        info!(?backends, "Creating wgpu warp context");

        let adapter_options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        };
        let adapter = match instance.request_adapter(&adapter_options).await {
            Ok(adapter) => adapter,
            Err(error) if prefer_vulkan => {
                warn!(%error, "Vulkan adapter unavailable; trying other GPU backends");
                instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                    backends: wgpu::Backends::all(),
                    ..wgpu::InstanceDescriptor::new_without_display_handle()
                });
                instance
                    .request_adapter(&adapter_options)
                    .await
                    .context("Failed to find a GPU adapter using any backend")?
            }
            Err(error) => return Err(error).context("Failed to find a GPU adapter"),
        };
        let adapter_info = adapter.get_info();
        if adapter_info.backend == wgpu::Backend::Vulkan {
            info!(adapter = %adapter_info.name, "Vulkan backend selected for GPU warp");
        } else {
            warn!(
                adapter = %adapter_info.name,
                backend = ?adapter_info.backend,
                "Using a non-Vulkan backend for GPU warp"
            );
        }

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Stabilization Warp Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                memory_hints: wgpu::MemoryHints::Performance,
                ..Default::default()
            })
            .await
            .context("Failed to create the wgpu device")?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Affine Warp Compute Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("warp_shader.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Affine Warp Compute Pipeline"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Ok(Self {
            _instance: instance,
            adapter,
            device,
            queue,
            pipeline,
        })
    }

    /// Returns the selected adapter and device limits.
    #[must_use]
    pub fn capabilities(&self) -> GpuCapabilities {
        let info = self.adapter.get_info();
        GpuCapabilities {
            backend: info.backend,
            name: info.name,
            vendor: info.vendor,
            device_type: info.device_type,
            supports_compute: true,
            max_texture_size: self.device.limits().max_texture_dimension_2d,
        }
    }

    /// Warps tightly packed BGRA pixels; `transform` maps output coordinates to source coordinates.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid dimensions, mismatched input data, or failed GPU readback.
    pub fn warp_bgra(
        &self,
        pixels: &[u8],
        width: u32,
        height: u32,
        transform: [f32; 6],
    ) -> anyhow::Result<Vec<u8>> {
        let limits = self.device.limits();
        ensure!(width > 0 && height > 0, "Frame dimensions must be non-zero");
        ensure!(
            width <= limits.max_texture_dimension_2d && height <= limits.max_texture_dimension_2d,
            "Frame dimensions exceed the GPU texture-size limit"
        );

        let unpadded_bytes_per_row = width.checked_mul(4).context("Frame row size overflows")?;
        let input_size = usize::try_from(unpadded_bytes_per_row)
            .ok()
            .and_then(|row| row.checked_mul(usize::try_from(height).ok()?))
            .context("Frame dimensions overflow")?;
        ensure!(
            pixels.len() == input_size,
            "BGRA frame buffer does not match its dimensions"
        );

        let input_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Warp Input Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let output_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Warp Output Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &input_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(unpadded_bytes_per_row),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let params = [
            width as f32,
            height as f32,
            width as f32,
            height as f32,
            transform[0],
            transform[1],
            transform[2],
            transform[3],
            transform[4],
            transform[5],
            0.0,
            0.0,
        ];
        let uniform_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Warp Parameters"),
            size: std::mem::size_of_val(&params) as u64,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: true,
        });
        {
            let mut mapped = uniform_buffer
                .slice(..)
                .get_mapped_range_mut()
                .context("Failed to map warp parameters")?;
            let parameter_bytes = params
                .iter()
                .flat_map(|value| value.to_ne_bytes())
                .collect::<Vec<_>>();
            mapped.copy_from_slice(&parameter_bytes);
        }
        uniform_buffer.unmap();

        let input_view = input_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group_layout = self.pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Affine Warp Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&output_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let padded_bytes_per_row = unpadded_bytes_per_row
            .checked_add(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT - 1)
            .context("Padded frame row size overflows")?
            / wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let output_buffer_size = u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(height))
            .context("GPU readback buffer size overflows")?;
        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Warp Output Readback"),
            size: output_buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Affine Warp Commands"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Affine Warp Compute Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(width.div_ceil(16), height.div_ceil(16), 1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &output_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let output_slice = output_buffer.slice(..);
        let (sender, receiver) = mpsc::channel();
        output_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .context("GPU polling failed")?;
        receiver
            .recv()
            .context("GPU readback callback was dropped")?
            .context("Failed to map GPU warp output")?;

        let mapped = output_slice
            .get_mapped_range()
            .context("Failed to access mapped GPU warp output")?;
        let row_size = usize::try_from(unpadded_bytes_per_row)
            .context("Frame row size exceeds host limits")?;
        let padded_row_size =
            usize::try_from(padded_bytes_per_row).context("Padded row size exceeds host limits")?;
        let mut output = Vec::with_capacity(input_size);
        for row in mapped.chunks_exact(padded_row_size).take(height as usize) {
            output.extend_from_slice(&row[..row_size]);
        }
        drop(mapped);
        output_buffer.unmap();
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::GpuWarpContext;

    #[tokio::test]
    async fn identity_warp_preserves_pixels_and_padded_readback_rows() {
        let context = match GpuWarpContext::new(true).await {
            Ok(context) => context,
            Err(error) => {
                eprintln!("Skipping GPU warp test because no adapter is available: {error}");
                return;
            }
        };
        let width = 9;
        let height = 7;
        let pixels = (0..width * height * 4)
            .map(|index| (index * 37) as u8)
            .collect::<Vec<_>>();

        let warped = context
            .warp_bgra(&pixels, width, height, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0])
            .expect("identity GPU warp should succeed");

        assert_eq!(warped, pixels);

        let pixels = [0, 0, 0, 255, 100, 100, 100, 255, 200, 200, 200, 255];
        let warped = context
            .warp_bgra(&pixels, 3, 1, [1.0, 0.0, 0.5, 0.0, 1.0, 0.0])
            .expect("translated GPU warp should succeed");
        assert_eq!(
            warped,
            [50, 50, 50, 255, 150, 150, 150, 255, 200, 200, 200, 255]
        );
    }
}
