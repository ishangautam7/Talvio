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
  static const ended = 'Ended';
  static const stoppedByYou = 'You stopped watching.';
  static const watchAgain = 'Watch again';
  static const close = 'Close';

  static const rotate = 'Turn sideways';
  static const unrotate = 'Turn upright';
  static const fill = 'Fill the screen';
  static const fit = 'Show all of it';

  static const follow = 'Follow the cursor';
  static const following = 'Following the cursor';
  static const soundOn = 'Turn sound on';
  static const soundOff = 'Turn sound off';
  static const stop = 'Stop';

  static const minimized = 'This window is minimized on the computer.';
  static const covered = 'Another window covers this one on the computer.';

  /// Why a watch ended. Codes of the other phone features fall through to
  /// [DictateStrings.error].
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
    'timeout' => "$name didn't answer in time.",
    _ => DictateStrings.error(code, name),
  };
}
