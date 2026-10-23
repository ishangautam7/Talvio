import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:talvio_mobile/src/core/api_client.dart';
import 'package:talvio_mobile/src/core/app_services.dart';
import 'package:talvio_mobile/src/features/dictate/dictate_controller.dart';
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
    final messenger = tester.binding.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/path_provider'),
      (_) async => '/nonexistent-talvio-test',
    );
    messenger.setMockMethodCallHandler(
      const MethodChannel('com.llfbandit.record/messages'),
      (_) async => null,
    );
    final (server, relay, session) = await connectedSession(tester);
    final prefs = await SharedPreferences.getInstance();
    final dictate = DictateController(relay: relay, prefs: prefs);
    cleanups.add(dictate.dispose);
    final links = FakeLinks();
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
          home: WatchScreen(
            dictate: dictate,
            computer: relay.devices.value!.firstWhere((d) => d.isComputer),
            target: const WatchTarget(
              id: 'w1',
              label: 'Notepad',
              screen: false,
            ),
            linkFactory: links.create,
          ),
        ),
      ),
    );
    cleanups.add(() => tester.pumpWidget(const SizedBox()));
    await tester.pump();
    final id = await goLive(tester, server, links);
    return (server, links, id);
  }

  screenTest(
    'keys go over the channel, and the caret is followed until a pan',
    (tester) async {
      final (_, links, _) = await pumpViewer(tester);
      final link = links.last;
      expect(find.text('Live'), findsOneWidget);
      expect(tester.takeException(), isNull, reason: 'fits');

      // The key pad is folded away until asked for.
      expect(find.byTooltip('Enter'), findsNothing);
      await tester.tap(find.byTooltip('Keys'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));
      expect(tester.takeException(), isNull, reason: 'fits with the pad');
      await tester.tap(find.byTooltip('Enter'));
      await tester.pump();
      expect(link.sent.last, {
        't': 'keys',
        'keys': ['enter'],
        'id': 'k1',
      });

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
    },
  );

  screenTest('Control: taps map to the picture and Send asks first', (
    tester,
  ) async {
    final (server, links, id) = await pumpViewer(tester);
    final center = tester.getCenter(find.byType(InteractiveViewer));

    // View: a tap only looks.
    await tester.tapAt(center);
    await tester.pump(const Duration(milliseconds: 500));
    expect(links.last.sent, isEmpty);

    await tester.tap(find.text('Control'));
    await tester.pump();
    expect(sentOf(server, 'cancel').single['requestId'], id);
    server.relay('pc', {
      'kind': 'result',
      'requestId': id,
      'ok': false,
      'error': 'cancelled',
    });
    await tester.pump();
    await tester.pump();
    final watch = sentOf(server, 'screen.watch').last;
    expect(watch['control'], isTrue);
    await goLive(tester, server, links, watch['requestId']! as String);
    final link = links.last;

    await tester.tapAt(center);
    await tester.pump(const Duration(milliseconds: 500));
    expect(link.sent.last, {
      't': 'tap',
      'x': 0.5,
      'y': 0.5,
      'button': 'left',
      'count': 1,
      'confirmed': false,
      'id': 'k1',
    });
    link.receive({
      't': 'ack',
      'id': 'k1',
      'ok': false,
      'error': 'confirm',
      'label': 'Send',
    });
    await tester.pump();
    await tester.pump();
    expect(find.text('Press "Send" on the computer?'), findsOneWidget);
    await tester.tap(find.text('Press'));
    await tester.pump();
    expect(link.sent.last['confirmed'], isTrue);
    expect(link.sent.last['id'], 'k2');

    // A tap on the letterbox bar above the picture clicks nothing.
    await tester.tapAt(
      tester.getTopLeft(find.byType(InteractiveViewer)) + const Offset(20, 90),
    );
    await tester.pump(const Duration(milliseconds: 500));
    expect(link.sent.last['id'], 'k2');

    await tester.longPressAt(center);
    await tester.pump();
    expect(link.sent.last['button'], 'right');
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

    await tester.tap(find.byTooltip('Keys'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
    expect(tester.takeException(), isNull, reason: 'fits with the pad');
    final up = tester.getRect(find.byTooltip('Up arrow'));
    expect(up.left, greaterThan(stage.left + 400), reason: 'pad at the side');
    expect(
      tester.getRect(find.byType(InteractiveViewer)).height,
      400,
      reason: 'the pad takes width, not height',
    );

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
