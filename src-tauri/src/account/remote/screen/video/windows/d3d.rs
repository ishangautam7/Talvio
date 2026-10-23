//! The Direct3D 11 device everything shares, and the colour conversion: the
//! captured BGRA picture, scaled and converted to NV12 on the GPU by the
//! video processor (`ID3D11VideoProcessor`), ready for the encoder.
//!
//! The GPU does the scaling and the conversion for both encoders: a hardware
//! encoder takes the NV12 texture as it is, and the software one gets it read
//! back, which moves 1.5 bytes a pixel instead of 4 and leaves no per-pixel
//! work on the CPU.

use std::mem::ManuallyDrop;

use windows::core::{Interface, Result};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Win32::Foundation::{HMODULE, LUID, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
    ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView, D3D11_BIND_RENDER_TARGET,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_VIDEO_ENCODER, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_VIDEO_COLOR, D3D11_VIDEO_COLOR_0,
    D3D11_VIDEO_COLOR_RGBA, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
    D3D11_VIDEO_PROCESSOR_COLOR_SPACE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_OPTIMAL_SPEED, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;

use super::pacing::fit_rect;

/// How many NV12 pictures the converter rotates through. The encoder is
/// waited for before the next picture is converted, so two would do; a third
/// covers a hardware encoder that answers late.
const RING: usize = 3;

/// Output colour space: YCbCr BT.709, studio range (16-235), which is what
/// every H.264 decoder assumes when the stream does not say. The fields are a
/// C bitfield: Usage (bit 0), RGB_Range (bit 1), YCbCr_Matrix (bit 2: 1 =
/// BT.709), YCbCr_xvYCC (bit 3), Nominal_Range (bits 4-5: 1 = 16-235).
const OUTPUT_COLOR_SPACE: u32 = (1 << 2) | (1 << 4);
/// Input colour space: full-range RGB (RGB_Range 0), as the desktop is.
const INPUT_COLOR_SPACE: u32 = 0;

/// The device, its immediate context, and the views of it the capture and the
/// video processor need. Created once per stream, on the stream's thread.
pub struct Gpu {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    /// The same device, as Windows.Graphics.Capture wants it.
    pub winrt: IDirect3DDevice,
    /// Which adapter it runs on, to pick a hardware encoder on the same one.
    pub adapter_luid: LUID,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
}

impl Gpu {
    pub fn new() -> Result<Self> {
        let mut device = None;
        let mut context = None;
        // SAFETY: out-pointers are live locals; no adapter means the default
        // one, and the feature levels default to what the driver supports.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device: ID3D11Device = device.ok_or_else(windows::core::Error::empty)?;
        let context: ID3D11DeviceContext = context.ok_or_else(windows::core::Error::empty)?;
        // The capture's thread pool and a hardware encoder's own threads use
        // this device too; without this, two of them in the immediate context
        // at once corrupt it.
        let multithread: ID3D11Multithread = device.cast()?;
        // SAFETY: a plain flag on a live device.
        unsafe {
            let _ = multithread.SetMultithreadProtected(true);
        }
        let dxgi: IDXGIDevice = device.cast()?;
        // SAFETY: `dxgi` is a live device; the descriptor is returned by value.
        let adapter_luid = unsafe { dxgi.GetAdapter()?.GetDesc()?.AdapterLuid };
        // SAFETY: as above; the result is the same device behind a WinRT
        // interface.
        let winrt: IDirect3DDevice =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        let video_device: ID3D11VideoDevice = device.cast()?;
        let video_context: ID3D11VideoContext = context.cast()?;
        Ok(Self {
            device,
            context,
            winrt,
            adapter_luid,
            video_device,
            video_context,
        })
    }

    /// A 2D texture of `format`, usable as a render target (and, for
    /// `extra`, more).
    pub fn texture(
        &self,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
        bind: u32,
    ) -> Result<ID3D11Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: bind,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture = None;
        // SAFETY: the descriptor and the out-pointer are live locals.
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut texture))?
        };
        texture.ok_or_else(windows::core::Error::empty)
    }
}

/// The size of a texture.
pub fn texture_size(texture: &ID3D11Texture2D) -> (u32, u32) {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: fills a live local.
    unsafe { texture.GetDesc(&mut desc) };
    (desc.Width, desc.Height)
}

/// Where the captured picture is kept between captures: a copy of the last
/// frame, so the capture's buffer goes straight back to it and the picture
/// can be encoded again (keepalive, a requested keyframe) at any time.
pub struct Source {
    pub texture: ID3D11Texture2D,
    pub width: u32,
    pub height: u32,
    /// The part of the texture the window or screen fills (the capture's
    /// buffer can be larger than the content while a window grows).
    pub content: (u32, u32),
}

impl Source {
    /// A black picture, for a stream that has not captured anything yet (a
    /// window minimized from the start). A video processor stream without an
    /// input surface hung the AMD driver, so black is drawn from a texture.
    pub fn black(gpu: &Gpu) -> Result<Self> {
        let source = Self::new(gpu, 64, 64)?;
        let mut view = None;
        // SAFETY: the texture is a live render target; the view is created
        // and used right here.
        unsafe {
            gpu.device
                .CreateRenderTargetView(&source.texture, None, Some(&mut view))?;
            let view = view.ok_or_else(windows::core::Error::empty)?;
            gpu.context
                .ClearRenderTargetView(&view, &[0.0, 0.0, 0.0, 1.0]);
        }
        Ok(source)
    }

    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Result<Self> {
        let texture = gpu.texture(
            width,
            height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        )?;
        Ok(Self {
            texture,
            width,
            height,
            content: (width, height),
        })
    }
}

struct Processor {
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    input_size: (u32, u32),
}

struct Target {
    texture: ID3D11Texture2D,
    view: ID3D11VideoProcessorOutputView,
}

/// BGRA in, NV12 out at the encoder's size.
pub struct Converter {
    size: (u32, u32),
    processor: Option<Processor>,
    input: Option<(ID3D11Texture2D, ID3D11VideoProcessorInputView)>,
    targets: Vec<Target>,
    next: usize,
}

impl Converter {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            size: (width, height),
            processor: None,
            input: None,
            targets: Vec::new(),
            next: 0,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The video processor for `input_size` in, `self.size` out; made again
    /// when either changes, and with it every view, since views belong to
    /// the enumerator they were made with.
    fn processor(&mut self, gpu: &Gpu, input_size: (u32, u32)) -> Result<()> {
        if self
            .processor
            .as_ref()
            .is_some_and(|p| p.input_size == input_size)
        {
            return Ok(());
        }
        self.processor = None;
        self.input = None;
        self.targets.clear();
        let rate = DXGI_RATIONAL {
            Numerator: 30,
            Denominator: 1,
        };
        let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: rate,
            InputWidth: input_size.0,
            InputHeight: input_size.1,
            OutputFrameRate: rate,
            OutputWidth: self.size.0,
            OutputHeight: self.size.1,
            Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
        };
        // SAFETY: the descriptor is a live local; the results are owned COM
        // pointers.
        let (enumerator, processor) = unsafe {
            let enumerator = gpu.video_device.CreateVideoProcessorEnumerator(&desc)?;
            let processor = gpu.video_device.CreateVideoProcessor(&enumerator, 0)?;
            (enumerator, processor)
        };
        let vc = &gpu.video_context;
        let black = D3D11_VIDEO_COLOR {
            Anonymous: D3D11_VIDEO_COLOR_0 {
                RGBA: D3D11_VIDEO_COLOR_RGBA {
                    R: 0.0,
                    G: 0.0,
                    B: 0.0,
                    A: 1.0,
                },
            },
        };
        let input_space = D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
            _bitfield: INPUT_COLOR_SPACE,
        };
        let output_space = D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
            _bitfield: OUTPUT_COLOR_SPACE,
        };
        // SAFETY: state setters on the processor just made; every pointer is
        // a live local.
        unsafe {
            vc.VideoProcessorSetStreamFrameFormat(
                &processor,
                0,
                D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            );
            vc.VideoProcessorSetStreamColorSpace(&processor, 0, &input_space);
            vc.VideoProcessorSetOutputColorSpace(&processor, &output_space);
            vc.VideoProcessorSetOutputBackgroundColor(&processor, false, &black);
            // No denoising, edge enhancement or the like: a screen is not a
            // camera, and drivers that "improve" text make it worse.
            vc.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
        }
        self.processor = Some(Processor {
            enumerator,
            processor,
            input_size,
        });
        Ok(())
    }

    fn targets(&mut self, gpu: &Gpu) -> Result<()> {
        if !self.targets.is_empty() {
            return Ok(());
        }
        let Some(processor) = &self.processor else {
            return Ok(());
        };
        for _ in 0..RING {
            // A hardware encoder may want to read the texture directly; the
            // video-encoder bind flag says so, and drivers that do not know it
            // refuse it, so try without.
            let bind = D3D11_BIND_RENDER_TARGET.0 as u32;
            let texture = gpu
                .texture(
                    self.size.0,
                    self.size.1,
                    DXGI_FORMAT_NV12,
                    bind | D3D11_BIND_VIDEO_ENCODER.0 as u32,
                )
                .or_else(|_| gpu.texture(self.size.0, self.size.1, DXGI_FORMAT_NV12, bind))?;
            let desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                },
            };
            let mut view = None;
            // SAFETY: texture, enumerator and descriptor are live.
            unsafe {
                gpu.video_device.CreateVideoProcessorOutputView(
                    &texture,
                    &processor.enumerator,
                    &desc,
                    Some(&mut view),
                )?;
            }
            let view = view.ok_or_else(windows::core::Error::empty)?;
            self.targets.push(Target { texture, view });
        }
        Ok(())
    }

    fn input_view(
        &mut self,
        gpu: &Gpu,
        texture: &ID3D11Texture2D,
    ) -> Result<ID3D11VideoProcessorInputView> {
        if let Some((cached, view)) = &self.input {
            if cached == texture {
                return Ok(view.clone());
            }
        }
        let Some(processor) = &self.processor else {
            return Err(windows::core::Error::empty());
        };
        let desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut view = None;
        // SAFETY: texture, enumerator and descriptor are live.
        unsafe {
            gpu.video_device.CreateVideoProcessorInputView(
                texture,
                &processor.enumerator,
                &desc,
                Some(&mut view),
            )?;
        }
        let view = view.ok_or_else(windows::core::Error::empty)?;
        self.input = Some((texture.clone(), view.clone()));
        Ok(view)
    }

    /// Draw `source` into the next NV12 texture and return it. The picture
    /// keeps its aspect: when it does not match the output (a window
    /// mid-resize), it is letterboxed.
    pub fn convert(&mut self, gpu: &Gpu, source: &Source) -> Result<ID3D11Texture2D> {
        self.processor(gpu, (source.width, source.height))?;
        self.targets(gpu)?;
        let input = self.input_view(gpu, &source.texture)?;
        let Some(processor) = &self.processor else {
            return Err(windows::core::Error::empty());
        };
        let index = self.next % self.targets.len().max(1);
        self.next = self.next.wrapping_add(1);
        let Some(target) = self.targets.get(index) else {
            return Err(windows::core::Error::empty());
        };
        let (out_w, out_h) = self.size;
        let (content_w, content_h) = (
            source.content.0.min(source.width),
            source.content.1.min(source.height),
        );
        let (l, t, r, b) = fit_rect(content_w, content_h, out_w, out_h);
        let src_rect = RECT {
            left: 0,
            top: 0,
            right: content_w as i32,
            bottom: content_h as i32,
        };
        let dst_rect = RECT {
            left: l as i32,
            top: t as i32,
            right: r as i32,
            bottom: b as i32,
        };
        let full = RECT {
            left: 0,
            top: 0,
            right: out_w as i32,
            bottom: out_h as i32,
        };
        let vc = &gpu.video_context;
        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: std::ptr::null_mut(),
            pInputSurface: ManuallyDrop::new(Some(input)),
            ppFutureSurfaces: std::ptr::null_mut(),
            ppPastSurfacesRight: std::ptr::null_mut(),
            pInputSurfaceRight: ManuallyDrop::new(None),
            ppFutureSurfacesRight: std::ptr::null_mut(),
        };
        let mut streams = [stream];
        // SAFETY: the processor, views and rectangles are live; the stream
        // array lives across the call and its view is released right after.
        let result = unsafe {
            vc.VideoProcessorSetStreamSourceRect(&processor.processor, 0, true, Some(&src_rect));
            vc.VideoProcessorSetStreamDestRect(&processor.processor, 0, true, Some(&dst_rect));
            vc.VideoProcessorSetOutputTargetRect(&processor.processor, true, Some(&full));
            let result = vc.VideoProcessorBlt(&processor.processor, &target.view, 0, &streams);
            ManuallyDrop::drop(&mut streams[0].pInputSurface);
            result
        };
        result?;
        Ok(target.texture.clone())
    }
}
