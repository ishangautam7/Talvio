import 'dart:io' show Platform;

import 'package:flutter/material.dart';

import '../../core/api_client.dart';
import '../../core/app_services.dart';
import '../../core/config.dart';
import '../../core/session_store.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
import 'feedback.dart';
import 'strings.dart';

/// Where the draft and the reply address are kept between visits, so leaving
/// the screen or a failed send loses nothing. Under the account's prefix, so
/// signing out takes the address with it.
const feedbackDraftKey = '${accountPrefsPrefix}feedback.draft';
const feedbackEmailKey = '${accountPrefsPrefix}feedback.email';

/// The line this phone sends when "Include app version and phone" is on.
String thisPhoneSystemLine() => FeedbackRules.systemLine(
  appVersion: appVersion,
  os: Platform.operatingSystem,
  osVersion: Platform.operatingSystemVersion,
);

/// "Send feedback" from Account: a message, an optional reply address and,
/// if left ticked, the app version and phone line, posted to the account
/// server (PROTOCOL.md, "Feedback"). What is sent is exactly what is shown.
class FeedbackScreen extends StatefulWidget {
  const FeedbackScreen({super.key, this.systemLine});

  /// Stands in for [thisPhoneSystemLine] (tests).
  final String? systemLine;

  @override
  State<FeedbackScreen> createState() => _FeedbackScreenState();
}

class _FeedbackScreenState extends State<FeedbackScreen> {
  late final AppServices _services = AppServices.of(context);
  late final _message = TextEditingController(
    text: _services.prefs.getString(feedbackDraftKey) ?? '',
  );
  late final _email = TextEditingController(
    text: _services.prefs.getString(feedbackEmailKey) ?? '',
  );
  late final String _system = widget.systemLine ?? thisPhoneSystemLine();
  var _includeSystem = true;
  var _sending = false;
  String? _error;
  String? _sentTo;
  var _sent = false;

  @override
  void initState() {
    super.initState();
    _message.addListener(_changed);
    _email.addListener(_changed);
  }

  @override
  void dispose() {
    _message.dispose();
    _email.dispose();
    super.dispose();
  }

  void _changed() {
    if (!_sent) _services.prefs.setString(feedbackDraftKey, _message.text);
    setState(() {});
  }

  bool get _ready =>
      !_sending &&
      FeedbackRules.messageOk(_message.text) &&
      FeedbackRules.emailOk(_email.text);

  Future<void> _send() async {
    if (!_ready) return;
    final email = _email.text.trim();
    setState(() {
      _sending = true;
      _error = null;
    });
    try {
      await _services.api.sendFeedback(
        message: _message.text.trim(),
        email: email.isEmpty ? null : email,
        system: _includeSystem ? _system : null,
      );
      final prefs = _services.prefs;
      await prefs.remove(feedbackDraftKey);
      if (email.isEmpty) {
        await prefs.remove(feedbackEmailKey);
      } else {
        await prefs.setString(feedbackEmailKey, email);
      }
      if (!mounted) return;
      setState(() {
        _sent = true;
        _sentTo = email.isEmpty ? null : email;
      });
    } on ApiException catch (e) {
      if (mounted) setState(() => _error = FeedbackRules.errorText(e));
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Scaffold(
      appBar: AppBar(title: const Text(FeedbackStrings.title)),
      body: SafeArea(top: false, child: _sent ? _sentView(t) : _form(t)),
    );
  }

  Widget _sentView(TalvioColors t) => Padding(
    padding: const EdgeInsets.all(Space.s24),
    child: Column(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        Semantics(
          liveRegion: true,
          child: Text(
            FeedbackStrings.sentTitle,
            textAlign: TextAlign.center,
            style: TalvioType.title2.copyWith(color: t.ink),
          ),
        ),
        const SizedBox(height: Space.s8),
        Text(
          _sentTo == null
              ? FeedbackStrings.sentBody
              : FeedbackStrings.sentWithEmail(_sentTo!),
          textAlign: TextAlign.center,
          style: TalvioType.body.copyWith(color: t.muted),
        ),
        const SizedBox(height: Space.s24),
        Btn(FeedbackStrings.done, onPressed: () => Navigator.maybePop(context)),
      ],
    ),
  );

  Widget _form(TalvioColors t) {
    final length = FeedbackRules.length(_message.text);
    final email = _email.text.trim();
    final emailBad = email.isNotEmpty && !FeedbackRules.looksLikeEmail(email);
    return ListView(
      padding: const EdgeInsets.fromLTRB(Space.s16, Space.s8, Space.s16, 24),
      children: [
        Text(
          FeedbackStrings.intro,
          style: TalvioType.body.copyWith(color: t.muted),
        ),
        const SizedBox(height: Space.s16),
        TextField(
          controller: _message,
          enabled: !_sending,
          minLines: 6,
          maxLines: 12,
          textCapitalization: TextCapitalization.sentences,
          style: TalvioType.body.copyWith(color: t.ink),
          decoration: InputDecoration(
            labelText: FeedbackStrings.message,
            hintText: FeedbackStrings.messageHint,
            alignLabelWithHint: true,
            errorText: length > FeedbackRules.maxChars
                ? FeedbackStrings.messageLong
                : null,
            helperText: length > 0 && length < FeedbackRules.minChars
                ? FeedbackStrings.messageShort
                : null,
          ),
        ),
        const SizedBox(height: Space.s12),
        TextField(
          controller: _email,
          enabled: !_sending,
          keyboardType: TextInputType.emailAddress,
          autocorrect: false,
          autofillHints: const [AutofillHints.email],
          style: TalvioType.body.copyWith(color: t.ink),
          decoration: InputDecoration(
            labelText: FeedbackStrings.email,
            errorText: emailBad ? FeedbackStrings.emailInvalid : null,
          ),
        ),
        const SizedBox(height: Space.s8),
        CheckboxListTile(
          value: _includeSystem,
          contentPadding: EdgeInsets.zero,
          controlAffinity: ListTileControlAffinity.leading,
          onChanged: _sending
              ? null
              : (on) => setState(() => _includeSystem = on ?? false),
          title: Text(
            FeedbackStrings.includeSystem,
            style: TalvioType.body.copyWith(color: t.ink),
          ),
          subtitle: Text(
            FeedbackStrings.sentAs(_system),
            style: TalvioType.footnote.copyWith(color: t.muted),
          ),
        ),
        if (_error case final error?) ...[
          const SizedBox(height: Space.s12),
          InlineBanner(
            tone: Tone.danger,
            margin: EdgeInsets.zero,
            message: error,
          ),
        ],
        const SizedBox(height: Space.s16),
        Btn(
          FeedbackStrings.send,
          expand: true,
          loading: _sending,
          onPressed: _ready ? _send : null,
        ),
      ],
    );
  }
}
