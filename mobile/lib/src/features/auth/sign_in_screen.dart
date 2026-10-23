import 'dart:async';

import 'package:flutter/material.dart';

import '../../core/api_client.dart';
import '../../core/app_services.dart';
import '../../core/config.dart';
import '../../core/models.dart';
import '../../core/session_store.dart';
import '../../core/strings.dart';
import '../../core/widgets.dart';
import 'google_mark.dart';
import 'google_sign_in.dart';
import 'strings.dart';

enum _Mode { signIn, create }

enum _Busy { none, password, google }

class SignInScreen extends StatefulWidget {
  const SignInScreen({super.key});

  @override
  State<SignInScreen> createState() => _SignInScreenState();
}

class _SignInScreenState extends State<SignInScreen> {
  /// How soon to ask again whether the server offers Google, while it cannot
  /// be reached.
  static const _configRetry = Duration(seconds: 15);

  final _formKey = GlobalKey<FormState>();
  final _name = TextEditingController();
  final _email = TextEditingController();
  final _password = TextEditingController();
  var _mode = _Mode.signIn;
  var _busy = _Busy.none;
  var _obscure = true;
  var _google = false;
  Timer? _configTimer;
  String? _error;

  /// The server [_google] and [_error] are about.
  String? _server;
  late final SessionStore _session = _services.session;

  AppServices get _services => AppServices.of(context);

  @override
  void initState() {
    super.initState();
    _server = _session.serverUrl;
    _session.addListener(_onSession);
    _loadConfig();
  }

  /// Shows what discovery learns; a different server starts over.
  void _onSession() {
    final server = _session.serverUrl;
    final moved = server != _server;
    _server = server;
    setState(() {
      if (moved) {
        _error = null;
        _google = false;
      }
    });
    if (moved) unawaited(_loadConfig());
  }

  @override
  void dispose() {
    _session.removeListener(_onSession);
    _configTimer?.cancel();
    _name.dispose();
    _email.dispose();
    _password.dispose();
    super.dispose();
  }

  Future<void> _loadConfig() async {
    _configTimer?.cancel();
    final session = _services.session;
    final server = session.serverUrl;
    // Asked again once discovery names one.
    if (server == null) return;
    try {
      final google = await _services.api.googleEnabled();
      // Ignore an answer from a server the user has since moved away from.
      if (mounted && session.serverUrl == server) {
        setState(() => _google = google);
      }
    } on ApiException {
      // Unreachable (a network still coming up at launch) or not a Talvio
      // server: no Google button for now, and signing in says what is wrong.
      // Ask again, or someone who signed up with Google never sees it.
      if (mounted && session.serverUrl == server) {
        _configTimer?.cancel();
        _configTimer = Timer(_configRetry, _loadConfig);
      }
    }
  }

  Future<void> _submit() async {
    if (_busy != _Busy.none || !_formKey.currentState!.validate()) return;
    final api = _services.api;
    final email = _email.text.trim();
    final name = _name.text.trim();
    await _run(
      _Busy.password,
      () => _mode == _Mode.signIn
          ? api.logIn(
              email: email,
              password: _password.text,
              device: thisDevice(),
            )
          : api.signUp(
              email: email,
              password: _password.text,
              name: name.isEmpty ? null : name,
              device: thisDevice(),
            ),
    );
  }

  Future<void> _signInWithGoogle() =>
      _run(_Busy.google, () => signInWithGoogle(_services.api));

  Future<void> _run(_Busy busy, Future<AuthResponse?> Function() call) async {
    if (_busy != _Busy.none) return;
    final session = _services.session;
    setState(() {
      _busy = busy;
      _error = null;
    });
    try {
      final auth = await call();
      if (auth != null) await session.signIn(auth);
    } on ApiException catch (e) {
      if (mounted) setState(() => _error = AuthStrings.error(e));
      // The server answered, or the network is back: maybe Google is there.
      if (mounted && !_google) unawaited(_loadConfig());
    } finally {
      if (mounted) setState(() => _busy = _Busy.none);
    }
  }

  /// The server discovery named, or that it is still being looked for (a
  /// release build whose discovery has never answered). There is nothing to
  /// change here: the server only ever comes from discovery.
  Widget _serverControl(ThemeData theme) {
    final session = _services.session;
    final finder = session.finder;
    final url = session.serverUrl;
    final choice = session.serverChoice;
    return Wrap(
      alignment: WrapAlignment.center,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        Text(
          url != null
              ? serverText(choice, url)
              : finder.looking
              ? AuthStrings.serverLooking
              : AuthStrings.serverNotFound,
          style: theme.textTheme.bodySmall?.copyWith(
            color: theme.colorScheme.onSurfaceVariant,
          ),
        ),
        if (url == null && !finder.looking)
          TextButton(
            onPressed: finder.lookUp,
            child: const Text(AuthStrings.tryAgain),
          ),
        if (url == null && !finder.looking && finder.lastError != null)
          SizedBox(
            width: double.infinity,
            child: Text(
              AuthStrings.serverProblem(finder.lastError!),
              textAlign: TextAlign.center,
              style: theme.textTheme.bodySmall?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ),
          ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final dark = theme.brightness == Brightness.dark;
    final creating = _mode == _Mode.create;
    final busy = _busy != _Busy.none;

    return Scaffold(
      body: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(24),
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 420),
              child: AutofillGroup(
                child: Form(
                  key: _formKey,
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Text(
                        creating
                            ? AuthStrings.createTitle
                            : AuthStrings.signInTitle,
                        style: theme.textTheme.headlineSmall,
                      ),
                      const SizedBox(height: 8),
                      Text(
                        AuthStrings.subtitle,
                        style: theme.textTheme.bodyMedium?.copyWith(
                          color: theme.colorScheme.onSurfaceVariant,
                        ),
                      ),
                      const SizedBox(height: 24),
                      SegmentedButton<_Mode>(
                        showSelectedIcon: false,
                        segments: const [
                          ButtonSegment(
                            value: _Mode.signIn,
                            label: Text(AuthStrings.signInTab),
                          ),
                          ButtonSegment(
                            value: _Mode.create,
                            label: Text(AuthStrings.createTab),
                          ),
                        ],
                        selected: {_mode},
                        onSelectionChanged: busy
                            ? null
                            : (mode) => setState(() {
                                _mode = mode.single;
                                _error = null;
                              }),
                      ),
                      const SizedBox(height: 24),
                      if (creating) ...[
                        TextFormField(
                          controller: _name,
                          enabled: !busy,
                          textCapitalization: TextCapitalization.words,
                          textInputAction: TextInputAction.next,
                          autofillHints: const [AutofillHints.name],
                          decoration: const InputDecoration(
                            labelText: AuthStrings.name,
                          ),
                          validator: (value) =>
                              (value ?? '').trim().runes.length > 64
                              ? AuthStrings.nameLong
                              : null,
                        ),
                        const SizedBox(height: 16),
                      ],
                      TextFormField(
                        controller: _email,
                        enabled: !busy,
                        keyboardType: TextInputType.emailAddress,
                        autocorrect: false,
                        textInputAction: TextInputAction.next,
                        autofillHints: const [AutofillHints.email],
                        decoration: const InputDecoration(
                          labelText: AuthStrings.email,
                        ),
                        validator: (value) {
                          final email = (value ?? '').trim();
                          if (email.isEmpty) return AuthStrings.emailMissing;
                          if (!RegExp(r'^\S+@\S+$').hasMatch(email)) {
                            return AuthStrings.emailInvalid;
                          }
                          return null;
                        },
                      ),
                      const SizedBox(height: 16),
                      TextFormField(
                        controller: _password,
                        enabled: !busy,
                        obscureText: _obscure,
                        autocorrect: false,
                        enableSuggestions: false,
                        textInputAction: TextInputAction.done,
                        autofillHints: [
                          creating
                              ? AutofillHints.newPassword
                              : AutofillHints.password,
                        ],
                        decoration: InputDecoration(
                          labelText: AuthStrings.password,
                          helperText: creating
                              ? AuthStrings.passwordHelp
                              : null,
                          suffixIcon: IconButton(
                            tooltip: _obscure
                                ? AuthStrings.showPassword
                                : AuthStrings.hidePassword,
                            icon: Icon(
                              _obscure
                                  ? Icons.visibility_outlined
                                  : Icons.visibility_off_outlined,
                            ),
                            onPressed: () =>
                                setState(() => _obscure = !_obscure),
                          ),
                        ),
                        validator: (value) {
                          final password = value ?? '';
                          if (password.isEmpty) {
                            return AuthStrings.passwordMissing;
                          }
                          if (!creating) return null;
                          if (password.runes.length < 8) {
                            return AuthStrings.passwordShort;
                          }
                          if (password.runes.length > 256) {
                            return AuthStrings.passwordLong;
                          }
                          return null;
                        },
                        onFieldSubmitted: (_) => _submit(),
                      ),
                      if (_error case final error?) ...[
                        const SizedBox(height: 16),
                        Semantics(
                          liveRegion: true,
                          child: Text(
                            error,
                            style: theme.textTheme.bodyMedium?.copyWith(
                              color: theme.colorScheme.error,
                            ),
                          ),
                        ),
                      ],
                      const SizedBox(height: 24),
                      FilledButton(
                        onPressed: busy ? null : _submit,
                        style: FilledButton.styleFrom(
                          minimumSize: const Size.fromHeight(48),
                        ),
                        child: _busy == _Busy.password
                            ? const ButtonSpinner()
                            : Text(
                                creating
                                    ? AuthStrings.createAccount
                                    : AuthStrings.signIn,
                              ),
                      ),
                      if (_google) ...[
                        const SizedBox(height: 24),
                        Row(
                          children: [
                            const Expanded(child: Divider()),
                            Padding(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 16,
                              ),
                              child: Text(
                                AuthStrings.or,
                                style: theme.textTheme.bodySmall,
                              ),
                            ),
                            const Expanded(child: Divider()),
                          ],
                        ),
                        const SizedBox(height: 24),
                        // Google's branding: its "G" and its button colours.
                        OutlinedButton(
                          onPressed: busy ? null : _signInWithGoogle,
                          style: OutlinedButton.styleFrom(
                            minimumSize: const Size.fromHeight(48),
                            backgroundColor: dark
                                ? const Color(0xFF131314)
                                : Colors.white,
                            foregroundColor: dark
                                ? const Color(0xFFE3E3E3)
                                : const Color(0xFF1F1F1F),
                            side: BorderSide(
                              color: dark
                                  ? const Color(0xFF8E918F)
                                  : const Color(0xFF747775),
                            ),
                          ),
                          child: _busy == _Busy.google
                              ? const ButtonSpinner()
                              : const Row(
                                  mainAxisSize: MainAxisSize.min,
                                  children: [
                                    GoogleMark(),
                                    SizedBox(width: 10),
                                    Flexible(child: Text(AuthStrings.google)),
                                  ],
                                ),
                        ),
                      ],
                      const SizedBox(height: 32),
                      _serverControl(theme),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
