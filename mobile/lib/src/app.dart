import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'core/api_client.dart';
import 'core/app_services.dart';
import 'core/relay_client.dart';
import 'core/session_store.dart';
import 'features/auth/sign_in_screen.dart';
import 'shell/features.dart';
import 'shell/home_shell.dart';
import 'theme.dart';

class ZynveliqApp extends StatefulWidget {
  const ZynveliqApp({
    super.key,
    required this.session,
    required this.api,
    required this.relay,
    required this.prefs,
  });

  final SessionStore session;
  final ApiClient api;
  final RelayClient relay;
  final SharedPreferences prefs;

  @override
  State<ZynveliqApp> createState() => _ZynveliqAppState();
}

class _ZynveliqAppState extends State<ZynveliqApp> {
  final _navigator = GlobalKey<NavigatorState>();
  late final AppLifecycleListener _lifecycle;
  var _foreground = true;

  @override
  void initState() {
    super.initState();
    // The socket is only open while someone can use it: signed in, and the
    // app on screen or a request still waiting for its result. A computer
    // asking its user to allow this phone sends the user over to it, and the
    // phone locks meanwhile; that result must still arrive.
    _lifecycle = AppLifecycleListener(
      onResume: () => _setForeground(true),
      onPause: () => _setForeground(false),
    );
    widget.session.addListener(_sync);
    widget.relay.busy.addListener(_sync);
    _sync();
  }

  @override
  void dispose() {
    _lifecycle.dispose();
    widget.session.removeListener(_sync);
    widget.relay.busy.removeListener(_sync);
    super.dispose();
  }

  void _setForeground(bool foreground) {
    _foreground = foreground;
    _sync();
  }

  void _sync() {
    final signedIn = widget.session.signedIn;
    if (signedIn && (_foreground || widget.relay.busy.value)) {
      widget.relay.connect();
    } else {
      widget.relay.disconnect();
    }
    // A sign-out can come from the server at any time; close whatever sheet
    // or dialog was open over the signed-in screens.
    if (!signedIn) _navigator.currentState?.popUntil((route) => route.isFirst);
  }

  @override
  Widget build(BuildContext context) {
    return AppServices(
      session: widget.session,
      api: widget.api,
      relay: widget.relay,
      prefs: widget.prefs,
      child: MaterialApp(
        navigatorKey: _navigator,
        title: 'Zynveliq',
        theme: buildTheme(Brightness.light),
        darkTheme: buildTheme(Brightness.dark),
        home: ListenableBuilder(
          listenable: widget.session,
          builder: (context, _) => widget.session.signedIn
              ? HomeShell(tabs: featureTabs)
              : const SignInScreen(),
        ),
      ),
    );
  }
}
