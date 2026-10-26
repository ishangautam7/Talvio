import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';

import 'live_protocol.dart';
import 'rtc_link.dart';

/// [RtcLink] on flutter_webrtc: receive only (the phone sends no camera or
/// microphone, so it asks for neither permission), one video and one audio
/// track from the computer, and the `control` channel the computer opens.
class WebRtcLink implements RtcLink {
  WebRtcLink._(this._pc, this._renderer, this._events);

  static Future<RtcLink> create(
    List<Map<String, Object?>> iceServers,
    RtcLinkEvents events,
  ) async {
    final renderer = RTCVideoRenderer();
    await renderer.initialize();
    final RTCPeerConnection pc;
    try {
      pc = await createPeerConnection({
        'iceServers': iceServers,
        'sdpSemantics': 'unified-plan',
      });
    } on Object {
      await renderer.dispose();
      rethrow;
    }
    return WebRtcLink._(pc, renderer, events).._wire();
  }

  final RTCPeerConnection _pc;
  final RTCVideoRenderer _renderer;
  final RtcLinkEvents _events;
  final _videoSize = ValueNotifier<Size?>(null);
  final _audio = <MediaStreamTrack>[];
  MediaStream? _stream;
  RTCDataChannel? _channel;
  var _soundOn = false;
  var _closed = false;

  void _wire() {
    _pc
      ..onIceCandidate = (candidate) {
        final text = candidate.candidate;
        if (_closed || text == null || text.isEmpty) return;
        _events.onCandidate(
          IceCandidate(
            candidate: text,
            sdpMid: candidate.sdpMid,
            sdpMLineIndex: candidate.sdpMLineIndex,
          ),
        );
      }
      ..onConnectionState = (state) {
        if (_closed) return;
        _events.onState(switch (state) {
          RTCPeerConnectionState.RTCPeerConnectionStateConnected =>
            RtcLinkState.connected,
          RTCPeerConnectionState.RTCPeerConnectionStateDisconnected =>
            RtcLinkState.disconnected,
          RTCPeerConnectionState.RTCPeerConnectionStateFailed =>
            RtcLinkState.failed,
          RTCPeerConnectionState.RTCPeerConnectionStateClosed =>
            RtcLinkState.closed,
          _ => RtcLinkState.connecting,
        });
      }
      ..onTrack = (event) {
        unawaited(_onTrack(event));
      }
      ..onDataChannel = _onChannel;
    _renderer.onResize = () {
      final width = _renderer.videoWidth;
      final height = _renderer.videoHeight;
      if (width > 0 && height > 0) {
        _videoSize.value = Size(width.toDouble(), height.toDouble());
      }
    };
  }

  Future<void> _onTrack(RTCTrackEvent event) async {
    if (_closed) return;
    final track = event.track;
    if (track.kind == 'audio') {
      track.enabled = _soundOn;
      _audio.add(track);
      return;
    }
    if (track.kind != 'video') return;
    // A track sent without a stream (no msid) still needs one to render.
    var stream = event.streams.firstOrNull;
    if (stream == null) {
      stream = await createLocalMediaStream('talvio-live');
      await stream.addTrack(track);
    }
    if (_closed) return;
    _stream = stream;
    _renderer.srcObject = stream;
  }

  void _onChannel(RTCDataChannel channel) {
    if (channel.label != ScreenKind.channel || _closed) return;
    _channel = channel;
    channel
      ..onDataChannelState = (state) {
        if (_closed) return;
        if (state == RTCDataChannelState.RTCDataChannelOpen) {
          _events.onChannel(true);
        } else if (state == RTCDataChannelState.RTCDataChannelClosed) {
          _events.onChannel(false);
        }
      }
      ..onMessage = (message) {
        if (!_closed && !message.isBinary) _events.onMessage(message.text);
      };
    // A channel can arrive already open.
    if (channel.state == RTCDataChannelState.RTCDataChannelOpen) {
      _events.onChannel(true);
    }
  }

  @override
  Future<String> answer(String offerSdp) async {
    await _pc.setRemoteDescription(RTCSessionDescription(offerSdp, 'offer'));
    final answer = await _pc.createAnswer();
    await _pc.setLocalDescription(answer);
    return answer.sdp ?? '';
  }

  @override
  Future<void> addCandidate(IceCandidate candidate) => _pc.addCandidate(
    RTCIceCandidate(
      candidate.candidate,
      candidate.sdpMid,
      candidate.sdpMLineIndex,
    ),
  );

  @override
  bool send(String text) {
    final channel = _channel;
    if (_closed ||
        channel == null ||
        channel.state != RTCDataChannelState.RTCDataChannelOpen) {
      return false;
    }
    unawaited(channel.send(RTCDataChannelMessage(text)).catchError((_) {}));
    return true;
  }

  @override
  set soundOn(bool on) {
    _soundOn = on;
    for (final track in _audio) {
      track.enabled = on;
    }
  }

  @override
  ValueListenable<Size?> get videoSize => _videoSize;

  @override
  Widget buildVideo() => RTCVideoView(
    _renderer,
    objectFit: RTCVideoViewObjectFit.RTCVideoViewObjectFitContain,
  );

  @override
  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    _renderer.onResize = null;
    try {
      _renderer.srcObject = null;
    } on Object {
      // Already detached.
    }
    await _ignore(_channel?.close());
    await _ignore(_pc.close());
    await _ignore(_stream?.dispose());
    await _ignore(_renderer.dispose());
    await _ignore(_pc.dispose());
    // [_videoSize] is left to the garbage collector: a viewer may still be
    // listening to it for the frame that drops this link.
  }

  /// Teardown goes on past a step the platform refuses.
  static Future<void> _ignore(Future<void>? step) async {
    try {
      await step;
    } on Object {
      // Nothing left to free there.
    }
  }
}
