import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
import '../../design/voice.dart';
import '../../shell/feature_tab.dart';
import '../screen/live_dock.dart' show openWatch;
import '../screen/live_protocol.dart';
import 'computer_widgets.dart';
import 'dictate_controller.dart';
import 'dictate_parts.dart';
import 'dictate_scope.dart';
import 'key_pad.dart';
import 'strings.dart';
import 'voice_recorder.dart';

export 'computer_widgets.dart' show windowLabel;

/// Dictating and typing into the computer, and pressing its keys.
final sendTab = FeatureTab(
  id: sendTabId,
  label: DictateStrings.tab,
  icon: AppIcons.dictate,
  selectedIcon: AppIcons.dictateSelected,
  requires: DictateKind.dictate,
  builder: (_) => const DictateScreen(),
  composer: true,
);

const sendTabId = 'send';

/// How long "Pasted on …" stays before the hint comes back.
const pastedNoteFor = Duration(seconds: 5);

/// Caps typed text at the 10 000 characters a `text` request may carry,
/// counted as PROTOCOL.md counts them (code points). `maxLength` would count
/// grapheme clusters, which lets emoji-heavy text past the computer's limit.
final typedTextLimit = TextInputFormatter.withFunction((old, next) {
  if (next.text.runes.length <= 10000) return next;
  final text = String.fromCharCodes(next.text.runes.take(10000));
  return TextEditingValue(
    text: text,
    selection: TextSelection.collapsed(offset: text.length),
  );
});

class DictateScreen extends StatefulWidget {
  const DictateScreen({super.key});

  @override
  State<DictateScreen> createState() => _DictateScreenState();
}

class _DictateScreenState extends State<DictateScreen>
    with SingleTickerProviderStateMixin {
  late final AppServices _services = AppServices.of(context);
  late final DictateController _controller = DictateScope.of(context);
  final _text = TextEditingController();
  final _textFocus = FocusNode();
  var _keysOpen = false;

  /// The outcome last seen, and whether its "Pasted" line has had its time.
  DictateOutcome? _seen;
  var _settled = false;

  /// Counts the time "Pasted" stays (frames, not a timer: an outcome can
  /// arrive outside the test clock).
  late final _settle = AnimationController(vsync: this, duration: pastedNoteFor)
    ..addStatusListener((status) {
      if (status == AnimationStatus.completed && mounted) {
        setState(() => _settled = true);
      }
    });

  @override
  void initState() {
    super.initState();
    _controller.addListener(_onOutcome);
  }

  @override
  void dispose() {
    _controller.removeListener(_onOutcome);
    _settle.dispose();
    _text.dispose();
    _textFocus.dispose();
    super.dispose();
  }

  void _onOutcome() {
    final outcome = _controller.outcome;
    if (identical(outcome, _seen)) return;
    _seen = outcome;
    _settled = false;
    _settle.stop();
    if (outcome == null) return;
    final error = outcome.error;
    if (error != null && error != 'cancelled') Haptic.error();
    if (error == null && (outcome.pasted || outcome.typed)) {
      unawaited(_settle.forward(from: 0));
    }
  }

  void _copy(String text) {
    unawaited(Clipboard.setData(ClipboardData(text: text)));
    Haptic.tick();
    showSnack(context, DictateStrings.copied);
  }

  Future<void> _chooseDestination() => showTalvioSheet<void>(
    context,
    title: DictateStrings.goesTo,
    builder: (sheet) => WindowPicker(
      _controller,
      onWatch: _canWatch
          ? (target, window) {
              Navigator.pop(sheet);
              openWatch(context, _controller, target, window: window);
            }
          : null,
    ),
  );

  /// The computer streams windows and screens (capability `screen`).
  bool get _canWatch =>
      _controller.hasWindows &&
      (_controller.computer?.capabilities.contains(ScreenKind.capability) ??
          false);

  /// Presses [keys] in the chosen window; a failure shows as a snack bar,
  /// since the stage belongs to the last dictation.
  Future<void> _press(List<String> keys) async {
    Haptic.tick();
    final name = _controller.computer?.name ?? '';
    _showKeyError(await _controller.pressKeys(keys), name);
  }

  void _showKeyError(String? error, String name) {
    if (error == null || !mounted) return;
    showSnack(
      context,
      DictateStrings.error(error, name),
      action: error == 'target_gone'
          ? SnackBarAction(
              label: DictateStrings.chooseWindow,
              onPressed: _chooseDestination,
            )
          : null,
    );
  }

  @override
  Widget build(BuildContext context) {
    final relay = _services.relay;
    return ListenableBuilder(
      listenable: Listenable.merge([
        _controller,
        relay.devices,
        relay.status,
        _textFocus,
        VoiceRecorder.holder,
      ]),
      builder: (context, _) =>
          ComputerGate(controller: _controller, builder: _buildBody),
    );
  }

  Widget _buildBody(BuildContext context, Device computer, bool connected) {
    final c = _controller;
    final idle = c.phase == DictatePhase.idle;
    final ready = connected && computer.online;
    final canDictate =
        ready && computer.capabilities.contains(DictateKind.dictate);
    final canType = ready && computer.capabilities.contains(DictateKind.text);
    // The window and key rows exist only for a computer that offers them.
    final hasWindows = c.hasWindows;
    final canAim = ready && hasWindows;
    final typing = _textFocus.hasFocus;
    final name = computer.name;

    // A computer that advertises nothing yet has not said `hello`.
    final offline = !computer.online;
    final outdated =
        !offline &&
        connected &&
        computer.capabilities.isNotEmpty &&
        !computer.capabilities.contains(DictateKind.dictate);

    final composer = Composer(
      controller: _text,
      focusNode: _textFocus,
      hint: offline
          ? DictateStrings.offlineTitle(name)
          : DictateStrings.typeHint(name),
      sendLabel: DictateStrings.send,
      enabled: canType && idle,
      inputFormatters: [typedTextLimit],
      maxLines: 4,
      onSubmit: c.sendText,
      onEnter: hasWindows && canAim ? () => _press(const ['enter']) : null,
      enterLabel: DictateStrings.pressEnter,
      leading: hasWindows
          ? ComposerButton(
              label: _keysOpen
                  ? DictateStrings.hideKeys
                  : DictateStrings.showKeys,
              icon: AppIcons.keyboard,
              active: _keysOpen,
              onPressed: offline || outdated
                  ? null
                  : () {
                      Haptic.tick();
                      FocusManager.instance.primaryFocus?.unfocus();
                      setState(() => _keysOpen = !_keysOpen);
                    },
            )
          : null,
    );

    if (offline || outdated) {
      return Column(
        children: [
          Expanded(
            child: EmptyState(
              icon: offline ? DuoIcons.offline : DuoIcons.update,
              title: offline
                  ? DictateStrings.offlineTitle(name)
                  : DictateStrings.updateTitle(name),
              body: offline
                  ? DictateStrings.offlineNote(name)
                  : DictateStrings.updateNote(name),
            ),
          ),
          composer,
        ],
      );
    }

    final voice = VoiceState(
      recording: c.phase == DictatePhase.recording,
      paused: c.paused,
      level: c.level,
      elapsed: c.elapsed,
      onStart: idle && canDictate && !c.micBlocked ? c.startRecording : null,
      onStop: c.stopAndSend,
      onDiscard: c.discardRecording,
      startLabel: DictateStrings.startRecording,
      stopLabel: DictateStrings.stopAndSend,
      discardLabel: DictateStrings.discard,
      pausedLabel: DictateStrings.paused,
    );

    final keys = _keysOpen && hasWindows;
    final destination = DestinationRow(
      window: c.window,
      computerName: name,
      deliver: c.deliver,
      hasWindows: hasWindows,
      // A recording or request keeps its window and delivery.
      onTap: idle ? _chooseDestination : null,
    );
    final recent = RecentDictationsList(
      items: c.recents.items,
      onCopy: _copy,
      onOpen: (item) => openDictation(context, c, item),
    );
    final Widget? bottom = keys
        ? Padding(
            padding: const EdgeInsets.fromLTRB(16, 16, 16, 8),
            child: KeyPad(
              platform: computer.platform,
              onPress: canAim ? _press : null,
            ),
          )
        : null;
    return LayoutBuilder(
      builder: (context, constraints) {
        // A short screen (a phone on its side, large text) scrolls the
        // whole page above the composer, the mic included.
        final scale = MediaQuery.textScalerOf(context).scale(1);
        final compact = constraints.maxHeight < 520 * scale;
        final stage = keys || typing
            ? null
            : _stage(context, c, voice, name, canDictate, compact: compact);
        if (compact) {
          return Column(
            children: [
              Expanded(
                child: SingleChildScrollView(
                  reverse: true,
                  child: Column(
                    children: [destination, recent, ?bottom, ?stage],
                  ),
                ),
              ),
              composer,
            ],
          );
        }
        return Column(
          children: [
            destination,
            Expanded(
              child: ListView(
                padding: const EdgeInsets.only(bottom: Space.s16),
                children: [recent],
              ),
            ),
            ?bottom,
            ?stage,
            composer,
          ],
        );
      },
    );
  }

  /// The status line or card, the mic, and the recording row.
  Widget _stage(
    BuildContext context,
    DictateController c,
    VoiceState voice,
    String name,
    bool canDictate, {
    bool compact = false,
  }) {
    final t = context.talvio;
    final outcome = c.outcome;
    final sending = c.phase == DictatePhase.sending;
    final idle = c.phase == DictatePhase.idle;

    Widget hint() => Padding(
      padding: const EdgeInsets.symmetric(horizontal: 32),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 300),
        child: Text(
          c.deliver == Deliver.keep
              ? DictateStrings.hintKeep
              : DictateStrings.hintPaste(name),
          key: const ValueKey('hint'),
          textAlign: TextAlign.center,
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
          style: TalvioType.footnote.copyWith(color: t.muted),
        ),
      ),
    );

    final Widget status;
    if (voice.recording && compact) {
      status = const SizedBox(key: ValueKey('none'));
    } else if (voice.recording) {
      final where = c.deliver == Deliver.keep
          ? DictateStrings.listeningKeep
          : switch (c.window) {
              final window? => DictateStrings.listeningFor(
                windowLabel(window).$1,
              ),
              null => DictateStrings.listeningCursor(name),
            };
      status = Semantics(
        key: const ValueKey('listening'),
        liveRegion: true,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 32),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  StatusDot(
                    color: voice.paused ? t.warning : t.live,
                    size: 8,
                    blink: !voice.paused,
                  ),
                  const SizedBox(width: Space.s8),
                  Text(
                    voice.paused
                        ? DictateStrings.paused
                        : DictateStrings.listening,
                    style: TalvioType.title2.copyWith(color: t.ink),
                  ),
                ],
              ),
              const SizedBox(height: Space.s4),
              Text(
                where,
                textAlign: TextAlign.center,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: TalvioType.footnote.copyWith(color: t.muted),
              ),
            ],
          ),
        ),
      );
    } else if (sending && c.stage == 'approval' && !c.cancelling) {
      status = RequestCard(
        key: const ValueKey('approval'),
        approval: true,
        title: DictateStrings.approval(name),
        detail: DictateStrings.approvalDetail,
        onCancel: c.cancelRequest,
        cancelLabel: DictateStrings.cancel,
      );
    } else if (sending) {
      final text = c.cancelling
          ? DictateStrings.cancelling
          : c.stage == 'pasting'
          ? DictateStrings.pasting(name)
          : DictateStrings.transcribing(name);
      status = Column(
        key: const ValueKey('sending'),
        mainAxisSize: MainAxisSize.min,
        children: [
          Semantics(
            liveRegion: true,
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Spinner(size: 14, color: t.muted),
                const SizedBox(width: Space.s8),
                Flexible(
                  child: Text(
                    text,
                    style: TalvioType.footnote.copyWith(color: t.muted),
                  ),
                ),
              ],
            ),
          ),
          Btn(
            DictateStrings.cancel,
            kind: BtnKind.ghostNeutral,
            compact: true,
            onPressed: c.cancelling ? null : c.cancelRequest,
          ),
        ],
      );
    } else if (outcome?.error case final error?) {
      status = InlineBanner(
        key: ObjectKey(outcome),
        tone: error == 'cancelled' ? Tone.neutral : Tone.danger,
        message: DictateStrings.error(error, outcome!.computer ?? name),
        action: c.windowLost && c.hasWindows && idle
            ? Btn(
                DictateStrings.chooseWindow,
                kind: BtnKind.ghost,
                compact: true,
                onPressed: _chooseDestination,
              )
            : c.canRetry && canDictate
            ? Btn(
                DictateStrings.tryAgain,
                kind: BtnKind.ghost,
                compact: true,
                onPressed: () {
                  Haptic.tap();
                  unawaited(c.retry());
                },
              )
            : null,
        onClose: c.dismissOutcome,
        closeLabel: DictateStrings.dismiss,
      );
    } else if (outcome != null && !outcome.pasted && !outcome.typed) {
      final text = outcome.text ?? '';
      final computer = c.computer;
      final canPaste =
          computer != null &&
          computer.online &&
          computer.capabilities.contains(DictateKind.text) &&
          idle;
      status = ReturnedCard(
        key: ObjectKey(outcome),
        outcome: outcome,
        computerName: outcome.computer ?? name,
        onOpen: () => openText(context, c, text),
        onCopy: () => _copy(text),
        onPaste: canPaste && text.isNotEmpty
            ? () {
                Haptic.tap();
                unawaited(c.sendText(text));
              }
            : null,
        onDismiss: c.dismissOutcome,
      );
    } else if (outcome != null && !_settled) {
      final app = switch (outcome.window) {
        final window? => windowLabel(window).$1,
        null => null,
      };
      final where = outcome.computer ?? name;
      final title = switch ((outcome.typed, app)) {
        (true, final app?) => DictateStrings.typedPastedIn(app, where),
        (true, null) => DictateStrings.typedPasted(where),
        (false, final app?) => DictateStrings.pastedIn(app, where),
        (false, null) => DictateStrings.pasted(where),
      };
      status = Semantics(
        key: ObjectKey(outcome),
        liveRegion: true,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 32),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(AppIcons.done, size: 16, color: t.success),
              const SizedBox(width: Space.s8),
              Flexible(
                child: Text(
                  title,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: TalvioType.footnote.copyWith(color: t.ink),
                ),
              ),
            ],
          ),
        ),
      );
    } else if (compact) {
      status = const SizedBox(key: ValueKey('none'));
    } else {
      status = hint();
    }

    return Padding(
      padding: EdgeInsets.only(
        top: compact ? Space.s8 : Space.s16,
        bottom: compact ? Space.s12 : Space.s24,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          AnimatedSwitcher(duration: Motion.quick, child: status),
          if (status.key != const ValueKey('none'))
            SizedBox(height: voice.recording ? Space.s40 : Space.s24),
          MicOrb(voice: voice, size: compact ? 64 : 80),
          if (voice.recording) ...[
            SizedBox(height: compact ? Space.s16 : Space.s40),
            RecordingStrip(voice: voice),
          ],
        ],
      ),
    );
  }
}
