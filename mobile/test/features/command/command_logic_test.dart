import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:talvio_mobile/src/features/command/command_controller.dart';
import 'package:talvio_mobile/src/features/command/recent_commands.dart';
import 'package:talvio_mobile/src/features/command/strings.dart';

void main() {
  test('a result is read defensively', () {
    final result = CommandResult.parse({
      'status': 'needs_answer',
      'feedback': ' Two Rons. ',
      'id': 7,
      'question': 'Which Ron?',
      'choices': ['Ron Weasley', 3, '', 'Ron Swanson'],
    });
    expect(result.feedback, 'Two Rons.');
    expect(result.choices, ['Ron Weasley', 'Ron Swanson']);
    expect(result.answerable, isTrue);
    expect(result.confirmable, isFalse);

    final odd = CommandResult.parse({'status': 7, 'id': '7'});
    expect(odd.status, CommandStatus.done);
    expect(odd.feedback, '');
    expect(odd.id, isNull);
    expect(
      CommandResult.parse({'status': 'awaiting_confirmation'}).confirmable,
      isFalse,
      reason: 'nothing to confirm without an id',
    );
  });

  test('recent commands: newest first, ten kept, follow-ups update', () async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await SharedPreferences.getInstance();
    final recents = RecentCommands(prefs);
    for (var i = 0; i < 12; i++) {
      await recents.add(
        RecentCommand(
          said: 'command $i',
          feedback: 'did $i',
          status: 'done',
          at: DateTime(2026),
          id: i == 11 ? 42 : null,
        ),
      );
    }
    expect(recents.items, hasLength(10));
    expect(recents.items.first.said, 'command 11');

    await recents.update(42, feedback: 'Sent it.', status: 'done');
    expect(recents.items.first.feedback, 'Sent it.');
    expect(recents.items.first.id, isNull);

    // Kept under the account prefix, so signing out clears it.
    expect(prefs.getKeys(), contains('account.command.recent'));
    final again = RecentCommands(prefs);
    expect(again.items.map((i) => i.said).first, 'command 11');
    expect(again.items.first.feedback, 'Sent it.');
  });

  test('error codes read as short messages', () {
    String text(String code) => CommandStrings.error(code, 'ISHAN-PC');
    expect(
      text('command_mode_off'),
      'Turn on Voice commands in Talvio on ISHAN-PC (Commands page).',
    );
    expect(
      text('not_configured'),
      'Talvio on ISHAN-PC has no AI model set up for commands.',
    );
    for (final code in [
      'busy',
      'not_approved',
      'remote_input_off',
      'bad_audio',
      'no_speech',
      'transcription_failed',
      'unsupported',
      'offline',
      'timeout',
      'interrupted',
      'cancelled',
    ]) {
      expect(text(code), isNot(contains('Something went wrong')), reason: code);
    }
  });
}
