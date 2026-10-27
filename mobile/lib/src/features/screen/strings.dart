import '../dictate/strings.dart';

abstract final class ScreenStrings {
  // The Screen page: the computer's screens and windows.
  static const homeTab = 'Screen';
  static String heroTitle(String name) => "See $name's screen";
  static const heroBody =
      'Watch it in a small view at the top while you talk to your computer. '
      'Open it full screen to type, or switch to Control to click and '
      'scroll too.';
  static const watching = 'Watching';
  static const moreOptions = 'More options';
  static const viewScreen = 'View screen';
  static String viewScreenDetail(String screen, String size) =>
      '$screen · $size';
  static const openWindows = 'Windows';
  static const otherScreens = 'Screens';
  static const loadingWindows = 'Looking for windows and screens…';
  static String noWindows(String name) => 'No windows are open on $name.';
  static const refresh = 'Refresh';
  static const inFront = DictateStrings.inFront;
  static String turnOnTitle(String name) => "See $name's screen here";
  static const turnOnSteps = [
    'Open Talvio on the computer',
    'Go to Settings, then Account',
    'Turn on "Let phones see and use windows"',
  ];
  static const turnOnBody = 'Then pull down here to refresh.';
  static String updateBody(String name) =>
      'Update Talvio on $name to see its screen from your phone.';
  static String offlineBody(String name) =>
      'Open Talvio on $name to see its screen.';
  static String waitingBody(String name) => 'Connecting to $name…';
  static String online(String platform) => DictateStrings.onlineOn(platform);
  static const offline = 'Offline';

  static const watch = 'Watch';
  static String watchItem(String label) => 'Watch $label';
  static const screens = 'Screens';
  static const windows = 'Windows';
  static const primary = 'Main';
  static String screenSize(int width, int height) => '$width × $height';
  static const untitledScreen = 'Screen';
  static const showScreen = 'Show the screen';
  static const hideScreen = 'Hide the screen';

  // The live view, small and full screen.
  static const stopWatching = 'Stop watching';
  static const starting = 'Starting…';
  static const approval = 'Allow on computer';
  static const approvalDetail =
      "Look at the computer's screen. Talvio is asking there.";
  static const connecting = 'Connecting…';
  static const live = 'Live';
  static const reconnecting = 'Reconnecting…';
  static const ended = 'Ended';
  static const stoppedByYou = 'You stopped watching.';
  static const watchAgain = 'Watch again';
  static const close = 'Close';
  static const fullScreen = 'Full screen';
  static const exitFullScreen = 'Back to the small view';
  static const collapse = 'Make it smaller';
  static const expand = 'Make it bigger';
  static const inFullScreen = 'Showing full screen';

  static const rotate = 'Turn sideways';
  static const unrotate = 'Turn upright';
  static const fill = 'Fill the screen';
  static const fit = 'Show all of it';

  static const follow = 'Follow the cursor';
  static const following = 'Following the cursor';
  static const soundOn = 'Turn sound on';
  static const soundOff = 'Turn sound off';
  static const stop = 'Stop';

  // View or Control: a new watch either way.
  static const view = 'View';
  static const controlMode = 'Control';
  static const viewHint = 'View: pinch to zoom, drag to move around.';
  static const controlHint =
      'Control: tap to click, double tap to double click, hold for a right '
      'click, drag two fingers to scroll. Pinch to zoom.';

  // Typing into the watched window.
  static const type = 'Type';
  static const hideType = 'Hide typing';
  static String typeHint(String title) => 'Type into $title';
  static const sendText = 'Send text';

  // A tap on a control that sends, buys, posts or deletes.
  static String confirmTitle(String label) => 'Press "$label" on the computer?';
  static const confirmUnlabelled = 'Press this on the computer?';
  static const confirmBody =
      'It may send, buy, post or delete something, and that may not be '
      'undone.';
  static const cancel = 'Cancel';
  static const doIt = 'Do it';

  static const minimized = 'This window is minimized on the computer.';
  static const covered = 'Another window covers this one on the computer.';

  /// Why a watch ended, or why input was not delivered. Codes of the other
  /// phone features fall through to [DictateStrings.error].
  static String error(String code, String name) => switch (code) {
    'stopped' => 'Stopped on the computer.',
    'target_gone' =>
      'That window closed on $name, or it has been a while since it was '
          'listed. Choose it again.',
    'idle' => 'Stopped after 10 minutes with nothing happening.',
    'connect_failed' =>
      "Couldn't connect. On different networks this needs a TURN server — "
          'see the server README.',
    'capture_failed' => "$name couldn't capture that window or screen.",
    'busy' => 'Another view of $name is still running. Try again in a moment.',
    'replaced' => 'This view was replaced by a newer one from this phone.',
    'remote_windows_off' =>
      "$name doesn't let phones see and use windows. Turn on \"Let phones "
          'see and use windows" in Talvio on that computer, under Settings, '
          'Account.',
    'timeout' => "$name didn't answer in time.",
    'not_allowed' => 'Switch to Control to click and scroll.',
    'password_field' => "Talvio doesn't type or click into password fields.",
    'tap_failed' => "Couldn't click there on $name. Try again.",
    'not_live' => 'Not connected to the window yet. Try again in a moment.',
    'confirm' => 'Confirm on the phone first.',
    'confirm_expired' =>
      'That was asked too long ago, and the window may have changed. '
          'Tap it again.',
    _ => DictateStrings.error(code, name),
  };
}
