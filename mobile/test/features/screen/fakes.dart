import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:talvio_mobile/src/core/relay_client.dart';
import 'package:talvio_mobile/src/core/session_store.dart';
import 'package:talvio_mobile/src/features/screen/live_protocol.dart';
import 'package:talvio_mobile/src/features/screen/rtc_link.dart';

import '../../helpers.dart';

/// A peer connection that only records what it is asked to do.
class FakeLink implements RtcLink {
  FakeLink(this.iceServers, this.events);

  final List<Map<String, Object?>> iceServers;
  final RtcLinkEvents events;
  String? offer;
  final candidates = <IceCandidate>[];
  final sent = <Map<String, Object?>>[];
  var open = false;
  var sound = false;
  var closed = false;
  final size = ValueNotifier<Size?>(const Size(1920, 1080));

  @override
  Future<String> answer(String offerSdp) async {
    offer = offerSdp;
    return 'v=0 answer';
  }

  @override
  Future<void> addCandidate(IceCandidate candidate) async =>
      candidates.add(candidate);

  @override
  bool send(String text) {
    if (!open || closed) return false;
    sent.add(jsonDecode(text) as Map<String, Object?>);
    return true;
  }

  @override
  set soundOn(bool on) => sound = on;

  @override
  ValueListenable<Size?> get videoSize => size;

  @override
  Widget buildVideo() =>
      const ColoredBox(color: Colors.grey, child: SizedBox.expand());

  @override
  Future<void> close() async => closed = true;

  void openChannel() {
    open = true;
    events.onChannel(true);
  }

  /// The computer sends [message] on the channel.
  void receive(Map<String, Object?> message) =>
      events.onMessage(jsonEncode(message));
}

/// Makes [FakeLink]s and keeps them.
class FakeLinks {
  final links = <FakeLink>[];
  FakeLink get last => links.last;

  Future<RtcLink> create(
    List<Map<String, Object?>> iceServers,
    RtcLinkEvents events,
  ) async {
    final link = FakeLink(iceServers, events);
    links.add(link);
    return link;
  }
}

/// Run by [screenTest] at the end of its body, before the test checks that
/// no timer is left (a tear-down would run too late for that).
final cleanups = <FutureOr<void> Function()>[];

/// A widget test whose [cleanups] run at the end of its body.
void screenTest(String description, WidgetTesterCallback body) =>
    testWidgets(description, (tester) async {
      try {
        await body(tester);
      } finally {
        final run = [...cleanups.reversed];
        cleanups.clear();
        for (final cleanup in run) {
          await cleanup();
        }
        await tester.pump();
      }
    });

/// A relay client signed in and connected to [FakeServer], with a computer
/// that has [capabilities].
Future<(FakeServer, RelayClient)> connectedRelay(
  WidgetTester tester, {
  List<String> capabilities = const ['dictate', 'text', 'windows', 'screen'],
}) async {
  final (server, relay, _) = await connectedSession(
    tester,
    capabilities: capabilities,
  );
  return (server, relay);
}

/// [connectedRelay], with its session.
Future<(FakeServer, RelayClient, SessionStore)> connectedSession(
  WidgetTester tester, {
  List<String> capabilities = const ['dictate', 'text', 'windows', 'screen'],
}) async {
  final session = await sessionStore();
  final server = FakeServer();
  final relay = RelayClient(session, connectChannel: server.connect)..connect();
  cleanups.add(relay.dispose);
  await tester.pump();
  server
    ..send({'type': 'ready', 'deviceId': 'phone', 'user': userJson()})
    ..send({
      'type': 'devices',
      'devices': [
        deviceJson(id: 'phone', kind: 'mobile', current: true),
        deviceJson(capabilities: capabilities),
      ],
    });
  await tester.pump();
  await tester.pump();
  return (server, relay, session);
}

/// The relay frames the phone sent, envelope and all.
List<Map<String, Object?>> relayFrames(FakeServer server) => [
  for (final frame in server.frames)
    if (frame['type'] == 'relay') frame,
];

/// The bodies of the relays the phone sent of [kind].
List<Map<String, Object?>> sentOf(FakeServer server, String kind) => [
  for (final frame in relayFrames(server))
    if ((frame['body']! as Map<String, Object?>)['kind'] == kind)
      frame['body']! as Map<String, Object?>,
];

/// The computer sends `progress` [stage] (with [fields]) for request [id].
Future<void> progressOf(
  WidgetTester tester,
  FakeServer server,
  String id,
  String stage, [
  Map<String, Object?> fields = const {},
]) async {
  server.relay('pc', {
    'kind': 'progress',
    'requestId': id,
    'stage': stage,
    ...fields,
  });
  await tester.pump();
  await tester.pump();
}

const offerFields = {
  'sdp': 'v=0 offer',
  'iceServers': [
    {
      'urls': ['stun:stun.example.com:3478', 'turn:turn.example.com:3478'],
      'username': 'u',
      'credential': 'c',
    },
    {'urls': 'stun:single.example.com'},
    {'username': 'no-urls'},
  ],
};
