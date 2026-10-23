import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:talvio_mobile/src/features/command/command_controller.dart';

import '../../app_test.dart' show finish, goTo, progress, pumpApp;
import '../../helpers.dart';

final commandPc = deviceJson(capabilities: ['dictate', 'text', 'command']);

Map<String, Object?> lastBody(FakeServer server) =>
    server.relays.last['body']! as Map<String, Object?>;

/// The computer answers request [id] with [fields].
Future<void> answer(
  WidgetTester tester,
  FakeServer server,
  Object? id,
  Map<String, Object?> fields,
) async {
  server.relay('pc', {'kind': 'result', 'requestId': id, ...fields});
  await tester.pump();
  await tester.pump();
}

Future<void> tapVisible(WidgetTester tester, Finder finder) async {
  await tester.ensureVisible(finder);
  await tester.pump();
  await tester.tap(finder);
  await tester.pump();
}

/// Types [text] into the command field and presses Go; the request's id.
Future<Object?> typeCommand(
  WidgetTester tester,
  FakeServer server,
  String text,
) async {
  await tester.enterText(
    find.widgetWithText(TextField, 'Or type a command'),
    text,
  );
  await tester.pump();
  await tester.tap(find.text('Go'));
  await tester.pump();
  // The keyboard closes, and the mic comes back.
  await tester.pump(const Duration(milliseconds: 300));
  return lastBody(server)['requestId'];
}

/// Scrolls the page's list until [finder] is built and on screen.
Future<void> scrollTo(WidgetTester tester, Finder finder) =>
    tester.scrollUntilVisible(
      finder,
      100,
      scrollable: find
          .descendant(
            of: find.byType(ListView),
            matching: find.byType(Scrollable),
          )
          .first,
    );

List<String?> menuLabels(WidgetTester tester) => [
  for (final d in tester.widgetList<NavigationDrawerDestination>(
    find.byType(NavigationDrawerDestination),
  ))
    (d.label as Text).data,
];

void main() {
  testWidgets('Do shows only for a computer that takes commands', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [deviceJson()], page: null);
    await tester.tap(find.byTooltip('Open menu'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    expect(menuLabels(tester), ['Screen', 'Send', 'History', 'Account']);

    server.send({
      'type': 'devices',
      'devices': [
        deviceJson(id: 'phone', kind: 'mobile', current: true),
        commandPc,
      ],
    });
    await tester.pump();
    await tester.pump();
    expect(menuLabels(tester), ['Screen', 'Do', 'Send', 'History', 'Account']);
    await finish(tester, relay);
  });

  testWidgets('a typed command: working, then what was done', (tester) async {
    final (server, relay) = await pumpApp(tester, [commandPc], page: 'Do');
    expect(find.text('Tell ISHAN-PC what to do'), findsOneWidget);
    expect(tester.takeException(), isNull, reason: 'fits a small phone');

    final id = await typeCommand(tester, server, 'open youtube');
    expect(lastBody(server), {
      'kind': 'command',
      'text': 'open youtube',
      'requestId': id,
    });
    expect(find.text('Sending to ISHAN-PC…'), findsOneWidget);
    await progress(tester, server, id! as String, 'working');
    expect(find.text('ISHAN-PC is on it…'), findsOneWidget);

    await answer(tester, server, id, {
      'ok': true,
      'transcript': 'open youtube',
      'status': 'done',
      'feedback': 'Opened YouTube.',
    });
    expect(find.text('Done'), findsOneWidget);
    expect(find.text('“open youtube”'), findsOneWidget);
    expect(find.text('Opened YouTube.'), findsOneWidget);
    await scrollTo(tester, find.byTooltip('Run "open youtube" again'));
    expect(find.text('Recent'), findsOneWidget);
    expect(find.text('Opened YouTube.'), findsWidgets, reason: 'in Recent');
    expect(tester.takeException(), isNull);

    // An example is one tap.
    await tapVisible(tester, find.text('Pause the music'));
    expect(lastBody(server)['text'], 'Pause the music');
    await finish(tester, relay);
  });

  testWidgets('a confirmation is sent with its id', (tester) async {
    final (server, relay) = await pumpApp(tester, [commandPc], page: 'Do');
    final id = await typeCommand(tester, server, 'email Ron');
    await answer(tester, server, id, {
      'ok': true,
      'transcript': 'email Ron',
      'status': 'awaiting_confirmation',
      'feedback': 'The email to Ron is ready.',
      'id': 7,
      'pending': 'Send the email to Ron',
    });
    expect(find.text('Go ahead?'), findsOneWidget);
    expect(find.text('Send the email to Ron'), findsOneWidget);

    await tapVisible(tester, find.text('Do it'));
    final confirm = lastBody(server);
    expect(confirm, {
      'kind': 'command.confirm',
      'id': 7,
      'accept': true,
      'requestId': confirm['requestId'],
    });
    await answer(tester, server, confirm['requestId'], {
      'ok': true,
      'status': 'done',
      'feedback': 'Sent the email to Ron.',
    });
    expect(find.text('Done'), findsOneWidget);
    expect(find.text('“email Ron”'), findsOneWidget, reason: 'kept');
    expect(find.text('Sent the email to Ron.'), findsOneWidget);
    await scrollTo(tester, find.byTooltip('Run "email Ron" again'));
    expect(
      find.descendant(
        of: find.byType(ListTile),
        matching: find.text('Sent the email to Ron.'),
      ),
      findsOneWidget,
      reason: 'Recent shows how it ended',
    );
    expect(find.text('Do it'), findsNothing);

    // Cancel says no.
    final again = await typeCommand(tester, server, 'email Ron');
    await answer(tester, server, again, {
      'ok': true,
      'status': 'awaiting_confirmation',
      'feedback': 'Ready.',
      'id': 8,
      'pending': 'Send it',
    });
    await tapVisible(
      tester,
      find.descendant(
        of: find.byType(Card),
        matching: find.widgetWithText(TextButton, 'Cancel'),
      ),
    );
    expect(lastBody(server)['kind'], 'command.confirm');
    expect(lastBody(server)['id'], 8);
    expect(lastBody(server)['accept'], isFalse);
    await finish(tester, relay);
  });

  testWidgets('a question is answered with a choice or typed text', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [commandPc], page: 'Do');
    final id = await typeCommand(tester, server, 'call Ron');
    const asked = {
      'ok': true,
      'status': 'needs_answer',
      'feedback': 'There are two.',
      'id': 3,
      'question': 'Which Ron?',
      'choices': ['Ron Weasley', 'Ron Swanson'],
    };
    await answer(tester, server, id, asked);
    expect(find.text('One question'), findsOneWidget);
    expect(find.text('Which Ron?'), findsOneWidget);
    await tapVisible(tester, find.text('Ron Swanson'));
    final picked = lastBody(server);
    expect(picked, {
      'kind': 'command.answer',
      'id': 3,
      'answer': 'Ron Swanson',
      'requestId': picked['requestId'],
    });
    await answer(tester, server, picked['requestId'], asked);

    await tester.enterText(
      find.widgetWithText(TextField, 'Or type an answer'),
      'the work one',
    );
    await tester.pump();
    await tapVisible(tester, find.byTooltip('Answer'));
    expect(lastBody(server)['kind'], 'command.answer');
    expect(lastBody(server)['id'], 3);
    expect(lastBody(server)['answer'], 'the work one');
    await finish(tester, relay);
  });

  testWidgets('failed and needs_computer_use still say what happened', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [commandPc], page: 'Do');
    var id = await typeCommand(tester, server, 'open spotfy');
    await answer(tester, server, id, {
      'ok': true,
      'status': 'failed',
      'feedback': "I couldn't find an app called Spotfy.",
    });
    expect(find.text("Couldn't do that"), findsOneWidget);
    expect(find.text("I couldn't find an app called Spotfy."), findsWidgets);
    final card = tester.widget<Card>(find.byType(Card).first);
    expect(
      card.color,
      Theme.of(
        tester.element(find.byType(Card).first),
      ).colorScheme.errorContainer,
    );

    id = await typeCommand(tester, server, 'click the blue button');
    await answer(tester, server, id, {
      'ok': true,
      'status': 'needs_computer_use',
      'feedback': 'No tool does that.',
    });
    expect(find.text('Not from a phone yet'), findsOneWidget);
    expect(
      find.text(
        "That needs Talvio to work the screen itself, which isn't on for "
        'phones yet.',
      ),
      findsOneWidget,
    );
    await finish(tester, relay);
  });

  testWidgets('error codes read as short messages, and Try again resends', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [commandPc], page: 'Do');
    final id = await typeCommand(tester, server, 'open youtube');
    await answer(tester, server, id, {
      'ok': false,
      'error': 'command_mode_off',
    });
    expect(
      find.text(
        'Turn on Voice commands in Talvio on ISHAN-PC (Commands page).',
      ),
      findsOneWidget,
    );
    await tapVisible(tester, find.text('Try again'));
    final again = lastBody(server);
    expect(again['text'], 'open youtube');
    expect(again['requestId'], isNot(id));
    await answer(tester, server, again['requestId'], {
      'ok': false,
      'error': 'not_configured',
    });
    expect(
      find.text('Talvio on ISHAN-PC has no AI model set up for commands.'),
      findsOneWidget,
    );
    await finish(tester, relay);
  });

  testWidgets('no answer in 20 s is a timeout, and cancels', (tester) async {
    final (server, relay) = await pumpApp(tester, [commandPc], page: 'Do');
    final id = await typeCommand(tester, server, 'open youtube');
    await tester.pump(commandTimeout - const Duration(seconds: 1));
    expect(
      find.text("ISHAN-PC didn't answer in time. Try again."),
      findsNothing,
    );
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(lastBody(server), {'kind': 'cancel', 'requestId': id});
    expect(
      find.text("ISHAN-PC didn't answer in time. Try again."),
      findsOneWidget,
    );
    await finish(tester, relay);
  });

  testWidgets('a spoken command sends the recording', (tester) async {
    final file = File(
      '${Directory.systemTemp.createTempSync('talvio-test').path}/c.wav',
    )..writeAsBytesSync(List.filled(44, 7));
    final (server, relay) = await pumpApp(
      tester,
      [commandPc],
      page: 'Do',
      recordEvents: (_) {},
      recording: file.path,
    );
    final mic = find.bySemanticsLabel('Say a command');
    final handle = tester.ensureSemantics();
    expect(mic, findsOneWidget);
    await tester.tap(find.widgetWithIcon(FilledButton, Icons.mic));
    await tester.pump();
    await tester.pump();
    expect(find.bySemanticsLabel('Stop and send the command'), findsOneWidget);
    await tester.tap(find.widgetWithIcon(FilledButton, Icons.stop_rounded));
    // The audio is encoded on another isolate.
    await tester.runAsync(
      () => Future<void>.delayed(const Duration(milliseconds: 300)),
    );
    await tester.pump();
    final body = lastBody(server);
    expect(body.keys, unorderedEquals(['kind', 'audio', 'requestId']));
    expect(body['kind'], 'command');
    expect(base64Decode(body['audio']! as String), List.filled(44, 7));
    expect(find.text('Transcribing on ISHAN-PC…'), findsOneWidget);
    await progress(tester, server, body['requestId']! as String, 'working');
    await answer(tester, server, body['requestId'], {
      'ok': true,
      'transcript': 'turn the volume down',
      'status': 'done',
      'feedback': 'Turned the volume down.',
    });
    expect(find.text('“turn the volume down”'), findsOneWidget);
    handle.dispose();
    await finish(tester, relay);
  });

  testWidgets('a computer without commands says to update', (tester) async {
    final (server, relay) = await pumpApp(tester, [commandPc], page: 'Do');
    server.send({
      'type': 'devices',
      'devices': [
        deviceJson(id: 'phone', kind: 'mobile', current: true),
        commandPc,
        deviceJson(id: 'old', name: 'OLD-PC'),
      ],
    });
    await tester.pump();
    await tester.pump();
    await tester.tap(find.text('ISHAN-PC'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    await tester.tap(find.text('OLD-PC'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    expect(
      find.text(
        'Update Talvio on OLD-PC to tell it what to do from your phone.',
      ),
      findsOneWidget,
    );
    expect(
      tester
          .widget<FilledButton>(find.widgetWithIcon(FilledButton, Icons.mic))
          .onPressed,
      isNull,
    );
    await goTo(tester, 'Send');
    await finish(tester, relay);
  });
}
