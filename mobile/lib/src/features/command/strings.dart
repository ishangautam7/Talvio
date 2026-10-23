import '../dictate/strings.dart';

abstract final class CommandStrings {
  static const tab = 'Do';
  static const title = 'Do';

  static String prompt(String name) => 'Tell $name what to do';
  static const promptDetail =
      'Talvio opens apps and sites, plays music, sets the volume and more, '
      'right on the computer.';
  static const examples = 'Try';

  /// Commands to tap and send as they are.
  static const exampleCommands = [
    'Open YouTube',
    'Play Believer by Imagine Dragons',
    'Pause the music',
    'Turn the volume down',
    'Open Gmail',
  ];

  static const startRecording = 'Say a command';
  static const stopAndSend = 'Stop and send the command';
  static const tapToSpeak = 'Tap and say what to do';
  static const typeHint = 'Or type a command';
  static const go = 'Go';
  static const sendCommand = 'Send the command';

  static String offlineNote(String name) =>
      '$name is offline. Open Talvio on it to tell it what to do.';
  static String updateNote(String name) =>
      'Update Talvio on $name to tell it what to do from your phone.';

  static String transcribing(String name) => 'Transcribing on $name…';
  static String working(String name) => '$name is on it…';
  static String sending(String name) => 'Sending to $name…';
  static const cancelling = DictateStrings.cancelling;
  static const cancel = DictateStrings.cancel;

  // The result card, by status.
  static const done = 'Done';
  static const awaitingConfirmation = 'Go ahead?';
  static const needsAnswer = 'One question';
  static const cancelled = 'Cancelled';
  static const failed = "Couldn't do that";
  static const needsComputerUse = 'Not from a phone yet';
  static const needsComputerUseBody =
      "That needs Talvio to work the screen itself, which isn't on for phones "
      'yet.';
  static const doIt = 'Do it';
  static const answerHint = 'Or type an answer';
  static const answer = 'Answer';
  static const dismiss = DictateStrings.dismiss;
  static const tryAgain = DictateStrings.tryAgain;
  static String said(String text) => '“$text”';

  static const recent = 'Recent';
  static const recentNote = 'Your last 10 commands, on this phone only.';
  static String runAgain(String text) => 'Run "$text" again';

  /// Text for a result error code (PROTOCOL.md, Commands) or one the phone
  /// sets itself; codes every request shares fall through to
  /// [DictateStrings.error].
  static String error(String code, String name) => switch (code) {
    'command_mode_off' =>
      'Turn on Voice commands in Talvio on $name (Commands page).',
    'not_configured' => 'Talvio on $name has no AI model set up for commands.',
    'unsupported' =>
      'Update Talvio on $name to tell it what to do from your phone.',
    _ => DictateStrings.error(code, name),
  };
}
