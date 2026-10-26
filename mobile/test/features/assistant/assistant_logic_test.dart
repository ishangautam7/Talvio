import 'package:flutter_test/flutter_test.dart';
import 'package:talvio_mobile/src/features/assistant/assistant_controller.dart';
import 'package:talvio_mobile/src/features/assistant/strings.dart';

void main() {
  test('actions and pending are read defensively', () {
    expect(
      AssistantAction.tryParse({'label': ' Opened Gmail '})?.label,
      'Opened Gmail',
    );
    expect(AssistantAction.tryParse({'label': 'x'})?.status, 'done');
    expect(AssistantAction.tryParse({'label': ''}), isNull);
    expect(AssistantAction.tryParse('x'), isNull);
    expect(AssistantPending.tryParse({'id': 7, 'summary': 'Send'})?.id, 7);
    expect(AssistantPending.tryParse({'id': '7', 'summary': 'Send'}), isNull);
  });

  test('the action waiting for Do it is listed once, as the card', () {
    final turn =
        AssistantTurn(computerId: 'pc', computerName: 'PC', spoken: false)
          ..actions = const [
            AssistantAction(label: 'Drafted it', status: 'done'),
            AssistantAction(label: 'Send it', status: 'awaiting_confirmation'),
          ];
    expect(turn.shownActions, hasLength(2), reason: 'nothing pending');
    turn.pending = const AssistantPending(id: 1, summary: 'Send it');
    expect(turn.shownActions.map((a) => a.label), ['Drafted it']);
  });

  test('a turn survives a round trip through storage', () {
    final turn =
        AssistantTurn(
            computerId: 'pc',
            computerName: 'ISHAN-PC',
            spoken: true,
            question: 'play it',
          )
          ..answer = 'Playing.'
          ..done = true
          ..actions = const [AssistantAction(label: 'Played', status: 'done')]
          ..pending = const AssistantPending(id: 3, summary: 'Send')
          ..confirmStatus = 'done'
          ..confirmFeedback = 'Sent.';
    final back = AssistantTurn.fromJson(turn.toJson())!;
    expect(back.question, 'play it');
    expect(back.spoken, isTrue);
    expect(back.answer, 'Playing.');
    expect(back.actions.single.label, 'Played');
    expect(back.pending?.id, 3);
    expect(back.confirmStatus, 'done');
    expect(back.awaitingConfirmation, isFalse);
    expect(AssistantTurn.fromJson({'answer': 1}), isNull);
  });

  test('error codes read as short messages', () {
    String text(String code, [String? message]) =>
        AssistantStrings.error(code, 'ISHAN-PC', message: message);
    expect(
      text('assistant_off'),
      'Turn on the assistant in Talvio on ISHAN-PC.',
    );
    expect(
      text('not_configured'),
      'Talvio on ISHAN-PC has no AI model set up for the assistant.',
    );
    expect(text('busy'), 'ISHAN-PC is still answering your last question.');
    expect(text('failed', 'Rate limited.'), endsWith('Rate limited.'));
    expect(text('failed'), contains("couldn't answer"));
    for (final code in [
      'not_approved',
      'remote_input_off',
      'bad_audio',
      'no_speech',
      'no_speech_model',
      'transcription_failed',
      'unsupported',
      'cancelled',
      'offline',
      'timeout',
      'interrupted',
    ]) {
      expect(text(code), isNot(contains('Something went wrong')), reason: code);
    }
  });
}
