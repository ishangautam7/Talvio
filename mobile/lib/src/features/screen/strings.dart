import '../dictate/strings.dart';

abstract final class ScreenStrings {
  // Home: the computer's screen and windows.
  static const homeTab = 'Screen';
  static const viewScreen = 'View screen';
  static String viewScreenDetail(String screen, String size) =>
      '$screen · $size';
  static const openWindows = 'Open windows';
  static const otherScreens = 'Screens';
  static const loadingWindows = 'Looking for windows and screens…';
  static String noWindows(String name) => 'No windows are open on $name.';
  static const refresh = 'Refresh';
  static const inFront = DictateStrings.inFront;
  static const dictateOrType = 'Dictate or type';
  static const sendInstead = 'Dictate or type instead';
  static String turnOnTitle(String name) => "See $name's screen here";
  static const turnOnBody =
      'In Talvio on the computer, open Settings, Account, and turn on '
      '"Let phones see and use windows". Then pull down here to refresh.';
  static String updateBody(String name) =>
      'Update Talvio on $name to see its screen from your phone.';
  static String offlineBody(String name) =>
      'Open Talvio on $name to see its screen and send to it.';
  static String waitingBody(String name) => 'Connecting to $name…';
  static String online(String platform) => switch (platform) {
    'windows' => 'Online · Windows',
    'macos' => 'Online · Mac',
    'linux' => 'Online · Linux',
    _ => 'Online',
  };
  static const offline = 'Offline';

  static const watch = 'Watch';
  static String watchItem(String label) => 'Watch $label';
  static const screens = 'Screens';
  static const windows = 'Windows';
  static const primary = 'Main';
  static String screenSize(int width, int height) => '$width × $height';
  static const untitledScreen = 'Screen';

  static const stopWatching = 'Stop watching';
  static const starting = 'Starting…';
  static const approval = 'Allow on computer';
  static const approvalDetail =
      "Look at the computer's screen. Talvio is asking there.";
  static const connecting = 'Connecting…';
  static const live = 'Live';
  static const reconnecting = 'Reconnecting…';
  static const switching = 'Switching…';
  static const ended = 'Ended';
  static const stoppedByYou = 'You stopped watching.';
  static const watchAgain = 'Watch again';
  static const close = 'Close';

  static const view = 'View';
  static const controlMode = 'Control';
  static const viewHint = 'View: pinch to zoom, drag to move around.';
  static const controlHint =
      'Control: tap to click, hold for right click, double tap to double '
      'click, drag two fingers up or down to scroll.';

  static const keys = 'Keys';
  static const hideKeys = 'Hide keys';
  static const rotate = 'Turn sideways';
  static const unrotate = 'Turn upright';
  static const fill = 'Fill the screen';
  static const fit = 'Show all of it';

  static const follow = 'Follow the cursor';
  static const following = 'Following the cursor';
  static const soundOn = 'Turn sound on';
  static const soundOff = 'Turn sound off';
  static const keyboard = 'Type';
  static const hideKeyboard = 'Hide keyboard';
  static const dictate = 'Dictate into this window';
  static const stopDictating = 'Stop and send';
  static String typeHint(String title) => 'Type into $title';
  static const send = 'Send';
  static String transcribing(String name) => 'Transcribing on $name…';

  static const minimized = 'This window is minimized on the computer.';
  static const covered = 'Another window covers this one on the computer.';

  static String confirmTitle(String label) => 'Press "$label" on the computer?';
  static const confirmBody =
      'This button may send, buy, post or delete something.';
  static const confirmUnlabelled = 'Press this button on the computer?';
  static const cancel = 'Cancel';
  static const press = 'Press';

  /// Why a watch ended, or why an input was not delivered. Codes of the
  /// other phone features fall through to [DictateStrings.error].
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
    'busy' => 'Someone is already watching this computer.',
    'remote_windows_off' =>
      "$name doesn't let phones see and use windows. Turn on \"Let phones "
          'see and use windows" in Talvio on that computer, under Settings, '
          'Account.',
    'not_allowed' => 'Switch to Control to click and scroll.',
    'password_field' => "Talvio doesn't type or click into password fields.",
    'not_live' => 'Not connected to the window yet. Try again in a moment.',
    'timeout' => "$name didn't answer in time.",
    'confirm' => 'Confirm on the phone first.',
    _ => DictateStrings.error(code, name),
  };
}
