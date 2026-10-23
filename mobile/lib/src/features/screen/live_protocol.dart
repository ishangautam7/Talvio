// The messages of live view (PROTOCOL.md, "Live view: watching and using a
// window or screen"): what the relay carries for the WebRTC handshake, and
// what goes over the `control` data channel once it is open.
//
// Parsing never throws: the computer is a trust boundary, so a message of a
// shape the protocol does not allow is dropped (null), not half-read.

import 'dart:convert';
import 'dart:ui' show Rect;

/// The capability, request kind and handshake kinds of live view.
abstract final class ScreenKind {
  static const capability = 'screen';
  static const watch = 'screen.watch';
  static const answer = 'rtc.answer';
  static const ice = 'rtc.ice';

  /// `progress` stages of a watch.
  static const offerStage = 'rtc.offer';
  static const iceStage = 'rtc.ice';
  static const liveStage = 'live';
  static const approvalStage = 'approval';

  /// The data channel the computer opens for input.
  static const channel = 'control';
}

/// One screen a `windows.list` offered (only from a computer with `screen`).
/// Its [id] follows the window id rules.
class RemoteScreen {
  const RemoteScreen({
    required this.id,
    required this.name,
    required this.width,
    required this.height,
    required this.primary,
  });

  static RemoteScreen? tryParse(Object? json) => switch (json) {
    {
      'id': String id,
      'name': String name,
      'width': num width,
      'height': num height,
    }
        when id.isNotEmpty && id.length <= 64 && width > 0 && height > 0 =>
      RemoteScreen(
        id: id,
        name: name.trim(),
        width: width.toInt(),
        height: height.toInt(),
        primary: json['primary'] == true,
      ),
    _ => null,
  };

  final String id;
  final String name;
  final int width;
  final int height;
  final bool primary;
}

/// The screens of a `windows.list` result; empty when it lists none.
List<RemoteScreen> screensOf(Map<String, Object?> result) => switch (result) {
  {'screens': List<Object?> screens} => [
    for (final json in screens) ?RemoteScreen.tryParse(json),
  ],
  _ => const [],
};

/// What a watch watches: a window or a screen from `windows.list`, with the
/// label the phone showed for it (the computer's `target` message replaces
/// the title once it arrives).
class WatchTarget {
  const WatchTarget({
    required this.id,
    required this.label,
    required this.screen,
  });

  final String id;
  final String label;

  /// A whole screen rather than one window: input goes wherever the
  /// computer's keyboard is.
  final bool screen;
}

/// An ICE candidate as the protocol writes it.
class IceCandidate {
  const IceCandidate({
    required this.candidate,
    required this.sdpMid,
    required this.sdpMLineIndex,
  });

  static IceCandidate? tryParse(Object? json) => switch (json) {
    {'candidate': String candidate} => IceCandidate(
      candidate: candidate,
      sdpMid: json['sdpMid'] is String ? json['sdpMid']! as String : null,
      sdpMLineIndex: json['sdpMLineIndex'] is int
          ? json['sdpMLineIndex']! as int
          : null,
    ),
    _ => null,
  };

  final String candidate;
  final String? sdpMid;
  final int? sdpMLineIndex;

  Map<String, Object?> toJson() => {
    'candidate': candidate,
    'sdpMid': sdpMid,
    'sdpMLineIndex': sdpMLineIndex,
  };
}

/// An `rtc.offer` progress: the computer's SDP and the ICE servers to use.
class RtcOffer {
  const RtcOffer(this.sdp, this.iceServers);

  static RtcOffer? tryParse(Map<String, Object?> body) => switch (body) {
    {'sdp': String sdp} when sdp.isNotEmpty => RtcOffer(
      sdp,
      _iceServers(body['iceServers']),
    ),
    _ => null,
  };

  final String sdp;

  /// In the shape WebRTC's configuration takes: `urls` a list of strings,
  /// `username` and `credential` only when given.
  final List<Map<String, Object?>> iceServers;

  static List<Map<String, Object?>> _iceServers(Object? json) => [
    if (json is List<Object?>)
      for (final server in json)
        if (server case {'urls': final Object urls})
          if (_urls(urls) case final list when list.isNotEmpty)
            {
              'urls': list,
              if (server['username'] case final String username)
                'username': username,
              if (server['credential'] case final String credential)
                'credential': credential,
            },
  ];

  /// `urls` may be one string or a list (as in the browser's RTCIceServer).
  static List<String> _urls(Object urls) => switch (urls) {
    String url => [url],
    List<Object?> list => list.whereType<String>().toList(),
    _ => const [],
  };
}

/// Where the text cursor is, as a rect in 0..1 of the picture.
typedef Caret = Rect;

/// The watched window as the computer last described it.
class WatchedTarget {
  const WatchedTarget({
    required this.title,
    required this.width,
    required this.height,
    required this.state,
  });

  final String title;
  final int width;
  final int height;

  /// normal | minimized | covered
  final String state;
}

/// A message the computer sends on the `control` channel.
sealed class ControlMessage {
  const ControlMessage();

  static ControlMessage? decode(String text) {
    final Object? json;
    try {
      json = jsonDecode(text);
    } on FormatException {
      return null;
    }
    return switch (json) {
      {'t': 'ack', 'id': String id} => AckMessage(
        id,
        ok: json['ok'] == true,
        error: json['error'] is String ? json['error']! as String : null,
        label: json['label'] is String ? json['label']! as String : null,
      ),
      {'t': 'caret', 'x': num x, 'y': num y, 'w': num w, 'h': num h}
          when _unit(x) && _unit(y) && w >= 0 && h >= 0 =>
        CaretMessage(
          Rect.fromLTWH(
            x.toDouble(),
            y.toDouble(),
            w.toDouble(),
            h.toDouble(),
          ).intersect(const Rect.fromLTWH(0, 0, 1, 1)),
        ),
      // No text cursor. A caret with a position out of range is dropped.
      final Map<String, Object?> map && {'t': 'caret'}
          when !map.containsKey('x') =>
        const CaretMessage(null),
      {'t': 'target'} => TargetMessage(
        WatchedTarget(
          title: json['title'] is String
              ? (json['title']! as String).trim()
              : '',
          width: json['width'] is num ? (json['width']! as num).toInt() : 0,
          height: json['height'] is num ? (json['height']! as num).toInt() : 0,
          state: json['state'] is String ? json['state']! as String : 'normal',
        ),
      ),
      _ => null,
    };
  }

  static bool _unit(num v) => v >= 0 && v <= 1;
}

class AckMessage extends ControlMessage {
  const AckMessage(this.id, {required this.ok, this.error, this.label});
  final String id;
  final bool ok;
  final String? error;

  /// The control's label, with `error: "confirm"`.
  final String? label;
}

class CaretMessage extends ControlMessage {
  const CaretMessage(this.caret);

  /// Null: there is no text cursor.
  final Caret? caret;
}

class TargetMessage extends ControlMessage {
  const TargetMessage(this.target);
  final WatchedTarget target;
}

/// How an input message ended: [error] null when the computer did it.
class InputResult {
  const InputResult.ok() : error = null, label = null;
  const InputResult.failed(String this.error, {this.label});

  /// An ack's error code, or the phone's own: `timeout`, `not_live`,
  /// `cancelled`.
  final String? error;

  /// With `confirm`: what the control is labelled.
  final String? label;

  bool get ok => error == null;
  bool get needsConfirm => error == 'confirm';
}
