import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:talvio_mobile/src/features/assistant/assistant_controller.dart';

import '../../app_test.dart' show finish, pumpApp, tabs;
import '../../helpers.dart';
import '../command/command_page_test.dart' show answer, lastBody, tapVisible;

final assistantPc = deviceJson(
  capabilities: ['dictate', 'text', 'command', 'assistant'],
);

/// The bodies of the relays the phone sent of [kind].
List<Map<String, Object?>> sent(FakeServer server, String kind) => [
  for (final relay in server.relays)
    if ((relay['body']! as Map<String, Object?>)['kind'] == kind)
      relay['body']! as Map<String, Object?>,
];

/// Types [text] into the question field and sends it; the request's id.
Future<String> ask(WidgetTester tester, FakeServer server, String text) async {
  await tester.enterText(find.widgetWithText(TextField, 'Ask anything'), text);
  await tester.pump();
  await tester.tap(find.byTooltip('Send'));
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 300));
  return lastBody(server)['requestId']! as String;
}

/// The computer reports `progress` [stage] with [fields] for [id].
Future<void> progress(
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

void main() {
  testWidgets('Ask shows only for a computer that has it, and is home', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [deviceJson()], page: null);
    expect(tabs(tester), isNot(contains('Ask')));

    server.send({
      'type': 'devices',
      'devices': [
        deviceJson(id: 'phone', kind: 'mobile', current: true),
        assistantPc,
      ],
    });
    await tester.pump();
    await tester.pump();
    expect(tabs(tester), ['Ask', 'Do', 'Dictate', 'Screen']);
    await finish(tester, relay);
  });

  testWidgets('a question streams its answer, then what was done', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [assistantPc], page: 'Ask');
    expect(find.text('Ask Talvio on ISHAN-PC'), findsOneWidget);
    expect(find.text('Play Believer on YouTube'), findsOneWidget);
    expect(tester.takeException(), isNull, reason: 'fits a small phone');

    final id = await ask(tester, server, 'play believer on youtube');
    expect(lastBody(server), {
      'kind': 'assistant.ask',
      'text': 'play believer on youtube',
      'requestId': id,
    });
    expect(find.text('play believer on youtube'), findsOneWidget);
    expect(find.text('Sending to ISHAN-PC…'), findsOneWidget);

    await progress(tester, server, id, 'thinking', {'transcript': 'x'});
    expect(find.text('Thinking…'), findsOneWidget);
    await progress(tester, server, id, 'tool', {'label': 'Opening YouTube'});
    expect(find.text('Opening YouTube…'), findsOneWidget);

    // Each `answer` carries the whole answer so far: it replaces.
    await progress(tester, server, id, 'answer', {'text': 'Playing'});
    expect(find.text('Playing', findRichText: true), findsOneWidget);
    await progress(tester, server, id, 'answer', {'text': 'Playing **now**'});
    expect(find.text('Playing now', findRichText: true), findsOneWidget);
    expect(find.text('Playing', findRichText: true), findsNothing);

    await answer(tester, server, id, {
      'ok': true,
      'transcript': 'play believer on youtube',
      'answer': 'Playing “Believer” by Imagine Dragons on YouTube.',
      'conversation': 'c1',
      'actions': [
        {'label': 'Played “Believer” on YouTube', 'status': 'done'},
        {'label': 'Turned it up', 'status': 'failed'},
        'nonsense',
      ],
    });
    expect(
      find.text(
        'Playing “Believer” by Imagine Dragons on YouTube.',
        findRichText: true,
      ),
      findsOneWidget,
    );
    expect(find.text('Played “Believer” on YouTube'), findsOneWidget);
    expect(find.text('Turned it up'), findsOneWidget);
    expect(find.byIcon(Icons.error_rounded), findsOneWidget);
    expect(find.text('Thinking…'), findsNothing);
    expect(tester.takeException(), isNull);

    // The next question continues the conversation.
    final next = await ask(tester, server, 'louder');
    expect(lastBody(server)['conversation'], 'c1');
    await answer(tester, server, next, {
      'ok': true,
      'answer': 'Done.',
      'conversation': 'c1',
    });

    // New chat forgets it, here and on the next question.
    await tester.tap(find.byTooltip('New chat'));
    await tester.pump();
    expect(find.text('louder'), findsNothing);
    expect(find.text('Ask Talvio on ISHAN-PC'), findsOneWidget);
    await ask(tester, server, 'hello');
    expect(lastBody(server).containsKey('conversation'), isFalse);
    await finish(tester, relay);
  });

  testWidgets('a pending action is confirmed with command.confirm', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [assistantPc], page: 'Ask');
    final id = await ask(tester, server, 'email Ron');
    await answer(tester, server, id, {
      'ok': true,
      'answer': 'The email to Ron is ready.',
      'conversation': 'c1',
      'actions': [
        {'label': 'Wrote the email to Ron', 'status': 'awaiting_confirmation'},
      ],
      'pending': {'id': 7, 'summary': 'Send the email to Ron'},
    });
    expect(find.text('Go ahead?'), findsOneWidget);
    expect(find.text('Send the email to Ron'), findsOneWidget);
    // Listed in `actions` too, but shown once: as the card.
    expect(find.text('Wrote the email to Ron'), findsNothing);

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
    expect(find.text('Do it'), findsNothing);
    expect(find.text('Sent the email to Ron.'), findsOneWidget);

    // Cancel says no.
    final again = await ask(tester, server, 'email Ron again');
    await answer(tester, server, again, {
      'ok': true,
      'answer': 'Ready.',
      'pending': {'id': 8, 'summary': 'Send it'},
    });
    await tapVisible(tester, find.widgetWithText(OutlinedButton, 'Cancel'));
    expect(lastBody(server)['kind'], 'command.confirm');
    expect(lastBody(server)['id'], 8);
    expect(lastBody(server)['accept'], isFalse);
    await answer(tester, server, lastBody(server)['requestId'], {
      'ok': true,
      'status': 'cancelled',
      'feedback': '',
    });
    expect(find.text('Cancelled'), findsOneWidget);
    await finish(tester, relay);
  });

  testWidgets('errors read as short messages, and Try again asks again', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [assistantPc], page: 'Ask');
    final id = await ask(tester, server, 'hi');
    await answer(tester, server, id, {'ok': false, 'error': 'assistant_off'});
    expect(
      find.text('Turn on the assistant in Talvio on ISHAN-PC.'),
      findsOneWidget,
    );
    await tapVisible(tester, find.text('Try again'));
    final again = lastBody(server);
    expect(again['text'], 'hi');
    expect(again['requestId'], isNot(id));
    expect(find.text('hi'), findsOneWidget, reason: 'in its place, once');

    await answer(tester, server, again['requestId'], {
      'ok': false,
      'error': 'failed',
      'message': 'The model is overloaded.',
    });
    expect(
      find.text(
        "The assistant on ISHAN-PC couldn't answer: The model is overloaded.",
      ),
      findsOneWidget,
    );
    await finish(tester, relay);
  });

  testWidgets('no answer in 60 s is a timeout; progress restarts it', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [assistantPc], page: 'Ask');
    final id = await ask(tester, server, 'hi');
    await tester.pump(assistantTimeout - const Duration(seconds: 5));
    await progress(tester, server, id, 'thinking');
    await tester.pump(assistantTimeout - const Duration(seconds: 5));
    expect(find.textContaining("didn't answer in time"), findsNothing);
    await tester.pump(const Duration(seconds: 5));
    await tester.pump();
    expect(lastBody(server), {'kind': 'cancel', 'requestId': id});
    expect(
      find.text("ISHAN-PC didn't answer in time. Try again."),
      findsOneWidget,
    );
    await finish(tester, relay);
  });

  testWidgets('a spoken question sends the recording', (tester) async {
    final file = File(
      '${Directory.systemTemp.createTempSync('talvio-test').path}/a.wav',
    )..writeAsBytesSync(List.filled(44, 9));
    final (server, relay) = await pumpApp(
      tester,
      [assistantPc],
      page: 'Ask',
      recordEvents: (_) {},
      recording: file.path,
    );
    await tester.tap(find.byTooltip('Ask by voice'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    expect(find.byTooltip('Stop and ask'), findsOneWidget);
    await tester.tap(find.byTooltip('Stop and ask'));
    await tester.runAsync(
      () => Future<void>.delayed(const Duration(milliseconds: 300)),
    );
    await tester.pump();
    final body = lastBody(server);
    expect(body.keys, unorderedEquals(['kind', 'audio', 'requestId']));
    expect(body['kind'], 'assistant.ask');
    expect(base64Decode(body['audio']! as String), List.filled(44, 9));
    expect(find.text('Voice question'), findsOneWidget);
    expect(find.text('Transcribing on ISHAN-PC…'), findsOneWidget);
    await progress(tester, server, body['requestId']! as String, 'thinking', {
      'transcript': "what's the weather",
    });
    expect(find.text("what's the weather"), findsOneWidget);
    await finish(tester, relay);
  });

  testWidgets('the chat is kept on this phone, under the account', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [assistantPc], page: 'Ask');
    await tapVisible(
      tester,
      find.text('Open Gmail', skipOffstage: false).first,
    );
    final id = lastBody(server)['requestId'];
    expect(lastBody(server)['text'], 'Open Gmail');
    await answer(tester, server, id, {
      'ok': true,
      'answer': 'Opened Gmail.',
      'conversation': 'c9',
    });
    final prefs = await SharedPreferences.getInstance();
    final saved =
        jsonDecode(prefs.getString(AssistantController.prefsKey)!)
            as Map<String, Object?>;
    expect(AssistantController.prefsKey, startsWith('account.'));
    expect(saved['conversation'], 'c9');
    expect(
      (saved['turns']! as List).single,
      containsPair('question', 'Open Gmail'),
    );
    expect(sent(server, 'assistant.ask'), hasLength(1));
    await finish(tester, relay);
  });

  testWidgets('the stop button cancels an answer; tools get minutes', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [assistantPc], page: 'Ask');
    final id = await ask(tester, server, 'tidy my desktop');
    await progress(tester, server, id, 'tool', {'label': 'Tidying'});
    expect(find.text('Tidying…'), findsOneWidget);
    // A tool may run on the computer for a while, saying nothing.
    await tester.pump(const Duration(minutes: 5));
    expect(find.textContaining("didn't answer in time"), findsNothing);

    await tester.tap(find.byTooltip('Stop answering'));
    await tester.pump();
    expect(lastBody(server), {'kind': 'cancel', 'requestId': id});
    expect(find.text('Cancelling…'), findsOneWidget);
    await answer(tester, server, id, {'ok': false, 'error': 'cancelled'});
    expect(find.text('Cancelled.'), findsOneWidget);
    await finish(tester, relay);
  });

  testWidgets('a confirmation error reads as a command error', (tester) async {
    final (server, relay) = await pumpApp(tester, [assistantPc], page: 'Ask');
    final id = await ask(tester, server, 'email Ron');
    await answer(tester, server, id, {
      'ok': true,
      'answer': 'Ready.',
      'pending': {'id': 7, 'summary': 'Send the email to Ron'},
    });
    await tapVisible(tester, find.text('Do it'));
    await answer(tester, server, lastBody(server)['requestId'], {
      'ok': false,
      'error': 'command_mode_off',
    });
    expect(
      find.text(
        'Turn on Voice commands in Talvio on ISHAN-PC (Commands page).',
      ),
      findsOneWidget,
    );
    await finish(tester, relay);
  });
}
