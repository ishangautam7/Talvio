import '../dictate/strings.dart';

abstract final class AssistantStrings {
  static const tab = 'Ask';
  static const title = 'Assistant';

  static String prompt(String name) => 'Ask Talvio on $name';
  static const promptDetail =
      'It answers, and it can do things on the computer too: play a song, '
      'open a site, start an email.';
  static const examples = 'Try';

  /// Questions to tap and send as they are.
  static const exampleQuestions = [
    'Play Believer on YouTube',
    "What's the weather tomorrow?",
    'Open Gmail',
  ];

  /// What each example is, shown under it.
  static const exampleDetails = [
    'Acts on the computer',
    'Answers from the web',
    'Opens it in the browser',
  ];

  static const newChat = 'New chat';
  static const stopAnswer = 'Stop answering';
  static const typeHint = 'Ask anything';
  static const send = 'Send';
  static const startRecording = 'Ask by voice';
  static const stopAndSend = 'Stop and ask';
  static const discard = DictateStrings.discard;
  static const cancel = DictateStrings.cancel;
  static const cancelling = DictateStrings.cancelling;
  static const tryAgain = DictateStrings.tryAgain;
  static const copy = DictateStrings.copy;
  static const copied = DictateStrings.copied;

  /// What the user asked, by voice, before the computer transcribed it.
  static const voiceQuestion = 'Voice question';
  static const you = 'You';
  static String assistantOn(String name) => 'Talvio on $name';

  // The status line under a question while it is answered.
  static String sending(String name) => 'Sending to $name…';
  static String transcribing(String name) => 'Transcribing on $name…';
  static const thinking = 'Thinking…';
  static String tool(String label) => '$label…';
  static const answering = 'Answering…';

  static String newConversation(String name) =>
      'The earlier chat ended on $name, so this is a new one.';

  // What the assistant did on the computer.
  static const didOnComputer = 'On the computer';
  static String actionLabel(String label, String status) => switch (status) {
    'done' => 'Done: $label',
    'failed' => 'Failed: $label',
    'awaiting_confirmation' => 'Waiting: $label',
    'cancelled' => 'Cancelled: $label',
    _ => label,
  };

  // A pending action.
  static const goAhead = 'Go ahead?';
  static const doIt = 'Do it';
  static const confirming = 'Sending…';
  static const confirmedDone = 'Done';
  static const confirmedCancelled = 'Cancelled';
  static const confirmedFailed = "Couldn't do that";
  static const confirmedAsks = 'It has a question';

  static String offlineNote(String name) =>
      '$name is offline. Open Talvio on it to ask its assistant.';
  static String updateNote(String name) =>
      'Update Talvio on $name to ask its assistant from your phone.';

  // In live view.
  static const askHere = 'Ask the assistant';
  static const hideAssistant = 'Hide the assistant';
  static const openAssistant = 'Open in Assistant';
  static const showAnswer = 'Show the answer';
  static const hideAnswer = 'Hide the answer';
  static const keys = 'Keys';
  static const hideKeys = 'Hide keys';

  /// Text for a result error code (PROTOCOL.md, Assistant), or one the phone
  /// sets itself; [message] is a `failed` result's own reason. Codes every
  /// request shares fall through to [DictateStrings.error].
  static String error(String code, String name, {String? message}) =>
      switch (code) {
        'assistant_off' => 'Turn on the assistant in Talvio on $name.',
        'not_configured' =>
          'Talvio on $name has no AI model set up for the assistant.',
        'busy' => '$name is still answering your last question.',
        'failed' =>
          message == null
              ? "The assistant on $name couldn't answer. Try again."
              : "The assistant on $name couldn't answer: $message",
        'unsupported' =>
          'Update Talvio on $name to ask its assistant from your phone.',
        _ => DictateStrings.error(code, name),
      };
}
