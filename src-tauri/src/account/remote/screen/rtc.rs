//! The WebRTC connection a live view runs over: one H.264 video track, one
//! Opus audio track and the `control` data channel, offered by this computer
//! (`PROTOCOL.md`, "Live view").
//!
//! [`Peer`] hides the `webrtc` crate behind the handful of things the session
//! does: make the offer, take the answer and the phone's candidates, write
//! frames and packets, and send on the data channel. Everything it hears comes
//! back as one stream of [`PeerEvent`]s, so the session is a single loop.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use log::{debug, warn};
use rtc::interceptor::{Attribute, Interceptor, Packet, Slot, StreamInfo, TaggedPacket};
use rtc::media::Sample;
use rtc::rtcp::payload_feedbacks::full_intra_request::FullIntraRequest;
use rtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use rtc::rtp_transceiver::rtp_sender::{
    RTCPFeedback, RTCRtpCodec, RTCRtpCodecParameters, RTCRtpCodingParameters,
    RTCRtpEncodingParameters, RtpCodecKind,
};
use rtc::rtp_transceiver::{PayloadType, SSRC};
use rtc::sansio::Protocol;
use tokio::sync::mpsc;
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use webrtc::media_stream::track_local::static_sample::TrackLocalStaticSample;
use webrtc::media_stream::track_local::{TrackLocal, TrackLocalEvent};
use webrtc::media_stream::MediaStreamTrack;
use webrtc::peer_connection::{
    register_default_interceptors, MediaEngine, PeerConnection, PeerConnectionBuilder,
    PeerConnectionEventHandler, RTCConfigurationBuilder, RTCIceCandidateInit, RTCIceServer,
    RTCPeerConnectionIceEvent, RTCPeerConnectionState, RTCSessionDescription, Registry,
};
use webrtc::rtp_transceiver::RtpSender;

/// The label of the data channel input and notices travel on.
pub const CONTROL_LABEL: &str = "control";

const H264_PAYLOAD_TYPE: PayloadType = 102;
const OPUS_PAYLOAD_TYPE: PayloadType = 111;
/// Constrained Baseline, the profile every phone decodes in hardware; the
/// level is what is offered, and `level-asymmetry-allowed` lets the encoder
/// send a higher one for a 1080p window.
const H264_FMTP: &str = "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f";
const OPUS_FMTP: &str = "minptime=10;useinbandfec=1";
/// Events the session has not read yet; more than this and the connection's
/// own tasks would be waiting on a session that has stopped reading.
const EVENT_CAPACITY: usize = 256;

/// What the connection tells the session.
#[derive(Debug)]
pub enum PeerEvent {
    /// A local ICE candidate, to trickle to the phone.
    Candidate(RTCIceCandidateInit),
    /// Media can flow.
    Connected,
    /// The connection failed or closed and will not come back.
    Closed,
    /// The `control` channel opened.
    ControlOpen,
    /// A text message on the `control` channel.
    Control(String),
    /// The phone lost the picture (an RTCP PLI or FIR).
    KeyframeRequest,
}

pub use crate::account::api::IceServer;

pub struct Peer {
    connection: Arc<dyn PeerConnection>,
    video: Arc<TrackLocalStaticSample>,
    video_ssrc: SSRC,
    audio: Arc<TrackLocalStaticSample>,
    audio_ssrc: SSRC,
    control: Arc<dyn DataChannel>,
}

impl Peer {
    /// A connection with its tracks and data channel in place, ready to offer.
    pub async fn new(
        ice_servers: &[IceServer],
    ) -> Result<(Self, mpsc::Receiver<PeerEvent>), String> {
        let (events, receiver) = mpsc::channel(EVENT_CAPACITY);
        let video_codec = video_codec();
        let audio_codec = audio_codec();
        let mut media = MediaEngine::default();
        media
            .register_codec(video_codec.clone(), RtpCodecKind::Video)
            .map_err(|e| e.to_string())?;
        media
            .register_codec(audio_codec.clone(), RtpCodecKind::Audio)
            .map_err(|e| e.to_string())?;
        let registry = register_default_interceptors(Registry::new(), &mut media)
            .map_err(|e| e.to_string())?
            // Last, so every interceptor that reads RTCP has seen it first.
            .with(Slot::from(14_000), KeyframeRequests::default());
        let config = RTCConfigurationBuilder::new()
            .with_ice_servers(ice_servers.iter().map(rtc_ice_server).collect())
            .build();
        let handler = Arc::new(Events {
            events: events.clone(),
        });
        let connection = PeerConnectionBuilder::new()
            .with_configuration(config)
            .with_media_engine(media)
            .with_interceptor_registry(registry)
            .with_handler(handler)
            .with_udp_addrs(vec!["0.0.0.0:0"])
            .build()
            .await
            .map_err(|e| e.to_string())?;
        let connection: Arc<dyn PeerConnection> = Arc::new(connection);
        let started = Instant::now();

        let (video, video_ssrc) = track(started, RtpCodecKind::Video, &video_codec)?;
        let video_sender = connection
            .add_track(video.clone() as Arc<dyn TrackLocal>)
            .await
            .map_err(|e| e.to_string())?;
        let (audio, audio_ssrc) = track(started, RtpCodecKind::Audio, &audio_codec)?;
        connection
            .add_track(audio.clone() as Arc<dyn TrackLocal>)
            .await
            .map_err(|e| e.to_string())?;
        watch_keyframe_requests(video.clone(), video_sender, events.clone());

        let control = connection
            .create_data_channel(CONTROL_LABEL, None)
            .await
            .map_err(|e| e.to_string())?;
        watch_control(control.clone(), events);

        Ok((
            Self {
                connection,
                video,
                video_ssrc,
                audio,
                audio_ssrc,
                control,
            },
            receiver,
        ))
    }

    /// The offer, already set as this side's description.
    pub async fn offer(&self) -> Result<String, String> {
        let offer = self
            .connection
            .create_offer(None)
            .await
            .map_err(|e| e.to_string())?;
        let sdp = offer.sdp.clone();
        self.connection
            .set_local_description(offer)
            .await
            .map_err(|e| e.to_string())?;
        Ok(sdp)
    }

    pub async fn answer(&self, sdp: String) -> Result<(), String> {
        let answer = RTCSessionDescription::answer(sdp).map_err(|e| e.to_string())?;
        self.connection
            .set_remote_description(answer)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn add_candidate(&self, candidate: RTCIceCandidateInit) -> Result<(), String> {
        self.connection
            .add_ice_candidate(candidate)
            .await
            .map_err(|e| e.to_string())
    }

    /// One H.264 access unit, `since_last` after the one before it.
    pub async fn write_video(&self, data: Vec<u8>, since_last: Duration) -> Result<(), String> {
        self.write(
            &self.video,
            self.video_ssrc,
            H264_PAYLOAD_TYPE,
            data,
            since_last,
        )
        .await
    }

    /// One Opus packet.
    pub async fn write_audio(&self, data: Vec<u8>, duration: Duration) -> Result<(), String> {
        self.write(
            &self.audio,
            self.audio_ssrc,
            OPUS_PAYLOAD_TYPE,
            data,
            duration,
        )
        .await
    }

    async fn write(
        &self,
        track: &TrackLocalStaticSample,
        ssrc: SSRC,
        payload_type: PayloadType,
        data: Vec<u8>,
        duration: Duration,
    ) -> Result<(), String> {
        let sample = Sample {
            data: Bytes::from(data),
            duration,
            ..Sample::new(Instant::now())
        };
        track
            .sample_writer(ssrc, payload_type)
            .write_sample(&sample)
            .await
            .map_err(|e| e.to_string())
    }

    /// A text message on the `control` channel. Dropped (and logged) when the
    /// channel is not open or backed up: notices are refreshed anyway.
    pub async fn send_control(&self, text: &str) {
        if let Err(e) = self.control.try_send_text(text).await {
            debug!("A live-view notice was not sent: {e}");
        }
    }

    /// The connection itself, to close after the session has let go of it.
    pub fn handle(&self) -> Arc<dyn PeerConnection> {
        self.connection.clone()
    }
}

fn video_codec() -> RTCRtpCodecParameters {
    RTCRtpCodecParameters {
        rtp_codec: RTCRtpCodec {
            mime_type: "video/H264".to_owned(),
            clock_rate: 90_000,
            channels: 0,
            sdp_fmtp_line: H264_FMTP.to_owned(),
            rtcp_feedback: vec![
                feedback("nack", ""),
                feedback("nack", "pli"),
                feedback("ccm", "fir"),
            ],
        },
        payload_type: H264_PAYLOAD_TYPE,
    }
}

fn audio_codec() -> RTCRtpCodecParameters {
    RTCRtpCodecParameters {
        rtp_codec: RTCRtpCodec {
            mime_type: "audio/opus".to_owned(),
            clock_rate: 48_000,
            channels: 2,
            sdp_fmtp_line: OPUS_FMTP.to_owned(),
            rtcp_feedback: vec![],
        },
        payload_type: OPUS_PAYLOAD_TYPE,
    }
}

fn feedback(typ: &str, parameter: &str) -> RTCPFeedback {
    RTCPFeedback {
        typ: typ.to_owned(),
        parameter: parameter.to_owned(),
    }
}

fn rtc_ice_server(server: &IceServer) -> RTCIceServer {
    RTCIceServer {
        urls: server.urls.clone(),
        username: server.username.clone().unwrap_or_default(),
        credential: server.credential.clone().unwrap_or_default(),
    }
}

/// A send-only track of `kind` with a random SSRC.
fn track(
    started: Instant,
    kind: RtpCodecKind,
    codec: &RTCRtpCodecParameters,
) -> Result<(Arc<TrackLocalStaticSample>, SSRC), String> {
    let mut ssrc = [0u8; 4];
    getrandom::fill(&mut ssrc).map_err(|e| e.to_string())?;
    let ssrc = SSRC::from_ne_bytes(ssrc);
    let track = TrackLocalStaticSample::new(
        started,
        MediaStreamTrack::new(
            "talvio-live-view".to_owned(),
            format!("talvio-{kind}"),
            format!("talvio-{kind}"),
            kind,
            vec![RTCRtpEncodingParameters {
                rtp_coding_parameters: RTCRtpCodingParameters {
                    ssrc: Some(ssrc),
                    ..Default::default()
                },
                codec: codec.rtp_codec.clone(),
                ..Default::default()
            }],
        ),
    )
    .map_err(|e| e.to_string())?;
    Ok((Arc::new(track), ssrc))
}

/// Turns the connection's callbacks into [`PeerEvent`]s.
struct Events {
    events: mpsc::Sender<PeerEvent>,
}

impl Events {
    fn send(&self, event: PeerEvent) {
        if self.events.try_send(event).is_err() {
            warn!("A live-view event was dropped: the session is not reading");
        }
    }
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Events {
    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        match event.candidate.to_json() {
            Ok(candidate) => self.send(PeerEvent::Candidate(candidate)),
            Err(e) => debug!("A local ICE candidate could not be described: {e}"),
        }
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        debug!("Live view connection: {state}");
        match state {
            RTCPeerConnectionState::Connected => self.send(PeerEvent::Connected),
            RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                self.send(PeerEvent::Closed)
            }
            _ => {}
        }
    }
}

fn watch_control(control: Arc<dyn DataChannel>, events: mpsc::Sender<PeerEvent>) {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = control.poll().await {
            let event = match event {
                DataChannelEvent::OnOpen => PeerEvent::ControlOpen,
                DataChannelEvent::OnMessage(message) => {
                    match String::from_utf8(message.data.to_vec()) {
                        Ok(text) => PeerEvent::Control(text),
                        Err(_) => continue,
                    }
                }
                DataChannelEvent::OnClose => break,
                _ => continue,
            };
            if events.send(event).await.is_err() {
                break;
            }
        }
    });
}

/// The phone's keyframe requests arrive as RTCP on the video sender's track.
fn watch_keyframe_requests(
    video: Arc<TrackLocalStaticSample>,
    _sender: Arc<dyn RtpSender>,
    events: mpsc::Sender<PeerEvent>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = video.poll().await {
            let TrackLocalEvent::OnRtcpPacket(packets) = event else {
                continue;
            };
            if packets
                .iter()
                .any(|packet| is_keyframe_request(packet.as_ref()))
                && events.send(PeerEvent::KeyframeRequest).await.is_err()
            {
                break;
            }
        }
    });
}

fn is_keyframe_request(packet: &dyn rtc::rtcp::Packet) -> bool {
    let payload = packet.as_any();
    payload.is::<PictureLossIndication>() || payload.is::<FullIntraRequest>()
}

/// Passes the phone's keyframe requests (PLI, FIR) on to the application and
/// lets every other inbound RTCP packet stop at the interceptors that use it.
/// Inbound RTCP reaches the application only when an interceptor marks it.
#[derive(Default)]
struct KeyframeRequests {
    reads: VecDeque<TaggedPacket>,
    writes: VecDeque<TaggedPacket>,
}

impl Protocol<TaggedPacket, TaggedPacket, ()> for KeyframeRequests {
    type Rout = TaggedPacket;
    type Wout = TaggedPacket;
    type Eout = ();
    type Error = rtc::shared::error::Error;
    type Time = Instant;

    fn handle_read(&mut self, mut message: TaggedPacket) -> Result<(), Self::Error> {
        if let Packet::Rtcp(packets) = &message.message.packet {
            let requests: Vec<_> = packets
                .iter()
                .filter(|packet| is_keyframe_request(packet.as_ref()))
                .cloned()
                .collect();
            if requests.is_empty() {
                return Ok(());
            }
            message.message.packet = Packet::Rtcp(requests);
            message.message.add(Attribute::DeliverToApplication);
        }
        self.reads.push_back(message);
        Ok(())
    }

    fn poll_read(&mut self) -> Option<Self::Rout> {
        self.reads.pop_front()
    }

    fn handle_write(&mut self, message: TaggedPacket) -> Result<(), Self::Error> {
        self.writes.push_back(message);
        Ok(())
    }

    fn poll_write(&mut self) -> Option<Self::Wout> {
        self.writes.pop_front()
    }
}

impl Interceptor for KeyframeRequests {
    fn bind_local_stream(&mut self, _info: &StreamInfo) {}
    fn unbind_local_stream(&mut self, _info: &StreamInfo) {}
    fn bind_remote_stream(&mut self, _info: &StreamInfo) {}
    fn unbind_remote_stream(&mut self, _info: &StreamInfo) {}
}
