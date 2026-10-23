//! `dictate`: the phone's recording, transcribed by this computer's own speech
//! setup, then pasted at its cursor (`deliver: "paste"`) or only sent back
//! (`"return"`). Kept in local History like a dictation made here.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Instant;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use log::{debug, info};
use serde_json::{json, Value};

use super::{windows, Pasted, Reply, Router};

const SAMPLE_RATE: u32 = 16_000;
const MAX_SAMPLES: u32 = 5 * 60 * SAMPLE_RATE;
/// Base64 of the longest valid recording plus room for its header and any
/// extra chunks; anything longer is refused before it is decoded.
const MAX_AUDIO_BASE64: usize = (MAX_SAMPLES as usize * 2 + 64 * 1024).div_ceil(3) * 4;
// The longest recording, in its relay frame, fits the server's 16 MiB frame.
const _: () = assert!(MAX_AUDIO_BASE64 + 1024 < 16 * 1024 * 1024);

enum Deliver {
    Paste,
    Return,
}

pub(super) async fn handle(router: Arc<Router>, body: Value, mut reply: Reply) {
    let host = router.host();
    if !router.input_allowed() {
        return reply.fail("remote_input_off");
    }
    if !reply.take_input() {
        return reply.fail("busy");
    }
    // A delivery this version does not know is a request it cannot carry out.
    let deliver = match body.get("deliver").and_then(Value::as_str) {
        Some("paste") => Deliver::Paste,
        Some("return") => Deliver::Return,
        _ => return reply.fail("unsupported"),
    };
    // Checked before the transcription, so a stale window fails at once.
    let target = match deliver {
        Deliver::Paste => match windows::target(&router, &reply, &body) {
            Ok(target) => target,
            Err(code) => return reply.fail(code),
        },
        Deliver::Return => None,
    };
    if !host.speech_ready() {
        return reply.fail("no_speech_model");
    }
    let decoded = body
        .get("audio")
        .and_then(Value::as_str)
        .ok_or("no audio")
        .and_then(decode);
    let (wav, samples) = match decoded {
        Ok(decoded) => decoded,
        Err(reason) => {
            debug!("Phone dictation refused: {reason}");
            return reply.fail("bad_audio");
        }
    };
    if reply.is_cancelled() {
        return reply.fail("cancelled");
    }

    info!(
        "Phone dictation: {:.1} s of audio",
        samples.len() as f32 / SAMPLE_RATE as f32
    );
    reply.progress("transcribing");
    let started = Instant::now();
    let Some(text) = host.transcribe(samples).await else {
        return reply.fail("transcription_failed");
    };
    debug!("Phone dictation transcribed in {:?}", started.elapsed());
    if text.is_empty() {
        return reply.fail("no_speech");
    }
    // A cancel during transcription lands here rather than stopping the run:
    // stopping it can stop the shared engine worker, failing a dictation made
    // on this computer that is queued behind it.
    if reply.is_cancelled() {
        return reply.fail("cancelled");
    }
    // Turned off, or signed out, while this was transcribing.
    if !router.input_allowed() {
        return reply.fail("remote_input_off");
    }
    host.save_history(wav, text.clone());

    match deliver {
        Deliver::Return => reply.ok(json!({ "text": text, "pasted": false })),
        Deliver::Paste => {
            reply.progress("pasting");
            match windows::aim(&router, target.as_ref(), || host.paste(text.clone())).await {
                Ok(Pasted::Done) => reply.ok(json!({ "text": text, "pasted": true })),
                // Nothing is pasted on this computer: the phone gets the text.
                Ok(Pasted::Off) => reply.ok(json!({ "text": text, "pasted": false })),
                Ok(Pasted::Failed) => reply.fail("paste_failed"),
                Err(code) => reply.fail(code),
            }
        }
    }
}

/// Base64 to the WAV bytes and their samples, only for 16 kHz mono 16-bit
/// PCM between one sample and five minutes long.
fn decode(audio: &str) -> Result<(Vec<u8>, Vec<f32>), &'static str> {
    if audio.len() > MAX_AUDIO_BASE64 {
        return Err("too large");
    }
    let wav = STANDARD.decode(audio).map_err(|_| "not base64")?;
    let samples = read_pcm(&wav)?;
    Ok((wav, samples))
}

fn read_pcm(wav: &[u8]) -> Result<Vec<f32>, &'static str> {
    let reader = hound::WavReader::new(Cursor::new(wav)).map_err(|_| "not a WAV file")?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != SAMPLE_RATE
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err("not 16 kHz mono 16-bit PCM");
    }
    match reader.len() {
        0 => return Err("empty"),
        len if len > MAX_SAMPLES => return Err("longer than five minutes"),
        _ => {}
    }
    // The same scaling as `audio_toolkit::read_wav_samples`, which History's
    // retranscribe reads recordings with.
    reader
        .into_samples::<i16>()
        .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
        .collect::<Result<_, _>>()
        .map_err(|_| "truncated")
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::testing::{self, FakeHost};
    use super::*;

    fn wav(spec: hound::WavSpec, samples: u32) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut bytes, spec).unwrap();
        for i in 0..samples * spec.channels as u32 {
            match spec.sample_format {
                hound::SampleFormat::Float => writer.write_sample(0.1f32).unwrap(),
                _ => writer.write_sample((i % 100) as i16).unwrap(),
            }
        }
        writer.finalize().unwrap();
        bytes.into_inner()
    }

    fn pcm16(sample_rate: u32, channels: u16) -> hound::WavSpec {
        hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        }
    }

    pub(in crate::account) fn wav_base64(samples: u32) -> String {
        STANDARD.encode(wav(pcm16(SAMPLE_RATE, 1), samples))
    }

    #[test]
    fn accepts_16_khz_mono_16_bit_pcm() {
        let (wav, samples) = decode(&wav_base64(1_600)).unwrap();
        assert_eq!(samples.len(), 1_600);
        assert!(samples.iter().all(|s| (0.0..0.01).contains(s)));
        assert!(wav.starts_with(b"RIFF"));
        assert_eq!(
            decode(&wav_base64(MAX_SAMPLES)).unwrap().1.len(),
            MAX_SAMPLES as usize
        );
    }

    #[test]
    fn refuses_anything_else() {
        let float = hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let eight_bit = hound::WavSpec {
            bits_per_sample: 8,
            ..pcm16(SAMPLE_RATE, 1)
        };
        for (what, bytes) in [
            ("stereo", wav(pcm16(SAMPLE_RATE, 2), 100)),
            ("44.1 kHz", wav(pcm16(44_100, 1), 100)),
            ("float", wav(float, 100)),
            ("8-bit", wav(eight_bit, 100)),
            ("empty", wav(pcm16(SAMPLE_RATE, 1), 0)),
            ("too long", wav(pcm16(SAMPLE_RATE, 1), MAX_SAMPLES + 1)),
            ("not a WAV", b"hello world, not audio at all".to_vec()),
        ] {
            assert!(decode(&STANDARD.encode(bytes)).is_err(), "{what}");
        }
        let mut truncated = wav(pcm16(SAMPLE_RATE, 1), 1_000);
        truncated.truncate(truncated.len() - 501);
        assert!(decode(&STANDARD.encode(truncated)).is_err(), "truncated");
        assert!(decode("%%% not base64 %%%").is_err());
        assert!(decode(&"A".repeat(MAX_AUDIO_BASE64 + 4)).is_err());
    }

    #[test]
    fn reads_past_extra_chunks_before_the_data() {
        // iOS's recorder pads its header with a `FLLR` chunk before `data`.
        let mut bytes = wav(pcm16(SAMPLE_RATE, 1), 1_600);
        assert_eq!(&bytes[36..40], b"data");
        bytes.splice(
            36..36,
            [*b"FLLR", 8u32.to_le_bytes(), [0; 4], [0; 4]].concat(),
        );
        let riff_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) + 16;
        bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
        assert_eq!(decode(&STANDARD.encode(bytes)).unwrap().1.len(), 1_600);
    }

    async fn run(host: FakeHost, body: Value) -> (Vec<Value>, Arc<FakeHost>) {
        let host = Arc::new(host);
        let (router, mut frames) = testing::router(host.clone());
        router.handle("phone", body);
        let mut bodies = Vec::new();
        loop {
            let frame = tokio::time::timeout(std::time::Duration::from_secs(5), frames.recv())
                .await
                .unwrap()
                .unwrap();
            let body = serde_json::from_str::<Value>(&frame).unwrap()["body"].clone();
            let done = body["kind"] == "result";
            bodies.push(body);
            if done {
                return (bodies, host);
            }
        }
    }

    fn request(deliver: &str) -> Value {
        json!({ "kind": "dictate", "requestId": "r1", "deliver": deliver, "audio": wav_base64(16_000) })
    }

    #[tokio::test]
    async fn pastes_and_keeps_it_in_history() {
        let (bodies, host) = run(FakeHost::default(), request("paste")).await;
        let stages: Vec<_> = bodies.iter().map(|b| b["stage"].clone()).collect();
        assert_eq!(stages[..2], [json!("transcribing"), json!("pasting")]);
        assert_eq!(
            bodies[2],
            json!({ "kind": "result", "requestId": "r1", "ok": true, "text": "hello there", "pasted": true })
        );
        assert_eq!(
            *host.pasted.lock().unwrap(),
            vec!["hello there".to_string()]
        );
        assert_eq!(*host.saved.lock().unwrap(), vec!["hello there".to_string()]);
    }

    #[tokio::test]
    async fn return_sends_the_text_back_without_pasting() {
        let (bodies, host) = run(FakeHost::default(), request("return")).await;
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[1]["text"], "hello there");
        assert_eq!(bodies[1]["pasted"], false);
        assert!(host.pasted.lock().unwrap().is_empty());
        assert_eq!(host.saved.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn each_failure_has_its_code() {
        let cases = [
            (
                FakeHost {
                    enabled: false.into(),
                    ..FakeHost::default()
                },
                request("paste"),
                "remote_input_off",
            ),
            (
                FakeHost {
                    ready: false,
                    ..FakeHost::default()
                },
                request("paste"),
                "no_speech_model",
            ),
            (FakeHost::default(), request("speak"), "unsupported"),
            (
                FakeHost::default(),
                json!({ "kind": "dictate", "requestId": "r1", "deliver": "paste", "audio": "AAAA" }),
                "bad_audio",
            ),
            (
                FakeHost {
                    transcript: None,
                    ..FakeHost::default()
                },
                request("paste"),
                "transcription_failed",
            ),
            (
                FakeHost {
                    transcript: Some(String::new()),
                    ..FakeHost::default()
                },
                request("paste"),
                "no_speech",
            ),
            (
                FakeHost {
                    paste: Pasted::Failed,
                    ..FakeHost::default()
                },
                request("paste"),
                "paste_failed",
            ),
        ];
        for (host, body, code) in cases {
            let (bodies, _) = run(host, body).await;
            assert_eq!(bodies.last().unwrap()["error"], code);
        }
    }

    #[tokio::test]
    async fn paste_off_sends_the_text_back() {
        let (bodies, host) = run(
            FakeHost {
                paste: Pasted::Off,
                ..FakeHost::default()
            },
            request("paste"),
        )
        .await;
        assert_eq!(
            bodies.last().unwrap(),
            &json!({ "kind": "result", "requestId": "r1", "ok": true, "text": "hello there", "pasted": false })
        );
        assert_eq!(host.saved.lock().unwrap().len(), 1);
    }

    /// A transcribing dictation whose `gate` is held, with its first frame read.
    async fn held() -> (
        Arc<Router>,
        Arc<FakeHost>,
        tokio::sync::mpsc::Receiver<String>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (release, gate) = tokio::sync::oneshot::channel();
        let host = Arc::new(FakeHost::default());
        *host.gate.lock().unwrap() = Some(gate);
        let (router, mut frames) = testing::router(host.clone());
        router.handle("phone", request("paste"));
        let first = frames.recv().await.unwrap();
        assert!(first.contains("transcribing"));
        (router, host, frames, release)
    }

    #[tokio::test]
    async fn switching_phone_dictation_off_while_it_transcribes_stops_the_paste() {
        let (_router, host, mut frames, release) = held().await;
        host.enabled
            .store(false, std::sync::atomic::Ordering::SeqCst);
        release.send(()).unwrap();
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), frames.recv())
            .await
            .unwrap()
            .unwrap();
        let body = serde_json::from_str::<Value>(&frame).unwrap()["body"].clone();
        assert_eq!(body["error"], "remote_input_off");
        assert!(host.pasted.lock().unwrap().is_empty());
        assert!(host.saved.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn signing_out_while_it_transcribes_stops_the_paste() {
        let (router, host, frames, release) = held().await;
        // Ending the session drops the socket, and with it the outbox's receiver.
        drop(frames);
        release.send(()).unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while !router.requests.lock().unwrap().is_empty() {
            assert!(Instant::now() < deadline, "the request never ended");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(host.pasted.lock().unwrap().is_empty());
        assert!(host.saved.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn losing_access_while_it_transcribes_stops_the_paste() {
        let ways: [fn(&Router); 2] = [
            // Remove access in Settings → Account.
            |router| router.approval_changed("phone", false),
            // The phone leaves the account.
            |router| router.set_phones(vec![testing::phone("phone2")]),
        ];
        for lose_access in ways {
            let (router, host, mut frames, release) = held().await;
            lose_access(&router);
            release.send(()).unwrap();
            let frame = tokio::time::timeout(std::time::Duration::from_secs(5), frames.recv())
                .await
                .unwrap()
                .unwrap();
            let body = serde_json::from_str::<Value>(&frame).unwrap()["body"].clone();
            assert_eq!(body["error"], "not_approved");
            assert!(host.pasted.lock().unwrap().is_empty());
            assert!(host.saved.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn the_size_cap_fits_the_longest_recording() {
        let longest = STANDARD
            .encode(wav(pcm16(SAMPLE_RATE, 1), MAX_SAMPLES))
            .len();
        assert!(longest <= MAX_AUDIO_BASE64);
    }
}
