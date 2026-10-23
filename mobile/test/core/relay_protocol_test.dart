import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:zynveliq_mobile/src/core/relay_protocol.dart';

import '../helpers.dart';

void main() {
  group('ServerMessage.decode', () {
    test('ready', () {
      final message = ServerMessage.decode(
        jsonEncode({'type': 'ready', 'deviceId': 'phone', 'user': userJson()}),
      );
      expect(message, isA<ReadyMessage>());
      message as ReadyMessage;
      expect(message.deviceId, 'phone');
      expect(message.user.email, 'a@b.com');
      expect(message.user.name, 'Ishan');
    });

    test('devices, including an offline one with no capabilities', () {
      final message = ServerMessage.decode(
        jsonEncode({
          'type': 'devices',
          'devices': [
            deviceJson(id: 'phone', kind: 'mobile', current: true),
            deviceJson(online: false),
          ],
        }),
      );
      final devices = (message! as DevicesMessage).devices;
      expect(devices.map((d) => d.id), ['phone', 'pc']);
      expect(devices.first.current, isTrue);
      expect(devices.first.isComputer, isFalse);
      expect(devices.last.online, isFalse);
      expect(devices.last.capabilities, isEmpty);
    });

    test('relay keeps the body as sent', () {
      final body = {
        'kind': 'agent.step',
        'requestId': 'r1',
        'extra': [1, 2],
      };
      final message = ServerMessage.decode(
        jsonEncode({'type': 'relay', 'from': 'pc', 'body': body}),
      );
      message as RelayMessage;
      expect(message.from, 'pc');
      expect(message.body, body);
    });

    test('relay_failed and pong', () {
      final failed = ServerMessage.decode(
        jsonEncode({
          'type': 'relay_failed',
          'id': 'r1',
          'to': 'pc',
          'reason': 'offline',
        }),
      );
      expect((failed! as RelayFailedMessage).id, 'r1');
      expect(ServerMessage.decode('{"type":"pong"}'), isA<PongMessage>());
    });

    test('anything else is null, never a throw', () {
      for (final frame in [
        'not json',
        '[]',
        '{}',
        '{"type":"surprise"}',
        '{"type":"ready","deviceId":"x"}',
        '{"type":"ready","deviceId":"x","user":{"id":1}}',
        '{"type":"devices","devices":[{"id":"x"}]}',
        '{"type":"relay","from":"pc","body":"text"}',
        '{"type":"relay_failed"}',
      ]) {
        expect(ServerMessage.decode(frame), isNull, reason: frame);
      }
    });
  });

  test('client frames', () {
    expect(jsonDecode(ClientFrame.hello(['dictate'])), {
      'type': 'hello',
      'capabilities': ['dictate'],
    });
    expect(jsonDecode(ClientFrame.ping()), {'type': 'ping'});
    expect(
      jsonDecode(
        ClientFrame.relay(
          id: 'r1',
          to: 'pc',
          body: {'kind': 'text', 'requestId': 'r1', 'text': 'hi'},
        ),
      ),
      {
        'type': 'relay',
        'id': 'r1',
        'to': 'pc',
        'body': {'kind': 'text', 'requestId': 'r1', 'text': 'hi'},
      },
    );
    expect(failedResult('r1', 'unsupported'), {
      'kind': 'result',
      'requestId': 'r1',
      'ok': false,
      'error': 'unsupported',
    });
  });
}
