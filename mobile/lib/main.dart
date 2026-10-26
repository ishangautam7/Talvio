import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'src/app.dart';
import 'src/core/api_client.dart';
import 'src/core/config.dart';
import 'src/core/relay_client.dart';
import 'src/core/server_finder.dart';
import 'src/core/session_store.dart';
import 'src/shell/features.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // The fonts' and icons' licences, for the Licences page under Account.
  LicenseRegistry.addLicense(() async* {
    for (final (family, file) in const [
      ('Geist', 'OFL-Geist.txt'),
      ('Plus Jakarta Sans', 'OFL-PlusJakartaSans.txt'),
      ('Phosphor Icons', 'LICENSE-Phosphor.txt'),
    ]) {
      yield LicenseEntryWithLineBreaks([
        family,
      ], await rootBundle.loadString('assets/fonts/$file'));
    }
  });
  // Drawn edge to edge, under transparent system bars.
  unawaited(SystemChrome.setEnabledSystemUIMode(SystemUiMode.edgeToEdge));
  // A build made with an invalid TALVIO_SERVER_URL stops here.
  final pinned = pinnedServerUrl();
  final prefs = await SharedPreferences.getInstance();
  final session = SessionStore(
    prefs,
    ServerFinder(prefs),
    pinned: pinned,
    fallback: developmentServerUrl(),
  );
  await session.load();
  runApp(
    TalvioApp(
      session: session,
      api: ApiClient(session),
      relay: RelayClient(session, handlers: relayHandlers),
      prefs: prefs,
    ),
  );
}
