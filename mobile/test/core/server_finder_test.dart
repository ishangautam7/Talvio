import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:talvio_mobile/src/core/config.dart';
import 'package:talvio_mobile/src/core/server_finder.dart';

/// A release build's reading of the answer [json].
String? parse(Object? json, {bool release = true}) =>
    serverFromDiscovery(jsonEncode(json), release: release);

http.Response answer(String domain) =>
    http.Response(jsonEncode({'domain': domain}), 200);

Future<ServerFinder> finder(
  MockClientHandler discovery, {
  Map<String, Object> saved = const {},
  DateTime Function()? now,
}) async {
  SharedPreferences.setMockInitialValues(saved);
  return ServerFinder(
    await SharedPreferences.getInstance(),
    httpClient: MockClient(discovery),
    now: now,
    release: true,
  );
}

void main() {
  test('reads domain, else the only string field', () {
    expect(parse({'domain': 'sync.example.com'}), 'https://sync.example.com');
    expect(
      parse({'domain': 'sync.example.com', 'note': 'moved'}),
      'https://sync.example.com',
    );
    expect(
      parse({'host': 'sync.example.com', 'ttl': 60}),
      'https://sync.example.com',
    );
    expect(parse({'a': 'one.example.com', 'b': 'two.example.com'}), isNull);
    expect(parse({'domain': 42, 'host': 'sync.example.com'}), isNull);
  });

  test('takes a bare host, host:port, or an https URL of one', () {
    expect(parse({'domain': ' Sync.Example.com '}), 'https://sync.example.com');
    expect(
      parse({'domain': 'sync.example.com:8443'}),
      'https://sync.example.com:8443',
    );
    expect(
      parse({'domain': 'https://sync.example.com:8443/some/path?q=1#top'}),
      'https://sync.example.com:8443',
    );
    expect(parse({'domain': '203.0.113.7'}), 'https://203.0.113.7');
  });

  test('takes an http URL only in development builds', () {
    const lan = {'domain': 'http://192.168.1.5:8787'};
    expect(parse(lan, release: false), 'http://192.168.1.5:8787');
    expect(parse(lan), isNull);
  });

  test('takes a loopback host only in development builds', () {
    for (final host in [
      'localhost:8787',
      'api.localhost',
      '127.0.0.1',
      'https://[::1]:8787',
    ]) {
      expect(parse({'domain': host}), isNull, reason: host);
    }
    expect(
      parse({'domain': 'localhost:8787'}, release: false),
      'https://localhost:8787',
    );
  });

  test('rejects a user part, other schemes, and garbage', () {
    for (final value in [
      'https://user:pw@sync.example.com',
      'user@sync.example.com',
      'ftp://sync.example.com',
      'wss://sync.example.com',
      '',
      '   ',
      'https://',
      'not a host',
      'sync.example.com:99999',
    ]) {
      expect(parse({'domain': value}, release: false), isNull, reason: value);
    }
    for (final body in ['not json', '', '[]', '"sync.example.com"', '{}']) {
      expect(serverFromDiscovery(body, release: false), isNull, reason: body);
    }
  });

  test('asks the discovery address with nothing of the user, and keeps '
      'the answer', () async {
    final sent = <http.Request>[];
    final f = await finder((request) async {
      sent.add(request);
      return answer('sync.example.com');
    });
    expect(f.server, isNull);
    f.enabled = true;
    expect(f.looking, isTrue);
    await pumpEventQueue();

    expect(f.looking, isFalse);
    expect(f.server, 'https://sync.example.com');
    expect(sent.single.method, 'GET');
    expect(sent.single.url.toString(), discoveryUrl);
    expect(sent.single.headers.keys.map((key) => key.toLowerCase()), [
      'accept',
    ]);
    expect(sent.single.body, isEmpty);
    final prefs = await SharedPreferences.getInstance();
    expect(prefs.getString('server_found'), 'https://sync.example.com');
  });

  test('follows a redirect on its own host only', () async {
    for (final (location, found) in [
      ('/v1/domain', 'https://sync.example.com'),
      ('https://elsewhere.example.com/', null),
      ('http://domain.gautamishan.com.np/v1/domain', null),
    ]) {
      final f = await finder((request) async {
        if (request.url.path == '/v1/domain') return answer('sync.example.com');
        return http.Response('', 302, headers: {'location': location});
      });
      f.enabled = true;
      await pumpEventQueue();
      expect(f.server, found, reason: location);
      f.enabled = false;
    }
  });

  test('a failed or unusable answer keeps the last good server', () async {
    for (final response in [
      http.Response('', 503),
      answer('http://sync.example.com'),
      http.Response('<html>', 200),
    ]) {
      final f = await finder(
        (_) async => response,
        saved: {'server_found': 'https://old.example.com'},
      );
      f.enabled = true;
      await pumpEventQueue();
      expect(f.server, 'https://old.example.com');
    }
  });

  testWidgets('looks again at most once a minute, and keeps looking while '
      'nothing was ever found', (tester) async {
    var lookups = 0;
    var online = false;
    final f = await finder((_) async {
      lookups++;
      if (!online) throw http.ClientException('Failed host lookup');
      return answer('sync.example.com');
    }, now: () => tester.binding.clock.now());

    f.enabled = true;
    await tester.pump();
    expect(lookups, 1);
    expect(f.server, isNull);

    // Retried a minute later, however often the server is unreachable.
    for (var i = 0; i < 3; i++) {
      f.unreachable();
    }
    await tester.pump(const Duration(seconds: 59));
    expect(lookups, 1);
    await tester.pump(const Duration(seconds: 1));
    expect(lookups, 2);

    online = true;
    await tester.pump(const Duration(minutes: 1));
    expect(lookups, 3);
    expect(f.server, 'https://sync.example.com');

    // Found: looked up again only when the server cannot be reached.
    await tester.pump(const Duration(minutes: 5));
    expect(lookups, 3);
    f.unreachable();
    await tester.pump(Duration.zero);
    expect(lookups, 4, reason: 'over a minute since the last');
    f.unreachable();
    await tester.pump(const Duration(seconds: 30));
    expect(lookups, 4);
    await tester.pump(const Duration(seconds: 30));
    expect(lookups, 5);

    // Off (Custom mode): nothing at all.
    f.enabled = false;
    f.unreachable();
    await f.lookUp();
    await tester.pump(const Duration(minutes: 2));
    expect(lookups, 5);
  });
}
