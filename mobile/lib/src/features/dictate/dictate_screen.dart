import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../core/app_services.dart';
import '../../core/device_icon.dart';
import '../../core/models.dart';
import '../../core/relay_client.dart';
import '../../core/widgets.dart';
import '../../shell/feature_tab.dart';
import '../screen/live_protocol.dart';
import '../screen/strings.dart';
import '../screen/watch_screen.dart';
import 'dictate_controller.dart';
import 'recent_dictations.dart';
import 'remote_windows.dart';
import 'strings.dart';

final dictateTab = FeatureTab(
  id: 'dictate',
  label: DictateStrings.tab,
  icon: Icons.mic_none,
  selectedIcon: Icons.mic,
  requires: DictateKind.dictate,
  builder: (_) => const DictateScreen(),
);

class DictateScreen extends StatefulWidget {
  const DictateScreen({super.key});

  @override
  State<DictateScreen> createState() => _DictateScreenState();
}

class _DictateScreenState extends State<DictateScreen> {
  late final AppServices _services = AppServices.of(context);
  late final DictateController _controller = DictateController(
    relay: _services.relay,
    prefs: _services.prefs,
  );
  final _text = TextEditingController();
  final _textFocus = FocusNode();

  @override
  void dispose() {
    _controller.dispose();
    _text.dispose();
    _textFocus.dispose();
    super.dispose();
  }

  Future<void> _sendText() async {
    final text = _text.text.trim();
    if (text.isEmpty) return;
    if (await _controller.sendText(text) && mounted) _text.clear();
  }

  void _copy(String text) {
    Clipboard.setData(ClipboardData(text: text));
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(const SnackBar(content: Text(DictateStrings.copied)));
  }

  Future<void> _chooseComputer() => showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    builder: (context) => SafeArea(
      child: ListView(
        shrinkWrap: true,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(24, 0, 24, 8),
            child: Text(
              DictateStrings.chooseComputer,
              style: Theme.of(context).textTheme.titleMedium,
            ),
          ),
          for (final computer in _controller.computers)
            _ComputerTile(
              computer,
              selected: computer.id == _controller.computer?.id,
              onTap: () {
                _controller.choose(computer.id);
                Navigator.pop(context);
              },
            ),
        ],
      ),
    ),
  );

  Future<void> _chooseWindow() => showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    isScrollControlled: true,
    builder: (sheet) => FractionallySizedBox(
      heightFactor: 0.9,
      child: SafeArea(
        child: _WindowPicker(
          _controller,
          onWatch: _canWatch
              ? (target, window) {
                  Navigator.pop(sheet);
                  _watch(target, window: window);
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

  /// Opens live view of [target]. What is watched is also where dictation
  /// and typing go: [window], or for a screen wherever the keyboard is.
  void _watch(WatchTarget target, {RemoteWindow? window}) {
    final computer = _controller.computer;
    if (computer == null) return;
    _controller.chooseWindow(window);
    unawaited(
      Navigator.of(context).push(
        watchRoute(dictate: _controller, computer: computer, target: target),
      ),
    );
  }

  /// Presses [keys] in the chosen window; a failure shows as a snack bar,
  /// since the screen's card belongs to the last dictation.
  Future<void> _press(List<String> keys) async {
    unawaited(HapticFeedback.lightImpact());
    final name = _controller.computer?.name ?? '';
    _showKeyError(await _controller.pressKeys(keys), name);
  }

  Future<void> _pressEnter() async {
    unawaited(HapticFeedback.lightImpact());
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
      ]),
      builder: (context, _) {
        final connected = relay.status.value == RelayStatus.connected;
        final known = relay.devices.value != null;
        final computers = _controller.computers;
        final computer = _controller.computer;
        return Scaffold(
          appBar: AppBar(
            title: computer == null
                ? const Text(DictateStrings.title)
                : _ComputerTitle(
                    computer,
                    // A recording or request keeps its computer to the end.
                    onTap:
                        computers.length > 1 &&
                            _controller.phase == DictatePhase.idle
                        ? _chooseComputer
                        : null,
                  ),
          ),
          body: Column(
            children: [
              if (!connected) const _ConnectionBanner(),
              Expanded(
                child: switch ((known, computer)) {
                  (false, _) => const Center(
                    child: CircularProgressIndicator(),
                  ),
                  (true, null) when computers.isNotEmpty => _PickComputer(
                    computers,
                    onPick: _controller.choose,
                  ),
                  (true, null) => _NoComputer(
                    email: _services.session.session?.user.email ?? '',
                  ),
                  (true, final Device computer) => _buildBody(
                    context,
                    computer,
                    connected,
                  ),
                },
              ),
            ],
          ),
        );
      },
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
    final (progress, progressDetail) = c.cancelling
        ? (DictateStrings.cancelling, null)
        : switch (c.stage) {
            'approval' => (
              DictateStrings.approval(computer.name),
              DictateStrings.approvalDetail,
            ),
            'pasting' => (DictateStrings.pasting(computer.name), null),
            _ => (DictateStrings.transcribing(computer.name), null),
          };

    // A computer that advertises nothing yet has not said `hello`.
    final note = !computer.online
        ? DictateStrings.offlineNote(computer.name)
        : connected &&
              computer.capabilities.isNotEmpty &&
              !computer.capabilities.contains(DictateKind.dictate)
        ? DictateStrings.updateNote(computer.name)
        : null;

    return LayoutBuilder(
      builder: (context, constraints) => Column(
        children: [
          Expanded(
            child: ListView(
              padding: const EdgeInsets.all(16),
              children: [
                if (note != null && c.phase != DictatePhase.sending)
                  _Note(note),
                if (c.phase == DictatePhase.sending)
                  _ProgressCard(
                    text: progress,
                    detail: progressDetail,
                    onCancel: c.cancelling ? null : c.cancelRequest,
                  )
                else if (outcome != null)
                  _OutcomeCard(
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
                if (note == null &&
                    outcome == null &&
                    idle &&
                    c.recents.items.isEmpty)
                  _Hint(
                    c.deliver == Deliver.keep
                        ? DictateStrings.hintKeep
                        : DictateStrings.hintPaste(computer.name),
                  ),
                if (c.recents.items.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  SectionTitle(DictateStrings.recent),
                  for (final item in c.recents.items)
                    _RecentTile(item, onCopy: _copy),
                ],
              ],
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
                      constraints.maxHeight - 96,
                      constraints.maxHeight / 2,
                    ),
            ),
            child: SingleChildScrollView(
              child: SafeArea(
                top: false,
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(16, 8, 16, 12),
                  child: Column(
                    children: [
                      if (hasWindows) ...[
                        Row(
                          children: [
                            Expanded(
                              child: _WindowRow(
                                window: c.window,
                                computerName: computer.name,
                                // A recording or request keeps its window too.
                                onTap: canAim && idle ? _chooseWindow : null,
                              ),
                            ),
                            if (c.window case final window? when _canWatch)
                              IconButton(
                                tooltip: ScreenStrings.watch,
                                onPressed: canAim && idle
                                    ? () => _watch(
                                        WatchTarget(
                                          id: window.id,
                                          label: windowLabel(window).$1,
                                          screen: false,
                                        ),
                                        window: window,
                                      )
                                    : null,
                                icon: const Icon(Icons.visibility_outlined),
                              ),
                          ],
                        ),
                        const SizedBox(height: 8),
                      ],
                      // While typing the keyboard needs the room, and the delivery
                      // choice and mic only apply to speech.
                      if (!typing) ...[
                        SegmentedButton<Deliver>(
                          showSelectedIcon: false,
                          segments: const [
                            ButtonSegment(
                              value: Deliver.paste,
                              label: Text(DictateStrings.pasteOnComputer),
                            ),
                            ButtonSegment(
                              value: Deliver.keep,
                              label: Text(DictateStrings.keepOnPhone),
                            ),
                          ],
                          selected: {c.deliver},
                          onSelectionChanged: idle
                              ? (value) => c.deliver = value.single
                              : null,
                        ),
                        const SizedBox(height: 12),
                        _MicButton(
                          phase: c.phase,
                          level: c.level,
                          onPressed: switch (c.phase) {
                            DictatePhase.idle when canDictate =>
                              c.startRecording,
                            DictatePhase.recording => c.stopAndSend,
                            _ => null,
                          },
                        ),
                        ConstrainedBox(
                          constraints: const BoxConstraints(minHeight: 48),
                          child: c.phase == DictatePhase.recording
                              ? _RecordingCaption(
                                  elapsed: c.elapsed,
                                  paused: c.paused,
                                  onDiscard: c.discardRecording,
                                )
                              : Center(
                                  child: Text(
                                    idle ? DictateStrings.tapToSpeak : '',
                                    style: _muted(context),
                                  ),
                                ),
                        ),
                      ],
                      _Composer(
                        controller: _text,
                        focusNode: _textFocus,
                        hint: DictateStrings.typeHint(computer.name),
                        enabled: canType && idle,
                        onSend: _sendText,
                      ),
                      // Below the mic and the field, which matter more on a
                      // small phone; next to the field, where Enter is wanted.
                      if (hasWindows) ...[
                        const SizedBox(height: 8),
                        KeyRow(
                          platform: computer.platform,
                          onPress: canAim ? _press : null,
                        ),
                      ],
                    ],
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

TextStyle? _muted(BuildContext context) => Theme.of(context)
    .textTheme
    .bodyMedium
    ?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant);

class _ComputerTitle extends StatelessWidget {
  const _ComputerTitle(this.computer, {required this.onTap});

  final Device computer;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Semantics(
      button: onTap != null,
      label: DictateStrings.chooseComputer,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(8),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              DeviceIcon(computer),
              const SizedBox(width: 12),
              Flexible(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      computer.name,
                      overflow: TextOverflow.ellipsis,
                      style: theme.textTheme.titleMedium,
                    ),
                    Text(
                      computer.online
                          ? DictateStrings.online
                          : DictateStrings.offline,
                      style: theme.textTheme.bodySmall?.copyWith(
                        color: theme.colorScheme.onSurfaceVariant,
                      ),
                    ),
                  ],
                ),
              ),
              if (onTap != null) const Icon(Icons.arrow_drop_down),
            ],
          ),
        ),
      ),
    );
  }
}

class _ConnectionBanner extends StatelessWidget {
  const _ConnectionBanner();

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Theme.of(context).colorScheme.surfaceContainerHighest,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
        child: Row(
          children: [
            const SizedBox.square(
              dimension: 16,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Text(
                DictateStrings.connecting,
                style: Theme.of(context).textTheme.bodyMedium,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _ComputerTile extends StatelessWidget {
  const _ComputerTile(
    this.computer, {
    required this.selected,
    required this.onTap,
  });

  final Device computer;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return ListTile(
      contentPadding: const EdgeInsets.symmetric(horizontal: 24),
      leading: DeviceIcon(computer),
      title: Text(computer.name),
      subtitle: Text(
        computer.online ? DictateStrings.online : DictateStrings.offline,
      ),
      trailing: selected ? const Icon(Icons.check) : null,
      onTap: onTap,
    );
  }
}

/// Several computers and no choice yet (or the chosen one left the account).
class _PickComputer extends StatelessWidget {
  const _PickComputer(this.computers, {required this.onPick});

  final List<Device> computers;
  final ValueChanged<String> onPick;

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.symmetric(vertical: 16),
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(24, 8, 24, 4),
          child: Text(
            DictateStrings.pickComputerTitle,
            style: Theme.of(context).textTheme.titleLarge,
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(24, 0, 24, 8),
          child: Text(DictateStrings.pickComputerBody, style: _muted(context)),
        ),
        for (final computer in computers)
          _ComputerTile(
            computer,
            selected: false,
            onTap: () => onPick(computer.id),
          ),
      ],
    );
  }
}

class _NoComputer extends StatelessWidget {
  const _NoComputer({required this.email});

  final String email;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              Icons.computer_outlined,
              size: 48,
              color: theme.colorScheme.onSurfaceVariant,
            ),
            const SizedBox(height: 16),
            Text(
              DictateStrings.noComputerTitle,
              textAlign: TextAlign.center,
              style: theme.textTheme.titleLarge,
            ),
            const SizedBox(height: 8),
            Text(
              DictateStrings.noComputerBody(email),
              textAlign: TextAlign.center,
              style: _muted(context),
            ),
          ],
        ),
      ),
    );
  }
}

class _Note extends StatelessWidget {
  const _Note(this.text);

  final String text;

  @override
  Widget build(BuildContext context) {
    return Card.filled(
      child: ListTile(
        leading: const Icon(Icons.info_outline),
        title: Text(text),
      ),
    );
  }
}

class _Hint extends StatelessWidget {
  const _Hint(this.text);

  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 32),
      child: Text(text, textAlign: TextAlign.center, style: _muted(context)),
    );
  }
}

class _ProgressCard extends StatelessWidget {
  const _ProgressCard({
    required this.text,
    required this.detail,
    required this.onCancel,
  });

  final String text;
  final String? detail;
  final VoidCallback? onCancel;

  @override
  Widget build(BuildContext context) {
    return Card.outlined(
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const LinearProgressIndicator(),
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 8, 8),
            child: Row(
              children: [
                Expanded(
                  child: Semantics(
                    liveRegion: true,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(text),
                        if (detail case final detail?)
                          Text(detail, style: _muted(context)),
                      ],
                    ),
                  ),
                ),
                TextButton(
                  onPressed: onCancel,
                  child: const Text(DictateStrings.cancel),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _OutcomeCard extends StatelessWidget {
  const _OutcomeCard({
    required this.outcome,
    required this.computerName,
    required this.onCopy,
    required this.onRetry,
    required this.onChooseWindow,
    required this.onPressEnter,
    required this.onDismiss,
  });

  final DictateOutcome outcome;
  final String computerName;
  final ValueChanged<String> onCopy;
  final VoidCallback? onRetry;

  /// Offered when the request's window is gone.
  final VoidCallback? onChooseWindow;

  /// Offered for a few seconds after a paste.
  final VoidCallback? onPressEnter;
  final VoidCallback onDismiss;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final error = outcome.error;
    final text = outcome.text;

    if (error != null) {
      return Card.filled(
        color: theme.colorScheme.errorContainer,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 16, 8, 8),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Semantics(
                liveRegion: true,
                child: Text(
                  DictateStrings.error(error, computerName),
                  style: theme.textTheme.bodyLarge?.copyWith(
                    color: theme.colorScheme.onErrorContainer,
                  ),
                ),
              ),
              OverflowBar(
                alignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                    onPressed: onDismiss,
                    child: const Text(DictateStrings.dismiss),
                  ),
                  if (onChooseWindow != null)
                    TextButton(
                      onPressed: onChooseWindow,
                      child: const Text(DictateStrings.chooseWindow),
                    ),
                  if (onRetry != null)
                    TextButton(
                      onPressed: onRetry,
                      child: const Text(DictateStrings.tryAgain),
                    ),
                ],
              ),
            ],
          ),
        ),
      );
    }

    final app = switch (outcome.window) {
      final window? => windowLabel(window).$1,
      null => null,
    };
    final title = switch ((outcome.typed, outcome.pasted, app)) {
      (true, _, final app?) => DictateStrings.typedPastedIn(app, computerName),
      (true, _, null) => DictateStrings.typedPasted(computerName),
      (false, true, final app?) => DictateStrings.pastedIn(app, computerName),
      (false, true, null) => DictateStrings.pasted(computerName),
      (false, false, _) => DictateStrings.result,
    };
    return Card.outlined(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 12, 8, 8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Icon(
                  Icons.check_circle_outline,
                  size: 20,
                  color: theme.colorScheme.primary,
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Semantics(
                    liveRegion: true,
                    child: Text(title, style: theme.textTheme.titleSmall),
                  ),
                ),
                IconButton(
                  tooltip: DictateStrings.dismiss,
                  icon: const Icon(Icons.close),
                  onPressed: onDismiss,
                ),
              ],
            ),
            if (!outcome.typed && text != null && text.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(right: 8),
                child: SelectableText(text, style: theme.textTheme.bodyLarge),
              ),
            if (onPressEnter != null ||
                (!outcome.typed && text != null && text.isNotEmpty))
              OverflowBar(
                alignment: MainAxisAlignment.end,
                spacing: 8,
                children: [
                  if (onPressEnter case final onPressEnter?)
                    ActionChip(
                      avatar: const Icon(Icons.keyboard_return, size: 18),
                      label: const Text(DictateStrings.pressEnter),
                      onPressed: onPressEnter,
                    ),
                  if (!outcome.typed && text != null && text.isNotEmpty)
                    TextButton.icon(
                      onPressed: () => onCopy(text),
                      icon: const Icon(Icons.copy, size: 18),
                      label: const Text(DictateStrings.copy),
                    ),
                ],
              ),
          ],
        ),
      ),
    );
  }
}

class _RecentTile extends StatelessWidget {
  const _RecentTile(this.item, {required this.onCopy});

  final RecentDictation item;
  final ValueChanged<String> onCopy;

  @override
  Widget build(BuildContext context) {
    final localizations = MaterialLocalizations.of(context);
    final now = DateTime.now();
    final today =
        item.at.year == now.year &&
        item.at.month == now.month &&
        item.at.day == now.day;
    return ListTile(
      title: Text(item.text, maxLines: 3, overflow: TextOverflow.ellipsis),
      subtitle: Text(
        today
            ? localizations.formatTimeOfDay(TimeOfDay.fromDateTime(item.at))
            : localizations.formatShortMonthDay(item.at),
      ),
      trailing: IconButton(
        tooltip: DictateStrings.copy,
        icon: const Icon(Icons.copy),
        onPressed: () => onCopy(item.text),
      ),
    );
  }
}

class _MicButton extends StatelessWidget {
  const _MicButton({
    required this.phase,
    required this.level,
    required this.onPressed,
  });

  static const _size = 88.0;
  static const _ring = 128.0;

  final DictatePhase phase;
  final ValueListenable<double> level;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final recording = phase == DictatePhase.recording;
    return SizedBox.square(
      dimension: _ring,
      child: Stack(
        alignment: Alignment.center,
        children: [
          if (recording)
            ValueListenableBuilder(
              valueListenable: level,
              builder: (context, value, _) => AnimatedScale(
                scale: _size / _ring + (1 - _size / _ring) * value,
                duration: const Duration(milliseconds: 100),
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    color: scheme.error.withValues(alpha: 0.2),
                  ),
                  child: const SizedBox.square(dimension: _ring),
                ),
              ),
            ),
          Semantics(
            label: recording
                ? DictateStrings.stopAndSend
                : DictateStrings.startRecording,
            // Excluding the button's own semantics drops its tap too.
            excludeSemantics: true,
            button: true,
            enabled: onPressed != null,
            onTap: onPressed,
            child: SizedBox.square(
              dimension: _size,
              child: FilledButton(
                onPressed: onPressed,
                style: FilledButton.styleFrom(
                  shape: const CircleBorder(),
                  padding: EdgeInsets.zero,
                  backgroundColor: recording ? scheme.error : null,
                  foregroundColor: recording ? scheme.onError : null,
                ),
                child: phase == DictatePhase.sending
                    ? const SizedBox.square(
                        dimension: 28,
                        child: CircularProgressIndicator(strokeWidth: 3),
                      )
                    : Icon(
                        recording ? Icons.stop_rounded : Icons.mic,
                        size: 40,
                      ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _RecordingCaption extends StatelessWidget {
  const _RecordingCaption({
    required this.elapsed,
    required this.paused,
    required this.onDiscard,
  });

  final ValueListenable<Duration> elapsed;
  final bool paused;
  final VoidCallback onDiscard;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        Flexible(
          child: ValueListenableBuilder(
            valueListenable: elapsed,
            builder: (context, value, _) {
              final seconds = (value.inSeconds % 60).toString().padLeft(2, '0');
              final time = paused
                  ? DictateStrings.paused
                  : '${value.inMinutes}:$seconds';
              return Text(
                '$time · ${DictateStrings.tapToSend}',
                style: _muted(
                  context,
                )?.copyWith(fontFeatures: const [FontFeature.tabularFigures()]),
              );
            },
          ),
        ),
        const SizedBox(width: 8),
        TextButton(
          onPressed: onDiscard,
          child: const Text(DictateStrings.discard),
        ),
      ],
    );
  }
}

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

class _Composer extends StatelessWidget {
  const _Composer({
    required this.controller,
    required this.focusNode,
    required this.hint,
    required this.enabled,
    required this.onSend,
  });

  final TextEditingController controller;
  final FocusNode focusNode;
  final String hint;
  final bool enabled;
  final VoidCallback onSend;

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.end,
      children: [
        Expanded(
          child: TextField(
            controller: controller,
            focusNode: focusNode,
            enabled: enabled,
            minLines: 1,
            maxLines: 4,
            inputFormatters: [typedTextLimit],
            textCapitalization: TextCapitalization.sentences,
            onTapOutside: (_) => focusNode.unfocus(),
            decoration: InputDecoration(hintText: hint),
          ),
        ),
        const SizedBox(width: 8),
        ValueListenableBuilder(
          valueListenable: controller,
          builder: (context, value, _) => IconButton.filled(
            tooltip: DictateStrings.send,
            iconSize: 24,
            constraints: const BoxConstraints.tightFor(width: 56, height: 56),
            onPressed: enabled && value.text.trim().isNotEmpty ? onSend : null,
            icon: const Icon(Icons.send),
          ),
        ),
      ],
    );
  }
}

/// A window as two lines: the app, and its title (null when the app is all
/// there is to say).
(String, String?) windowLabel(RemoteWindow window) => switch (window) {
  RemoteWindow(app: '', title: '') => (DictateStrings.untitled, null),
  RemoteWindow(app: '', :final title) => (title, null),
  RemoteWindow(:final app, title: '') => (app, null),
  RemoteWindow(:final app, :final title) => (app, title),
};

/// Where input goes: the chosen window, or wherever the cursor is.
class _WindowRow extends StatelessWidget {
  const _WindowRow({
    required this.window,
    required this.computerName,
    required this.onTap,
  });

  final RemoteWindow? window;
  final String computerName;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final (title, detail) = switch (window) {
      final window? => windowLabel(window),
      null => (
        DictateStrings.cursorWindow,
        DictateStrings.cursorWindowDetail(computerName),
      ),
    };
    return Semantics(
      button: true,
      enabled: onTap != null,
      label: DictateStrings.chooseWindow,
      value: detail == null ? title : '$title, $detail',
      // Excluding the row's own semantics drops its tap too.
      excludeSemantics: true,
      onTap: onTap,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(12),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
          child: Row(
            children: [
              Icon(
                window == null ? Icons.ads_click : Icons.web_asset,
                color: theme.colorScheme.onSurfaceVariant,
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: theme.textTheme.titleSmall,
                    ),
                    if (detail != null)
                      Text(
                        detail,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: theme.colorScheme.onSurfaceVariant,
                        ),
                      ),
                  ],
                ),
              ),
              if (onTap != null) const Icon(Icons.arrow_drop_down),
            ],
          ),
        ),
      ),
    );
  }
}

/// One key the key row presses, and how it is shown.
typedef _Key = ({String combo, String label, IconData? icon, String? text});

/// Keys to press in the chosen window: the common ones as buttons, the rest
/// behind "More keys". Scrolls sideways when it does not fit.
class KeyRow extends StatelessWidget {
  const KeyRow({super.key, required this.platform, required this.onPress});

  /// The computer's, for Ctrl or Cmd.
  final String platform;
  final ValueChanged<List<String>>? onPress;

  static const List<_Key> _keys = [
    (
      combo: 'enter',
      label: DictateStrings.keyEnter,
      icon: Icons.keyboard_return,
      text: null,
    ),
    (
      combo: 'tab',
      label: DictateStrings.keyTab,
      icon: Icons.keyboard_tab,
      text: null,
    ),
    (
      combo: 'escape',
      label: DictateStrings.keyEscapeLabel,
      icon: null,
      text: DictateStrings.keyEscape,
    ),
    (
      combo: 'left',
      label: DictateStrings.keyLeft,
      icon: Icons.keyboard_arrow_left,
      text: null,
    ),
    (
      combo: 'up',
      label: DictateStrings.keyUp,
      icon: Icons.keyboard_arrow_up,
      text: null,
    ),
    (
      combo: 'down',
      label: DictateStrings.keyDown,
      icon: Icons.keyboard_arrow_down,
      text: null,
    ),
    (
      combo: 'right',
      label: DictateStrings.keyRight,
      icon: Icons.keyboard_arrow_right,
      text: null,
    ),
    (
      combo: 'backspace',
      label: DictateStrings.keyBackspace,
      icon: Icons.backspace_outlined,
      text: null,
    ),
  ];

  List<(String, String)> _more() {
    final mod = DictateStrings.modifier(platform);
    return [
      ('shift+enter', DictateStrings.keyShiftEnter),
      (commandCombo(platform, 'c'), DictateStrings.keyCopy(mod)),
      (commandCombo(platform, 'v'), DictateStrings.keyPaste(mod)),
      (commandCombo(platform, 'z'), DictateStrings.keyUndo(mod)),
      (commandCombo(platform, 'a'), DictateStrings.keySelectAll(mod)),
      ('space', DictateStrings.keySpace),
    ];
  }

  @override
  Widget build(BuildContext context) {
    final onPress = this.onPress;
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: Row(
        children: [
          for (final key in _keys)
            Padding(
              padding: const EdgeInsets.only(right: 4),
              child: key.icon == null
                  ? Semantics(
                      label: key.label,
                      button: true,
                      excludeSemantics: true,
                      enabled: onPress != null,
                      onTap: onPress == null
                          ? null
                          : () => onPress([key.combo]),
                      child: OutlinedButton(
                        onPressed: onPress == null
                            ? null
                            : () => onPress([key.combo]),
                        style: OutlinedButton.styleFrom(
                          minimumSize: const Size(48, 40),
                          padding: const EdgeInsets.symmetric(horizontal: 12),
                        ),
                        child: Text(key.text!),
                      ),
                    )
                  : IconButton.outlined(
                      tooltip: key.label,
                      onPressed: onPress == null
                          ? null
                          : () => onPress([key.combo]),
                      icon: Icon(key.icon),
                    ),
            ),
          PopupMenuButton<String>(
            tooltip: DictateStrings.moreKeys,
            enabled: onPress != null,
            icon: const Icon(Icons.more_horiz),
            onSelected: (combo) => onPress?.call([combo]),
            itemBuilder: (context) => [
              for (final (combo, label) in _more())
                PopupMenuItem(value: combo, child: Text(label)),
            ],
          ),
        ],
      ),
    );
  }
}

/// The computer's windows, to aim input at one of them.
class _WindowPicker extends StatefulWidget {
  const _WindowPicker(this.controller, {required this.onWatch});

  final DictateController controller;

  /// Opens live view of a window or screen; null when the computer has no
  /// live view, which hides every Watch action and the screens.
  final void Function(WatchTarget target, RemoteWindow? window)? onWatch;

  @override
  State<_WindowPicker> createState() => _WindowPickerState();
}

class _WindowPickerState extends State<_WindowPicker> {
  List<RemoteWindow>? _windows;
  List<RemoteScreen> _screens = const [];
  String? _error;
  var _loading = false;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    if (_loading) return;
    setState(() => _loading = true);
    final listed = await widget.controller.listWindows();
    if (!mounted) return;
    setState(() {
      _loading = false;
      _error = listed.error;
      if (listed.error == null) {
        _windows = listed.windows;
        _screens = listed.screens;
      }
    });
  }

  void _choose(RemoteWindow? window) {
    unawaited(HapticFeedback.selectionClick());
    widget.controller.chooseWindow(window);
    Navigator.pop(context);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final c = widget.controller;
    final name = c.computer?.name ?? '';
    final windows = _windows;
    final error = _error;
    final onWatch = widget.onWatch;
    return ListenableBuilder(
      listenable: c,
      builder: (context, _) => RefreshIndicator(
        onRefresh: _load,
        child: ListView(
          physics: const AlwaysScrollableScrollPhysics(),
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(24, 0, 12, 0),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      DictateStrings.pickWindowTitle,
                      style: theme.textTheme.titleMedium,
                    ),
                  ),
                  if (_loading)
                    const Padding(
                      padding: EdgeInsets.all(14),
                      child: ButtonSpinner(),
                    )
                  else
                    IconButton(
                      tooltip: DictateStrings.refresh,
                      onPressed: _load,
                      icon: const Icon(Icons.refresh),
                    ),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(24, 0, 24, 8),
              child: Text(
                DictateStrings.pickWindowBody(name),
                style: _muted(context),
              ),
            ),
            ListTile(
              contentPadding: const EdgeInsets.symmetric(horizontal: 24),
              leading: const Icon(Icons.ads_click),
              title: const Text(DictateStrings.cursorWindow),
              subtitle: Text(DictateStrings.cursorWindowDetail(name)),
              trailing: c.window == null ? const Icon(Icons.check) : null,
              onTap: () => _choose(null),
            ),
            const Divider(),
            if (error != null)
              Padding(
                padding: const EdgeInsets.fromLTRB(24, 8, 24, 0),
                child: Semantics(
                  liveRegion: true,
                  child: Text(
                    DictateStrings.error(error, name),
                    style: theme.textTheme.bodyMedium?.copyWith(
                      color: theme.colorScheme.error,
                    ),
                  ),
                ),
              )
            else if (windows == null)
              Padding(
                padding: const EdgeInsets.all(24),
                child: Center(
                  child: Semantics(
                    label: DictateStrings.loadingWindows,
                    child: const CircularProgressIndicator(),
                  ),
                ),
              )
            else if (windows.isEmpty)
              Padding(
                padding: const EdgeInsets.fromLTRB(24, 8, 24, 0),
                child: Text(
                  DictateStrings.noWindows(name),
                  style: _muted(context),
                ),
              ),
            if (onWatch != null && _screens.isNotEmpty && error == null) ...[
              const SectionTitle(ScreenStrings.screens),
              for (final screen in _screens)
                _ScreenTile(
                  screen,
                  onWatch: () => onWatch(
                    WatchTarget(
                      id: screen.id,
                      label: screenLabel(screen),
                      screen: true,
                    ),
                    null,
                  ),
                ),
              if (windows != null && windows.isNotEmpty)
                const SectionTitle(ScreenStrings.windows),
            ],
            for (final window in windows ?? const <RemoteWindow>[])
              _WindowTile(
                window,
                selected: c.window?.id == window.id,
                onTap: () => _choose(window),
                onWatch: onWatch == null
                    ? null
                    : () => onWatch(
                        WatchTarget(
                          id: window.id,
                          label: windowLabel(window).$1,
                          screen: false,
                        ),
                        window,
                      ),
              ),
          ],
        ),
      ),
    );
  }
}

class _WindowTile extends StatelessWidget {
  const _WindowTile(
    this.window, {
    required this.selected,
    required this.onTap,
    required this.onWatch,
  });

  final RemoteWindow window;
  final bool selected;
  final VoidCallback onTap;

  /// Null without live view.
  final VoidCallback? onWatch;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final (title, detail) = windowLabel(window);
    return ListTile(
      contentPadding: const EdgeInsets.symmetric(horizontal: 24),
      leading: const Icon(Icons.web_asset),
      title: Text(
        title,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: const TextStyle(fontWeight: FontWeight.bold),
      ),
      subtitle: detail == null
          ? null
          : Text(detail, maxLines: 2, overflow: TextOverflow.ellipsis),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (window.front)
            DecoratedBox(
              decoration: BoxDecoration(
                color: theme.colorScheme.secondaryContainer,
                borderRadius: BorderRadius.circular(8),
              ),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                child: Text(
                  DictateStrings.inFront,
                  style: theme.textTheme.labelSmall?.copyWith(
                    color: theme.colorScheme.onSecondaryContainer,
                  ),
                ),
              ),
            ),
          if (selected) ...[const SizedBox(width: 8), const Icon(Icons.check)],
          if (onWatch case final onWatch?)
            IconButton(
              tooltip: ScreenStrings.watchItem(title),
              onPressed: onWatch,
              icon: const Icon(Icons.visibility_outlined),
            ),
        ],
      ),
      onTap: onTap,
    );
  }
}

/// A whole screen of the computer. Input cannot be aimed at a screen, so
/// tapping it watches it.
class _ScreenTile extends StatelessWidget {
  const _ScreenTile(this.screen, {required this.onWatch});

  final RemoteScreen screen;
  final VoidCallback onWatch;

  @override
  Widget build(BuildContext context) {
    final name = screenLabel(screen);
    final size = ScreenStrings.screenSize(screen.width, screen.height);
    return ListTile(
      contentPadding: const EdgeInsets.symmetric(horizontal: 24),
      leading: const Icon(Icons.desktop_windows_outlined),
      title: Text(
        name,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: const TextStyle(fontWeight: FontWeight.bold),
      ),
      subtitle: Text(
        screen.primary ? '$size · ${ScreenStrings.primary}' : size,
      ),
      trailing: IconButton(
        tooltip: ScreenStrings.watchItem(name),
        onPressed: onWatch,
        icon: const Icon(Icons.visibility_outlined),
      ),
      onTap: onWatch,
    );
  }
}
