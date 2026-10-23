import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:talvio_mobile/src/features/screen/live_protocol.dart';
import 'package:talvio_mobile/src/features/screen/rtc_link.dart';
import 'package:talvio_mobile/src/features/screen/watch_controller.dart';

import '../../helpers.dart';
import 'fakes.dart';

void main() {
  Future<(FakeServer, WatchController, FakeLinks, String)> started(
    WidgetTester tester, {
    bool control = false,
    bool screen = false,
  }) async {
    final (server, relay) = await connectedRelay(tester);
    final links = FakeLinks();
    final watch = WatchController(
      relay: relay,
      computer: relay.devices.value!.firstWhere((d) => d.isComputer),
      target: WatchTarget(
        id: screen ? 's1' : 'w1',
        label: 'Notepad',
        screen: screen,
      ),
      linkFactory: links.create,
      control: control,
    )..start();
    cleanups.add(watch.dispose);
    await tester.pump();
    final id = sentOf(server, ScreenKind.watch).last['requestId']! as String;
    return (server, watch, links, id);
  }

  /// Offer, answer, live and an open channel.
  Future<(FakeServer, WatchController, FakeLink, String)> live(
    WidgetTester tester, {
    bool control = false,
  }) async {
    final (server, watch, links, id) = await started(tester, control: control);
    await progressOf(tester, server, id, 'rtc.offer', offerFields);
    await progressOf(tester, server, id, 'live');
    links.last.openChannel();
    await tester.pump();
    return (server, watch, links.last, id);
  }

  screenTest('sends the watch and runs the handshake under its id', (
    tester,
  ) async {
    final (server, watch, links, id) = await started(tester);
    final frame = relayFrames(server).last;
    expect(frame['id'], id, reason: 'relay id is the requestId');
    expect(frame['to'], 'pc');
    expect(frame['body'], {
      'kind': 'screen.watch',
      'target': 'w1',
      'control': false,
      'audio': false,
      'requestId': id,
    });
    expect(watch.phase, WatchPhase.starting);

    await progressOf(tester, server, id, 'approval');
    expect(watch.phase, WatchPhase.approval);

    // A candidate before the offer waits for the remote description.
    await progressOf(tester, server, id, 'rtc.ice', {
      'candidate': {
        'candidate': 'candidate:early',
        'sdpMid': '0',
        'sdpMLineIndex': 0,
      },
    });
    await progressOf(tester, server, id, 'rtc.offer', offerFields);
    final link = links.last;
    expect(link.offer, 'v=0 offer');
    expect(link.iceServers, [
      {
        'urls': ['stun:stun.example.com:3478', 'turn:turn.example.com:3478'],
        'username': 'u',
        'credential': 'c',
      },
      {
        'urls': ['stun:single.example.com'],
      },
    ]);
    expect(link.candidates.map((c) => c.candidate), ['candidate:early']);
    expect(watch.phase, WatchPhase.connecting);

    final answer = relayFrames(server).last;
    expect(answer['id'], id);
    expect(answer['body'], {
      'kind': 'rtc.answer',
      'sdp': 'v=0 answer',
      'requestId': id,
    });

    await progressOf(tester, server, id, 'rtc.ice', {
      'candidate': {
        'candidate': 'candidate:late',
        'sdpMid': '0',
        'sdpMLineIndex': 0,
      },
    });
    expect(link.candidates.last.candidate, 'candidate:late');

    link.events.onCandidate(
      const IceCandidate(
        candidate: 'candidate:phone',
        sdpMid: '0',
        sdpMLineIndex: 0,
      ),
    );
    await tester.pump();
    final ice = relayFrames(server).last;
    expect(ice['id'], id);
    expect(ice['body'], {
      'kind': 'rtc.ice',
      'candidate': {
        'candidate': 'candidate:phone',
        'sdpMid': '0',
        'sdpMLineIndex': 0,
      },
      'requestId': id,
    });
    expect(
      sentOf(server, 'screen.watch'),
      hasLength(1),
      reason: 'rtc.* are not new requests',
    );

    await progressOf(tester, server, id, 'live');
    expect(watch.phase, WatchPhase.live);
  });

  screenTest('input carries ids and is matched to its ack', (tester) async {
    final (_, watch, link, _) = await live(tester, control: true);
    final pressed = watch.keys(['enter']);
    final typed = watch.text('hello');
    expect(link.sent, [
      {
        't': 'keys',
        'keys': ['enter'],
        'id': 'k1',
      },
      {'t': 'text', 'text': 'hello', 'id': 'k2'},
    ]);
    // Out of order: each ack finds its own message.
    link
      ..receive({'t': 'ack', 'id': 'k2', 'ok': false, 'error': 'paste_failed'})
      ..receive({'t': 'ack', 'id': 'k1', 'ok': true});
    expect((await pressed).ok, isTrue);
    expect((await typed).error, 'paste_failed');

    final tap = watch.tap(const Offset(0.42, 0.81));
    expect(link.sent.last, {
      't': 'tap',
      'x': 0.42,
      'y': 0.81,
      'button': 'left',
      'count': 1,
      'confirmed': false,
      'id': 'k3',
    });
    link.receive({
      't': 'ack',
      'id': 'k3',
      'ok': false,
      'error': 'confirm',
      'label': 'Send',
    });
    final first = await tap;
    expect(first.needsConfirm, isTrue);
    expect(first.label, 'Send');
    final again = watch.tap(const Offset(0.42, 0.81), confirmed: true);
    expect(link.sent.last['confirmed'], isTrue);
    link.receive({'t': 'ack', 'id': 'k4', 'ok': true});
    expect((await again).ok, isTrue);

    final scroll = watch.scroll(const Offset(0.5, 0.5), dy: -3);
    expect(link.sent.last, {
      't': 'scroll',
      'x': 0.5,
      'y': 0.5,
      'dy': -3,
      'dx': 0,
      'id': 'k5',
    });
    await tester.pump(WatchTimeouts.ack);
    expect((await scroll).error, 'timeout');
  });

  screenTest('keys and text fall back to relay requests without a channel', (
    tester,
  ) async {
    final (server, watch, _, _) = await started(tester);
    expect(watch.channelOpen, isFalse);
    final pressed = watch.keys(['tab']);
    await tester.pump();
    final body = sentOf(server, 'windows.keys').single;
    expect(body['keys'], ['tab']);
    expect(body['target'], 'w1');
    expect(body['stay'], isTrue, reason: 'the watched window stays in front');
    server.relay('pc', {
      'kind': 'result',
      'requestId': body['requestId'],
      'ok': true,
      'pressed': 1,
    });
    await tester.pump();
    expect((await pressed).ok, isTrue);
  });

  screenTest('a screen is not a window: fallback input goes to the cursor', (
    tester,
  ) async {
    final (server, watch, _, _) = await started(tester, screen: true);
    unawaited(watch.text('hi'));
    await tester.pump();
    final body = sentOf(server, 'text').single;
    expect(body.containsKey('target'), isFalse);
  });

  screenTest('tracks the caret and the target', (tester) async {
    final (_, watch, link, _) = await live(tester);
    expect(watch.caret, isNull);
    link.receive({'t': 'caret', 'x': 0.1, 'y': 0.88, 'w': 0.5, 'h': 0.03});
    expect(watch.caret, const Rect.fromLTWH(0.1, 0.88, 0.5, 0.03));
    link.receive({'t': 'caret', 'x': 2, 'y': 0.5, 'w': 0, 'h': 0.03});
    expect(
      watch.caret,
      const Rect.fromLTWH(0.1, 0.88, 0.5, 0.03),
      reason: 'outside 0..1 is ignored',
    );
    link.receive({'t': 'caret'});
    expect(watch.caret, isNull);

    expect(watch.title, 'Notepad');
    link.receive({
      't': 'target',
      'title': 'notes.txt - Notepad',
      'width': 1280,
      'height': 720,
      'state': 'covered',
    });
    expect(watch.title, 'notes.txt - Notepad');
    expect(watch.watched!.state, 'covered');
    expect(watch.watched!.width, 1280);
  });

  screenTest('sound and keyframes go over the channel', (tester) async {
    final (_, watch, link, _) = await live(tester);
    watch.setSound(true);
    expect(link.sound, isTrue);
    expect(link.sent.last, {'t': 'audio', 'on': true});
    watch.setForeground(false);
    watch.setForeground(true);
    expect(link.sent.last, {'t': 'keyframe'});
  });

  screenTest('stop cancels under the watch id and closes the link', (
    tester,
  ) async {
    final (server, watch, link, id) = await live(tester);
    final pending = watch.keys(['enter']);
    watch.stop();
    await tester.pump();
    final cancel = relayFrames(server).last;
    expect(cancel['id'], id);
    expect(cancel['body'], {'kind': 'cancel', 'requestId': id});
    expect(link.closed, isTrue);
    expect(watch.phase, WatchPhase.ended);
    expect(watch.endError, isNull);
    expect(watch.link, isNull);
    expect((await pending).error, 'cancelled');
  });

  screenTest('the computer ending it ends it with its code', (tester) async {
    final (server, watch, link, id) = await live(tester);
    server.relay('pc', {
      'kind': 'result',
      'requestId': id,
      'ok': false,
      'error': 'stopped',
    });
    await tester.pump();
    await tester.pump();
    expect(watch.phase, WatchPhase.ended);
    expect(watch.endError, 'stopped');
    expect(link.closed, isTrue);
  });

  screenTest('not live 30 seconds after the offer: connect_failed', (
    tester,
  ) async {
    final (server, watch, links, id) = await started(tester);
    await progressOf(tester, server, id, 'rtc.offer', offerFields);
    await tester.pump(WatchTimeouts.connect);
    expect(watch.endError, 'connect_failed');
    expect(links.last.closed, isTrue);
    expect(sentOf(server, 'cancel').single['requestId'], id);
  });

  screenTest('no answer at all times out', (tester) async {
    final (server, watch, _, _) = await started(tester);
    await tester.pump(WatchTimeouts.reply);
    expect(watch.endError, 'timeout');
    expect(sentOf(server, 'cancel'), hasLength(1));
  });

  screenTest('a failed peer connection ends the watch', (tester) async {
    final (_, watch, link, _) = await live(tester);
    link.events.onState(RtcLinkState.disconnected);
    expect(watch.reconnecting, isTrue);
    link.events.onState(RtcLinkState.connected);
    expect(watch.reconnecting, isFalse);
    link.events.onState(RtcLinkState.failed);
    expect(watch.endError, 'connect_failed');
    expect(link.closed, isTrue);
  });

  screenTest('a computer that went offline ends it as offline', (tester) async {
    final (server, watch, _, id) = await started(tester);
    server.send({'type': 'relay_failed', 'id': id, 'to': 'pc'});
    await tester.pump();
    expect(watch.endError, 'offline');
  });

  screenTest('Control restarts the watch once the old one has ended', (
    tester,
  ) async {
    final (server, watch, link, id) = await live(tester);
    final switched = watch.setControl(true);
    await tester.pump();
    expect(watch.restarting, isTrue);
    expect(sentOf(server, 'cancel').single['requestId'], id);
    expect(sentOf(server, 'screen.watch'), hasLength(1), reason: 'waits');
    server.relay('pc', {
      'kind': 'result',
      'requestId': id,
      'ok': false,
      'error': 'cancelled',
    });
    await tester.pump();
    await switched;
    expect(link.closed, isTrue);
    final again = sentOf(server, 'screen.watch');
    expect(again, hasLength(2));
    expect(again.last['control'], isTrue);
    expect(again.last['requestId'], isNot(id));
    expect(watch.phase, WatchPhase.starting);
    expect(watch.endError, isNull);
  });

  screenTest('a late offer for an ended watch is closed, not used', (
    tester,
  ) async {
    final (server, watch, links, id) = await started(tester);
    watch.stop();
    await progressOf(tester, server, id, 'rtc.offer', offerFields);
    expect(links.links, isEmpty, reason: 'the stream was cancelled');
  });
}
