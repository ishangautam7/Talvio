import 'package:flutter/widgets.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'api_client.dart';
import 'relay_client.dart';
import 'session_store.dart';

/// The app's long-lived objects, for any screen below [AppServices].
class AppServices extends InheritedWidget {
  const AppServices({
    super.key,
    required this.session,
    required this.api,
    required this.relay,
    required this.prefs,
    required super.child,
  });

  final SessionStore session;
  final ApiClient api;
  final RelayClient relay;
  final SharedPreferences prefs;

  static AppServices of(BuildContext context) =>
      context.getInheritedWidgetOfExactType<AppServices>()!;

  @override
  bool updateShouldNotify(AppServices oldWidget) => false;
}
