//! The platform-independent half of the sound capture: device samples in,
//! 20 ms frames of 48 kHz interleaved stereo out.
//!
//! Three stages, each kept free of WASAPI so they can be tested on their own:
//!
//! 1. [`SampleFormat::to_f32`]: the endpoint's raw bytes as `f32`.
//! 2. [`StereoMix`]: any channel layout folded to left and right.
//! 3. [`To48k`]: resampled to 48 kHz, the only rate Opus-in-WebRTC is clocked
//!    at, and interleaved.
//!
//! Then [`Framer`] cuts the result into 960-sample frames and decides *when*
//! each one is due, which is the part that needs care: WASAPI loopback hands
//! back nothing at all while nothing is playing, but the RTP clock on the other
//! end expects a packet every 20 ms regardless.

use std::collections::VecDeque;
use std::time::Duration;

use rubato::{FftFixedInOut, Resampler};

/// Opus and WebRTC's clock for this track.
pub const OUT_RATE: usize = 48_000;
/// Samples per channel in one 20 ms frame at [`OUT_RATE`].
pub const FRAME_SAMPLES: usize = 960;
/// One frame, interleaved stereo.
pub const FRAME_LEN: usize = FRAME_SAMPLES * 2;
/// The length of one frame.
pub const FRAME: Duration = Duration::from_millis(20);

/// How far behind the wall clock the frame schedule may fall before silence is
/// sent instead of waiting for more sound. Loopback delivers in ~10 ms packets
/// with some scheduling jitter, so too small a slack would pad the middle of a
/// song with zeros whenever a packet is a little late; too large a one only
/// shifts the silent stretches later, which nobody can hear.
const SLACK: Duration = Duration::from_millis(60);
/// Behind the schedule by more than this (the machine slept, the thread was
/// starved), the missed frames are skipped rather than sent in one burst: a
/// second of silence arriving all at once helps no jitter buffer.
const MAX_LAG: Duration = Duration::from_secs(1);
/// Most sound held waiting to be framed. It only grows when silence was padded
/// in and the late sound then arrived after all; past this the oldest is
/// dropped so the delay it adds does not stay for the rest of the session.
const MAX_BACKLOG: usize = FRAME_LEN * 5;
/// What the backlog is cut back to when it overflows.
const BACKLOG_KEEP: usize = FRAME_LEN * 2;

/// How the endpoint stores a sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    F32,
    I16,
    /// Packed three-byte samples.
    I24,
    I32,
}

/// `WAVE_FORMAT_PCM`.
const TAG_PCM: u16 = 1;
/// `WAVE_FORMAT_IEEE_FLOAT`.
const TAG_FLOAT: u16 = 3;
/// `WAVE_FORMAT_EXTENSIBLE`.
pub const TAG_EXTENSIBLE: u16 = 0xFFFE;

impl SampleFormat {
    /// The sample format a `WAVEFORMATEX` describes. `subformat` is the first
    /// field of `WAVEFORMATEXTENSIBLE::SubFormat` when the tag is extensible:
    /// the `KSDATAFORMAT_SUBTYPE_*` GUIDs share every other field and differ
    /// only there, with the same values as the plain tags.
    pub fn from_wave_format(tag: u16, bits: u16, subformat: Option<u32>) -> Option<Self> {
        let kind = if tag == TAG_EXTENSIBLE {
            u16::try_from(subformat?).ok()?
        } else {
            tag
        };
        match (kind, bits) {
            (TAG_FLOAT, 32) => Some(Self::F32),
            (TAG_PCM, 16) => Some(Self::I16),
            (TAG_PCM, 24) => Some(Self::I24),
            (TAG_PCM, 32) => Some(Self::I32),
            _ => None,
        }
    }

    pub fn bytes(self) -> usize {
        match self {
            Self::I16 => 2,
            Self::I24 => 3,
            Self::F32 | Self::I32 => 4,
        }
    }

    /// Append `bytes` (little-endian, as every Windows endpoint is) to `out`
    /// as `f32` in -1..1. Read byte-wise so the buffer's alignment never
    /// matters.
    pub fn to_f32(self, bytes: &[u8], out: &mut Vec<f32>) {
        let size = self.bytes();
        out.reserve(bytes.len() / size);
        for s in bytes.chunks_exact(size) {
            out.push(match self {
                Self::F32 => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                Self::I16 => i16::from_le_bytes([s[0], s[1]]) as f32 / 32_768.0,
                Self::I24 => {
                    // Shift into the top of an i32 so the sign extends.
                    let v = i32::from_le_bytes([0, s[0], s[1], s[2]]);
                    v as f32 / 2_147_483_648.0
                }
                Self::I32 => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0,
            });
        }
    }
}

/// Folds an interleaved layout of any width into left and right.
///
/// Each channel gets a fixed `(left, right)` weight from its speaker position,
/// the usual ITU-style downmix: front channels go straight to their side, the
/// centre and the surrounds at -3 dB, LFE nowhere (a phone speaker cannot play
/// it, and summed in it only muddies dialogue). The weights are not normalised
/// to their sum: stereo content on a 5.1 or 7.1 endpoint (the common case,
/// since Windows does not upmix) only has the front pair, and normalising would
/// make it several dB quieter for no reason. A real surround mix that peaks
/// past full scale is clamped when framed instead.
#[derive(Clone, Debug)]
pub struct StereoMix {
    weights: Vec<(f32, f32)>,
}

/// -3 dB.
const HALF_POWER: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// `SPEAKER_*` bits, in channel order, with their stereo weights.
const SPEAKERS: [(u32, (f32, f32)); 18] = [
    (0x1, (1.0, 0.0)),               // FRONT_LEFT
    (0x2, (0.0, 1.0)),               // FRONT_RIGHT
    (0x4, (HALF_POWER, HALF_POWER)), // FRONT_CENTER
    (0x8, (0.0, 0.0)),               // LOW_FREQUENCY
    (0x10, (HALF_POWER, 0.0)),       // BACK_LEFT
    (0x20, (0.0, HALF_POWER)),       // BACK_RIGHT
    (0x40, (1.0, 0.0)),              // FRONT_LEFT_OF_CENTER
    (0x80, (0.0, 1.0)),              // FRONT_RIGHT_OF_CENTER
    (0x100, (0.5, 0.5)),             // BACK_CENTER
    (0x200, (HALF_POWER, 0.0)),      // SIDE_LEFT
    (0x400, (0.0, HALF_POWER)),      // SIDE_RIGHT
    (0x800, (0.5, 0.5)),             // TOP_CENTER
    (0x1000, (HALF_POWER, 0.0)),     // TOP_FRONT_LEFT
    (0x2000, (0.5, 0.5)),            // TOP_FRONT_CENTER
    (0x4000, (0.0, HALF_POWER)),     // TOP_FRONT_RIGHT
    (0x8000, (HALF_POWER, 0.0)),     // TOP_BACK_LEFT
    (0x10000, (0.5, 0.5)),           // TOP_BACK_CENTER
    (0x20000, (0.0, HALF_POWER)),    // TOP_BACK_RIGHT
];

/// The layout Windows means by a channel count when no mask says otherwise.
fn default_mask(channels: usize) -> Option<u32> {
    match channels {
        3 => Some(0x7),   // FL FR FC
        4 => Some(0x33),  // quad: FL FR BL BR
        5 => Some(0x37),  // FL FR FC BL BR
        6 => Some(0x3F),  // 5.1
        7 => Some(0x13F), // 6.1
        8 => Some(0x63F), // 7.1 surround
        _ => None,
    }
}

impl StereoMix {
    /// `mask` is `WAVEFORMATEXTENSIBLE::dwChannelMask` when there is one. A
    /// mask that does not name exactly `channels` speakers is ignored in
    /// favour of the default layout, and a layout nothing describes keeps its
    /// first two channels as left and right.
    pub fn new(channels: usize, mask: Option<u32>) -> Self {
        let weights = match channels {
            0 => Vec::new(),
            1 => vec![(1.0, 1.0)],
            2 => vec![(1.0, 0.0), (0.0, 1.0)],
            n => {
                let mask = mask
                    .filter(|m| m.count_ones() as usize == n)
                    .or_else(|| default_mask(n));
                match mask {
                    Some(mask) => SPEAKERS
                        .iter()
                        .filter(|(bit, _)| mask & bit != 0)
                        .map(|(_, w)| *w)
                        .collect(),
                    None => {
                        let mut w = vec![(0.0, 0.0); n];
                        w[0] = (1.0, 0.0);
                        w[1] = (0.0, 1.0);
                        w
                    }
                }
            }
        };
        Self { weights }
    }

    pub fn channels(&self) -> usize {
        self.weights.len()
    }

    /// Append the left and right of every whole frame in `interleaved`.
    pub fn mix(&self, interleaved: &[f32], left: &mut Vec<f32>, right: &mut Vec<f32>) {
        let n = self.weights.len();
        if n == 0 {
            return;
        }
        for frame in interleaved.chunks_exact(n) {
            let (mut l, mut r) = (0.0, 0.0);
            for (s, (wl, wr)) in frame.iter().zip(&self.weights) {
                l += s * wl;
                r += s * wr;
            }
            left.push(l);
            right.push(r);
        }
    }
}

/// Stereo at the device's rate in, interleaved stereo at 48 kHz out.
pub struct To48k {
    inner: Option<Resampling>,
}

struct Resampling {
    resampler: FftFixedInOut<f32>,
    /// Input waiting for a whole chunk, per channel.
    pending: [Vec<f32>; 2],
    out: Vec<Vec<f32>>,
}

impl To48k {
    pub fn new(rate: u32) -> Result<Self, String> {
        let rate = rate as usize;
        if rate == OUT_RATE {
            return Ok(Self { inner: None });
        }
        // 10 ms chunks: about what one loopback packet holds, so a chunk is
        // rarely held back waiting for the next packet.
        let chunk = (rate / 100).max(1);
        let resampler = FftFixedInOut::<f32>::new(rate, OUT_RATE, chunk, 2)
            .map_err(|e| format!("cannot resample {rate} Hz to {OUT_RATE} Hz: {e}"))?;
        let out = resampler.output_buffer_allocate(true);
        Ok(Self {
            inner: Some(Resampling {
                resampler,
                pending: [Vec::new(), Vec::new()],
                out,
            }),
        })
    }

    /// Append `left`/`right` (equal lengths) to `out` as 48 kHz interleaved.
    pub fn push(&mut self, left: &[f32], right: &[f32], out: &mut Vec<f32>) {
        let Some(r) = self.inner.as_mut() else {
            interleave(left, right, out);
            return;
        };
        r.pending[0].extend_from_slice(left);
        r.pending[1].extend_from_slice(right);
        loop {
            let need = r.resampler.input_frames_next();
            if r.pending[0].len() < need {
                break;
            }
            let input = [&r.pending[0][..need], &r.pending[1][..need]];
            match r.resampler.process_into_buffer(&input, &mut r.out, None) {
                Ok((used, made)) => {
                    interleave(&r.out[0][..made], &r.out[1][..made], out);
                    r.pending[0].drain(..used);
                    r.pending[1].drain(..used);
                }
                Err(e) => {
                    // Cannot happen with the buffers sized by the resampler
                    // itself; dropping the chunk is the safe answer if it does.
                    log::debug!("screen audio: resampling failed: {e}");
                    r.pending[0].clear();
                    r.pending[1].clear();
                    break;
                }
            }
        }
    }
}

fn interleave(left: &[f32], right: &[f32], out: &mut Vec<f32>) {
    out.reserve(left.len() * 2);
    for (l, r) in left.iter().zip(right) {
        out.push(*l);
        out.push(*r);
    }
}

/// What a frame from [`Framer::next`] holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    /// Captured sound (possibly with silence padded onto its end).
    Sound,
    /// Nothing was playing.
    Silence,
}

/// Cuts 48 kHz stereo into frames and paces them.
///
/// While sound is arriving, frames go out as soon as a whole one is buffered,
/// so they follow the device's own clock. When none arrives for longer than
/// [`SLACK`] past the moment the next frame was due, silence goes out instead,
/// on the wall clock, so the receiver sees a packet every 20 ms either way.
/// Frame `n` is due at `anchor + n * 20 ms`; times are durations since an
/// arbitrary start so this can be driven by a fake clock in tests.
pub struct Framer {
    fifo: VecDeque<f32>,
    sent: u64,
    anchor: Duration,
}

impl Framer {
    pub fn new(now: Duration) -> Self {
        Self {
            fifo: VecDeque::with_capacity(MAX_BACKLOG + FRAME_LEN),
            sent: 0,
            anchor: now,
        }
    }

    /// Forget buffered sound and start the schedule again at `now`.
    pub fn reset(&mut self, now: Duration) {
        self.fifo.clear();
        self.sent = 0;
        self.anchor = now;
    }

    pub fn push(&mut self, interleaved: &[f32]) {
        self.fifo.extend(interleaved);
        if self.fifo.len() > MAX_BACKLOG {
            let drop = self.fifo.len() - BACKLOG_KEEP;
            // Keep left and right paired.
            let drop = drop - drop % 2;
            self.fifo.drain(..drop);
            log::debug!("screen audio: {} ms of sound backlog dropped", drop / 96);
        }
    }

    fn due(&self, frame: u64) -> Duration {
        self.anchor + FRAME * frame as u32
    }

    /// The next frame to send at `now`, written into `out` (at least
    /// [`FRAME_LEN`] long, clamped to -1..1), or `None` until one is due. Call
    /// it until it returns `None`.
    pub fn next(&mut self, now: Duration, out: &mut [f32]) -> Option<FrameKind> {
        let out = &mut out[..FRAME_LEN];
        if self.due(self.sent) + MAX_LAG < now {
            // Skip the missed frames: the schedule restarts just inside the
            // slack, so one silent frame goes out now rather than a second's
            // worth of them.
            self.anchor = now.saturating_sub(SLACK + FRAME * self.sent as u32);
        }
        if self.fifo.len() >= FRAME_LEN {
            for (o, s) in out.iter_mut().zip(self.fifo.drain(..FRAME_LEN)) {
                *o = s.clamp(-1.0, 1.0);
            }
            self.sent += 1;
            return Some(FrameKind::Sound);
        }
        if now < self.due(self.sent) + SLACK {
            return None;
        }
        let have = self.fifo.len();
        for (o, s) in out.iter_mut().zip(self.fifo.drain(..)) {
            *o = s.clamp(-1.0, 1.0);
        }
        out[have..].fill(0.0);
        self.sent += 1;
        Some(if have > 0 {
            FrameKind::Sound
        } else {
            FrameKind::Silence
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn sample_formats_from_wave_format() {
        assert_eq!(
            SampleFormat::from_wave_format(TAG_EXTENSIBLE, 32, Some(3)),
            Some(SampleFormat::F32)
        );
        assert_eq!(
            SampleFormat::from_wave_format(TAG_EXTENSIBLE, 24, Some(1)),
            Some(SampleFormat::I24)
        );
        assert_eq!(
            SampleFormat::from_wave_format(TAG_FLOAT, 32, None),
            Some(SampleFormat::F32)
        );
        assert_eq!(
            SampleFormat::from_wave_format(TAG_PCM, 16, None),
            Some(SampleFormat::I16)
        );
        assert_eq!(SampleFormat::from_wave_format(TAG_PCM, 8, None), None);
        assert_eq!(
            SampleFormat::from_wave_format(TAG_EXTENSIBLE, 32, None),
            None
        );
        assert_eq!(SampleFormat::from_wave_format(TAG_FLOAT, 64, None), None);
    }

    #[test]
    fn bytes_become_floats() {
        let mut out = Vec::new();
        SampleFormat::F32.to_f32(&0.25f32.to_le_bytes(), &mut out);
        SampleFormat::I16.to_f32(&i16::MIN.to_le_bytes(), &mut out);
        SampleFormat::I16.to_f32(&16_384i16.to_le_bytes(), &mut out);
        // -2^22 in 24 bits is -0.5.
        SampleFormat::I24.to_f32(&[0x00, 0x00, 0xC0], &mut out);
        SampleFormat::I24.to_f32(&[0x00, 0x00, 0x40], &mut out);
        SampleFormat::I32.to_f32(&(i32::MIN / 4).to_le_bytes(), &mut out);
        assert_eq!(out, vec![0.25, -1.0, 0.5, -0.5, 0.5, -0.25]);
    }

    #[test]
    fn mono_is_copied_to_both_sides() {
        let mix = StereoMix::new(1, Some(0x4));
        let (mut l, mut r) = (Vec::new(), Vec::new());
        mix.mix(&[0.1, 0.2, 0.3], &mut l, &mut r);
        assert_eq!(l, vec![0.1, 0.2, 0.3]);
        assert_eq!(r, l);
    }

    #[test]
    fn stereo_passes_through() {
        let mix = StereoMix::new(2, None);
        let (mut l, mut r) = (Vec::new(), Vec::new());
        mix.mix(&[0.1, -0.1, 0.2, -0.2, 0.9], &mut l, &mut r);
        // The trailing half frame is not a frame.
        assert_eq!(l, vec![0.1, 0.2]);
        assert_eq!(r, vec![-0.1, -0.2]);
    }

    #[test]
    fn five_one_folds_centre_and_surrounds_and_drops_lfe() {
        let mix = StereoMix::new(6, Some(0x3F));
        assert_eq!(mix.channels(), 6);
        let (mut l, mut r) = (Vec::new(), Vec::new());
        // Only the front pair: unchanged level.
        mix.mix(&[0.5, -0.5, 0.0, 0.0, 0.0, 0.0], &mut l, &mut r);
        // Only the centre: -3 dB on both sides.
        mix.mix(&[0.0, 0.0, 1.0, 0.0, 0.0, 0.0], &mut l, &mut r);
        // Only the LFE: nothing.
        mix.mix(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0], &mut l, &mut r);
        // Only the back right: -3 dB on the right.
        mix.mix(&[0.0, 0.0, 0.0, 0.0, 0.0, 1.0], &mut l, &mut r);
        assert_eq!(l, vec![0.5, HALF_POWER, 0.0, 0.0]);
        assert_eq!(r, vec![-0.5, HALF_POWER, 0.0, HALF_POWER]);
    }

    #[test]
    fn a_mask_that_disagrees_with_the_count_is_ignored() {
        // Eight channels claiming a stereo mask get the 7.1 layout, where
        // channel 7 and 8 are the side pair.
        let mix = StereoMix::new(8, Some(0x3));
        let (mut l, mut r) = (Vec::new(), Vec::new());
        mix.mix(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0], &mut l, &mut r);
        assert_eq!((l[0], r[0]), (HALF_POWER, 0.0));

        // A width nothing describes keeps its first two channels.
        let mix = StereoMix::new(12, None);
        let (mut l, mut r) = (Vec::new(), Vec::new());
        let mut frame = vec![1.0; 12];
        frame[0] = 0.25;
        frame[1] = 0.75;
        mix.mix(&frame, &mut l, &mut r);
        assert_eq!((l[0], r[0]), (0.25, 0.75));
    }

    #[test]
    fn forty_eight_khz_is_only_interleaved() {
        let mut to = To48k::new(48_000).unwrap();
        let mut out = Vec::new();
        to.push(&[0.1, 0.2], &[-0.1, -0.2], &mut out);
        assert_eq!(out, vec![0.1, -0.1, 0.2, -0.2]);
    }

    /// One second of a 1 kHz tone at the device's rate becomes one second at
    /// 48 kHz (less what the resampler still holds), with the tone intact.
    #[test]
    fn other_rates_are_resampled_to_48k() {
        for rate in [44_100u32, 96_000, 16_000] {
            let mut to = To48k::new(rate).unwrap();
            let tone: Vec<f32> = (0..rate)
                .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / rate as f32).sin() * 0.5)
                .collect();
            let mut out = Vec::new();
            // In uneven pieces, like loopback packets.
            for piece in tone.chunks(rate as usize / 97) {
                to.push(piece, piece, &mut out);
            }
            let frames = out.len() / 2;
            assert!(
                (47_000..=48_000).contains(&frames),
                "{rate} Hz gave {frames} frames"
            );
            // Skip the resampler's start-up, then check level and pitch.
            let left: Vec<f32> = out.iter().step_by(2).skip(4_800).copied().collect();
            let rms = (left.iter().map(|s| s * s).sum::<f32>() / left.len() as f32).sqrt();
            assert!(
                (rms - 0.5 / 2f32.sqrt()).abs() < 0.02,
                "{rate} Hz rms {rms}"
            );
            let crossings = left
                .windows(2)
                .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
                .count() as f32;
            let hz = crossings / (left.len() as f32 / OUT_RATE as f32);
            assert!((hz - 1000.0).abs() < 5.0, "{rate} Hz came out at {hz} Hz");
        }
    }

    #[test]
    fn silence_comes_every_20_ms_after_the_slack() {
        let mut framer = Framer::new(ms(0));
        let mut out = vec![1.0; FRAME_LEN];
        let mut times = Vec::new();
        // A 5 ms poll for a second, nothing captured.
        for t in (0..=1000).step_by(5) {
            while let Some(kind) = framer.next(ms(t), &mut out) {
                assert_eq!(kind, FrameKind::Silence);
                assert!(out.iter().all(|s| *s == 0.0));
                times.push(t);
            }
        }
        // Frame n goes out at n * 20 ms + the slack.
        let expected: Vec<u64> = (0..)
            .map(|n| n * 20 + 60)
            .take_while(|t| *t <= 1000)
            .collect();
        assert_eq!(times, expected);
    }

    #[test]
    fn sound_is_framed_as_soon_as_a_frame_is_whole() {
        let mut framer = Framer::new(ms(0));
        let mut out = vec![0.0; FRAME_LEN];
        let packet: Vec<f32> = (0..FRAME_LEN / 2).map(|i| i as f32 / 10_000.0).collect();
        // Half a frame: nothing yet, it is not due.
        framer.push(&packet);
        assert_eq!(framer.next(ms(10), &mut out), None);
        framer.push(&packet);
        assert_eq!(framer.next(ms(20), &mut out), Some(FrameKind::Sound));
        assert_eq!(out[..FRAME_LEN / 2], packet[..]);
        assert_eq!(framer.next(ms(20), &mut out), None);
    }

    #[test]
    fn steady_sound_never_gets_silence_inserted() {
        let mut framer = Framer::new(ms(0));
        let mut out = vec![0.0; FRAME_LEN];
        let packet = vec![0.25; FRAME_LEN / 2];
        let mut kinds = Vec::new();
        // 10 ms packets arriving up to 30 ms late.
        for t in (0..2000u64).step_by(10) {
            framer.push(&packet);
            let jitter = (t / 10 % 4) * 10;
            while let Some(kind) = framer.next(ms(t + jitter), &mut out) {
                kinds.push(kind);
            }
        }
        assert!(kinds.iter().all(|k| *k == FrameKind::Sound));
        assert_eq!(kinds.len(), 100);
    }

    #[test]
    fn the_end_of_a_sound_is_padded_then_silence_follows() {
        let mut framer = Framer::new(ms(0));
        let mut out = vec![0.0; FRAME_LEN];
        framer.push(&vec![0.5; 100]);
        assert_eq!(framer.next(ms(59), &mut out), None);
        assert_eq!(framer.next(ms(60), &mut out), Some(FrameKind::Sound));
        assert!(out[..100].iter().all(|s| *s == 0.5));
        assert!(out[100..].iter().all(|s| *s == 0.0));
        assert_eq!(framer.next(ms(80), &mut out), Some(FrameKind::Silence));
    }

    #[test]
    fn loud_sound_is_clamped() {
        let mut framer = Framer::new(ms(0));
        let mut out = vec![0.0; FRAME_LEN];
        framer.push(&vec![1.7; FRAME_LEN]);
        framer.next(ms(0), &mut out);
        assert!(out.iter().all(|s| *s == 1.0));
    }

    #[test]
    fn a_long_stall_skips_instead_of_bursting() {
        let mut framer = Framer::new(ms(0));
        let mut out = vec![0.0; FRAME_LEN];
        let mut count = 0;
        while framer.next(ms(10_000), &mut out).is_some() {
            count += 1;
        }
        assert_eq!(count, 1);
        // And the cadence carries on from there.
        assert_eq!(framer.next(ms(10_019), &mut out), None);
        assert!(framer.next(ms(10_020), &mut out).is_some());
    }

    #[test]
    fn backlog_is_bounded() {
        let mut framer = Framer::new(ms(0));
        framer.push(&vec![0.1; FRAME_LEN * 20]);
        let mut out = vec![0.0; FRAME_LEN];
        let mut count = 0;
        while framer.next(ms(0), &mut out).is_some() {
            count += 1;
        }
        assert_eq!(count, 2);
    }
}
