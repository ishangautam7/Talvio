import 'dart:async';
import 'dart:io' as io;
import 'dart:math';

import 'package:flutter_test/flutter_test.dart';
import 'package:web_socket/web_socket.dart';
import 'package:web_socket_channel/adapter_web_socket_channel.dart';
import 'package:zynveliq_mobile/src/core/relay_client.dart';
import 'package:zynveliq_mobile/src/core/session_store.dart';

import '../helpers.dart';

class _MinRandom implements Random {
  @override
  double nextDouble() => 0;
  @override
  int nextInt(int max) => 0;
  @override
  bool nextBool() => false;
}

Future<void> settle() => pumpEventQueue();

void main() {
  late SessionStore session;
  late FakeServer server;

  setUp(() async {
    session = await sessionStore();
    server = FakeServer();
  });

  Future<RelayClient> connected({
    Map<String, RelayHandler> handlers = const {},
  }) async {
    final relay = RelayClient(
      session,
      handlers: handlers,
      connectChannel: server.connect,
      random: _MinRandom(),
    );
    addTearDown(relay.dispose);
    relay.connect();
    await settle();
    server.ready();
    await settle();
    return relay;
  }

  test('authenticates with a header, says hello, publishes devices', () async {
    final relay = await connected(
      handlers: {'agent.confirm': (_, _) => const Stream.empty()},
    );

    final connect = server.connects.single;
    expect(connect.uri.toString(), 'ws://sync.test:8787/v1/ws');
    expect(connect.headers, {'Authorization': 'Bearer zv1_token'});
    expect(connect.uri.query, isEmpty);
    expect(server.frames.first, {
      'type': 'hello',
      'capabilities': ['agent.confirm'],
    });
    expect(relay.status.value, RelayStatus.connected);
    expect(relay.devices.value!.map((d) => d.id), ['phone', 'pc']);
  });

  test('a request gets every progress, then its result, then ends', () async {
    final relay = await connected();
    final request = relay.request('pc', {
      'kind': 'dictate',
      'audio': 'UklGRg==',
    });
    final replies = request.replies.toList();
    await settle();

    final frame = server.frames.last;
    expect(frame['id'], request.id);
    expect(frame['to'], 'pc');
    expect(frame['body'], {
      'kind': 'dictate',
      'audio': 'UklGRg==',
      'requestId': request.id,
    });

    final progress = {
      'kind': 'progress',
      'requestId': request.id,
      'stage': 'transcribing',
    };
    final step = {
      'kind': 'progress',
      'requestId': request.id,
      'stage': 'agent.step',
      'n': 1,
    };
    final result = {
      'kind': 'result',
      'requestId': request.id,
      'ok': true,
      'text': 'hi',
      'pasted': true,
    };
    server.relay('pc', progress);
    // Neither another request's reply nor another device may answer this one.
    server.relay('pc', {'kind': 'result', 'requestId': 'other', 'ok': true});
    server.relay('laptop', {
      'kind': 'result',
      'requestId': request.id,
      'ok': false,
      'error': 'busy',
    });
    server.relay('pc', step);
    server.relay('pc', result);
    server.relay('pc', result);

    expect(await replies, [progress, step, result]);
  });

  test('two requests in flight are kept apart', () async {
    final relay = await connected();
    final a = relay.request('pc', {'kind': 'text', 'text': 'a'});
    final b = relay.request('pc', {'kind': 'text', 'text': 'b'});
    final aReplies = a.replies.toList();
    final bReplies = b.replies.toList();
    expect(a.id, isNot(b.id));

    server.relay('pc', {'kind': 'result', 'requestId': b.id, 'ok': true});
    server.relay('pc', {
      'kind': 'result',
      'requestId': a.id,
      'ok': false,
      'error': 'busy',
    });

    expect((await aReplies).single['error'], 'busy');
    expect((await bReplies).single['ok'], isTrue);
  });

  test('relay_failed ends the request as offline', () async {
    final relay = await connected();
    final request = relay.request('pc', {'kind': 'text', 'text': 'hi'});
    server.send({
      'type': 'relay_failed',
      'id': request.id,
      'to': 'pc',
      'reason': 'offline',
    });

    await expectLater(
      request.replies.toList(),
      throwsA(
        isA<RelayException>().having(
          (e) => e.error,
          'error',
          RelayError.offline,
        ),
      ),
    );
  });

  test('a request with no connection fails at once', () async {
    final relay = RelayClient(session, connectChannel: server.connect);
    addTearDown(relay.dispose);
    final request = relay.request('pc', {'kind': 'text', 'text': 'hi'});
    await expectLater(
      request.replies.toList(),
      throwsA(
        isA<RelayException>().having(
          (e) => e.error,
          'error',
          RelayError.notConnected,
        ),
      ),
    );
  });

  test(
    'cancel asks the same device; the result still ends the request',
    () async {
      final relay = await connected();
      final request = relay.request('pc', {'kind': 'dictate', 'audio': ''});
      final replies = request.replies.toList();
      relay.cancel(request.id);
      relay.cancel('unknown');
      await settle();

      // Once, for the known request only.
      expect(
        server.relays.where((r) => (r['body']! as Map)['kind'] == 'cancel'),
        [
          {
            'to': 'pc',
            'body': {'kind': 'cancel', 'requestId': request.id},
          },
        ],
      );

      final cancelled = {
        'kind': 'result',
        'requestId': request.id,
        'ok': false,
        'error': 'cancelled',
      };
      server.relay('pc', cancelled);
      expect(await replies, [cancelled]);
    },
  );

  test('a cancel that cannot reach the target ends the request', () async {
    final relay = await connected();
    final offline = relay.request('pc', {'kind': 'dictate', 'audio': ''});
    final offlineReplies = offline.replies.toList();
    relay.cancel(offline.id);
    await settle();
    // Sent under the request's id, so the server's relay_failed matches it.
    expect(server.frames.last['id'], offline.id);
    server.send({
      'type': 'relay_failed',
      'id': offline.id,
      'to': 'pc',
      'reason': 'offline',
    });
    await expectLater(
      offlineReplies,
      throwsA(
        isA<RelayException>().having(
          (e) => e.error,
          'error',
          RelayError.offline,
        ),
      ),
    );
  });

  test('a request does not outlive its socket, and busy says so', () async {
    final relay = await connected();
    final busy = <bool>[];
    relay.busy.addListener(() => busy.add(relay.busy.value));
    Future<void> interrupted() => expectLater(
      relay.request('pc', {'kind': 'text', 'text': 'hi'}).replies.toList(),
      throwsA(
        isA<RelayException>().having(
          (e) => e.error,
          'error',
          RelayError.interrupted,
        ),
      ),
    );

    // Closed by the server: a result sent while it is gone never arrives.
    final closedByServer = interrupted();
    await server.socket.close(1000, 'going away');
    await closedByServer;

    await Future<void>.delayed(const Duration(milliseconds: 600));
    server.ready();
    await settle();
    final closedHere = interrupted();
    relay.disconnect();
    await closedHere;
    expect(busy, [true, false, true, false]);
  });

  testWidgets('a large upload holding up pongs is not silence', (tester) async {
    final relay = RelayClient(
      session,
      connectChannel: server.connect,
      random: _MinRandom(),
      now: () => tester.binding.clock.now(),
    );
    relay.connect();
    await tester.pump();
    server.ready();
    await tester.pump();

    // About 3 MB: the server reads nothing else, pings included, until it
    // has all of it, so no pong comes for over two minutes.
    relay.request('pc', {'kind': 'dictate', 'audio': 'A' * 3000000});
    await tester.pump(const Duration(seconds: 125));
    expect(server.connects, hasLength(1));
    expect(relay.status.value, RelayStatus.connected);

    // Once it is read, the next pong confirms it and the usual limit is back.
    server.send({'type': 'pong'});
    await tester.pump(const Duration(seconds: 50));
    expect(server.connects, hasLength(1));
    await tester.pump(const Duration(seconds: 26));
    expect(server.connects, hasLength(2), reason: 'silent for 75 s');

    relay.dispose();
  });

  test(
    'requests for a kind with no handler are answered unsupported',
    () async {
      final relay = await connected();
      final incoming = <String>[];
      relay.messages.listen((m) => incoming.add(m.body['kind']! as String));

      server.relay('pc', {
        'kind': 'agent.prompt',
        'requestId': 'r1',
        'prompt': 'x',
      });
      server.relay('pc', {'kind': 'agent.note'}); // no requestId: ignored
      await settle();

      expect(server.relays, [
        {
          'to': 'pc',
          'body': {
            'kind': 'result',
            'requestId': 'r1',
            'ok': false,
            'error': 'unsupported',
          },
        },
      ]);
      expect(incoming, ['agent.prompt', 'agent.note']);
    },
  );

  test(
    'a handler streams replies under the requestId and stops on cancel',
    () async {
      final steps = StreamController<Map<String, Object?>>();
      late Map<String, Object?> received;
      await connected(
        handlers: {
          'agent.confirm': (from, body) {
            received = body;
            return steps.stream;
          },
        },
      );

      server.relay('pc', {
        'kind': 'agent.confirm',
        'requestId': 'r1',
        'question': 'Delete?',
      });
      await settle();
      expect(received['question'], 'Delete?');

      steps.add({'kind': 'progress', 'stage': 'agent.waiting'});
      await settle();
      server.relay('pc', {'kind': 'cancel', 'requestId': 'r1'});
      await settle();

      expect(server.relays, [
        {
          'to': 'pc',
          'body': {
            'kind': 'progress',
            'stage': 'agent.waiting',
            'requestId': 'r1',
          },
        },
        {
          'to': 'pc',
          'body': {
            'kind': 'result',
            'requestId': 'r1',
            'ok': false,
            'error': 'cancelled',
          },
        },
      ]);
      expect(steps.hasListener, isFalse);
    },
  );

  test('close 4401 signs out and does not reconnect', () async {
    final relay = await connected();
    await server.socket.close(4401, 'revoked');
    await Future<void>.delayed(const Duration(milliseconds: 700));

    expect(session.signedIn, isFalse);
    expect(relay.status.value, RelayStatus.disconnected);
    expect(server.connects, hasLength(1));
  });

  test('a rejected upgrade (HTTP 401) signs out', () async {
    final relay = RelayClient(
      session,
      connectChannel: (_, _) => AdapterWebSocketChannel(
        Future<WebSocket>.error(
          const io.WebSocketException('not upgraded', 401),
        ),
      ),
    );
    addTearDown(relay.dispose);
    relay.connect();
    await settle();
    expect(session.signedIn, isFalse);
  });

  test('close 4000 (replaced) does not reconnect', () async {
    await connected();
    await server.socket.close(4000, 'replaced');
    await Future<void>.delayed(const Duration(milliseconds: 700));
    expect(session.signedIn, isTrue);
    expect(server.connects, hasLength(1));
  });

  test('any other close reconnects after a backoff', () async {
    final relay = await connected();
    await server.socket.close(1000, 'restart');
    await settle();
    expect(relay.status.value, RelayStatus.connecting);
    expect(server.connects, hasLength(1));

    // Attempt 0 waits 0.5–1 s; this Random picks the low end.
    await Future<void>.delayed(const Duration(milliseconds: 600));
    expect(server.connects, hasLength(2));
    server.ready();
    await settle();
    expect(relay.status.value, RelayStatus.connected);
  });

  test('disconnect keeps the device list until sign-out', () async {
    final relay = await connected();
    relay.disconnect();
    expect(relay.status.value, RelayStatus.disconnected);
    expect(relay.devices.value, isNotNull);

    await session.clear();
    relay.disconnect();
    expect(relay.devices.value, isNull);
  });

  test(
    'reconnectDelay doubles from 1 s to 30 s, jittered into the upper half',
    () {
      final low = _MinRandom();
      expect(reconnectDelay(0, low), const Duration(milliseconds: 500));
      expect(reconnectDelay(1, low), const Duration(seconds: 1));
      expect(reconnectDelay(4, low), const Duration(seconds: 8));
      expect(reconnectDelay(5, low), const Duration(seconds: 15));
      expect(reconnectDelay(50, low), const Duration(seconds: 15));
      final random = Random(1);
      for (var attempt = 0; attempt < 10; attempt++) {
        final delay = reconnectDelay(attempt, random).inMilliseconds;
        final ceiling = min(30000, 1000 << min(attempt, 5));
        expect(delay, inInclusiveRange(ceiling ~/ 2, ceiling));
      }
    },
  );
}
