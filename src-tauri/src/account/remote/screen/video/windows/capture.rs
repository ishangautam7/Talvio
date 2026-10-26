//! Windows.Graphics.Capture: one window or monitor into a free-threaded frame
//! pool on our Direct3D device.
//!
//! Free-threaded, so no dispatcher queue: the pool raises `FrameArrived` on a
//! system thread, and the handler only wakes the stream's own thread, which
//! takes the frame when the pacing allows. Until then the pool's two buffers
//! stay full and Windows stops composing new frames for us, so a picture that
//! changes faster than the frame rate costs nothing extra.
//!
//! The yellow capture border is left on (`IsBorderRequired` is never
//! touched): it is how the person at the computer sees that a phone is
//! watching.

use log::debug;
use windows::core::{factory, IInspectable, Interface, Result};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

use super::d3d::{texture_size, Gpu};

/// Two buffers: one being composed into while we read the other.
const BUFFERS: i32 = 2;
const FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;

pub fn is_supported() -> bool {
    GraphicsCaptureSession::IsSupported().unwrap_or(false)
}

pub fn item_for_window(hwnd: HWND) -> Result<GraphicsCaptureItem> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    // SAFETY: the interop factory takes any handle; a bad one fails the call.
    unsafe { interop.CreateForWindow(hwnd) }
}

pub fn item_for_monitor(monitor: HMONITOR) -> Result<GraphicsCaptureItem> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    // SAFETY: as above.
    unsafe { interop.CreateForMonitor(monitor) }
}

/// The newest frame: its texture (owned by the pool until `_frame` drops)
/// and the size of what it shows.
pub struct Frame {
    pub texture: ID3D11Texture2D,
    pub content: (u32, u32),
    _frame: Direct3D11CaptureFrame,
}

pub struct Capture {
    item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    frame_token: i64,
    closed_token: i64,
    pool_size: SizeInt32,
}

impl Capture {
    /// Start capturing `item`. `on_frame` runs (on a system thread) whenever
    /// a frame is ready, `on_closed` when the window closes or the monitor
    /// goes away.
    pub fn start(
        gpu: &Gpu,
        item: GraphicsCaptureItem,
        cursor: bool,
        max_fps: u32,
        on_frame: impl Fn() + Send + 'static,
        on_closed: impl Fn() + Send + 'static,
    ) -> Result<Self> {
        let size = item.Size()?;
        let pool_size = SizeInt32 {
            Width: size.Width.max(1),
            Height: size.Height.max(1),
        };
        let pool =
            Direct3D11CaptureFramePool::CreateFreeThreaded(&gpu.winrt, FORMAT, BUFFERS, pool_size)?;
        let frame_token = pool.FrameArrived(&TypedEventHandler::<
            Direct3D11CaptureFramePool,
            IInspectable,
        >::new(move |_, _| {
            on_frame();
            Ok(())
        }))?;
        let closed_token = item.Closed(
            &TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new(move |_, _| {
                on_closed();
                Ok(())
            }),
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        session.SetIsCursorCaptureEnabled(cursor)?;
        // Windows 11 24H2 and later: let Windows itself skip frames we would
        // drop anyway, and include the window's menus and pop-ups, which a
        // phone needs to see to use them. Older builds lack both; that is fine.
        // Half the frame interval: Windows rounds the interval up to whole
        // display refreshes, so the full interval delivered ~24 frames a
        // second for a 30 fps stream. The pacing does the rest.
        let interval = 10_000_000 / i64::from(max_fps.clamp(1, 240)) / 2;
        if let Err(err) = session.SetMinUpdateInterval(TimeSpan { Duration: interval }) {
            debug!("live view: no MinUpdateInterval on this Windows ({err})");
        }
        if let Err(err) = session.SetIncludeSecondaryWindows(true) {
            debug!("live view: no IncludeSecondaryWindows on this Windows ({err})");
        }
        session.StartCapture()?;
        Ok(Self {
            item,
            pool,
            session,
            frame_token,
            closed_token,
            pool_size,
        })
    }

    /// The newest frame in the pool, or `None` when there is none. Older ones
    /// are dropped. When the window has changed size the pool is made again
    /// at the new size, so the next frames carry the whole window.
    pub fn latest(&mut self, gpu: &Gpu) -> Result<Option<Frame>> {
        let mut newest = None;
        loop {
            match self.pool.TryGetNextFrame() {
                Ok(frame) => newest = Some(frame),
                // An empty pool answers null, which arrives as an empty error.
                Err(err) if err.code().is_ok() => break,
                Err(err) => return Err(err),
            }
        }
        let Some(frame) = newest else {
            return Ok(None);
        };
        let content = frame.ContentSize()?;
        if content.Width > 0
            && content.Height > 0
            && (content.Width != self.pool_size.Width || content.Height != self.pool_size.Height)
        {
            debug!(
                "live view: capture size {}x{} -> {}x{}",
                self.pool_size.Width, self.pool_size.Height, content.Width, content.Height
            );
            self.pool_size = content;
            self.pool.Recreate(&gpu.winrt, FORMAT, BUFFERS, content)?;
        }
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        // SAFETY: the surface is a live DXGI-backed WinRT surface; asking it
        // for its texture adds a reference we own.
        let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
        let (width, height) = texture_size(&texture);
        let content = (
            (content.Width.max(1) as u32).min(width),
            (content.Height.max(1) as u32).min(height),
        );
        Ok(Some(Frame {
            texture,
            content,
            _frame: frame,
        }))
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // Closing the session is what takes the border away; the pool and
        // the handlers go with it, so nothing calls into a dead stream.
        let _ = self.pool.RemoveFrameArrived(self.frame_token);
        let _ = self.item.RemoveClosed(self.closed_token);
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}
