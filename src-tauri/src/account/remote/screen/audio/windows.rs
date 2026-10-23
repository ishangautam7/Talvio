//! Windows: WASAPI loopback of the default output device, encoded to Opus.
//!
//! One thread owns everything: COM, the audio client, the resampler, the
//! encoder. It polls rather than waiting on an event, because a loopback
//! stream's event is not signalled while nothing plays on older Windows
//! versions, and silence still has to go out every 20 ms (see
//! [`pipeline::Framer`]).
//!
//! The capture is WASAPI directly rather than `cpal`, for the same reasons the
//! meeting recorder's loopback is (`audio_toolkit/audio/loopback.rs`): `cpal`
//! 0.16 gives no way to learn which endpoint is the default one *now*, which is
//! what following headphones being plugged in needs, and it hides the
//! endpoint's channel mask. Loopback is a shared-mode, read-only client of the
//! render endpoint, so it never touches its volume (audio ducking) or any
//! capture device (the microphone recorder).

mod pipeline;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use opus::{Application, Bitrate, Channels, Encoder};
use tokio::sync::mpsc;
use windows::core::GUID;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};

use self::pipeline::{Framer, SampleFormat, StereoMix, To48k, FRAME, FRAME_LEN};
use super::{AudioStream, OpusPacket, StartError};

/// Packets the receiver may fall behind by before new ones are dropped: one
/// second. The session forwards them straight to a WebRTC track, so a backlog
/// only means it stalled, and old sound is worth less than new.
const CHANNEL_CAPACITY: usize = 50;
/// Opus at this rate is transparent enough for music on a phone speaker or
/// earbuds and is what browsers send for stereo.
const BITRATE: i32 = 64_000;
/// Opus's largest packet for one frame.
const MAX_PACKET: usize = 1_275;
/// How often the default output device is looked up again, and a failed open
/// retried.
const DEVICE_CHECK: Duration = Duration::from_secs(1);
/// Poll interval while sound is being sent: well under a frame, and under
/// the 10 ms a loopback packet holds.
const POLL_ENABLED: Duration = Duration::from_millis(5);
/// Poll interval while disabled: the endpoint buffer below holds 200 ms, so
/// draining it this often loses nothing and costs nearly nothing.
const POLL_DISABLED: Duration = Duration::from_millis(40);
/// WASAPI buffer request, in 100 ns units.
const BUFFER_DURATION_100NS: i64 = 200 * 10_000;
/// How long [`start`] waits for COM and the encoder to come up.
const START_TIMEOUT: Duration = Duration::from_secs(5);

struct Shared {
    enabled: AtomicBool,
    stop: AtomicBool,
}

struct WasapiAudio {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl AudioStream for WasapiAudio {
    fn set_enabled(&self, on: bool) {
        self.shared.enabled.store(on, Ordering::SeqCst);
    }

    /// Ends the capture and waits for the thread (at most one poll interval
    /// plus releasing the audio client), so a new live view started right
    /// after never races this one for the endpoint.
    fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let handle = match self.worker.lock() {
            Ok(mut worker) => worker.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(handle) = handle {
            if handle.thread().id() != std::thread::current().id() && handle.join().is_err() {
                log::warn!("screen audio: capture thread panicked");
            }
        }
    }
}

impl Drop for WasapiAudio {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn start() -> Result<(Box<dyn AudioStream>, mpsc::Receiver<OpusPacket>), StartError> {
    let shared = Arc::new(Shared {
        enabled: AtomicBool::new(false),
        stop: AtomicBool::new(false),
    });
    let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
    let (init_tx, init_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);

    let for_worker = shared.clone();
    let handle = std::thread::Builder::new()
        .name("screen-audio".into())
        .spawn(move || worker(&for_worker, tx, init_tx))
        .map_err(|e| StartError::Failed(format!("cannot start the capture thread: {e}")))?;

    let audio = WasapiAudio {
        shared,
        worker: Mutex::new(Some(handle)),
    };
    match init_rx.recv_timeout(START_TIMEOUT) {
        Ok(Ok(())) => Ok((Box::new(audio), rx)),
        // Dropping `audio` stops and joins the thread, which has returned.
        Ok(Err(why)) => Err(StartError::Failed(why)),
        Err(_) => {
            // Wedged in COM: tell it to stop and leave it, rather than make
            // the caller wait on a join that may never return.
            audio.shared.stop.store(true, Ordering::SeqCst);
            if let Ok(mut worker) = audio.worker.lock() {
                worker.take();
            }
            Err(StartError::Failed(
                "the capture thread did not start".into(),
            ))
        }
    }
}

fn worker(
    shared: &Shared,
    tx: mpsc::Sender<OpusPacket>,
    init_tx: std::sync::mpsc::SyncSender<Result<(), String>>,
) {
    // SAFETY: called once on this thread, which nothing else has initialised
    // COM on, and balanced by the `CoUninitialize` below whatever the result
    // (a failed init that is not balanced is harmless; S_FALSE must be).
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if com.is_err() {
        let _ = init_tx.send(Err(format!("COM did not initialise: {com:?}")));
        return;
    }
    let setup = (|| -> Result<(IMMDeviceEnumerator, Encoder), String> {
        // SAFETY: COM is initialised on this thread (above).
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(|e| format!("cannot open the audio device list: {e}"))?;
        Ok((enumerator, new_encoder()?))
    })();
    match setup {
        Ok((enumerator, encoder)) => {
            let _ = init_tx.send(Ok(()));
            drop(init_tx);
            run(shared, &tx, &enumerator, encoder);
        }
        Err(why) => {
            let _ = init_tx.send(Err(why));
        }
    }
    // SAFETY: balances the successful `CoInitializeEx` above; every COM object
    // made on this thread was dropped with `run`'s or `setup`'s locals.
    unsafe { CoUninitialize() };
    log::debug!("screen audio: capture ended");
}

fn new_encoder() -> Result<Encoder, String> {
    let mut encoder = Encoder::new(48_000, Channels::Stereo, Application::Audio)
        .map_err(|e| format!("cannot create the Opus encoder: {e}"))?;
    // In-band FEC only helps speech-mode (SILK) frames and costs bitrate; the
    // DTX switch would stop the packets the RTP clock relies on during
    // silence. Both are off by default; set explicitly so it is on record.
    let settings = [
        encoder.set_bitrate(Bitrate::Bits(BITRATE)),
        encoder.set_inband_fec(false),
        encoder.set_dtx(false),
    ];
    for result in settings {
        result.map_err(|e| format!("cannot configure the Opus encoder: {e}"))?;
    }
    Ok(encoder)
}

/// The capture loop. Returns when stopped, when the receiver is gone, or when
/// encoding fails (which leaves nothing worth sending).
fn run(
    shared: &Shared,
    tx: &mpsc::Sender<OpusPacket>,
    enumerator: &IMMDeviceEnumerator,
    mut encoder: Encoder,
) {
    let start = Instant::now();
    let mut capture: Option<Capture> = None;
    // The endpoint that last failed to open, so a device that keeps refusing
    // (exclusive mode, a driver fault) is logged once and not every second.
    let mut failed: Option<String> = None;
    let mut no_device_logged = false;
    let mut next_check = start;
    let mut enabled = false;
    let mut framer = Framer::new(Duration::ZERO);
    // Scratch buffers kept across polls.
    let mut samples = Vec::<f32>::new();
    let mut left = Vec::<f32>::new();
    let mut right = Vec::<f32>::new();
    let mut stereo = Vec::<f32>::new();
    let mut frame = vec![0.0f32; FRAME_LEN];
    let mut packet = vec![0u8; MAX_PACKET];
    let mut dropped: u64 = 0;

    while !shared.stop.load(Ordering::Relaxed) && !tx.is_closed() {
        let now = Instant::now();

        if now >= next_check {
            next_check = now + DEVICE_CHECK;
            match default_device_id(enumerator) {
                Some(id) => {
                    no_device_logged = false;
                    let current = capture.as_ref().map(|c| c.id.as_str());
                    if current != Some(id.as_str()) {
                        // Release the old endpoint before opening the new one.
                        capture = None;
                        match Capture::open(enumerator, &id) {
                            Ok(c) => {
                                log::debug!(
                                    "screen audio: capturing the default output ({} Hz, {} ch)",
                                    c.rate,
                                    c.mix.channels()
                                );
                                failed = None;
                                capture = Some(c);
                            }
                            Err(why) => {
                                if failed.as_deref() != Some(id.as_str()) {
                                    log::debug!(
                                        "screen audio: cannot capture the default output, retrying: {why}"
                                    );
                                    failed = Some(id);
                                }
                            }
                        }
                    }
                }
                None => {
                    if capture.take().is_some() || !no_device_logged {
                        log::debug!("screen audio: no output device; sending silence");
                    }
                    no_device_logged = true;
                }
            }
        }

        let on = shared.enabled.load(Ordering::Relaxed);
        if on != enabled {
            enabled = on;
            log::debug!("screen audio: {}", if on { "on" } else { "off" });
            if on {
                // Start clean: no sound or encoder state from before it was
                // switched off.
                framer.reset(now - start);
                if let Some(c) = capture.as_mut() {
                    c.reset();
                }
                if let Err(e) = encoder.reset_state() {
                    log::debug!("screen audio: encoder reset failed: {e}");
                }
                dropped = 0;
            }
        }

        if let Some(c) = capture.as_mut() {
            let drained = c.drain(enabled, &mut samples, &mut left, &mut right, &mut stereo);
            match drained {
                Ok(()) => framer.push(&stereo),
                Err(why) => {
                    // Usually AUDCLNT_E_DEVICE_INVALIDATED: the device was
                    // unplugged or its format changed. Look again at once.
                    log::debug!("screen audio: capture lost, reopening: {why}");
                    capture = None;
                    failed = None;
                    next_check = now;
                }
            }
            stereo.clear();
        }

        if enabled {
            let elapsed = Instant::now() - start;
            while framer.next(elapsed, &mut frame).is_some() {
                let len = match encoder.encode_float(&frame, &mut packet) {
                    Ok(len) => len,
                    Err(e) => {
                        log::warn!("screen audio: Opus encoding failed, stopping: {e}");
                        return;
                    }
                };
                let packet = OpusPacket {
                    data: packet[..len].to_vec(),
                    duration: FRAME,
                };
                match tx.try_send(packet) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        dropped += 1;
                        if dropped == 1 || dropped.is_multiple_of(250) {
                            log::debug!("screen audio: receiver behind, {dropped} packets dropped");
                        }
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => return,
                }
            }
        }

        std::thread::sleep(if enabled { POLL_ENABLED } else { POLL_DISABLED });
    }
}

/// The id of the endpoint Windows plays to right now, or `None` when there is
/// no output device at all.
///
/// eConsole, as the meeting recorder uses: the default device in Sound
/// settings, which is where ordinary playback goes.
fn default_device_id(enumerator: &IMMDeviceEnumerator) -> Option<String> {
    // SAFETY: `enumerator` was created on this thread's MTA; `GetId` returns a
    // CoTaskMem string that is read once and then freed exactly once.
    unsafe {
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let raw = device.GetId().ok()?;
        let id = raw.to_string().ok();
        CoTaskMemFree(Some(raw.as_ptr() as *const _));
        id
    }
}

/// An open loopback client on one endpoint, and the conversion its format
/// needs.
struct Capture {
    id: String,
    client: IAudioClient,
    capture: IAudioCaptureClient,
    format: SampleFormat,
    block_align: usize,
    rate: u32,
    mix: StereoMix,
    resample: To48k,
}

impl Capture {
    fn open(enumerator: &IMMDeviceEnumerator, id: &str) -> Result<Self, String> {
        let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: COM is initialised on this thread; `wide` is a
        // NUL-terminated string that outlives the call; the mix format pointer
        // is checked for null, read before it is freed and freed exactly once.
        unsafe {
            let device = enumerator
                .GetDevice(windows::core::PCWSTR(wide.as_ptr()))
                .map_err(|e| format!("cannot open the endpoint: {e}"))?;
            let client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| format!("cannot activate the endpoint: {e}"))?;
            let mix_format = client
                .GetMixFormat()
                .map_err(|e| format!("cannot read the mix format: {e}"))?;
            if mix_format.is_null() {
                return Err("the endpoint has no mix format".into());
            }
            let parsed = read_format(mix_format);
            // The shared-mode mix format is the only one loopback accepts, so
            // it is passed back as is.
            let init = parsed.as_ref().map_err(Clone::clone).and_then(|_| {
                client
                    .Initialize(
                        AUDCLNT_SHAREMODE_SHARED,
                        AUDCLNT_STREAMFLAGS_LOOPBACK,
                        BUFFER_DURATION_100NS,
                        0,
                        mix_format,
                        None,
                    )
                    // Nearly always another app holding the device in
                    // exclusive mode.
                    .map_err(|e| format!("cannot start loopback: {e}"))
            });
            CoTaskMemFree(Some(mix_format as *const _));
            init?;
            let (format, channels, mask, rate, block_align) = parsed?;

            let capture: IAudioCaptureClient = client
                .GetService()
                .map_err(|e| format!("cannot get the capture service: {e}"))?;
            client
                .Start()
                .map_err(|e| format!("cannot start the client: {e}"))?;
            Ok(Self {
                id: id.to_owned(),
                client,
                capture,
                format,
                block_align,
                rate,
                mix: StereoMix::new(channels, mask),
                resample: To48k::new(rate)?,
            })
        }
    }

    /// Drop resampler state left from before the sound was switched off.
    fn reset(&mut self) {
        if let Ok(fresh) = To48k::new(self.rate) {
            self.resample = fresh;
        }
    }

    /// Read every packet the endpoint has ready. When `keep` is set, append
    /// it to `out` as 48 kHz interleaved stereo; otherwise just release it.
    fn drain(
        &mut self,
        keep: bool,
        samples: &mut Vec<f32>,
        left: &mut Vec<f32>,
        right: &mut Vec<f32>,
        out: &mut Vec<f32>,
    ) -> Result<(), String> {
        loop {
            // SAFETY: the capture client belongs to this thread and its
            // started audio client; the buffer it returns is only read for
            // `frames * block_align` bytes, and released before the next
            // `GetBuffer`.
            unsafe {
                let ready = self
                    .capture
                    .GetNextPacketSize()
                    .map_err(|e| format!("GetNextPacketSize: {e}"))?;
                if ready == 0 {
                    return Ok(());
                }
                let mut data: *mut u8 = std::ptr::null_mut();
                let mut frames: u32 = 0;
                let mut flags: u32 = 0;
                self.capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                    .map_err(|e| format!("GetBuffer: {e}"))?;
                if keep {
                    let channels = self.mix.channels();
                    samples.clear();
                    let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                    if silent || data.is_null() {
                        samples.resize(frames as usize * channels, 0.0);
                    } else {
                        let bytes =
                            std::slice::from_raw_parts(data, frames as usize * self.block_align);
                        self.format.to_f32(bytes, samples);
                    }
                    left.clear();
                    right.clear();
                    self.mix.mix(samples, left, right);
                }
                self.capture
                    .ReleaseBuffer(frames)
                    .map_err(|e| format!("ReleaseBuffer: {e}"))?;
            }
            if keep {
                self.resample.push(left, right, out);
            }
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // SAFETY: stopping a started client on the thread that owns it.
        let _ = unsafe { self.client.Stop() };
    }
}

/// `(format, channels, channel mask, rate, block align)` of a mix format.
///
/// # Safety
///
/// `format` must point to a valid `WAVEFORMATEX`, followed by the rest of a
/// `WAVEFORMATEXTENSIBLE` when its tag says so (as `GetMixFormat` returns).
unsafe fn read_format(
    format: *const WAVEFORMATEX,
) -> Result<(SampleFormat, usize, Option<u32>, u32, usize), String> {
    // The structs are packed, so they are copied out unaligned.
    let base = std::ptr::read_unaligned(format);
    let tag = base.wFormatTag;
    let bits = base.wBitsPerSample;
    let channels = base.nChannels as usize;
    let rate = base.nSamplesPerSec;
    let block_align = base.nBlockAlign as usize;
    let (subformat, mask) = if tag == pipeline::TAG_EXTENSIBLE && base.cbSize >= 22 {
        let ext = std::ptr::read_unaligned(format as *const WAVEFORMATEXTENSIBLE);
        let sub: GUID = ext.SubFormat;
        (Some(sub.data1), Some(ext.dwChannelMask))
    } else {
        (None, None)
    };
    let sample = SampleFormat::from_wave_format(tag, bits, subformat)
        .ok_or_else(|| format!("unsupported mix format (tag {tag:#x}, {bits}-bit)"))?;
    if channels == 0 || rate == 0 || block_align != channels * sample.bytes() {
        return Err(format!(
            "unexpected mix format ({channels} ch, {rate} Hz, block {block_align})"
        ));
    }
    Ok((sample, channels, mask, rate, block_align))
}

#[cfg(test)]
mod live_tests {
    //! Against the real default output device:
    //! `cargo test --lib account::remote::screen::audio -- --ignored --nocapture`

    use super::*;

    fn collect(rx: &mut mpsc::Receiver<OpusPacket>, for_: Duration) -> Option<Vec<OpusPacket>> {
        let end = Instant::now() + for_;
        let mut got = Vec::new();
        while Instant::now() < end {
            match rx.try_recv() {
                Ok(p) => got.push(p),
                Err(mpsc::error::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(mpsc::error::TryRecvError::Disconnected) => return None,
            }
        }
        Some(got)
    }

    fn rms_per_packet(packets: &[OpusPacket]) -> Vec<f32> {
        let mut decoder = opus::Decoder::new(48_000, Channels::Stereo).unwrap();
        let mut pcm = vec![0.0f32; 5_760 * 2];
        packets
            .iter()
            .map(|p| {
                let n = decoder.decode_float(&p.data, &mut pcm, false).unwrap();
                assert_eq!(n, 960, "a packet is not 20 ms");
                let s = &pcm[..n * 2];
                (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
            })
            .collect()
    }

    #[test]
    #[ignore = "plays a sound and captures the real default output device"]
    fn captures_what_the_computer_plays() {
        let (stream, mut rx) = start().expect("start");

        // Disabled at first: nothing.
        let off = collect(&mut rx, Duration::from_millis(500)).expect("open");
        assert!(off.is_empty(), "{} packets while disabled", off.len());

        stream.set_enabled(true);
        // A tone, the way any app plays sound.
        let player = std::thread::spawn(|| {
            use rodio::Source;
            let out = rodio::OutputStreamBuilder::from_default_device()
                .and_then(|b| b.open_stream())
                .expect("output");
            let sink = rodio::Sink::connect_new(out.mixer());
            sink.append(
                rodio::source::SineWave::new(440.0)
                    .take_duration(Duration::from_millis(1_500))
                    .amplify(0.3),
            );
            sink.sleep_until_end();
        });
        let during = collect(&mut rx, Duration::from_millis(2_000)).expect("open");
        player.join().unwrap();
        // Well after the tone, so only silence is left.
        let _ = collect(&mut rx, Duration::from_millis(500));
        let after = collect(&mut rx, Duration::from_millis(1_000)).expect("open");

        let during_rms = rms_per_packet(&during);
        let after_rms = rms_per_packet(&after);
        let loud = during_rms.iter().filter(|r| **r > 0.05).count();
        let peak = during_rms.iter().cloned().fold(0.0, f32::max);
        let sizes: Vec<usize> = after.iter().map(|p| p.data.len()).collect();
        println!(
            "during: {} packets, {loud} loud, peak rms {peak:.3}; after: {} packets, max rms {:.4}, sizes {:?}",
            during.len(),
            after.len(),
            after_rms.iter().cloned().fold(0.0, f32::max),
            &sizes[..sizes.len().min(10)]
        );
        // 2 s at 20 ms is 100, give or take the slack and the poll.
        assert!((90..=105).contains(&during.len()), "{}", during.len());
        assert!(
            loud >= 50,
            "the tone was not captured ({loud} loud packets)"
        );
        assert!((45..=52).contains(&after.len()), "silence: {}", after.len());
        assert!(after_rms.iter().all(|r| *r < 0.01), "silence is not silent");

        stream.set_enabled(false);
        let _ = collect(&mut rx, Duration::from_millis(100));
        let off = collect(&mut rx, Duration::from_millis(500)).expect("open");
        assert!(off.is_empty(), "{} packets after disabling", off.len());

        let stopped = Instant::now();
        stream.stop();
        println!("stop took {:?}", stopped.elapsed());
        assert!(
            collect(&mut rx, Duration::from_millis(100)).is_none(),
            "channel still open"
        );
    }
}
