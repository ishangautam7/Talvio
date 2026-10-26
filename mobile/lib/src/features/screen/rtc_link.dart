import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import 'live_protocol.dart';

/// Where the peer connection is, simplified to what the viewer shows.
enum RtcLinkState {
  connecting,
  connected,

  /// Was connected and lost the path; WebRTC may still recover it.
  disconnected,
  failed,
  closed,
}

/// What the peer connection reports back to its owner.
class RtcLinkEvents {
  const RtcLinkEvents({
    required this.onCandidate,
    required this.onState,
    required this.onChannel,
    required this.onMessage,
  });

  /// A local ICE candidate to send to the computer.
  final void Function(IceCandidate candidate) onCandidate;
  final void Function(RtcLinkState state) onState;

  /// The `control` data channel opened (true) or closed (false).
  final void Function(bool open) onChannel;

  /// A text message on the `control` channel.
  final void Function(String text) onMessage;
}

/// The one WebRTC session of a watch, as small as the controller needs it:
/// everything that talks to the platform is behind this, so the controller
/// runs in tests against a fake.
abstract class RtcLink {
  /// Applies the computer's offer and answers it; the answer's SDP.
  Future<String> answer(String offerSdp);

  /// A remote ICE candidate.
  Future<void> addCandidate(IceCandidate candidate);

  /// Sends one text message on the `control` channel. False when it is not
  /// open.
  bool send(String text);

  /// Plays the computer's sound on this phone, or not.
  set soundOn(bool on);

  /// The picture's size in pixels once a frame has arrived.
  ValueListenable<Size?> get videoSize;

  /// The picture, letterboxed to fit (contain) the space it is given.
  Widget buildVideo();

  /// Ends the session and frees everything it holds. Safe to call twice.
  Future<void> close();
}

/// Makes the link for one offer, with the offer's ICE servers.
typedef RtcLinkFactory =
    Future<RtcLink> Function(
      List<Map<String, Object?>> iceServers,
      RtcLinkEvents events,
    );
