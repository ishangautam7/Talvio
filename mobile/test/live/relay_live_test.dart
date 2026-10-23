// Live checks against a running account server, through the app's own
// ApiClient, SessionStore and RelayClient, with a bare socket playing the
// computer. Skipped unless TALVIO_LIVE_SERVER is set:
//
//   TALVIO_LIVE_SERVER=http://127.0.0.1:8787 flutter test test/live
//
// Each run signs up a throwaway account and deletes it at the end.

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:flutter/foundation.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:talvio_mobile/src/core/api_client.dart';
import 'package:talvio_mobile/src/core/config.dart';
import 'package:talvio_mobile/src/core/models.dart';
import 'package:talvio_mobile/src/core/relay_client.dart';
import 'package:talvio_mobile/src/core/server_finder.dart';
import 'package:talvio_mobile/src/core/session_store.dart';

final _server = Platform.environment['TALVIO_LIVE_SERVER'];
const _wait = Duration(seconds: 10);

DeviceInfo _computer(String name) => DeviceInfo(
  name: name,
  platform: 'linux',
  kind: 'desktop',
  appVersion: 'test',
);

/// The first value of [listenable] that passes [test], now or later.
Future<T> _until<T>(ValueListenable<T> listenable, bool Function(T) test) {
  if (test(listenable.value)) return Future.value(listenable.value);
  final found = Completer<T>();
  void check() {
    if (!found.isCompleted && test(listenable.value)) {
      found.complete(listenable.value);
    }
  }

  listenable.addListener(check);
  return found.future
      .timeout(_wait)
      .whenComplete(() => listenable.removeListener(check));
}

/// A computer's socket, speaking the protocol by hand.
class _Computer {
  _Computer._(this._socket) {
    _socket.listen((frame) {
      _inbox.add(jsonDecode(frame as String) as Map<String, Object?>);
      _arrived.add(null);
    });
  }

  static Future<_Computer> connect(String server, String token) async {
    final url = Uri.parse(server).replace(scheme: 'ws', path: '/v1/ws');
    return _Computer._(
      await WebSocket.connect(
        url.toString(),
        headers: {'Authorization': 'Bearer $token'},
      ),
    );
  }

  final WebSocket _socket;
  final _inbox = <Map<String, Object?>>[];
  final _arrived = StreamController<void>.broadcast();

  int? get closeCode => _socket.closeCode;
  Future<void> get done => _socket.done.timeout(_wait);

  /// The next server message of [type] that passes [test], skipping others.
  Future<Map<String, Object?>> next(
    String type, [
    bool Function(Map<String, Object?>)? test,
  ]) async {
    while (true) {
      final index = _inbox.indexWhere(
        (m) => m['type'] == type && (test?.call(m) ?? true),
      );
      if (index >= 0) return _inbox.removeAt(index);
      await _arrived.stream.first.timeout(_wait);
    }
  }

  void send(Map<String, Object?> frame) => _socket.add(jsonEncode(frame));

  void reply(String to, Map<String, Object?> body) =>
      send({'type': 'relay', 'to': to, 'body': body});
}

void main() {
  setUpAll(() {
    TestWidgetsFlutterBinding.ensureInitialized();
    // The test binding answers every HTTP request itself; these must reach
    // the server.
    HttpOverrides.global = null;
  });

  test(
    'sign up, see the computer, request and reply, offline and revoked',
    () async {
      final server = _server!;
      SharedPreferences.setMockInitialValues({});
      FlutterSecureStorage.setMockInitialValues({});
      final prefs = await SharedPreferences.getInstance();
      final session = SessionStore(prefs, ServerFinder(prefs), pinned: server);
      await session.load();
      final api = ApiClient(session);
      final random = Random.secure();
      final email = 'zv-live-${random.nextInt(1 << 32)}@example.com';
      final password =
          'pw-${random.nextInt(1 << 32)}-${random.nextInt(1 << 32)}';

      expect(await api.googleEnabled(), isA<bool>());
      final phone = await api.signUp(
        email: email,
        password: password,
        name: null,
        device: thisDevice(),
      );
      await session.signIn(phone);
      expect(phone.device.current, isTrue);
      expect(phone.device.kind, 'mobile');

      final pc = await api.logIn(
        email: email,
        password: password,
        device: _computer('Live test PC'),
      );
      final offline = await api.logIn(
        email: email,
        password: password,
        device: _computer('Offline PC'),
      );
      await expectLater(
        api.logIn(
          email: email,
          password: 'not-the-password',
          device: thisDevice(),
        ),
        throwsA(
          isA<ApiException>().having(
            (e) => e.code,
            'code',
            'invalid_credentials',
          ),
        ),
      );

      final computer = await _Computer.connect(server, pc.token);
      expect((await computer.next('ready'))['deviceId'], pc.device.id);
      computer.send({
        'type': 'hello',
        'capabilities': ['dictate', 'text'],
      });

      final relay = RelayClient(session)..connect();
      addTearDown(relay.dispose);
      final devices = await _until(
        relay.devices,
        (list) =>
            list?.any(
              (d) => d.id == pc.device.id && d.capabilities.contains('text'),
            ) ??
            false,
      );
      expect(relay.status.value, RelayStatus.connected);
      final byId = {for (final d in devices!) d.id: d};
      expect(devices.first.id, phone.device.id, reason: 'current first');
      expect(byId[phone.device.id]!.current, isTrue);
      expect(byId[pc.device.id]!.online, isTrue);
      expect(byId[pc.device.id]!.capabilities, {'dictate', 'text'});
      expect(byId[offline.device.id]!.online, isFalse);
      expect(byId[offline.device.id]!.capabilities, isEmpty);
      // The phone said hello with no capabilities, and the computer sees it.
      await computer.next(
        'devices',
        (m) => (m['devices']! as List).any(
          (d) =>
              (d as Map)['id'] == phone.device.id &&
              d['online'] == true &&
              (d['capabilities']! as List).isEmpty,
        ),
      );

      // A request and its replies: any number of progress, then one result.
      final text = relay.request(pc.device.id, {'kind': 'text', 'text': 'hi'});
      final asked = await computer.next('relay');
      expect(asked['from'], phone.device.id);
      expect(asked['body'], {
        'kind': 'text',
        'text': 'hi',
        'requestId': text.id,
      });
      computer
        ..reply(phone.device.id, {
          'kind': 'progress',
          'requestId': text.id,
          'stage': 'pasting',
        })
        ..reply(phone.device.id, {
          'kind': 'result',
          'requestId': text.id,
          'ok': true,
          'pasted': true,
        });
      expect(await text.replies.toList().timeout(_wait), [
        {'kind': 'progress', 'requestId': text.id, 'stage': 'pasting'},
        {'kind': 'result', 'requestId': text.id, 'ok': true, 'pasted': true},
      ]);

      // Cancel reaches the computer, and the request ends with its result.
      final dictate = relay.request(pc.device.id, {
        'kind': 'dictate',
        'deliver': 'return',
        'audio': '',
      });
      final replies = dictate.replies.toList();
      await computer.next('relay');
      computer.reply(phone.device.id, {
        'kind': 'progress',
        'requestId': dictate.id,
        'stage': 'transcribing',
      });
      relay.cancel(dictate.id);
      final cancel = await computer.next(
        'relay',
        (m) => (m['body']! as Map)['kind'] == 'cancel',
      );
      expect(cancel['body'], {'kind': 'cancel', 'requestId': dictate.id});
      computer.reply(phone.device.id, {
        'kind': 'result',
        'requestId': dictate.id,
        'ok': false,
        'error': 'cancelled',
      });
      expect(await replies.timeout(_wait), [
        {'kind': 'progress', 'requestId': dictate.id, 'stage': 'transcribing'},
        {
          'kind': 'result',
          'requestId': dictate.id,
          'ok': false,
          'error': 'cancelled',
        },
      ]);

      // A kind the phone does not handle is answered `unsupported`.
      computer.reply(phone.device.id, {
        'kind': 'agent.test',
        'requestId': 'r1',
      });
      final unsupported = await computer.next('relay');
      expect(unsupported['from'], phone.device.id);
      expect(unsupported['body'], {
        'kind': 'result',
        'requestId': 'r1',
        'ok': false,
        'error': 'unsupported',
      });

      // A device that is not connected: relay_failed, as `offline`.
      await expectLater(
        relay
            .request(offline.device.id, {'kind': 'text', 'text': 'hi'})
            .replies
            .first,
        throwsA(
          isA<RelayException>().having(
            (e) => e.error,
            'error',
            RelayError.offline,
          ),
        ),
      );

      // Deleting the account closes every socket with 4401 and signs the
      // phone out.
      await api.deleteAccount();
      await computer.done;
      expect(computer.closeCode, 4401);
      await _until(relay.status, (s) => s == RelayStatus.disconnected);
      expect(session.signedIn, isFalse);
    },
    skip: _server == null ? 'set TALVIO_LIVE_SERVER to run' : false,
  );
}
