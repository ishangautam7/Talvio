import 'dart:async';

import 'package:flutter/material.dart';

import '../../core/api_client.dart';
import '../../core/app_services.dart';
import '../../core/config.dart';
import '../../core/models.dart';
import '../../core/session_store.dart';
import '../../design/brand.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
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
  Widget _serverStatus(BuildContext context) {
    final t = context.talvio;
    final session = _services.session;
    final finder = session.finder;
    final url = session.serverUrl;
    final style = TalvioType.footnote.copyWith(color: t.muted);
    if (url != null) {
      final label = session.serverChoice == ServerChoice.pinned
          ? AuthStrings.developmentServer
          : AuthStrings.server;
      return Text.rich(
        TextSpan(
          children: [
            TextSpan(text: '$label · '),
            TextSpan(text: Uri.parse(url).authority, style: TalvioType.mono),
          ],
        ),
        textAlign: TextAlign.center,
        style: style,
      );
    }
    if (finder.looking) {
      return Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Spinner(size: 14, color: t.muted),
          const SizedBox(width: Space.s8),
          Flexible(child: Text(AuthStrings.serverLooking, style: style)),
        ],
      );
    }
    return InlineBanner(
      tone: Tone.warning,
      margin: EdgeInsets.zero,
      message: switch (finder.lastError) {
        final error? =>
          '${AuthStrings.serverNotFound}. ${AuthStrings.serverProblem(error)}',
        null => AuthStrings.serverNotFound,
      },
      action: Btn(
        AuthStrings.tryAgain,
        kind: BtnKind.ghostNeutral,
        compact: true,
        onPressed: finder.lookUp,
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final creating = _mode == _Mode.create;
    final busy = _busy != _Busy.none;
    const scroll = EdgeInsets.only(bottom: 120);

    final form = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const Align(
          alignment: AlignmentDirectional.centerStart,
          child: TalvioMark(size: 44),
        ),
        const SizedBox(height: Space.s24),
        Semantics(
          header: true,
          child: Text(
            creating ? AuthStrings.createTitle : AuthStrings.signInTitle,
            style: TalvioType.display.copyWith(color: t.ink),
          ),
        ),
        const SizedBox(height: Space.s8),
        Text(
          AuthStrings.subtitle,
          style: TalvioType.rowBody.copyWith(color: t.muted),
        ),
        const SizedBox(height: Space.s32),
        SegmentedPill<_Mode>(
          height: 40,
          options: const [
            (_Mode.signIn, AuthStrings.signInTab, null),
            (_Mode.create, AuthStrings.createTab, null),
          ],
          selected: _mode,
          onChanged: busy
              ? null
              : (mode) => setState(() {
                  _mode = mode;
                  _error = null;
                }),
        ),
        const SizedBox(height: Space.s24),
        if (creating) ...[
          TextFormField(
            controller: _name,
            enabled: !busy,
            scrollPadding: scroll,
            textCapitalization: TextCapitalization.words,
            textInputAction: TextInputAction.next,
            autofillHints: const [AutofillHints.name],
            style: TalvioType.body.copyWith(color: t.ink),
            decoration: const InputDecoration(labelText: AuthStrings.name),
            validator: (value) => (value ?? '').trim().runes.length > 64
                ? AuthStrings.nameLong
                : null,
          ),
          const SizedBox(height: Space.s12),
        ],
        TextFormField(
          controller: _email,
          enabled: !busy,
          scrollPadding: scroll,
          keyboardType: TextInputType.emailAddress,
          autocorrect: false,
          textInputAction: TextInputAction.next,
          autofillHints: const [AutofillHints.email],
          style: TalvioType.body.copyWith(color: t.ink),
          decoration: const InputDecoration(labelText: AuthStrings.email),
          validator: (value) {
            final email = (value ?? '').trim();
            if (email.isEmpty) return AuthStrings.emailMissing;
            if (!RegExp(r'^\S+@\S+$').hasMatch(email)) {
              return AuthStrings.emailInvalid;
            }
            return null;
          },
        ),
        const SizedBox(height: Space.s12),
        TextFormField(
          controller: _password,
          enabled: !busy,
          scrollPadding: scroll,
          obscureText: _obscure,
          autocorrect: false,
          enableSuggestions: false,
          textInputAction: TextInputAction.done,
          autofillHints: [
            creating ? AutofillHints.newPassword : AutofillHints.password,
          ],
          style: TalvioType.body.copyWith(color: t.ink),
          decoration: InputDecoration(
            labelText: AuthStrings.password,
            helperText: creating ? AuthStrings.passwordHelp : null,
            suffixIcon: Padding(
              padding: const EdgeInsetsDirectional.only(end: 4),
              child: IconBtn(
                tooltip: _obscure
                    ? AuthStrings.showPassword
                    : AuthStrings.hidePassword,
                icon: _obscure ? AppIcons.show : AppIcons.hide,
                color: t.muted,
                onPressed: () => setState(() => _obscure = !_obscure),
              ),
            ),
          ),
          validator: (value) {
            final password = value ?? '';
            if (password.isEmpty) return AuthStrings.passwordMissing;
            if (!creating) return null;
            if (password.runes.length < 8) return AuthStrings.passwordShort;
            if (password.runes.length > 256) return AuthStrings.passwordLong;
            return null;
          },
          onFieldSubmitted: (_) => _submit(),
        ),
        if (_error case final error?) ...[
          const SizedBox(height: Space.s24),
          Appear(
            key: ValueKey(error),
            child: InlineBanner(
              tone: Tone.danger,
              margin: EdgeInsets.zero,
              message: error,
            ),
          ),
          const SizedBox(height: Space.s16),
        ] else
          const SizedBox(height: Space.s24),
        Btn(
          creating ? AuthStrings.createAccount : AuthStrings.signIn,
          expand: true,
          loading: _busy == _Busy.password,
          onPressed: busy ? null : _submit,
        ),
        if (_google) ...[
          const SizedBox(height: Space.s20),
          Row(
            children: [
              Expanded(child: Divider(color: t.hairline)),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: Space.s12),
                child: Text(
                  AuthStrings.or,
                  style: TalvioType.footnote.copyWith(color: t.muted),
                ),
              ),
              Expanded(child: Divider(color: t.hairline)),
            ],
          ),
          const SizedBox(height: Space.s20),
          Btn(
            AuthStrings.google,
            kind: BtnKind.secondary,
            expand: true,
            loading: _busy == _Busy.google,
            leading: const GoogleMark(size: 20),
            onPressed: busy ? null : _signInWithGoogle,
          ),
        ],
      ],
    );

    return Scaffold(
      body: DecoratedBox(
        decoration: BoxDecoration(
          gradient: RadialGradient(
            center: Alignment.topCenter,
            radius: 0.8,
            colors: [
              t.coral.withValues(alpha: t.isDark ? 0.12 : 0.08),
              t.coral.withValues(alpha: 0),
            ],
          ),
        ),
        child: SafeArea(
          child: AutofillGroup(
            child: Form(
              key: _formKey,
              child: LayoutBuilder(
                builder: (context, constraints) => SingleChildScrollView(
                  child: ConstrainedBox(
                    constraints: BoxConstraints(
                      minHeight: constraints.maxHeight,
                    ),
                    child: Center(
                      child: ConstrainedBox(
                        constraints: const BoxConstraints(maxWidth: 420),
                        child: Padding(
                          padding: const EdgeInsets.fromLTRB(16, 48, 16, 16),
                          child: ConstrainedBox(
                            constraints: BoxConstraints(
                              minHeight: constraints.maxHeight - 64,
                            ),
                            child: Column(
                              mainAxisAlignment: MainAxisAlignment.spaceBetween,
                              crossAxisAlignment: CrossAxisAlignment.stretch,
                              children: [
                                form,
                                Padding(
                                  padding: const EdgeInsets.only(
                                    top: Space.s32,
                                  ),
                                  child: _serverStatus(context),
                                ),
                              ],
                            ),
                          ),
                        ),
                      ),
                    ),
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
