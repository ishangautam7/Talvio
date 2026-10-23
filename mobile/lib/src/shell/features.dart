import '../core/relay_client.dart';
import '../features/account/account_screen.dart';
import '../features/dictate/dictate_screen.dart';
import 'feature_tab.dart';

// The one place features register. A new feature lives in its own folder
// under lib/src/features/ and adds its tab here, plus a handler below if
// computers send this phone requests of its own.

/// The home shell's tabs, in order.
final featureTabs = <FeatureTab>[dictateTab, accountTab];

/// Request kinds this phone serves, by kind. Their keys are also what it
/// advertises in `hello.capabilities`; any other request is answered
/// `unsupported`.
final relayHandlers = <String, RelayHandler>{};
