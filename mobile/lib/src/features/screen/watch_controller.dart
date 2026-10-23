import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';

import '../../core/models.dart';
import '../../core/relay_client.dart';
import '../../core/relay_protocol.dart';
import 'live_protocol.dart';
import 'rtc_link.dart';

/// Keys sent on the channel and not yet acknowledged, at most; a tap past
/// that is ignored, so a stuck computer is not sent a burst of keys later.
const maxPendingLiveKeys = 3;

enum WatchPhase {
  /// The watch is sent; nothing back yet.
  starting,

  /// The computer asks its user to allow this phone.
  approval,

  /// The offer is answered; waiting for the computer's `live`.
  connecting,
  live,
  ended,
}

/// The waits of a watch (PROTOCOL.md). Tests shorten nothing: they run on
/// fake time.
abstract final class WatchTimeouts {
  /// For the first reply, restarted by every `progress` before the offer.
  static const reply = Duration(seconds: 20);

  /// While the computer's user is asked to allow this phone.
  static const approval = Duration(minutes: 3);

  /// From the offer to `live`, after which the computer gives up too.
  static const connect = Duration(seconds: 30);
}

/// Runs one live view of [target] on [computer] at a time: the `screen.watch`
/// request and its WebRTC handshake over the relay. Live view never clicks or
/// scrolls: every watch is sent with `control: false`, and the `control`
/// channel carries sound on/off, keyframe requests and keys ([pressKeys]). Every way a watch ends goes
/// through [_end], which closes the peer connection.
class WatchController extends ChangeNotifier {
  WatchController({
    required this._relay,
    required this.computer,
    required this.target,
    required this._linkFactory,
  });

  final RelayClient _relay;
  final RtcLinkFactory _linkFactory;
  final Device computer;
  final WatchTarget target;

  var _phase = WatchPhase.ended;
  String? _endError;
  var _sound = false;
  var _reconnecting = false;
  var _foreground = true;
  var _disposed = false;

  /// Bumped by every watch, so a late callback of an earlier one is ignored.
  var _run = 0;
  RelayRequest? _request;
  StreamSubscription<Map<String, Object?>>? _replies;
  Timer? _timer;

  RtcLink? _link;

  /// The watch's offer arrived; any later one is ignored (the protocol has
  /// one offer per watch).
  var _offered = false;
  var _answered = false;
  var _remoteReady = false;
  var _channelOpen = false;
  final _localCandidates = <IceCandidate>[];
  final _remoteCandidates = <IceCandidate>[];

  /// The relay connection dropped while media still flowed: the watch goes
  /// on, but nothing more can be said over the relay (PROTOCOL.md does not
  /// carry a watch across a reconnect).
  var _signalLost = false;

  Caret? _caret;
  WatchedTarget? _watched;

  /// Keys sent and not yet acknowledged, by id.
  final _keysPending = <String>{};
  var _keySeq = 0;
  final _keyErrors = StreamController<String>.broadcast();

  WatchPhase get phase => _phase;

  /// Why the last watch ended: an error code, or null when this phone ended
  /// it (or it is still running).
  String? get endError => _endError;

  /// The computer's sound plays on this phone.
  bool get soundOn => _sound;

  /// Live, but the media path is lost for the moment.
  bool get reconnecting => _reconnecting;

  /// The app is on screen; the viewer stops drawing the picture otherwise.
  bool get foreground => _foreground;
  bool get channelOpen => _channelOpen;

  /// The text cursor in 0..1 of the picture; null when there is none.
  Caret? get caret => _caret;

  /// The window as the computer last described it.
  WatchedTarget? get watched => _watched;

  /// The peer connection of the running watch, once the offer arrived.
  RtcLink? get link => _link;

  /// The title to show: the computer's, else the label it was chosen by.
  String get title => switch (_watched?.title) {
    final title? when title.isNotEmpty => title,
    _ => target.label,
  };

  /// Sends the watch. Ends a running one first.
  void start() {
    if (_disposed) return;
    if (_phase != WatchPhase.ended) _end(null, cancelRemote: true);
    final run = ++_run;
    _phase = WatchPhase.starting;
    _endError = null;
    _reconnecting = false;
    _signalLost = false;
    _caret = null;
    _watched = null;
    final request = _relay.request(computer.id, {
      'kind': ScreenKind.watch,
      'target': target.id,
      // Watch only: this phone never clicks, scrolls or types in a watch.
      'control': false,
      'audio': _sound,
    });
    _request = request;
    _arm(WatchTimeouts.reply, 'timeout');
    _replies = request.replies.listen(
      (reply) => _onReply(run, reply),
      onError: (Object error) => _onRelayError(run, error),
    );
    _notify();
  }

  /// Presses [keys] (combos, as `windows.keys` takes them) in the watched
  /// window, or wherever the keyboard is on a watched screen. False when the
  /// channel is not open, or [maxPendingLiveKeys] still wait for their ack. A
  /// key the computer could not press arrives on [keyErrors].
  bool pressKeys(List<String> keys) {
    if (!_channelOpen || _keysPending.length >= maxPendingLiveKeys) {
      return false;
    }
    final id = 'k${++_keySeq}';
    if (!_sendControl({'t': 'keys', 'id': id, 'keys': keys})) return false;
    _keysPending.add(id);
    return true;
  }

  /// Error codes of keys the computer did not press (as `windows.keys`').
  Stream<String> get keyErrors => _keyErrors.stream;

  /// Ends the watch from this phone.
  void stop() {
    if (_phase == WatchPhase.ended) return;
    _end(null, cancelRemote: true);
  }

  /// Turns the computer's sound on or off, here and on the computer (which
  /// only sends silence while it is off).
  void setSound(bool on) {
    if (on == _sound) return;
    _sound = on;
    _link?.soundOn = on;
    _sendControl({'t': 'audio', 'on': on});
    _notify();
  }

  /// The app went to the background or came back. A watch keeps running in
  /// the background; back on screen, a fresh full frame is asked for, since
  /// the decoder may have dropped what arrived meanwhile.
  void setForeground(bool foreground) {
    if (foreground == _foreground) return;
    _foreground = foreground;
    if (foreground && _phase == WatchPhase.live) {
      _sendControl({'t': 'keyframe'});
    }
    _notify();
  }

  void _onReply(int run, Map<String, Object?> body) {
    if (run != _run) return;
    if (body['kind'] == RelayKind.result) {
      final error = body['ok'] == true
          ? null
          : (body['error'] is String ? body['error']! as String : 'unknown');
      _end(error);
      return;
    }
    if (body['kind'] != RelayKind.progress) return;
    switch (body['stage']) {
      case ScreenKind.approvalStage:
        if (_phase == WatchPhase.starting || _phase == WatchPhase.approval) {
          _phase = WatchPhase.approval;
          _arm(WatchTimeouts.approval, 'timeout');
          _notify();
        }
      case ScreenKind.offerStage:
        if (RtcOffer.tryParse(body) case final offer? when !_offered) {
          _offered = true;
          unawaited(_onOffer(run, offer));
        }
      case ScreenKind.iceStage:
        if (IceCandidate.tryParse(body['candidate']) case final candidate?) {
          _onRemoteCandidate(candidate);
        }
      case ScreenKind.liveStage:
        _timer?.cancel();
        _timer = null;
        _phase = WatchPhase.live;
        _reconnecting = false;
        _notify();
      default:
        // Still at it: before the offer, the wait starts again.
        if (_phase == WatchPhase.starting || _phase == WatchPhase.approval) {
          _phase = WatchPhase.starting;
          _arm(WatchTimeouts.reply, 'timeout');
        }
    }
  }

  Future<void> _onOffer(int run, RtcOffer offer) async {
    _phase = WatchPhase.connecting;
    _arm(WatchTimeouts.connect, 'connect_failed');
    _notify();
    final RtcLink link;
    try {
      link = await _linkFactory(offer.iceServers, _eventsFor(run));
    } on Object {
      if (run == _run) _end('connect_failed', cancelRemote: true);
      return;
    }
    if (run != _run || _phase == WatchPhase.ended) {
      unawaited(link.close());
      return;
    }
    _link = link;
    link.soundOn = _sound;
    _notify();
    final String sdp;
    try {
      sdp = await link.answer(offer.sdp);
    } on Object {
      if (run == _run) _end('connect_failed', cancelRemote: true);
      return;
    }
    if (run != _run || _phase == WatchPhase.ended) return;
    _sendSignal({'kind': ScreenKind.answer, 'sdp': sdp});
    _answered = true;
    _remoteReady = true;
    for (final candidate in _localCandidates) {
      _sendSignal({'kind': ScreenKind.ice, 'candidate': candidate.toJson()});
    }
    _localCandidates.clear();
    for (final candidate in _remoteCandidates) {
      unawaited(link.addCandidate(candidate).catchError((_) {}));
    }
    _remoteCandidates.clear();
  }

  void _onRemoteCandidate(IceCandidate candidate) {
    final link = _link;
    if (link != null && _remoteReady) {
      unawaited(link.addCandidate(candidate).catchError((_) {}));
    } else {
      _remoteCandidates.add(candidate);
    }
  }

  RtcLinkEvents _eventsFor(int run) => RtcLinkEvents(
    onCandidate: (candidate) {
      if (run != _run || _phase == WatchPhase.ended) return;
      // Candidates found before the answer went out follow it.
      if (_answered) {
        _sendSignal({'kind': ScreenKind.ice, 'candidate': candidate.toJson()});
      } else {
        _localCandidates.add(candidate);
      }
    },
    onState: (state) {
      if (run != _run || _phase == WatchPhase.ended) return;
      switch (state) {
        case RtcLinkState.connected:
          if (_reconnecting) {
            _reconnecting = false;
            _notify();
          }
        case RtcLinkState.disconnected:
          if (_phase == WatchPhase.live && !_reconnecting) {
            _reconnecting = true;
            _notify();
          }
        case RtcLinkState.failed:
          _end(
            _signalLost ? 'interrupted' : 'connect_failed',
            cancelRemote: true,
          );
        case RtcLinkState.closed:
          if (_signalLost) _end('interrupted');
        case RtcLinkState.connecting:
          break;
      }
    },
    onChannel: (open) {
      if (run != _run || _phase == WatchPhase.ended) return;
      _channelOpen = open;
      if (open) {
        // The watch carried the sound as it was when sent.
        if (_sound) _sendControl({'t': 'audio', 'on': true});
      } else if (_signalLost) {
        _end('interrupted');
        return;
      }
      _notify();
    },
    onMessage: (text) {
      if (run != _run || _phase == WatchPhase.ended) return;
      switch (ControlMessage.decode(text)) {
        case CaretMessage(:final caret):
          if (caret == _caret) return;
          _caret = caret;
          _notify();
        case TargetMessage(:final target):
          _watched = target;
          _notify();
        case AckMessage(:final id, :final ok, :final error):
          if (!_keysPending.remove(id) || ok) return;
          _keyErrors.add(error ?? 'keys_failed');
        case null:
          break;
      }
    },
  );

  void _onRelayError(int run, Object error) {
    if (run != _run) return;
    final code = switch (error) {
      RelayException(error: RelayError.offline) => 'offline',
      RelayException(error: RelayError.notConnected) => 'not_connected',
      _ => 'interrupted',
    };
    // Media goes phone to computer directly: a dropped relay socket does not
    // stop a picture that is already flowing.
    if (code == 'interrupted' && _phase == WatchPhase.live) {
      _signalLost = true;
      _request = null;
      _replies = null;
      return;
    }
    _end(code);
  }

  /// Ends the watch as [error] (null: this phone ended it). With
  /// [cancelRemote] the computer is told to stop too.
  void _end(String? error, {bool cancelRemote = false}) {
    if (_phase == WatchPhase.ended) return;
    final request = _request;
    if (cancelRemote && request != null) _relay.cancel(request.id);
    _phase = WatchPhase.ended;
    _endError = error;
    _timer?.cancel();
    _timer = null;
    _request = null;
    // Also ends the request on this phone, whatever the computer answers.
    unawaited(_replies?.cancel());
    _replies = null;
    final link = _link;
    _link = null;
    _offered = false;
    _answered = false;
    _remoteReady = false;
    _channelOpen = false;
    _reconnecting = false;
    _localCandidates.clear();
    _remoteCandidates.clear();
    _keysPending.clear();
    if (link != null) unawaited(link.close());
    _notify();
  }

  void _arm(Duration limit, String error) {
    _timer?.cancel();
    final run = _run;
    _timer = Timer(limit, () {
      if (run == _run) _end(error, cancelRemote: true);
    });
  }

  void _sendSignal(Map<String, Object?> body) {
    final request = _request;
    if (request != null) _relay.sendWithin(request.id, body);
  }

  bool _sendControl(Map<String, Object?> message) {
    if (!_channelOpen) return false;
    return _link?.send(jsonEncode(message)) ?? false;
  }

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    stop();
    _disposed = true;
    unawaited(_keyErrors.close());
    super.dispose();
  }
}
