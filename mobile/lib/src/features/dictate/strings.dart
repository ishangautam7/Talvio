abstract final class DictateStrings {
  static const tab = 'Dictate';
  static const title = 'Dictate';
  static const chooseComputer = 'Send to';
  static const pickComputerTitle = 'Choose a computer';
  static const pickComputerBody =
      'What you dictate or type goes to the computer you pick.';
  static const online = 'Online';
  static const offline = 'Offline';
  static const connecting = 'Connecting to the server…';

  static const noComputerTitle = 'Add your computer';
  static String noComputerBody(String email) =>
      'Open Zynveliq on your computer and sign in with $email. '
      'It shows up here once it is signed in.';
  static String offlineNote(String name) =>
      '$name is offline. Open Zynveliq on it to dictate from your phone.';
  static String updateNote(String name) =>
      'Update Zynveliq on $name to dictate from your phone.';
  static String hintPaste(String name) =>
      'Tap the mic and speak. The text goes wherever the cursor is on $name.';
  static const hintKeep =
      'Tap the mic and speak. The text comes back to this phone.';

  static const pasteOnComputer = 'Paste on computer';
  static const keepOnPhone = 'Keep on phone';
  static const startRecording = 'Start dictating';
  static const stopAndSend = 'Stop and send';
  static const tapToSpeak = 'Tap to speak';
  static const tapToSend = 'Tap to send';
  static const paused = 'Paused';
  static const discard = 'Discard';
  static String typeHint(String name) => 'Type to paste on $name';
  static const send = 'Send';

  static String transcribing(String name) => 'Transcribing on $name…';
  static String pasting(String name) => 'Pasting on $name…';
  static String approval(String name) => 'Allow this phone on $name';
  static const approvalDetail =
      "Look at the computer's screen. Zynveliq is asking there.";
  static const cancelling = 'Cancelling…';
  static const cancel = 'Cancel';
  static String pasted(String name) => 'Pasted on $name';
  static String typedPasted(String name) => 'Sent to $name and pasted.';
  static const result = 'Your text';
  static const copy = 'Copy';
  static const copied = 'Copied';
  static const tryAgain = 'Try again';
  static const dismiss = 'Dismiss';
  static const recent = 'Recent';

  /// Text for a result error code from the computer (PROTOCOL.md) or one the
  /// phone sets itself (offline, not_connected, interrupted, timeout, mic_*).
  static String error(String code, String name) => switch (code) {
    'remote_input_off' =>
      'Phone dictation is off on $name. Turn it on in Zynveliq on that '
          'computer, then try again.',
    'busy' => '$name is busy with another request. Try again in a moment.',
    'no_speech_model' =>
      '$name has no speech model yet. Download one in Zynveliq on that '
          'computer.',
    'bad_audio' => "$name couldn't read that recording. Record it again.",
    'no_speech' => "Nothing was heard. Try again a bit closer to the phone.",
    'transcription_failed' => "$name couldn't transcribe that. Try again.",
    'paste_failed' =>
      "$name couldn't paste the text. Click into a text field there and try "
          'again.',
    'paste_off' =>
      "Pasting is turned off on $name. Turn it on in Zynveliq's Settings, "
          'under Dictation, Paste method.',
    'unsupported' => 'Update Zynveliq on $name to use this.',
    'cancelled' => 'Cancelled.',
    'not_approved' =>
      "This phone isn't allowed on $name. Allow it in Zynveliq's Settings on "
          'that computer, under Account, then try again.',
    'offline' => '$name is offline. Open Zynveliq on it and try again.',
    'not_connected' =>
      "You're not connected to the server. Check your connection and try "
          'again.',
    'interrupted' =>
      'Lost the connection before $name answered, so it may have finished '
          'anyway. Check $name before you try again.',
    'timeout' => "$name didn't answer in time. Try again.",
    'mic_denied' =>
      'Zynveliq needs the microphone to hear you. Allow it for Zynveliq in '
          "your phone's Settings, then try again.",
    'mic_failed' => "Couldn't use the microphone. Try again.",
    _ => 'Something went wrong. Try again.',
  };
}
