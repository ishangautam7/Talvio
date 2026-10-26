import 'dart:async';
import 'dart:io' as io;
import 'dart:math';

import 'package:flutter/foundation.dart';
import 'package:web_socket_channel/io.dart';
import 'package:web_socket_channel/web_socket_channel.dart';

import 'models.dart';
import 'relay_protocol.dart';
import 'session_store.dart';

typedef ChannelFactory =
    WebSocketChannel Function(Uri uri, Map<String, String> headers);

/// Serves one request kind another device sends this phone. Every body the
/// stream yields is sent back with the request's `requestId`: any number of
/// `progress`, then one `result`. Cancelling the request cancels the stream.
typedef RelayHandler =
    Stream<Map<String, Object?>> Function(
      String from,
      Map<String, Object?> body,
    );

enum RelayStatus { disconnected, connecting, connected }

enum RelayError {
  /// The target device is not connected to the server.
  offline,

  /// This phone is not connected to the server.
  notConnected,

  /// This phone's connection closed while the request waited: its result,
  /// if the target sent one meanwhile, is lost, so the target may or may not
  /// have done what it asked.
  interrupted,
}

class RelayException implements Exception {
  const RelayException(this.error);
  final RelayError error;

  @override
  String toString() => 'RelayException(${error.name})';
}

/// A request this phone sent. [replies] carries every `progress` and the one
/// `result` (then it is done), or fails with a [RelayException].
class RelayRequest {
  const RelayRequest._(this.id, this.replies);
  final String id;
  final Stream<Map<String, Object?>> replies;
}

/// A relay body that reached this phone, with the device that sent it.
class IncomingRelay {
  const IncomingRelay(this.from, this.body);
  final String from;
  final Map<String, Object?> body;
}

/// The phone's one socket to the account server: presence, the account's
/// device list, and request/reply with other devices (PROTOCOL.md).
class RelayClient {
  RelayClient(
    this._session, {
    Map<String, RelayHandler> handlers = const {},
    ChannelFactory? connectChannel,
    Random? random,
    DateTime Function()? now,
  }) : _handlers = Map.unmodifiable(handlers),
       _connectChannel = connectChannel ?? _ioChannel,
       _random = random ?? Random(),
       _now = now ?? DateTime.now;

  static const pingInterval = Duration(seconds: 25);

  /// Two missed pongs: the connection is dead even if the OS has not noticed
  /// (a phone moving from Wi-Fi to mobile data keeps a half-open socket).
  static const _silenceLimit = Duration(seconds: 60);

  /// The slowest upload (bytes per second) a large frame is given time for.
  /// The server answers a ping only once it has read what was sent before
  /// it, so a 5-minute dictation (about 13 MB) delays every pong for as long
  /// as its upload takes. Slower than this, it would time out anyway (60 s
  /// plus its length, while its audio is about 43 KB/s of base64).
  static const _slowestUpload = 32 * 1024;

  final SessionStore _session;

  /// By request kind. The kinds are also this phone's `hello.capabilities`.
  final Map<String, RelayHandler> _handlers;
  final ChannelFactory _connectChannel;
  final Random _random;
  final DateTime Function() _now;

  final _devices = ValueNotifier<List<Device>?>(null);
  final _status = ValueNotifier(RelayStatus.disconnected);
  final _incoming = StreamController<IncomingRelay>.broadcast();
  final _pending = <String, _Pending>{};
  final _busy = ValueNotifier(false);
  final _serving = <String, StreamSubscription<Map<String, Object?>>>{};

  WebSocketChannel? _channel;

  /// The server the socket was last opened to.
  String? _url;
  StreamSubscription<Object?>? _subscription;
  Timer? _pingTimer;
  Timer? _retryTimer;
  int _attempt = 0;
  bool _wanted = false;
  bool _rejected = false;
  DateTime _lastHeard = DateTime.now();

  /// Characters sent on this socket; as of each ping not yet answered; and as
  /// of the last one answered, which the server has therefore read.
  int _sent = 0;
  final _pingMarks = <int>[];
  int _confirmed = 0;

  /// Every device on the account, as of the last `devices` message; null
  /// until the first one. Kept while disconnected so screens do not flicker
  /// on a reconnect.
  ValueListenable<List<Device>?> get devices => _devices;

  ValueListenable<RelayStatus> get status => _status;

  /// Whether a request this phone sent still waits for its result.
  ValueListenable<bool> get busy => _busy;

  /// Every relay body that reaches this phone, replies included.
  Stream<IncomingRelay> get messages => _incoming.stream;

  /// Connects, and keeps reconnecting until [disconnect]. A session that
  /// moved to its server's new address (discovery) reconnects there.
  void connect() {
    _wanted = true;
    if (_url != _session.session?.serverUrl) _stop();
    if (_channel == null && _retryTimer == null) _open();
  }

  void disconnect() {
    _wanted = false;
    _stop();
    if (!_session.signedIn) _devices.value = null;
  }

  /// Sends [body] to device [to] under a new `requestId`.
  RelayRequest request(String to, Map<String, Object?> body) {
    final id = _newId();
    final controller = StreamController<Map<String, Object?>>(
      onCancel: () => _settle(id),
    );
    _pending[id] = _Pending(to, controller);
    _busy.value = true;
    final sent = _send(
      ClientFrame.relay(id: id, to: to, body: {...body, 'requestId': id}),
    );
    if (!sent) _fail(id, RelayError.notConnected);
    return RelayRequest._(id, controller.stream);
  }

  /// Asks the target to stop request [requestId]. Its `result` still arrives
  /// on the request's stream (`cancelled`, or the real result if it had
  /// already finished). A cancel that cannot reach the target ends the
  /// request at once, as [RelayError.notConnected] or [RelayError.offline].
  void cancel(String requestId) {
    final pending = _pending[requestId];
    if (pending == null) return;
    // Under the request's own id, so the server's `relay_failed` for it (the
    // target went offline) ends the request.
    final sent = _send(
      ClientFrame.relay(
        id: requestId,
        to: pending.to,
        body: {'kind': RelayKind.cancel, 'requestId': requestId},
      ),
    );
    if (!sent) _fail(requestId, RelayError.notConnected);
  }

  /// Sends [body] to the device request [requestId] went to, as part of that
  /// request rather than a new one: under the request's own `requestId` and
  /// relay `id`, so a `relay_failed` for it ends the request as offline (a
  /// live view's `rtc.answer` and `rtc.ice`). False when the request has
  /// ended or this phone is not connected; nothing is retried.
  bool sendWithin(String requestId, Map<String, Object?> body) {
    final pending = _pending[requestId];
    if (pending == null) return false;
    return _send(
      ClientFrame.relay(
        id: requestId,
        to: pending.to,
        body: {...body, 'requestId': requestId},
      ),
    );
  }

  void dispose() {
    disconnect();
    _incoming.close();
    _devices.dispose();
    _status.dispose();
    _busy.dispose();
  }

  void _open() {
    final session = _session.session;
    if (session == null) return;
    _url = session.serverUrl;
    _status.value = RelayStatus.connecting;
    _rejected = false;
    _lastHeard = _now();
    _sent = 0;
    _confirmed = 0;
    _pingMarks.clear();
    final base = Uri.parse(session.serverUrl);
    final uri = base.replace(
      scheme: base.scheme == 'https' ? 'wss' : 'ws',
      path: '${base.path}/v1/ws',
    );
    final channel = _connectChannel(uri, {
      'Authorization': 'Bearer ${session.token}',
    });
    _channel = channel;
    // Also catches a socket that upgrades but never says `ready`.
    _pingTimer = Timer.periodic(pingInterval, (_) => _ping());
    // A failed connect also fails `ready`; the stream's onError handles it.
    channel.ready.ignore();
    _subscription = channel.stream.listen(
      _onFrame,
      onError: (Object error) {
        final status = _httpStatus(error);
        if (status == 401) _rejected = true;
        // No answer at all, not an HTTP error: the server may have moved.
        if (status == null && _status.value == RelayStatus.connecting) {
          _session.finder.unreachable();
        }
      },
      onDone: () => _onClosed(channel),
    );
  }

  void _onFrame(Object? frame) {
    if (frame is! String) return;
    _lastHeard = _now();
    switch (ServerMessage.decode(frame)) {
      case ReadyMessage(:final user):
        _attempt = 0;
        _status.value = RelayStatus.connected;
        _send(ClientFrame.hello(_handlers.keys));
        unawaited(_session.updateUser(user));
      case DevicesMessage(:final devices):
        _devices.value = List.unmodifiable(devices);
      case RelayMessage(:final from, :final body):
        _onRelay(from, body);
      case RelayFailedMessage(:final id):
        _fail(id, RelayError.offline);
      case PongMessage():
        if (_pingMarks.isNotEmpty) _confirmed = _pingMarks.removeAt(0);
      case null:
        break;
    }
  }

  void _onRelay(String from, Map<String, Object?> body) {
    final kind = body['kind'];
    final requestId = body['requestId'];
    if (kind is! String) return;
    _incoming.add(IncomingRelay(from, body));
    if (requestId is! String) return;

    switch (kind) {
      case RelayKind.progress || RelayKind.result:
        final pending = _pending[requestId];
        // Only the device the request went to may answer it.
        if (pending == null || pending.to != from) return;
        pending.controller.add(body);
        if (kind == RelayKind.result) {
          _settle(requestId);
          pending.controller.close();
        }
      case RelayKind.cancel:
        final serving = _serving.remove('$from/$requestId');
        if (serving == null) return;
        serving.cancel();
        _sendTo(from, failedResult(requestId, 'cancelled'));
      default:
        _serve(from, requestId, kind, body);
    }
  }

  void _serve(
    String from,
    String requestId,
    String kind,
    Map<String, Object?> body,
  ) {
    final handler = _handlers[kind];
    if (handler == null) {
      _sendTo(from, failedResult(requestId, 'unsupported'));
      return;
    }
    final key = '$from/$requestId';
    _serving[key] = handler(from, body).listen((reply) {
      _sendTo(from, {...reply, 'requestId': requestId});
      if (reply['kind'] == RelayKind.result) _serving.remove(key)?.cancel();
    }, onDone: () => _serving.remove(key));
  }

  void _onClosed(WebSocketChannel channel) {
    if (!identical(channel, _channel)) return;
    final code = channel.closeCode;
    _teardown();
    if (_rejected || code == 4401) {
      // Token revoked or account deleted: sign out, never reconnect.
      _wanted = false;
      unawaited(_session.clear());
    } else if (code == 4000) {
      // This token connected again elsewhere; that socket wins.
      _wanted = false;
    } else if (_wanted) {
      _status.value = RelayStatus.connecting;
      _retryTimer = Timer(reconnectDelay(_attempt++, _random), () {
        _retryTimer = null;
        if (_wanted) _open();
      });
    }
  }

  void _stop() {
    _retryTimer?.cancel();
    _retryTimer = null;
    _attempt = 0;
    final channel = _channel;
    // Our own close: not a lost connection for [_onClosed] to retry.
    _channel = null;
    channel?.sink.close(1000);
    _teardown();
  }

  void _ping() {
    final channel = _channel;
    final uploading = Duration(
      milliseconds: (_sent - _confirmed) * 1000 ~/ _slowestUpload,
    );
    if (channel != null &&
        _now().difference(_lastHeard) > _silenceLimit + uploading) {
      channel.sink.close();
      _onClosed(channel);
      return;
    }
    if (_send(ClientFrame.ping())) _pingMarks.add(_sent);
  }

  void _teardown() {
    _pingTimer?.cancel();
    _pingTimer = null;
    _subscription?.cancel();
    _subscription = null;
    _channel = null;
    _status.value = RelayStatus.disconnected;
    // A reply sent to this socket after it went is gone for good, and the
    // server cannot say which: a request does not outlive its socket.
    for (final id in [..._pending.keys]) {
      _fail(id, RelayError.interrupted);
    }
  }

  bool _send(String frame) {
    final channel = _channel;
    if (channel == null || _status.value != RelayStatus.connected) {
      return false;
    }
    channel.sink.add(frame);
    _sent += frame.length;
    return true;
  }

  void _sendTo(String to, Map<String, Object?> body) =>
      _send(ClientFrame.relay(id: _newId(), to: to, body: body));

  _Pending? _settle(String requestId) {
    final pending = _pending.remove(requestId);
    _busy.value = _pending.isNotEmpty;
    return pending;
  }

  void _fail(String requestId, RelayError error) {
    final pending = _settle(requestId);
    if (pending == null) return;
    pending.controller
      ..addError(RelayException(error))
      ..close();
  }

  /// A random v4 UUID.
  String _newId() {
    final random = Random.secure();
    final bytes = List<int>.generate(16, (_) => random.nextInt(256));
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    final hex = bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
    return '${hex.substring(0, 8)}-${hex.substring(8, 12)}-'
        '${hex.substring(12, 16)}-${hex.substring(16, 20)}-${hex.substring(20)}';
  }

  /// The HTTP status a refused upgrade answered with; null when the server
  /// did not answer.
  static int? _httpStatus(Object error) => switch (error) {
    WebSocketChannelException(
      inner: io.WebSocketException(httpStatusCode: final int status),
    ) =>
      status,
    _ => null,
  };

  static WebSocketChannel _ioChannel(Uri uri, Map<String, String> headers) =>
      IOWebSocketChannel.connect(
        uri,
        headers: headers,
        connectTimeout: const Duration(seconds: 15),
      );
}

class _Pending {
  _Pending(this.to, this.controller);
  final String to;
  final StreamController<Map<String, Object?>> controller;
}

/// The wait before reconnect attempt [attempt] (0-based): 1 s doubling to
/// 30 s, each scaled into its upper half at random so phones that lost the
/// server together do not return together.
Duration reconnectDelay(int attempt, Random random) {
  final ceiling = min(30000, 1000 << min(attempt, 5));
  return Duration(
    milliseconds: (ceiling * (0.5 + random.nextDouble() * 0.5)).round(),
  );
}
