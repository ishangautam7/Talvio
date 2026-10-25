abstract final class DictateStrings {
  static const tab = 'Dictate';
  static const title = 'Dictate';
  static const chooseComputer = 'Choose computer';
  static const pickComputerTitle = 'Choose a computer';
  static const pickComputerBody =
      'What you dictate or type goes to the computer you pick.';
  static const online = 'Online';
  static const offline = 'Offline';
  static String onlineOn(String platform) => switch (platform) {
    'windows' => 'Online · Windows',
    'macos' => 'Online · Mac',
    'linux' => 'Online · Linux',
    _ => 'Online',
  };
  static const connecting = 'Connecting to the server…';
  static const connectingShort = 'Connecting…';
  static const connectingTitle = 'Connecting';

  static const noComputerTitle = 'Add your computer';
  static String noComputerBody(String email) =>
      'Open Talvio on your computer and sign in with $email. '
      'It shows up here once it is signed in.';
  static String offlineTitle(String name) => '$name is offline';
  static String offlineNote(String name) =>
      '$name is offline. Open Talvio on it to dictate from your phone.';
  static String updateNote(String name) =>
      'Update Talvio on $name to dictate from your phone.';
  static String hintPaste(String name) =>
      'Tap the mic and speak. The text goes wherever the cursor is on $name.';
  static const hintKeep =
      'Tap the mic and speak. The text comes back to this phone.';
  static const goesTo = 'Text goes to';
  static const showKeys = 'Keys';
  static const hideKeys = 'Hide keys';

  static const pasteOnComputer = 'Paste there';
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
      "Look at the computer's screen. Talvio is asking there.";
  static const cancelling = 'Cancelling…';
  static const cancel = 'Cancel';
  static String pasted(String name) => 'Pasted on $name';
  static String typedPasted(String name) => 'Sent to $name and pasted.';
  static String returned(String name) => 'Transcribed on $name';
  static String pasteOffNote(String name) =>
      "Pasting is turned off on $name, so the text came back here. Turn it "
      "on in Talvio's Settings, under Dictation, Paste method.";
  static const result = 'Your text';
  static const copy = 'Copy';
  static const copied = 'Copied';
  static const tryAgain = 'Try again';
  static const dismiss = 'Dismiss';
  static const recent = 'Recent';

  // History: what was dictated, on this phone only.
  static const historyTab = 'History';
  static const historyTitle = 'History';
  static const historyEmptyTitle = 'Nothing dictated yet';
  static const historyEmptyBody =
      'What you dictate shows here, newest first, so you can copy it again. '
      'It stays on this phone.';
  static const historyNote = 'On this phone';
  static const dictateNow = 'Dictate';
  static String sendAgain(String name) => 'Paste on $name';

  // Choosing a window and pressing keys (capability `windows`).
  static const cursorWindow = 'Where the cursor is';
  static String cursorWindowDetail(String name) =>
      'The window in front on $name';
  static const chooseWindow = 'Choose window';
  static const pickWindowTitle = 'Choose a window';
  static String pickWindowBody(String name) =>
      'It comes forward on $name for a moment, then goes back.';
  static const refresh = 'Refresh';
  static const inFront = 'In front';
  static const loadingWindows = 'Looking for windows…';
  static String noWindows(String name) => 'No other windows are open on $name.';
  static String pastedIn(String app, String name) => 'Pasted in $app on $name';
  static String typedPastedIn(String app, String name) =>
      'Sent to $app on $name and pasted.';
  static const untitled = 'Untitled';
  static const pressEnter = 'Press Enter';
  static const keys = 'Keys';
  static const moreKeys = 'More keys';
  static const keyEnter = 'Enter';
  static const keyTab = 'Tab';
  static const keyEscape = 'Esc';
  static const keyEscapeLabel = 'Escape';
  static const keyLeft = 'Left arrow';
  static const keyUp = 'Up arrow';
  static const keyDown = 'Down arrow';
  static const keyRight = 'Right arrow';
  static const keyBackspace = 'Backspace';
  static const keyShiftEnter = 'Shift + Enter (new line)';
  static const keySpace = 'Space';
  static String keyCopy(String mod) => '$mod + C (copy)';
  static String keyPaste(String mod) => '$mod + V (paste)';
  static String keyUndo(String mod) => '$mod + Z (undo)';
  static String keySelectAll(String mod) => '$mod + A (select all)';
  static String modifier(String platform) =>
      platform == 'macos' ? 'Cmd' : 'Ctrl';

  /// Text for a result error code from the computer (PROTOCOL.md) or one the
  /// phone sets itself (offline, not_connected, interrupted, timeout, mic_*).
  static String error(String code, String name) => switch (code) {
    'remote_input_off' =>
      'Phone dictation is off on $name. Turn it on in Talvio on that '
          'computer, then try again.',
    'busy' => '$name is busy with another request. Try again in a moment.',
    'no_speech_model' =>
      '$name has no speech model yet. Download one in Talvio on that '
          'computer.',
    'bad_audio' => "$name couldn't read that recording. Record it again.",
    'no_speech' => "Nothing was heard. Try again a bit closer to the phone.",
    'transcription_failed' => "$name couldn't transcribe that. Try again.",
    'paste_failed' =>
      "$name couldn't paste the text. Click into a text field there and try "
          'again.',
    'paste_off' =>
      "Pasting is turned off on $name. Turn it on in Talvio's Settings, "
          'under Dictation, Paste method.',
    'unsupported' => 'Update Talvio on $name to use this.',
    'cancelled' => 'Cancelled.',
    'remote_windows_off' =>
      "$name doesn't let phones use its windows. Turn on \"Let phones see "
          'and use windows" in Talvio on that computer, under Settings, Account.',
    'target_gone' =>
      'That window closed on $name, or it has been a while since you chose '
          'it. Choose the window again.',
    'focus_failed' =>
      "That window on $name didn't come forward. It may not be responding. "
          'Try again or choose another window.',
    'keys_failed' => "Couldn't press that key on $name. Try again.",
    'key_refused' =>
      "$name doesn't press keys that close apps or windows, lock it, or "
          'sign out.',
    'not_approved' =>
      "This phone isn't allowed on $name. Allow it in Talvio's Settings on "
          'that computer, under Account, then try again.',
    'offline' => '$name is offline. Open Talvio on it and try again.',
    'not_connected' =>
      "You're not connected to the server. Check your connection and try "
          'again.',
    'interrupted' =>
      'Lost the connection before $name answered, so it may have finished '
          'anyway. Check $name before you try again.',
    'timeout' => "$name didn't answer in time. Try again.",
    'mic_denied' =>
      'Talvio needs the microphone to hear you. Allow it for Talvio in '
          "your phone's Settings, then try again.",
    'mic_failed' => "Couldn't use the microphone. Try again.",
    'mic_busy' =>
      'Another page is recording. Send or discard that recording first.',
    _ => 'Something went wrong. Try again.',
  };
}
