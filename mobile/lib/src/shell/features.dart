import 'package:flutter/widgets.dart';

import '../core/relay_client.dart';
import '../features/assistant/assistant_scope.dart';
import '../features/assistant/assistant_screen.dart';
import '../features/command/command_scope.dart';
import '../features/command/command_screen.dart';
import '../features/dictate/dictate_scope.dart';
import '../features/dictate/dictate_screen.dart';
import '../features/screen/live_controller.dart';
import '../features/screen/screen_home.dart';
import 'feature_tab.dart';
import 'home_shell.dart';

// The one place features register. A new feature lives in its own folder
// under lib/src/features/ and adds its page here, plus a handler below if
// computers send this phone requests of its own.

/// The navigation's pages, in order; the first available is home. Account
/// is behind the avatar in the top bar, not a page.
final featureTabs = <FeatureTab>[assistantTab, doTab, sendTab, screenTab];

/// The signed-in app: the pages, around the state they share (the computer
/// chosen, its window, a recording or request in flight, a command waiting
/// for Do it, the assistant chat, the live view).
Widget signedInHome() => DictateScope(
  child: CommandScope(
    child: AssistantScope(
      child: LiveScope(child: HomeShell(tabs: featureTabs)),
    ),
  ),
);

/// Request kinds this phone serves, by kind. Their keys are also what it
/// advertises in `hello.capabilities`; any other request is answered
/// `unsupported`.
final relayHandlers = <String, RelayHandler>{};
