import 'dart:convert';

import 'models.dart';

/// A frame the server sends on `/v1/ws` (server/PROTOCOL.md).
sealed class ServerMessage {
  const ServerMessage();

  /// Null for a frame that is not JSON, has an unknown `type`, or breaks the
  /// protocol's shape: the server is a trust boundary, and an unknown type may
  /// come from a newer server.
  static ServerMessage? decode(String frame) {
    try {
      return switch (jsonDecode(frame)) {
        {'type': 'ready', 'deviceId': String deviceId, 'user': Object user} =>
          ReadyMessage(deviceId, User.fromJson(user)),
        {'type': 'devices', 'devices': List<Object?> devices} => DevicesMessage(
          devices.map(Device.fromJson).toList(),
        ),
        {
          'type': 'relay',
          'from': String from,
          'body': Map<String, Object?> body,
        } =>
          RelayMessage(from, body),
        {'type': 'relay_failed', 'id': String id} => RelayFailedMessage(id),
        {'type': 'pong'} => const PongMessage(),
        _ => null,
      };
    } on FormatException {
      return null;
    }
  }
}

class ReadyMessage extends ServerMessage {
  const ReadyMessage(this.deviceId, this.user);
  final String deviceId;
  final User user;
}

class DevicesMessage extends ServerMessage {
  const DevicesMessage(this.devices);
  final List<Device> devices;
}

/// A body another device of this account sent to this phone.
class RelayMessage extends ServerMessage {
  const RelayMessage(this.from, this.body);
  final String from;
  final Map<String, Object?> body;
}

/// The relay with this client `id` could not be delivered: the target is not
/// connected or not on this account.
class RelayFailedMessage extends ServerMessage {
  const RelayFailedMessage(this.id);
  final String id;
}

class PongMessage extends ServerMessage {
  const PongMessage();
}

/// Client → server frames.
abstract final class ClientFrame {
  static String hello(Iterable<String> capabilities) =>
      jsonEncode({'type': 'hello', 'capabilities': capabilities.toList()});

  static String relay({
    required String id,
    required String to,
    required Map<String, Object?> body,
  }) => jsonEncode({'type': 'relay', 'id': id, 'to': to, 'body': body});

  static String ping() => jsonEncode({'type': 'ping'});
}

/// Body kinds every feature shares. Feature kinds (`dictate`, `text`,
/// `agent.*`) live with their features.
abstract final class RelayKind {
  static const progress = 'progress';
  static const result = 'result';
  static const cancel = 'cancel';
}

/// `{kind: result, ok: false, error: code}` for [requestId].
Map<String, Object?> failedResult(String requestId, String error) => {
  'kind': RelayKind.result,
  'requestId': requestId,
  'ok': false,
  'error': error,
};
