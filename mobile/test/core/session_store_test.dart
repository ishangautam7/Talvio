import 'dart:convert';
import 'dart:io' as io;

import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:web_socket/web_socket.dart';
import 'package:web_socket_channel/adapter_web_socket_channel.dart';
import 'package:zynveliq_mobile/src/core/api_client.dart';
import 'package:zynveliq_mobile/src/core/models.dart';
import 'package:zynveliq_mobile/src/core/relay_client.dart';
import 'package:zynveliq_mobile/src/core/server_finder.dart';
import 'package:zynveliq_mobile/src/core/session_store.dart';

import '../helpers.dart';

http.Response answer(String domain) =>
    http.Response(jsonEncode({'domain': domain}), 200);

/// A session store of a release build over [saved] preferences, signed out.
Future<SessionStore> store(
  Map<String, Object> saved, {
  String? pinned,
  String? fallback,
  MockClientHandler? discovery,
  DateTime Function()? now,
}) async {
  TestWidgetsFlutterBinding.ensureInitialized();
  SharedPreferences.setMockInitialValues(saved);
  FlutterSecureStorage.setMockInitialValues({});
  final prefs = await SharedPreferences.getInstance();
  return SessionStore(
    prefs,
    ServerFinder(
      prefs,
      httpClient: MockClient(
        discovery ?? (_) async => throw http.ClientException('offline'),
      ),
      now: now,
      release: true,
    ),
    pinned: pinned,
    fallback: fallback,
  );
}

void main() {
  test(
    'the server: the build, then Custom, discovered, the fallback',
    () async {
      const custom = 'https://custom.example.com';
      const found = 'https://found.example.com';
      const pinned = 'https://pinned.example.com';
      const fallback = 'http://10.0.2.2:8787';
      const both = {'server_url': custom, 'server_found': found};

      Future<(String?, ServerChoice)> resolve(
        Map<String, Object> saved, {
        String? pinned,
        String? fallback,
      }) async {
        final s = await store(saved, pinned: pinned, fallback: fallback);
        return (s.serverUrl, s.serverChoice);
      }

      expect(await resolve(both, pinned: pinned, fallback: fallback), (
        pinned,
        ServerChoice.pinned,
      ));
      expect(await resolve(both, fallback: fallback), (
        custom,
        ServerChoice.custom,
      ));
      expect(await resolve({'server_found': found}, fallback: fallback), (
        found,
        ServerChoice.automatic,
      ));
      expect(await resolve({}, fallback: fallback), (
        fallback,
        ServerChoice.automatic,
      ));
      expect(await resolve({}), (null, ServerChoice.automatic));
    },
  );

  test('Automatic: the session follows the discovered server, and the '
      'socket reconnects there with the same token', () async {
    var domain = 'a.example.com';
    final s = await store({}, discovery: (_) async => answer(domain));
    await s.load();
    await pumpEventQueue();
    await s.signIn(AuthResponse.fromJson(authJson()));
    expect(s.session!.automatic, isTrue);
    expect(s.serverUrl, 'https://a.example.com');

    final server = FakeServer();
    final relay = RelayClient(s, connectChannel: server.connect);
    // As the app does on every session change.
    s.addListener(relay.connect);
    addTearDown(() {
      s.removeListener(relay.connect);
      relay.dispose();
    });
    relay.connect();
    await pumpEventQueue();
    server.ready();
    await pumpEventQueue();

    domain = 'b.example.com:8443';
    await s.finder.lookUp();
    await pumpEventQueue();

    expect(s.session!.serverUrl, 'https://b.example.com:8443');
    expect(s.session!.token, 'zv1_token');
    expect(
      [for (final c in server.connects) c.uri.toString()],
      ['wss://a.example.com/v1/ws', 'wss://b.example.com:8443/v1/ws'],
    );
    expect(server.connects.last.headers, {'Authorization': 'Bearer zv1_token'});

    // Kept for the next launch, even with no answer then.
    final prefs = await SharedPreferences.getInstance();
    final next = SessionStore(
      prefs,
      ServerFinder(
        prefs,
        httpClient: MockClient((_) async => throw http.ClientException('x')),
      ),
    );
    await next.load();
    await pumpEventQueue();
    expect(next.session!.serverUrl, 'https://b.example.com:8443');
    expect(next.session!.automatic, isTrue);
  });

  test('Custom: discovery stays off and the session stays bound', () async {
    var lookups = 0;
    final s = await store(
      {
        'server_url': 'https://custom.example.com',
        'server_found': 'https://found.example.com',
      },
      discovery: (_) async {
        lookups++;
        return answer('elsewhere.example.com');
      },
    );
    await s.load();
    await s.signIn(AuthResponse.fromJson(authJson()));
    s.finder.unreachable();
    await s.finder.lookUp();
    await pumpEventQueue();

    expect(lookups, 0);
    expect(s.session!.automatic, isFalse);
    expect(s.serverUrl, 'https://custom.example.com');

    // A session saved before discovery existed is bound too.
    final old = {...s.session!.toJson()}..remove('automatic');
    expect(Session.fromJson(old).automatic, isFalse);
  });

  testWidgets('a server that does not answer is looked up again, one that '
      'answers with an error is not', (tester) async {
    var lookups = 0;
    final s = await store(
      {},
      discovery: (_) async {
        lookups++;
        return answer('sync.example.com');
      },
      now: () => tester.binding.clock.now(),
    );
    await s.load();
    await tester.pump();
    expect(lookups, 1);

    // The HTTP API: an error answer, then no answer.
    var reachable = true;
    final api = ApiClient(
      s,
      httpClient: MockClient((_) async {
        if (!reachable) throw http.ClientException('Connection refused');
        return http.Response('{"error":{"code":"internal"}}', 500);
      }),
    );
    await expectLater(api.googleEnabled(), throwsA(isA<ApiException>()));
    await tester.pump(const Duration(minutes: 2));
    expect(lookups, 1);
    reachable = false;
    await expectLater(api.googleEnabled(), throwsA(isA<ApiException>()));
    await tester.pump(Duration.zero);
    expect(lookups, 2, reason: 'over a minute since the last');

    // The socket: a refused upgrade, then no answer.
    await s.signIn(AuthResponse.fromJson(authJson()));
    Object failure = const io.WebSocketException('Not upgraded', 503);
    final relay = RelayClient(
      s,
      connectChannel: (_, _) =>
          AdapterWebSocketChannel(Future<WebSocket>.error(failure)),
    );
    relay.connect();
    await tester.pump(const Duration(minutes: 2));
    expect(lookups, 2);
    failure = const io.SocketException('Connection timed out');
    await tester.pump(const Duration(seconds: 31));
    expect(lookups, 3);

    relay.dispose();
    s.finder.enabled = false;
  });
}
