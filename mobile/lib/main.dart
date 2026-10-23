import 'package:flutter/material.dart';
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
  // A build made with an invalid ZYNVELIQ_SERVER_URL stops here.
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
    ZynveliqApp(
      session: session,
      api: ApiClient(session),
      relay: RelayClient(session, handlers: relayHandlers),
      prefs: prefs,
    ),
  );
}
