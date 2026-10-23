import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../core/widgets.dart';
import '../../shell/feature_tab.dart';
import '../../shell/shell_scope.dart';
import '../screen/live_protocol.dart';
import '../screen/strings.dart';
import '../screen/watch_screen.dart';
import 'computer_widgets.dart';
import 'dictate_controller.dart';
import 'dictate_scope.dart';
import 'key_pad.dart';
import 'mic_widgets.dart';
import 'remote_windows.dart';
import 'strings.dart';

export 'computer_widgets.dart' show windowLabel;

/// Dictating and typing into the computer, and pressing its keys.
final sendTab = FeatureTab(
  id: sendTabId,
  label: DictateStrings.tab,
  icon: Icons.send_outlined,
  selectedIcon: Icons.send,
  requires: DictateKind.dictate,
  builder: (_) => const DictateScreen(),
);

const sendTabId = 'send';

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

  @override
  void dispose() {
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

  void _watch(WatchTarget target, {RemoteWindow? window}) =>
      unawaited(openWatch(context, _controller, target, window: window));

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
        final computer = _controller.computer;
        return Scaffold(
          appBar: AppBar(
            leading: shellLeading(context),
            title: computer == null
                ? const Text(DictateStrings.title)
                : ComputerTitle(
                    computer,
                    // A recording or request keeps its computer to the end.
                    onTap: canChooseComputer(_controller)
                        ? () => chooseComputer(context, _controller)
                        : null,
                  ),
          ),
          body: ComputerGate(controller: _controller, builder: _buildBody),
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
                if (note == null && outcome == null && idle)
                  _Hint(
                    c.deliver == Deliver.keep
                        ? DictateStrings.hintKeep
                        : DictateStrings.hintPaste(computer.name),
                  ),
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
                        MicButton(
                          recording: c.phase == DictatePhase.recording,
                          busy: c.phase == DictatePhase.sending,
                          level: c.level,
                          startLabel: DictateStrings.startRecording,
                          stopLabel: DictateStrings.stopAndSend,
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
                              ? RecordingCaption(
                                  elapsed: c.elapsed,
                                  paused: c.paused,
                                  onDiscard: c.discardRecording,
                                )
                              : Center(
                                  child: Text(
                                    idle ? DictateStrings.tapToSpeak : '',
                                    style: mutedText(context),
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
                        KeyPad(
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
      child: Text(text, textAlign: TextAlign.center, style: mutedText(context)),
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
                          Text(detail, style: mutedText(context)),
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
                style: mutedText(context),
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
                  style: mutedText(context),
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
