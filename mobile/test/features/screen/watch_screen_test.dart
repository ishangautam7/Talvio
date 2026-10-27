import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:talvio_mobile/src/core/api_client.dart';
import 'package:talvio_mobile/src/core/app_services.dart';
import 'package:talvio_mobile/src/features/assistant/assistant_controller.dart';
import 'package:talvio_mobile/src/features/dictate/dictate_controller.dart';
import 'package:talvio_mobile/src/design/components.dart';
import 'package:talvio_mobile/src/features/screen/live_controller.dart';
import 'package:talvio_mobile/src/features/screen/live_protocol.dart';
import 'package:talvio_mobile/src/features/screen/watch_controller.dart';
import 'package:talvio_mobile/src/features/screen/watch_screen.dart';

import '../../app_test.dart' show lock;
import '../../helpers.dart';
import 'fakes.dart';

/// Picks [label] in the viewer's More options sheet.
Future<void> option(WidgetTester tester, String label) async {
  await tester.tap(find.byTooltip('More options'));
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 400));
  await tester.tap(find.text(label));
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 400));
}

void main() {
  late LiveController live;

  /// The viewer of window w1 on a 400×800 phone, already live with its
  /// channel open, with the app's assistant chat when [assistant]. Returns
  /// the server, the links made, and the watch's id.
  Future<(FakeServer, FakeLinks, String)> pumpViewer(
    WidgetTester tester, {
    bool assistant = false,
    List<String> capabilities = const ['dictate', 'text', 'windows', 'screen'],
  }) async {
    ambientMotion = false;
    tester.view
      ..physicalSize = const Size(400, 800)
      ..devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final (server, relay, session) = await connectedSession(
      tester,
      capabilities: capabilities,
    );
    final prefs = await SharedPreferences.getInstance();
    final links = FakeLinks();
    AssistantController? chat;
    final dictate = DictateController(relay: relay, prefs: prefs);
    live = LiveController(
      relay: relay,
      dictate: dictate,
      linkFactory: links.create,
    );
    if (assistant) {
      chat = AssistantController(relay: relay, prefs: prefs, dictate: dictate);
    }
    cleanups.add(() {
      chat?.dispose();
      live.dispose();
      dictate.dispose();
    });
    final navigator = GlobalKey<NavigatorState>();
    await tester.pumpWidget(
      AppServices(
        session: session,
        api: ApiClient(
          session,
          httpClient: MockClient((_) async => http.Response('', 204)),
        ),
        relay: relay,
        prefs: prefs,
        child: MaterialApp(
          navigatorKey: navigator,
          home: const Text('Home page'),
        ),
      ),
    );
    live.start(
      relay.devices.value!.firstWhere((d) => d.isComputer),
      const WatchTarget(id: 'w1', label: 'Notepad', screen: false),
    );
    unawaited(
      navigator.currentState!.push(
        MaterialPageRoute<void>(
          builder: (_) =>
              WatchScreen(watch: live.watch!, live: live, assistant: chat),
        ),
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    cleanups.add(() => tester.pumpWidget(const SizedBox()));
    await tester.pump();
    final id = await goLive(tester, server, links);
    return (server, links, id);
  }

  screenTest('View clicks nothing; no assistant bar without it', (
    tester,
  ) async {
    final (server, links, _) = await pumpViewer(tester, assistant: true);
    expect(find.text('Live · ISHAN-PC'), findsOneWidget);
    expect(tester.takeException(), isNull, reason: 'fits');
    expect(sentOf(server, 'screen.watch').single['control'], isFalse);

    expect(find.text('View'), findsOneWidget);
    expect(find.text('Control'), findsOneWidget);
    expect(find.byTooltip('Type'), findsOneWidget);
    for (final gone in ['Dictate into this window', 'Enter']) {
      expect(find.byTooltip(gone), findsNothing, reason: gone);
    }
    // The computer has no assistant: no bar, though the chat exists.
    expect(find.byType(TextField), findsNothing);
    expect(find.byIcon(AppIcons.micFill), findsNothing);
    expect(find.byTooltip('Keys'), findsOneWidget);
    expect(find.byTooltip('Turn sound on'), findsOneWidget);
    expect(find.text('Stop'), findsOneWidget);

    // In View a tap, a double tap and a long press only look.
    final center = tester.getCenter(find.byType(InteractiveViewer));
    await tester.tapAt(center);
    await tester.pump(const Duration(milliseconds: 500));
    await tester.longPressAt(center);
    await tester.pump(const Duration(milliseconds: 500));
    expect(links.last.sent, isEmpty);

    // Sound is the one thing sent on the channel.
    await tester.tap(find.byTooltip('Turn sound on'));
    await tester.pump();
    expect(links.last.sent, [
      {'t': 'audio', 'on': true},
    ]);
    expect(find.byTooltip('Turn sound off'), findsOneWidget);
  });

  screenTest('Control clicks, asks before Send, and scrolls', (tester) async {
    final (server, links, first) = await pumpViewer(tester);
    await tester.tap(find.text('Control'));
    await tester.pump();
    expect(sentOf(server, 'cancel').single['requestId'], first);
    expect(sentOf(server, 'screen.watch').last['control'], isTrue);
    expect(find.textContaining('tap to click'), findsOneWidget);
    await goLive(tester, server, links);
    final link = links.last;

    // The 16:9 picture sits in the middle of the 400 wide stage; its
    // centre is the picture's centre.
    final stage = tester.getRect(find.byType(InteractiveViewer));
    await tester.tapAt(stage.center);
    await tester.pump(const Duration(milliseconds: 500));
    expect(link.sent.single, {
      't': 'tap',
      'x': 0.5,
      'y': 0.5,
      'button': 'left',
      'count': 1,
      'confirmed': false,
      'id': 'k1',
    });
    link.receive({'t': 'ack', 'id': 'k1', 'ok': true});

    // A tap on the bar above the picture clicks nothing.
    await tester.tapAt(stage.topCenter + const Offset(0, 90));
    await tester.pump(const Duration(milliseconds: 500));
    expect(link.sent, hasLength(1));

    // A double tap double clicks; a long press right clicks.
    await tester.tapAt(stage.center);
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tapAt(stage.center);
    await tester.pump(const Duration(milliseconds: 500));
    expect(link.sent.last['count'], 2);
    link.receive({'t': 'ack', 'id': link.sent.last['id'], 'ok': true});
    await tester.longPressAt(stage.center);
    await tester.pump(const Duration(milliseconds: 500));
    expect(link.sent.last['button'], 'right');

    // A Send button: asked first, sent again confirmed on Do it.
    link.receive({
      't': 'ack',
      'id': link.sent.last['id'],
      'ok': false,
      'error': 'confirm',
      'label': 'Send',
    });
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.text('Press "Send" on the computer?'), findsOneWidget);
    await tester.tap(find.text('Do it'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
    expect(link.sent.last['confirmed'], isTrue);
    expect(link.sent.last['button'], 'right');

    // A refusal says why.
    link.receive({
      't': 'ack',
      'id': link.sent.last['id'],
      'ok': false,
      'error': 'password_field',
    });
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    await tester.pump(const Duration(milliseconds: 500));
    expect(
      find.text("Talvio doesn't type or click into password fields."),
      findsOneWidget,
    );

    // Two fingers dragged up together scroll the computer down; the view
    // stays where it was.
    final view = viewOf(tester).clone();
    final sentBefore = link.sent.length;
    final a = await tester.startGesture(stage.center - const Offset(40, 0));
    final b = await tester.startGesture(
      stage.center + const Offset(40, 0),
      pointer: 2,
    );
    for (var i = 0; i < 10; i++) {
      await a.moveBy(const Offset(0, -12));
      await b.moveBy(const Offset(0, -12));
      await tester.pump(const Duration(milliseconds: 16));
    }
    await a.up();
    await b.up();
    await tester.pump(const Duration(milliseconds: 500));
    final scrolls = [
      for (final m in link.sent.skip(sentBefore))
        if (m['t'] == 'scroll') m,
    ];
    expect(scrolls, isNotEmpty);
    expect(scrolls.every((m) => m['dx'] == 0), isTrue);
    expect(
      scrolls.fold<int>(0, (sum, m) => sum + (m['dy']! as int)),
      greaterThanOrEqualTo(2),
      reason: '120 points up, less the 24 that decide it',
    );
    expect(viewOf(tester), view);
  });

  screenTest('Type sends text to the watched window', (tester) async {
    final (_, links, _) = await pumpViewer(tester);
    final link = links.last;
    await tester.tap(find.byTooltip('Type'));
    await tester.pump();
    expect(tester.takeException(), isNull, reason: 'the field fits');
    await tester.enterText(find.byType(TextField), 'hello there');
    await tester.pump();
    await tester.tap(find.byTooltip('Send text'));
    await tester.pump();
    expect(link.sent.last, {'t': 'text', 'text': 'hello there', 'id': 'k1'});
    link.receive({'t': 'ack', 'id': 'k1', 'ok': true});
    await tester.pump();
    await tester.pump();
    expect(
      tester.widget<TextField>(find.byType(TextField)).controller!.text,
      isEmpty,
      reason: 'sent, so cleared',
    );
    await tester.tap(find.byTooltip('Hide typing'));
    await tester.pump();
    expect(find.byType(TextField), findsNothing);
  });

  screenTest('Stop ends the watch and closes the viewer', (tester) async {
    final (server, links, id) = await pumpViewer(tester);
    await tester.tap(find.text('Stop'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    expect(sentOf(server, 'cancel').single['requestId'], id);
    expect(links.last.closed, isTrue);
    expect(live.watch, isNull, reason: 'the small view goes too');
    expect(find.text('Home page'), findsOneWidget);
    expect(find.byType(InteractiveViewer), findsNothing);
  });

  screenTest('a view left in the background stops after two minutes', (
    tester,
  ) async {
    final (server, links, id) = await pumpViewer(tester);
    lock(tester);
    await tester.pump(const Duration(minutes: 1));
    expect(sentOf(server, 'cancel'), isEmpty, reason: 'a quick look away');
    lock(tester, locked: false);
    await tester.pump();
    lock(tester);
    await tester.pump(LiveController.backgroundLimit);
    await tester.pump();
    expect(sentOf(server, 'cancel').single['requestId'], id);
    expect(links.last.closed, isTrue);
    lock(tester, locked: false);
    await tester.pump();
  });

  screenTest('Back returns to the small view, still watching', (tester) async {
    final (server, links, _) = await pumpViewer(tester);
    await tester.tap(find.byTooltip('Back to the small view'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.text('Home page'), findsOneWidget);
    expect(sentOf(server, 'cancel'), isEmpty);
    expect(links.last.closed, isFalse);
    expect(live.watch?.phase, WatchPhase.live);
  });

  screenTest('the caret is followed until a pan', (tester) async {
    final (_, links, _) = await pumpViewer(tester);
    final link = links.last;

    link.receive({'t': 'caret', 'x': 0.5, 'y': 0.5, 'w': 0, 'h': 0.02});
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    expect(scale(tester), greaterThan(2), reason: 'zoomed in to read');

    await tester.drag(
      find.byType(InteractiveViewer),
      const Offset(60, 40),
      touchSlopX: 0,
      touchSlopY: 0,
    );
    await tester.pump();
    final moved = viewOf(tester).clone();
    link.receive({'t': 'caret', 'x': 0.1, 'y': 0.1, 'w': 0, 'h': 0.02});
    await tester.pump(const Duration(milliseconds: 400));
    expect(viewOf(tester), moved, reason: 'Follow is off');

    await option(tester, 'Follow the cursor');
    expect(viewOf(tester), isNot(moved));
  });

  screenTest('a wide picture can fill the view, and the viewer turns', (
    tester,
  ) async {
    final orientations = <Object?>[];
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        if (call.method == 'SystemChrome.setPreferredOrientations') {
          orientations.add(call.arguments);
        }
        return null;
      },
    );
    addTearDown(
      () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );
    await pumpViewer(tester);

    // 16:9 on a tall phone leaves bars; filling crops them away.
    await option(tester, 'Fill the screen');
    expect(scale(tester), greaterThan(3));
    await option(tester, 'Show all of it');
    expect(scale(tester), moreOrLessEquals(1));

    await option(tester, 'Turn sideways');
    expect(orientations.last, [
      'DeviceOrientation.landscapeLeft',
      'DeviceOrientation.landscapeRight',
    ]);

    // On its side the controls move to a panel beside the picture.
    tester.view.physicalSize = const Size(800, 400);
    await tester.pump();
    expect(tester.takeException(), isNull, reason: 'fits sideways');
    final stage = tester.getRect(find.byType(InteractiveViewer));
    expect(stage.height, 400, reason: 'the picture has the full height');
    expect(stage.width, 440, reason: 'a 360 panel beside it');
    expect(find.text('Stop'), findsOneWidget, reason: 'Stop beside it');
    await tester.tap(find.byTooltip('More options'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    expect(find.text('Turn upright'), findsOneWidget);

    // Leaving lets the phone decide again.
    await tester.pumpWidget(const SizedBox());
    expect(orientations.last, isEmpty);
  });

  screenTest('an ending says why, and can watch again', (tester) async {
    final (server, links, id) = await pumpViewer(tester);
    server.relay('pc', {
      'kind': 'result',
      'requestId': id,
      'ok': false,
      'error': 'busy',
    });
    await tester.pump();
    await tester.pump();
    expect(links.last.closed, isTrue);
    expect(
      find.text(
        'Another view of ISHAN-PC is still running. Try again in a moment.',
      ),
      findsOne,
    );
    await tester.tap(find.text('Watch again'));
    await tester.pump();
    expect(sentOf(server, 'screen.watch'), hasLength(2));
  });

  screenTest('Keys press in the watched window, over the channel', (
    tester,
  ) async {
    final (_, links, _) = await pumpViewer(tester);
    final link = links.last;
    expect(find.byTooltip('Enter'), findsNothing, reason: 'Keys closed');
    await tester.tap(find.byTooltip('Keys'));
    await tester.pump();
    expect(tester.takeException(), isNull, reason: 'the pad fits');
    expect(find.byTooltip('Hide keys'), findsOneWidget);

    await tester.tap(find.byTooltip('Enter'));
    await tester.pump();
    await tester.tap(find.byTooltip('Up arrow'));
    await tester.pump();
    expect(link.sent, [
      {
        't': 'keys',
        'id': 'k1',
        'keys': ['enter'],
      },
      {
        't': 'keys',
        'id': 'k2',
        'keys': ['up'],
      },
    ]);

    // Three wait at most; a fourth tap is dropped.
    await tester.tap(find.byTooltip('Tab'));
    await tester.tap(find.byTooltip('Tab'));
    await tester.pump();
    expect(link.sent, hasLength(3));

    // A key the computer refused says why.
    link.receive({'t': 'ack', 'id': 'k1', 'ok': true});
    link.receive({'t': 'ack', 'id': 'k2', 'ok': false, 'error': 'key_refused'});
    await tester.pump();
    await tester.pump();
    expect(find.textContaining("doesn't press keys"), findsOneWidget);
    await tester.tap(find.byTooltip('Tab'));
    await tester.pump();
    expect(link.sent, hasLength(4), reason: 'acks free the queue');
  });

  screenTest('the assistant bar asks in the same chat, the answer above', (
    tester,
  ) async {
    final (server, links, _) = await pumpViewer(
      tester,
      assistant: true,
      capabilities: const ['dictate', 'text', 'windows', 'screen', 'assistant'],
    );
    expect(tester.takeException(), isNull, reason: 'fits with the bar');
    expect(find.byTooltip('Ask by voice'), findsOneWidget);

    await tester.enterText(find.byType(TextField), 'play believer');
    await tester.pump();
    await tester.tap(find.byTooltip('Send'));
    await tester.pump();
    final ask = sentOf(server, 'assistant.ask').single;
    expect(ask, {
      'kind': 'assistant.ask',
      'text': 'play believer',
      'requestId': ask['requestId'],
    });
    final id = ask['requestId']! as String;
    await progressOf(tester, server, id, 'tool', {'label': 'Opening YouTube'});
    expect(find.text('Opening YouTube…'), findsOneWidget);
    await progressOf(tester, server, id, 'answer', {'text': 'Playing *it*'});
    expect(find.text('Playing it', findRichText: true), findsOneWidget);

    server.relay('pc', {
      'kind': 'result',
      'requestId': id,
      'ok': true,
      'transcript': 'play believer',
      'answer': 'Playing “Believer” on YouTube.',
      'conversation': 'c1',
      'actions': [
        {'label': 'Played “Believer” on YouTube', 'status': 'done'},
      ],
      'pending': {'id': 7, 'summary': 'Send the email to Ron'},
    });
    await tester.pump();
    await tester.pump();
    expect(tester.takeException(), isNull);
    expect(find.text('Played “Believer” on YouTube'), findsOneWidget);
    final card = tester.getRect(find.text('play believer'));
    final stage = tester.getRect(find.byType(InteractiveViewer));
    expect(
      card.top,
      greaterThanOrEqualTo(stage.bottom),
      reason: 'the card sits under the picture, never over it',
    );

    // Do it, from inside the card.
    await tester.ensureVisible(find.text('Do it'));
    await tester.tap(find.text('Do it'));
    await tester.pump();
    final confirm = sentOf(server, 'command.confirm').single;
    expect(confirm, {
      'kind': 'command.confirm',
      'id': 7,
      'accept': true,
      'requestId': confirm['requestId'],
    });
    server.relay('pc', {
      'kind': 'result',
      'requestId': confirm['requestId'],
      'ok': true,
      'status': 'done',
      'feedback': 'Sent.',
    });
    await tester.pump();
    await tester.pump();
    expect(find.text('Do it'), findsNothing);

    // The next question continues the conversation.
    await tester.enterText(find.byType(TextField), 'louder');
    await tester.pump();
    await tester.tap(find.byTooltip('Send'));
    await tester.pump();
    expect(sentOf(server, 'assistant.ask').last['conversation'], 'c1');

    // Hidden, then shown again.
    await tester.tap(find.byTooltip('Hide the answer').first);
    await tester.pump();
    expect(find.text('louder'), findsNothing);
    await tester.tap(find.byTooltip('Show the answer'));
    await tester.pump();
    expect(find.text('louder'), findsOneWidget);
  });

  screenTest('on its side the keys and the assistant sit beside it', (
    tester,
  ) async {
    await pumpViewer(
      tester,
      assistant: true,
      capabilities: const ['dictate', 'text', 'windows', 'screen', 'assistant'],
    );
    tester.view.physicalSize = const Size(800, 400);
    await tester.pump();
    expect(find.byType(TextField), findsOneWidget, reason: 'in the panel');

    await tester.tap(find.byTooltip('Keys'));
    await tester.pump();
    expect(tester.takeException(), isNull, reason: 'fits sideways');
    expect(find.byType(TextField), findsOneWidget);
    expect(find.byTooltip('Enter'), findsOneWidget);
    expect(
      tester.getRect(find.byType(InteractiveViewer)).width,
      greaterThan(300),
      reason: 'the picture keeps most of the width',
    );
  });
}

/// Offer, live and an open channel for the latest watch (or [id]).
Future<String> goLive(
  WidgetTester tester,
  FakeServer server,
  FakeLinks links, [
  String? id,
]) async {
  final watchId =
      id ?? sentOf(server, 'screen.watch').last['requestId']! as String;
  await progressOf(tester, server, watchId, 'rtc.offer', offerFields);
  await progressOf(tester, server, watchId, 'live');
  links.last.openChannel();
  await tester.pump();
  return watchId;
}

Matrix4 viewOf(WidgetTester tester) => tester
    .widget<InteractiveViewer>(find.byType(InteractiveViewer))
    .transformationController!
    .value;

double scale(WidgetTester tester) => viewOf(tester).getMaxScaleOnAxis();
