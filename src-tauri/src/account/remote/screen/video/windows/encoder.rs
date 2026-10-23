//! Media Foundation H.264: a hardware encoder MFT when the GPU has one, the
//! Microsoft software encoder otherwise.
//!
//! Both are set up the same way, for a live view rather than a recording:
//! Constrained Baseline (no B-frames, so a frame is sent the moment it is
//! encoded), low-latency mode (no look-ahead, one picture in, one access unit
//! out), a fixed bitrate that can be changed while running, and keyframes only
//! when asked for, plus one every [`SAFETY_GOP_SECONDS`] in case a request
//! never arrives.
//!
//! The two differ in how they are driven. A hardware MFT is asynchronous: it
//! says when it wants input (`METransformNeedInput`) and when output is ready
//! (`METransformHaveOutput`) through its event queue, and it reads the NV12
//! texture straight from the GPU through a DXGI device manager. The software
//! encoder is synchronous and reads system memory, so the texture is copied to
//! a staging texture and read back.
//!
//! Hardware encoders differ in what they accept, so every setting that is not
//! essential is tried and a refusal only logged; what is essential (the types)
//! makes the encoder fail to configure, and the next candidate is tried.

use std::mem::ManuallyDrop;
use std::time::{Duration, Instant};

use log::{debug, info};
use windows::core::{Interface, GUID, PWSTR};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Media::MediaFoundation::{
    eAVEncCommonRateControlMode_CBR, eAVEncH264VProfile_Base, eAVEncH264VProfile_ConstrainedBase,
    CLSID_MSH264EncoderMFT, CODECAPI_AVEncCommonMeanBitRate, CODECAPI_AVEncCommonRateControlMode,
    CODECAPI_AVEncMPVDefaultBPictureCount, CODECAPI_AVEncMPVGOPSize,
    CODECAPI_AVEncVideoForceKeyFrame, CODECAPI_AVLowLatencyMode, ICodecAPI, IMFActivate,
    IMFAttributes, IMFDXGIDeviceManager, IMFMediaEventGenerator, IMFMediaType, IMFSample,
    IMFShutdown, IMFTransform, METransformHaveOutput, METransformNeedInput,
    MFCreateDXGIDeviceManager, MFCreateDXGISurfaceBuffer, MFCreateMediaType, MFCreateMemoryBuffer,
    MFCreateSample, MFMediaType_Video, MFNominalRange_16_235, MFTEnumEx,
    MFT_FRIENDLY_NAME_Attribute, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive, MFVideoTransferMatrix_BT709, MFT_CATEGORY_VIDEO_ENCODER,
    MFT_ENUM_ADAPTER_LUID, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
    MFT_MESSAGE_NOTIFY_END_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES,
    MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO, MF_EVENT_FLAG_NO_WAIT,
    MF_E_NOTACCEPTING, MF_E_NO_EVENTS_AVAILABLE, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_E_TRANSFORM_STREAM_CHANGE, MF_LOW_LATENCY, MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_MPEG2_PROFILE, MF_MT_MPEG_SEQUENCE_HEADER, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE,
    MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_YUV_MATRIX, MF_SA_D3D11_AWARE, MF_TRANSFORM_ASYNC,
    MF_TRANSFORM_ASYNC_UNLOCK,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Variant::VARIANT;

use super::d3d::Gpu;

/// A keyframe at least this often even when none is requested, so a phone
/// whose request got lost still recovers.
const SAFETY_GOP_SECONDS: u32 = 10;

/// How long a hardware encoder may take to ask for input before it is taken
/// for stuck.
const INPUT_TIMEOUT: Duration = Duration::from_millis(1000);
/// How long to wait for the access unit of the picture just given. Low-latency
/// encoders answer in a few milliseconds; past this, the output is collected
/// with the next picture instead.
const OUTPUT_WAIT: Duration = Duration::from_millis(250);
/// How often a hardware encoder's event queue is looked at while waiting.
const EVENT_POLL: Duration = Duration::from_millis(1);

/// Which encoders to try.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preference {
    /// Hardware first, then software.
    Auto,
    #[cfg_attr(not(test), allow(dead_code))]
    HardwareOnly,
    SoftwareOnly,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
}

/// One access unit out of the encoder.
pub struct Output {
    pub data: Vec<u8>,
    /// Sample time, 100 ns units.
    pub time: i64,
}

type WinResult<T> = windows::core::Result<T>;

pub struct Encoder {
    transform: IMFTransform,
    /// Kept to shut the hardware MFT down properly.
    activate: Option<IMFActivate>,
    codec: Option<ICodecAPI>,
    /// Only for an asynchronous (hardware) MFT.
    events: Option<IMFMediaEventGenerator>,
    need_input: u32,
    /// Set when the MFT reads D3D11 textures itself.
    manager: Option<IMFDXGIDeviceManager>,
    /// Read-back texture, when it does not.
    staging: Option<ID3D11Texture2D>,
    provides_samples: bool,
    output_size: u32,
    config: Config,
    hardware: bool,
    pub name: String,
}

impl Encoder {
    pub fn create(gpu: &Gpu, config: Config, preference: Preference) -> Result<Self, String> {
        let mut why = Vec::new();
        if preference != Preference::SoftwareOnly {
            match Self::hardware(gpu, config) {
                Ok(encoder) => return Ok(encoder),
                Err(err) => why.push(err),
            }
        }
        if preference != Preference::HardwareOnly {
            match Self::software(gpu, config) {
                Ok(encoder) => return Ok(encoder),
                Err(err) => why.push(err),
            }
        }
        Err(why.join("; "))
    }

    pub fn is_hardware(&self) -> bool {
        self.hardware
    }

    fn hardware(gpu: &Gpu, config: Config) -> Result<Self, String> {
        let candidates = enum_hardware().map_err(|e| format!("no hardware encoder list: {e}"))?;
        if candidates.is_empty() {
            return Err("no hardware H.264 encoder".into());
        }
        let mut why = Vec::new();
        for activate in candidates {
            let name = friendly_name(&activate);
            if !same_adapter(&activate, gpu) {
                debug!("live view: skipping {name}: it is on another GPU");
                continue;
            }
            // SAFETY: activating a registered MFT; the result is an owned
            // COM pointer.
            let transform: WinResult<IMFTransform> = unsafe { activate.ActivateObject() };
            let result = transform.map_err(|e| e.to_string()).and_then(|transform| {
                Self::configure(
                    gpu,
                    transform,
                    Some(activate.clone()),
                    config,
                    name.clone(),
                    true,
                )
            });
            match result {
                Ok(encoder) => return Ok(encoder),
                Err(err) => {
                    debug!("live view: {name} would not configure: {err}");
                    // SAFETY: releases what ActivateObject made.
                    unsafe {
                        let _ = activate.ShutdownObject();
                    }
                    why.push(format!("{name}: {err}"));
                }
            }
        }
        Err(if why.is_empty() {
            "no hardware H.264 encoder on this GPU".into()
        } else {
            why.join("; ")
        })
    }

    fn software(gpu: &Gpu, config: Config) -> Result<Self, String> {
        // SAFETY: an in-process COM object by its class id.
        let transform: IMFTransform =
            unsafe { CoCreateInstance(&CLSID_MSH264EncoderMFT, None, CLSCTX_INPROC_SERVER) }
                .map_err(|e| format!("no software H.264 encoder: {e}"))?;
        Self::configure(
            gpu,
            transform,
            None,
            config,
            "Microsoft H.264 software encoder".into(),
            false,
        )
    }

    fn configure(
        gpu: &Gpu,
        transform: IMFTransform,
        activate: Option<IMFActivate>,
        config: Config,
        name: String,
        hardware: bool,
    ) -> Result<Self, String> {
        fn text(what: &'static str) -> impl Fn(windows::core::Error) -> String {
            move |e| format!("{what}: {e}")
        }
        // SAFETY: an MFT without attributes answers E_NOTIMPL, which is fine.
        let attributes: Option<IMFAttributes> = unsafe { transform.GetAttributes() }.ok();
        let mut is_async = false;
        let mut d3d_aware = false;
        if let Some(attributes) = &attributes {
            // SAFETY: reads and writes on the MFT's own attribute store.
            unsafe {
                is_async = attributes.GetUINT32(&MF_TRANSFORM_ASYNC).unwrap_or(0) != 0;
                if is_async {
                    attributes
                        .SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)
                        .map_err(text("unlock"))?;
                }
                d3d_aware = attributes.GetUINT32(&MF_SA_D3D11_AWARE).unwrap_or(0) != 0;
                let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
            }
        }

        let mut manager = None;
        if hardware && d3d_aware {
            let mut token = 0u32;
            let mut created = None;
            // SAFETY: out-pointers are live locals; the manager is handed our
            // device, and the MFT gets the manager as a pointer-sized
            // parameter, as the message requires.
            let set = unsafe {
                MFCreateDXGIDeviceManager(&mut token, &mut created).and_then(|()| {
                    let m: IMFDXGIDeviceManager =
                        created.ok_or_else(windows::core::Error::empty)?;
                    m.ResetDevice(&gpu.device, token)?;
                    transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, m.as_raw() as usize)?;
                    Ok(m)
                })
            };
            match set {
                Ok(m) => manager = Some(m),
                Err(err) => debug!("live view: {name} refused the D3D11 device: {err}"),
            }
        }

        let codec: Option<ICodecAPI> = transform.cast().ok();
        let mut encoder = Self {
            transform,
            activate,
            codec,
            events: None,
            need_input: 0,
            staging: None,
            manager,
            provides_samples: false,
            output_size: 0,
            config,
            hardware,
            name,
        };
        // Some encoders only take these before the types, some only after;
        // set them both times.
        encoder.apply_codec_settings();
        encoder.set_types().map_err(text("media types"))?;
        encoder.apply_codec_settings();
        encoder.refresh_stream_info().map_err(text("stream info"))?;

        if encoder.manager.is_none() {
            encoder.staging = Some(staging_texture(gpu, config).map_err(text("staging texture"))?);
        }
        if is_async {
            encoder.events = Some(encoder.transform.cast().map_err(text("event queue"))?);
        }
        // SAFETY: plain messages on a configured MFT.
        unsafe {
            encoder
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(text("begin streaming"))?;
            encoder
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(text("start of stream"))?;
        }
        info!(
            "live view: encoding {}x{} with {} ({}, {})",
            config.width,
            config.height,
            encoder.name,
            if hardware { "hardware" } else { "software" },
            if encoder.manager.is_some() {
                "GPU textures"
            } else {
                "system memory"
            }
        );
        Ok(encoder)
    }

    fn set_value(&self, api: &GUID, value: VARIANT, what: &str) -> bool {
        let Some(codec) = &self.codec else {
            return false;
        };
        // SAFETY: `api` and `value` are live for the call.
        match unsafe { codec.SetValue(api, &value) } {
            Ok(()) => true,
            Err(err) => {
                debug!("live view: {} refused {what}: {err}", self.name);
                false
            }
        }
    }

    fn apply_codec_settings(&self) {
        // Low latency is documented as a VT_BOOL, but some encoders only take
        // a VT_UI4.
        if !self.set_value(
            &CODECAPI_AVLowLatencyMode,
            VARIANT::from(true),
            "low latency",
        ) {
            self.set_value(
                &CODECAPI_AVLowLatencyMode,
                VARIANT::from(1u32),
                "low latency",
            );
        }
        self.set_value(
            &CODECAPI_AVEncCommonRateControlMode,
            VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32),
            "CBR",
        );
        self.set_value(
            &CODECAPI_AVEncCommonMeanBitRate,
            VARIANT::from(self.config.bitrate),
            "bitrate",
        );
        self.set_value(
            &CODECAPI_AVEncMPVGOPSize,
            VARIANT::from(self.config.fps.max(1) * SAFETY_GOP_SECONDS),
            "GOP size",
        );
        self.set_value(
            &CODECAPI_AVEncMPVDefaultBPictureCount,
            VARIANT::from(0u32),
            "B-frames",
        );
    }

    fn set_types(&mut self) -> WinResult<()> {
        let Config {
            width,
            height,
            fps,
            bitrate,
        } = self.config;
        let size = pack(width, height);
        let rate = pack(fps.max(1), 1);
        // The output type goes first: the encoders derive their input types
        // from it.
        let mut last_err = None;
        for profile in [
            eAVEncH264VProfile_ConstrainedBase.0 as u32,
            eAVEncH264VProfile_Base.0 as u32,
        ] {
            // SAFETY: a fresh media type filled with plain values, then
            // handed to the MFT, which copies what it needs.
            let result = unsafe {
                let output = MFCreateMediaType()?;
                output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
                output.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
                output.SetUINT64(&MF_MT_FRAME_SIZE, size)?;
                output.SetUINT64(&MF_MT_FRAME_RATE, rate)?;
                output.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
                output.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                output.SetUINT32(&MF_MT_MPEG2_PROFILE, profile)?;
                output.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?;
                output.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?;
                self.transform.SetOutputType(0, &output, 0)
            };
            match result {
                Ok(()) => {
                    last_err = None;
                    break;
                }
                Err(err) => last_err = Some(err),
            }
        }
        if let Some(err) = last_err {
            return Err(err);
        }

        // Prefer the encoder's own NV12 type (it may carry attributes it
        // needs), with our size and rate on it.
        let mut input: Option<IMFMediaType> = None;
        for index in 0.. {
            // SAFETY: enumerates the MFT's input types until it runs out.
            let Ok(candidate) = (unsafe { self.transform.GetInputAvailableType(0, index) }) else {
                break;
            };
            // SAFETY: reads a GUID from a live media type.
            if unsafe { candidate.GetGUID(&MF_MT_SUBTYPE) }.ok() == Some(MFVideoFormat_NV12) {
                input = Some(candidate);
                break;
            }
        }
        // SAFETY: as for the output type.
        unsafe {
            let input = match input {
                Some(input) => input,
                None => {
                    let input = MFCreateMediaType()?;
                    input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                    input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
                    input
                }
            };
            input.SetUINT64(&MF_MT_FRAME_SIZE, size)?;
            input.SetUINT64(&MF_MT_FRAME_RATE, rate)?;
            input.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
            input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            input.SetUINT32(&MF_MT_DEFAULT_STRIDE, width)?;
            input.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?;
            input.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?;
            self.transform.SetInputType(0, &input, 0)?;
        }
        Ok(())
    }

    fn refresh_stream_info(&mut self) -> WinResult<()> {
        // SAFETY: a plain query on a configured MFT.
        let info = unsafe { self.transform.GetOutputStreamInfo(0)? };
        let provides = MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32;
        let can = MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32;
        self.provides_samples = info.dwFlags & (provides | can) != 0;
        self.output_size = info.cbSize;
        Ok(())
    }

    /// The parameter sets the encoder put in its output type, when it did.
    pub fn sequence_header(&self) -> Option<Vec<u8>> {
        // SAFETY: reads a blob from the MFT's current output type into a
        // buffer of the size it reports.
        unsafe {
            let output = self.transform.GetOutputCurrentType(0).ok()?;
            let size = output.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER).ok()? as usize;
            if size == 0 {
                return None;
            }
            let mut blob = vec![0u8; size];
            output
                .GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut blob, None)
                .ok()?;
            Some(blob)
        }
    }

    pub fn set_bitrate(&mut self, bps: u32) {
        self.config.bitrate = bps;
        self.set_value(
            &CODECAPI_AVEncCommonMeanBitRate,
            VARIANT::from(bps),
            "bitrate",
        );
    }

    /// Encode one NV12 picture. Returns the access units that came out (with
    /// a low-latency encoder, this picture's; an encoder that answers late
    /// returns it with the next one).
    pub fn encode(
        &mut self,
        gpu: &Gpu,
        texture: &ID3D11Texture2D,
        time: i64,
        keyframe: bool,
    ) -> WinResult<Vec<Output>> {
        let sample = self.sample(gpu, texture, time)?;
        if keyframe {
            self.set_value(
                &CODECAPI_AVEncVideoForceKeyFrame,
                VARIANT::from(1u32),
                "keyframe",
            );
        }
        let mut outputs = Vec::new();
        if self.events.is_some() {
            self.encode_async(&sample, &mut outputs)?;
        } else {
            self.encode_sync(&sample, &mut outputs)?;
        }
        Ok(outputs)
    }

    fn sample(&mut self, gpu: &Gpu, texture: &ID3D11Texture2D, time: i64) -> WinResult<IMFSample> {
        // SAFETY: every buffer and sample is created here and owned; the
        // mapped staging memory is read only between Map and Unmap, within
        // the row pitch and height the map reports.
        unsafe {
            let buffer = if self.manager.is_some() {
                let buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, texture, 0, false)?;
                let length = buffer
                    .cast::<windows::Win32::Media::MediaFoundation::IMF2DBuffer>()?
                    .GetContiguousLength()?;
                buffer.SetCurrentLength(length)?;
                buffer
            } else {
                let staging = self
                    .staging
                    .as_ref()
                    .ok_or_else(windows::core::Error::empty)?;
                let (width, height) = (self.config.width as usize, self.config.height as usize);
                let length = width * height * 3 / 2;
                let buffer = MFCreateMemoryBuffer(length as u32)?;
                gpu.context.CopyResource(staging, texture);
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                gpu.context
                    .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
                let mut dst = std::ptr::null_mut();
                let locked = buffer.Lock(&mut dst, None, None);
                if let Err(err) = locked {
                    gpu.context.Unmap(staging, 0);
                    return Err(err);
                }
                let src = mapped.pData as *const u8;
                let pitch = mapped.RowPitch as usize;
                // Y plane, then the interleaved UV plane, which a mapped NV12
                // texture keeps right after `height` rows of Y.
                for row in 0..height {
                    std::ptr::copy_nonoverlapping(
                        src.add(row * pitch),
                        dst.add(row * width),
                        width,
                    );
                }
                let uv_src = src.add(pitch * height);
                let uv_dst = dst.add(width * height);
                for row in 0..height / 2 {
                    std::ptr::copy_nonoverlapping(
                        uv_src.add(row * pitch),
                        uv_dst.add(row * width),
                        width,
                    );
                }
                let _ = buffer.Unlock();
                gpu.context.Unmap(staging, 0);
                buffer.SetCurrentLength(length as u32)?;
                buffer
            };
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(time)?;
            sample.SetSampleDuration(10_000_000 / i64::from(self.config.fps.max(1)))?;
            Ok(sample)
        }
    }

    fn encode_sync(&mut self, sample: &IMFSample, outputs: &mut Vec<Output>) -> WinResult<()> {
        // SAFETY: a configured synchronous MFT and a sample we own.
        let mut result = unsafe { self.transform.ProcessInput(0, sample, 0) };
        if matches!(&result, Err(err) if err.code() == MF_E_NOTACCEPTING) {
            self.drain(outputs)?;
            // SAFETY: as above.
            result = unsafe { self.transform.ProcessInput(0, sample, 0) };
        }
        result?;
        self.drain(outputs)
    }

    /// Collect output until the encoder wants more input.
    fn drain(&mut self, outputs: &mut Vec<Output>) -> WinResult<()> {
        loop {
            match self.process_output() {
                Ok(Some(output)) => outputs.push(output),
                Ok(None) => {}
                Err(err) if err.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                Err(err) => return Err(err),
            }
        }
    }

    fn encode_async(&mut self, sample: &IMFSample, outputs: &mut Vec<Output>) -> WinResult<()> {
        let deadline = Instant::now() + INPUT_TIMEOUT;
        while self.need_input == 0 {
            if !self.pump(outputs)? {
                if Instant::now() >= deadline {
                    return Err(windows::core::Error::new(
                        MF_E_NOTACCEPTING,
                        "the hardware encoder stopped asking for input",
                    ));
                }
                std::thread::sleep(EVENT_POLL);
            }
        }
        // SAFETY: the MFT asked for input; the sample is ours.
        unsafe { self.transform.ProcessInput(0, sample, 0)? };
        self.need_input -= 1;
        let deadline = Instant::now() + OUTPUT_WAIT;
        let before = outputs.len();
        while outputs.len() == before && Instant::now() < deadline {
            if !self.pump(outputs)? {
                std::thread::sleep(EVENT_POLL);
            }
        }
        // Anything else already queued (another input request, typically).
        while self.pump(outputs)? {}
        Ok(())
    }

    /// Handle one event from a hardware encoder's queue. `false` when there
    /// was none.
    fn pump(&mut self, outputs: &mut Vec<Output>) -> WinResult<bool> {
        let Some(events) = &self.events else {
            return Ok(false);
        };
        // SAFETY: a non-blocking read of the MFT's event queue.
        let event = match unsafe { events.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
            Ok(event) => event,
            Err(err) if err.code() == MF_E_NO_EVENTS_AVAILABLE => return Ok(false),
            Err(err) => return Err(err),
        };
        // SAFETY: plain reads on a live event.
        let (kind, status) = unsafe { (event.GetType()?, event.GetStatus()?) };
        status.ok()?;
        if kind == METransformNeedInput.0 as u32 {
            self.need_input += 1;
        } else if kind == METransformHaveOutput.0 as u32 {
            match self.process_output() {
                Ok(Some(output)) => outputs.push(output),
                Ok(None) => {}
                Err(err) if err.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {}
                Err(err) => return Err(err),
            }
        }
        Ok(true)
    }

    /// One `ProcessOutput`. `Ok(None)` after a format change, which is handled
    /// here.
    fn process_output(&mut self) -> WinResult<Option<Output>> {
        let sample = if self.provides_samples {
            None
        } else {
            let size = self
                .output_size
                .max(self.config.width * self.config.height * 3 / 2)
                .max(64 * 1024);
            // SAFETY: a fresh sample with a fresh buffer, both owned.
            unsafe {
                let sample = MFCreateSample()?;
                sample.AddBuffer(&MFCreateMemoryBuffer(size)?)?;
                Some(sample)
            }
        };
        let mut buffers = [MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        }];
        let mut status = 0u32;
        // SAFETY: the buffer array lives across the call; whatever the MFT
        // put in it is taken back out and released below on every path.
        let result = unsafe { self.transform.ProcessOutput(0, &mut buffers, &mut status) };
        // SAFETY: each field is taken exactly once.
        let (sample, _events) = unsafe {
            (
                ManuallyDrop::take(&mut buffers[0].pSample),
                ManuallyDrop::take(&mut buffers[0].pEvents),
            )
        };
        match result {
            Ok(()) => {}
            Err(err) if err.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                debug!("live view: {} changed its output format", self.name);
                // SAFETY: renegotiates with the first type the MFT offers.
                unsafe {
                    let output = self.transform.GetOutputAvailableType(0, 0)?;
                    self.transform.SetOutputType(0, &output, 0)?;
                }
                self.refresh_stream_info()?;
                return Ok(None);
            }
            Err(err) => return Err(err),
        }
        let Some(sample) = sample else {
            return Ok(None);
        };
        // SAFETY: the buffer is locked for exactly the copy of the length it
        // reports.
        unsafe {
            let time = sample.GetSampleTime().unwrap_or(0);
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut data = std::ptr::null_mut();
            let mut length = 0u32;
            buffer.Lock(&mut data, None, Some(&mut length))?;
            let bytes = std::slice::from_raw_parts(data, length as usize).to_vec();
            let _ = buffer.Unlock();
            if bytes.is_empty() {
                return Ok(None);
            }
            Ok(Some(Output { data: bytes, time }))
        }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: end-of-stream messages and shutdown on an MFT we own; a
        // refusal is harmless here.
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            if self.manager.is_some() {
                let _ = self
                    .transform
                    .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, 0);
            }
            // An asynchronous MFT keeps its event queue (and its threads)
            // until it is shut down.
            if let Ok(shutdown) = self.transform.cast::<IMFShutdown>() {
                let _ = shutdown.Shutdown();
            }
            if let Some(activate) = &self.activate {
                let _ = activate.ShutdownObject();
            }
        }
    }
}

/// Two 32-bit values in one 64-bit attribute, as `MFSetAttributeSize` and
/// `MFSetAttributeRatio` store them.
fn pack(high: u32, low: u32) -> u64 {
    (u64::from(high) << 32) | u64::from(low)
}

fn enum_hardware() -> WinResult<Vec<IMFActivate>> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: the out-pointers are live locals; the array MF allocates is
    // read for `count` entries, each one moved out, and then freed.
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut list,
            &mut count,
        )?;
        if list.is_null() {
            return Ok(Vec::new());
        }
        let found = std::slice::from_raw_parts_mut(list, count as usize)
            .iter_mut()
            .filter_map(Option::take)
            .collect();
        CoTaskMemFree(Some(list as *const _));
        Ok(found)
    }
}

fn friendly_name(activate: &IMFActivate) -> String {
    let mut text = PWSTR::null();
    let mut length = 0u32;
    // SAFETY: MF allocates the string; it is read and then freed.
    unsafe {
        if activate
            .GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut text, &mut length)
            .is_err()
            || text.is_null()
        {
            return "hardware H.264 encoder".into();
        }
        let name = text.to_string().unwrap_or_default();
        CoTaskMemFree(Some(text.0 as *const _));
        name
    }
}

/// Whether a hardware encoder runs on the GPU our textures are on. An encoder
/// that does not say is taken to.
fn same_adapter(activate: &IMFActivate, gpu: &Gpu) -> bool {
    let mut luid = [0u8; 8];
    // SAFETY: the blob is read into a buffer of its documented size.
    let read = unsafe { activate.GetBlob(&MFT_ENUM_ADAPTER_LUID, &mut luid, None) };
    if read.is_err() {
        return true;
    }
    let low = u32::from_le_bytes([luid[0], luid[1], luid[2], luid[3]]);
    let high = i32::from_le_bytes([luid[4], luid[5], luid[6], luid[7]]);
    low == gpu.adapter_luid.LowPart && high == gpu.adapter_luid.HighPart
}

fn staging_texture(gpu: &Gpu, config: Config) -> WinResult<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: config.width,
        Height: config.height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut texture = None;
    // SAFETY: descriptor and out-pointer are live locals.
    unsafe {
        gpu.device
            .CreateTexture2D(&desc, None, Some(&mut texture))?
    };
    texture.ok_or_else(windows::core::Error::empty)
}

#[cfg(test)]
mod tests {
    use super::pack;

    #[test]
    fn sizes_and_ratios_pack_high_then_low() {
        assert_eq!(pack(1920, 1080), 0x0000_0780_0000_0438);
        assert_eq!(pack(30, 1), (30u64 << 32) | 1);
    }
}
