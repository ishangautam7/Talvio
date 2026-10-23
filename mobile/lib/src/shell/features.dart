import 'package:flutter/widgets.dart';

import '../core/relay_client.dart';
import '../features/account/account_screen.dart';
import '../features/dictate/computer_widgets.dart';
import '../features/dictate/dictate_scope.dart';
import '../features/dictate/dictate_screen.dart';
import '../features/dictate/history_screen.dart';
import '../features/screen/screen_home.dart';
import 'feature_tab.dart';
import 'home_shell.dart';

// The one place features register. A new feature lives in its own folder
// under lib/src/features/ and adds its page here, plus a handler below if
// computers send this phone requests of its own.

/// The menu's pages, in order; the first is home.
final featureTabs = <FeatureTab>[screenTab, sendTab, historyTab, accountTab];

/// The signed-in app: the pages, around the state they share (the computer
/// chosen, its window, a recording or request in flight).
Widget signedInHome() => DictateScope(
  child: HomeShell(tabs: featureTabs, header: (_) => const MenuHeader()),
);

/// Request kinds this phone serves, by kind. Their keys are also what it
/// advertises in `hello.capabilities`; any other request is answered
/// `unsupported`.
final relayHandlers = <String, RelayHandler>{};
