import 'dart:async';
import 'dart:math' as math;

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
  icon: Icons.keyboard_voice_outlined,
  selectedIcon: Icons.keyboard_voice_rounded,
  requires: DictateKind.dictate,
  builder: (_) => const DictateScreen(),
);

const sendTabId = 'send';

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

class _DictateScreenState extends State<DictateScreen> {
  late final AppServices _services = AppServices.of(context);
  late final DictateController _controller = DictateScope.of(context);
  final _text = TextEditingController();
  final _textFocus = FocusNode();
  var _keysOpen = false;

  @override
  void dispose() {
    _text.dispose();
    _textFocus.dispose();
    super.dispose();
  }

  void _copy(String text) {
    unawaited(Clipboard.setData(ClipboardData(text: text)));
    Haptic.tick();
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(const SnackBar(content: Text(DictateStrings.copied)));
  }

  Future<void> _chooseWindow() => showModalBottomSheet<void>(
    context: context,
    isScrollControlled: true,
    builder: (sheet) => FractionallySizedBox(
      heightFactor: 0.9,
      child: SafeArea(
        child: WindowPicker(
          _controller,
          onWatch: _canWatch
              ? (target, window) {
                  Navigator.pop(sheet);
                  openWatch(context, _controller, target, window: window);
                }
              : null,
        ),
      ),
    ),
  );

  /// The computer streams windows and screens (capability `screen`).
  bool get _canWatch =>
      _controller.hasWindows &&
      (_controller.computer?.capabilities.contains(ScreenKind.capability) ??
          false);

  /// Presses [keys] in the chosen window; a failure shows as a snack bar,
  /// since the screen's card belongs to the last dictation.
  Future<void> _press(List<String> keys) async {
    Haptic.tick();
    final name = _controller.computer?.name ?? '';
    _showKeyError(await _controller.pressKeys(keys), name);
  }

  Future<void> _pressEnter() async {
    Haptic.tap();
    final name = _controller.computer?.name ?? '';
    _showKeyError(await _controller.pressEnter(), name);
  }

  void _showKeyError(String? error, String name) {
    if (error == null || !mounted) return;
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(
        SnackBar(
          content: Text(DictateStrings.error(error, name)),
          action: error == 'target_gone'
              ? SnackBarAction(
                  label: DictateStrings.chooseWindow,
                  onPressed: _chooseWindow,
                )
              : null,
        ),
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
    final outcome = c.outcome;
    final recents = c.recents.items;

    // A computer that advertises nothing yet has not said `hello`.
    final note = !computer.online
        ? DictateStrings.offlineNote(computer.name)
        : connected &&
              computer.capabilities.isNotEmpty &&
              !computer.capabilities.contains(DictateKind.dictate)
        ? DictateStrings.updateNote(computer.name)
        : null;

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

    return LayoutBuilder(
      builder: (context, constraints) {
        // A short screen (a small phone, landscape) gets a smaller mic and
        // no hint, so what happened stays in view above them.
        final short = constraints.maxHeight < 520;
        return Column(
          children: [
            Expanded(
              child: FadeBottom(
                child: ListView(
                  padding: const EdgeInsets.fromLTRB(
                    Space.lg,
                    Space.xs,
                    Space.lg,
                    Space.lg,
                  ),
                  children: [
                    if (note != null && c.phase != DictatePhase.sending)
                      Padding(
                        padding: const EdgeInsets.only(bottom: Space.md),
                        child: Notice(
                          tone: computer.online ? Tone.warning : Tone.neutral,
                          icon: computer.online
                              ? Icons.system_update_alt_rounded
                              : Icons.power_settings_new_rounded,
                          message: note,
                        ),
                      ),
                    if (c.phase == DictatePhase.sending)
                      SendingCard(
                        stage: c.stage,
                        cancelling: c.cancelling,
                        computerName: computer.name,
                        onCancel: c.cancelling ? null : c.cancelRequest,
                      )
                    else if (outcome != null)
                      Appear(
                        key: ObjectKey(outcome),
                        child: OutcomeCard(
                          outcome: outcome,
                          computerName: outcome.computer ?? computer.name,
                          onCopy: _copy,
                          onRetry: c.canRetry && canDictate ? c.retry : null,
                          onChooseWindow: c.windowLost && hasWindows && idle
                              ? _chooseWindow
                              : null,
                          onPressEnter: c.offersEnter && hasWindows
                              ? _pressEnter
                              : null,
                          onDismiss: c.dismissOutcome,
                        ),
                      ),
                    // What happened comes first; where the next text goes, under it.
                    if (c.phase == DictatePhase.sending || outcome != null)
                      const SizedBox(height: Space.md),
                    TargetCard(
                      window: c.window,
                      computerName: computer.name,
                      hasWindows: hasWindows,
                      deliver: c.deliver,
                      // A recording or request keeps its window and delivery.
                      onChooseWindow: canAim && idle ? _chooseWindow : null,
                      onDeliver: idle ? (value) => c.deliver = value : null,
                    ),
                    RecentDictationsList(
                      items: recents,
                      onCopy: _copy,
                      onOpen: (item) => openDictation(context, c, item),
                    ),
                  ],
                ),
              ),
            ),
            // Short screens (landscape, large text) scroll the controls instead
            // of pushing them under the navigation bar, and keep some room for
            // the cards above. While typing, the controls are the text field.
            ConstrainedBox(
              constraints: BoxConstraints(
                maxHeight: typing
                    ? constraints.maxHeight
                    : math.max(
                        constraints.maxHeight - 120,
                        constraints.maxHeight / 2,
                      ),
              ),
              child: SingleChildScrollView(
                reverse: true,
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(
                    Space.lg,
                    0,
                    Space.lg,
                    Space.md,
                  ),
                  child: Column(
                    children: [
                      // While typing the keyboard needs the room, and the mic
                      // only applies to speech.
                      if (!typing) ...[
                        MicOrb(
                          voice: voice,
                          busy: c.phase == DictatePhase.sending,
                          size: short ? 60 : 84,
                        ),
                        if (voice.recording || !short)
                          SizedBox(
                            height: 48,
                            child: voice.recording
                                ? RecordingStrip(voice: voice)
                                : Center(
                                    child: Text(
                                      idle
                                          ? (c.deliver == Deliver.keep
                                                ? DictateStrings.hintKeep
                                                : DictateStrings.hintPaste(
                                                    computer.name,
                                                  ))
                                          : '',
                                      textAlign: TextAlign.center,
                                      maxLines: 2,
                                      overflow: TextOverflow.ellipsis,
                                      style: context.text.bodySmall,
                                    ),
                                  ),
                          ),
                        const SizedBox(height: Space.sm),
                      ],
                      if (hasWindows) ...[
                        KeysRow(
                          open: _keysOpen,
                          onToggle: () =>
                              setState(() => _keysOpen = !_keysOpen),
                          onEnter: canAim
                              ? () => _press(const ['enter'])
                              : null,
                        ),
                        if (_keysOpen) ...[
                          const SizedBox(height: Space.sm),
                          KeyPad(
                            platform: computer.platform,
                            onPress: canAim ? _press : null,
                          ),
                        ],
                        const SizedBox(height: Space.sm),
                      ],
                      Composer(
                        controller: _text,
                        focusNode: _textFocus,
                        hint: DictateStrings.typeHint(computer.name),
                        sendLabel: DictateStrings.send,
                        enabled: canType && idle,
                        inputFormatters: [typedTextLimit],
                        maxLines: 4,
                        onSubmit: c.sendText,
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ],
        );
      },
    );
  }
}
