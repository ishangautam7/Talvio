import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:web_socket/testing.dart';
import 'package:web_socket/web_socket.dart';
import 'package:web_socket_channel/adapter_web_socket_channel.dart';
import 'package:talvio_mobile/src/core/models.dart';
import 'package:talvio_mobile/src/core/server_finder.dart';
import 'package:talvio_mobile/src/core/session_store.dart';

const server = 'http://sync.test:8787';

Map<String, Object?> userJson({String? name = 'Ishan'}) => {
  'id': 'user-1',
  'email': 'a@b.com',
  'name': name,
  'hasPassword': true,
  'googleLinked': false,
  'createdAt': 1729000000000,
};

Map<String, Object?> deviceJson({
  String id = 'pc',
  String name = 'ISHAN-PC',
  String kind = 'desktop',
  bool online = true,
  bool current = false,
  List<String> capabilities = const ['dictate', 'text'],
}) => {
  'id': id,
  'name': name,
  'platform': kind == 'desktop' ? 'windows' : 'android',
  'kind': kind,
  'appVersion': '2.2.0',
  'online': online,
  'lastSeenAt': 1729000000000,
  'createdAt': 1729000000000,
  'current': current,
  'capabilities': online ? capabilities : <String>[],
};

Device device({
  String id = 'pc',
  String name = 'ISHAN-PC',
  String kind = 'desktop',
  bool online = true,
  List<String> capabilities = const ['dictate', 'text'],
}) => Device.fromJson(
  deviceJson(
    id: id,
    name: name,
    kind: kind,
    online: online,
    capabilities: capabilities,
  ),
);

Map<String, Object?> authJson() => {
  'token': 'zv1_token',
  'user': userJson(),
  'device': deviceJson(id: 'phone', kind: 'mobile', current: true),
};

/// A session store over in-memory storage, signed in to [server] unless
/// [signedIn] is false.
Future<SessionStore> sessionStore({bool signedIn = true}) async {
  TestWidgetsFlutterBinding.ensureInitialized();
  SharedPreferences.setMockInitialValues({});
  FlutterSecureStorage.setMockInitialValues({});
  final prefs = await SharedPreferences.getInstance();
  // A pinned server (a development build's): discovery stays off.
  final store = SessionStore(prefs, ServerFinder(prefs), pinned: server);
  await store.load();
  if (signedIn) await store.signIn(AuthResponse.fromJson(authJson()));
  return store;
}

/// The server's side of each socket the client opened, and what it received.
class FakeServer {
  final connects = <({Uri uri, Map<String, String> headers})>[];
  final frames = <Map<String, Object?>>[];
  late WebSocket socket;

  AdapterWebSocketChannel connect(Uri uri, Map<String, String> headers) {
    connects.add((uri: uri, headers: headers));
    final (client, server) = fakes();
    socket = server;
    server.events.listen((event) {
      if (event case TextDataReceived(:final text)) {
        frames.add(jsonDecode(text) as Map<String, Object?>);
      }
    });
    return AdapterWebSocketChannel(client);
  }

  void send(Map<String, Object?> message) =>
      socket.sendText(jsonEncode(message));

  void ready() {
    send({'type': 'ready', 'deviceId': 'phone', 'user': userJson()});
    send({
      'type': 'devices',
      'devices': [
        deviceJson(id: 'phone', kind: 'mobile', current: true),
        deviceJson(),
      ],
    });
  }

  void relay(String from, Map<String, Object?> body) =>
      send({'type': 'relay', 'from': from, 'body': body});

  /// The relays the client sent, without their envelope ids.
  List<Map<String, Object?>> get relays => [
    for (final frame in frames)
      if (frame['type'] == 'relay') {'to': frame['to'], 'body': frame['body']},
  ];
}
