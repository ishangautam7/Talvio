import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'models.dart';
import 'server_finder.dart';

/// Prefix for preferences that belong to the signed-in account (recent
/// dictations, the chosen computer). Signing out removes every one of them, so
/// a feature that stores account data only has to use the prefix.
const accountPrefsPrefix = 'account.';

/// How the account server was chosen (PROTOCOL.md, "Finding the server").
enum ServerChoice {
  /// Found through discovery: the default.
  automatic,

  /// Typed in the Server setting.
  custom,

  /// Built in with `--dart-define=ZYNVELIQ_SERVER_URL`.
  pinned,
}

class Session {
  const Session({
    required this.token,
    required this.serverUrl,
    required this.deviceId,
    required this.user,
    this.automatic = false,
  });

  factory Session.fromJson(Object? json) => switch (json) {
    {
      'token': String token,
      'serverUrl': String serverUrl,
      'deviceId': String deviceId,
      'user': Object user,
    } =>
      Session(
        token: token,
        serverUrl: serverUrl,
        deviceId: deviceId,
        user: User.fromJson(user),
        // Sessions from before discovery have no flag and stay bound.
        automatic: (json as Map)['automatic'] == true,
      ),
    _ => throw const FormatException('Not a Session'),
  };

  final String token;

  /// The server that issued [token], or in [automatic] mode the address
  /// discovery last named for it. Stored with it so the token is never sent
  /// anywhere else, even if the sign-in server setting is lost (an iOS
  /// reinstall keeps the keychain but not the preferences).
  final String serverUrl;
  final String deviceId;
  final User user;

  /// Signed in on the Automatic server, so [serverUrl] follows discovery
  /// (the server is the same, only its name moved). Otherwise the session is
  /// bound to [serverUrl].
  final bool automatic;

  Session withUser(User user) => Session(
    token: token,
    serverUrl: serverUrl,
    deviceId: deviceId,
    user: user,
    automatic: automatic,
  );

  Session withServerUrl(String serverUrl) => Session(
    token: token,
    serverUrl: serverUrl,
    deviceId: deviceId,
    user: user,
    automatic: automatic,
  );

  Map<String, Object?> toJson() => {
    'token': token,
    'serverUrl': serverUrl,
    'deviceId': deviceId,
    'user': user.toJson(),
    'automatic': automatic,
  };
}

/// Who is signed in, to which server. Notifies on sign-in, sign-out,
/// profile changes, and whatever [finder] learns.
class SessionStore extends ChangeNotifier {
  /// `pinned` is the build's server (`pinnedServerUrl` in config.dart);
  /// `fallback` is where Automatic goes while discovery has never answered
  /// (`developmentServerUrl`).
  SessionStore(this._prefs, this.finder, {this._pinned, this._fallback}) {
    finder.addListener(_onFinder);
  }

  static const _sessionKey = 'session';
  static const _serverKey = 'server_url';

  final SharedPreferences _prefs;

  /// Looks the server up while the choice is [ServerChoice.automatic].
  final ServerFinder finder;
  final String? _pinned;
  final String? _fallback;
  // This device only: a token restored or carried to a new iPhone would
  // share one device record with the old phone, and each would knock the
  // other offline (close 4000). Android already keeps it out of backups.
  final _secure = const FlutterSecureStorage(
    iOptions: IOSOptions(
      accessibility: KeychainAccessibility.unlocked_this_device,
    ),
  );
  Session? _session;

  Session? get session => _session;
  bool get signedIn => _session != null;

  /// The signed-in session's server; else the build's, the Custom one, the
  /// one discovery found, or the [fallback], in that order. Null when there
  /// is none: a release build whose discovery has never answered.
  String? get serverUrl =>
      _session?.serverUrl ??
      _pinned ??
      _prefs.getString(_serverKey) ??
      finder.server ??
      _fallback;

  /// How [serverUrl] was chosen. A session keeps the choice it signed in
  /// with.
  ServerChoice get serverChoice => switch (_session) {
    Session(automatic: true) => ServerChoice.automatic,
    _ when _pinned != null => ServerChoice.pinned,
    Session() => ServerChoice.custom,
    null when _prefs.containsKey(_serverKey) => ServerChoice.custom,
    null => ServerChoice.automatic,
  };

  Future<void> load() async {
    try {
      final raw = await _secure.read(key: _sessionKey);
      _session = raw == null ? null : Session.fromJson(jsonDecode(raw));
    } on Exception {
      // An unreadable entry (a restored backup whose key is gone, an older
      // format) means signed out; drop it so it cannot fail every launch.
      _session = null;
      await _secure.delete(key: _sessionKey);
    }
    _syncFinder();
  }

  /// Uses the Custom server [url], or Automatic when null.
  Future<void> setServerUrl(String? url) async {
    assert(!signedIn, 'The server can only change while signed out');
    if (url == null) {
      await _prefs.remove(_serverKey);
    } else {
      await _prefs.setString(_serverKey, url);
    }
    _syncFinder();
    notifyListeners();
  }

  Future<void> signIn(AuthResponse auth) async {
    final session = Session(
      token: auth.token,
      // Set: the server just answered.
      serverUrl: serverUrl!,
      deviceId: auth.device.id,
      user: auth.user,
      automatic: serverChoice == ServerChoice.automatic,
    );
    await _secure.write(key: _sessionKey, value: jsonEncode(session.toJson()));
    _session = session;
    notifyListeners();
  }

  Future<void> updateUser(User user) async {
    final session = _session;
    if (session == null) return;
    _session = session.withUser(user);
    await _save();
    notifyListeners();
  }

  /// Signs out on this phone only; the server is told separately (or already
  /// knows, when it revoked the token).
  Future<void> clear() async {
    if (_session == null) return;
    _session = null;
    notifyListeners();
    await _secure.delete(key: _sessionKey);
    for (final key in _prefs.getKeys()) {
      if (key.startsWith(accountPrefsPrefix)) await _prefs.remove(key);
    }
    _syncFinder();
  }

  void _syncFinder() => finder.enabled = serverChoice == ServerChoice.automatic;

  /// An Automatic session moves to the server's new address with the same
  /// token; the socket reconnects there when it next connects.
  void _onFinder() {
    final session = _session;
    final found = finder.server;
    if (session != null &&
        session.automatic &&
        found != null &&
        found != session.serverUrl) {
      _session = session.withServerUrl(found);
      unawaited(_save());
    }
    notifyListeners();
  }

  Future<void> _save() =>
      _secure.write(key: _sessionKey, value: jsonEncode(_session!.toJson()));
}
