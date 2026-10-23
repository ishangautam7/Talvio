import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:talvio_mobile/src/core/api_client.dart';
import 'package:talvio_mobile/src/core/app_services.dart';
import 'package:talvio_mobile/src/features/screen/live_protocol.dart';
import 'package:talvio_mobile/src/features/screen/watch_screen.dart';

import '../../helpers.dart';
import 'fakes.dart';

void main() {
  /// The viewer of window w1 on a 400×800 phone, already live with its
  /// channel open. Returns the server, the links made, and the watch's id.
  Future<(FakeServer, FakeLinks, String)> pumpViewer(
    WidgetTester tester,
  ) async {
    tester.view
      ..physicalSize = const Size(400, 800)
      ..devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final (server, relay, session) = await connectedSession(tester);
    final prefs = await SharedPreferences.getInstance();
    final links = FakeLinks();
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
    unawaited(
      navigator.currentState!.push(
        watchRoute(
          computer: relay.devices.value!.firstWhere((d) => d.isComputer),
          target: const WatchTarget(id: 'w1', label: 'Notepad', screen: false),
          linkFactory: links.create,
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

  screenTest('watch only: no control, keys, typing or mic', (tester) async {
    final (server, links, _) = await pumpViewer(tester);
    expect(find.text('Live'), findsOneWidget);
    expect(tester.takeException(), isNull, reason: 'fits');
    expect(sentOf(server, 'screen.watch').single['control'], isFalse);

    for (final gone in ['View', 'Control']) {
      expect(find.text(gone), findsNothing, reason: gone);
    }
    for (final gone in ['Keys', 'Type', 'Dictate into this window', 'Enter']) {
      expect(find.byTooltip(gone), findsNothing, reason: gone);
    }
    expect(find.byType(TextField), findsNothing);
    expect(find.byIcon(Icons.mic), findsNothing);
    expect(find.byTooltip('Turn sound on'), findsOneWidget);
    expect(find.text('Stop'), findsOneWidget);

    // A tap, a double tap and a long press only look.
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

  screenTest('Stop ends the watch and closes the viewer', (tester) async {
    final (server, links, id) = await pumpViewer(tester);
    await tester.tap(find.text('Stop'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    expect(sentOf(server, 'cancel').single['requestId'], id);
    expect(links.last.closed, isTrue);
    expect(find.text('Home page'), findsOneWidget);
    expect(find.byType(InteractiveViewer), findsNothing);
  });

  screenTest('the caret is followed until a pan', (tester) async {
    final (_, links, _) = await pumpViewer(tester);
    final link = links.last;

    expect(find.byTooltip('Follow the cursor'), findsNothing, reason: 'none');
    link.receive({'t': 'caret', 'x': 0.5, 'y': 0.5, 'w': 0, 'h': 0.02});
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    expect(find.byTooltip('Following the cursor'), findsOneWidget);
    expect(scale(tester), greaterThan(2), reason: 'zoomed in to read');

    await tester.drag(
      find.byType(InteractiveViewer),
      const Offset(60, 40),
      touchSlopX: 0,
      touchSlopY: 0,
    );
    await tester.pump();
    expect(find.byTooltip('Follow the cursor'), findsOneWidget);
    final moved = viewOf(tester).clone();
    link.receive({'t': 'caret', 'x': 0.1, 'y': 0.1, 'w': 0, 'h': 0.02});
    await tester.pump(const Duration(milliseconds: 400));
    expect(viewOf(tester), moved, reason: 'Follow is off');

    await tester.tap(find.byTooltip('Follow the cursor'));
    await tester.pump(const Duration(milliseconds: 400));
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
    await tester.tap(find.byTooltip('Fill the screen'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    expect(scale(tester), greaterThan(3));
    await tester.tap(find.byTooltip('Show all of it'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    expect(scale(tester), moreOrLessEquals(1));

    await tester.tap(find.byTooltip('Turn sideways'));
    await tester.pump();
    expect(orientations.last, [
      'DeviceOrientation.landscapeLeft',
      'DeviceOrientation.landscapeRight',
    ]);

    // On its side the controls move beside the picture.
    tester.view.physicalSize = const Size(800, 400);
    await tester.pump();
    expect(tester.takeException(), isNull, reason: 'fits sideways');
    final stage = tester.getRect(find.byType(InteractiveViewer));
    expect(stage.height, 400, reason: 'the picture has the full height');
    expect(stage.width, greaterThan(650), reason: 'a slim column of buttons');
    expect(find.byTooltip('Turn upright'), findsOneWidget);
    expect(find.byTooltip('Stop'), findsOneWidget, reason: 'Stop beside it');

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
    expect(find.text('Someone is already watching this computer.'), findsOne);
    await tester.tap(find.text('Watch again'));
    await tester.pump();
    expect(sentOf(server, 'screen.watch'), hasLength(2));
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
