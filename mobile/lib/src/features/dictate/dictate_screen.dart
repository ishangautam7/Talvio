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
import 'dictate_controller.dart';
import 'recent_dictations.dart';
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
    required this.onDismiss,
  });

  final DictateOutcome outcome;
  final String computerName;
  final ValueChanged<String> onCopy;
  final VoidCallback? onRetry;
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

    final title = outcome.typed
        ? DictateStrings.typedPasted(computerName)
        : outcome.pasted
        ? DictateStrings.pasted(computerName)
        : DictateStrings.result;
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
            if (!outcome.typed && text != null && text.isNotEmpty) ...[
              Padding(
                padding: const EdgeInsets.only(right: 8),
                child: SelectableText(text, style: theme.textTheme.bodyLarge),
              ),
              Align(
                alignment: Alignment.centerRight,
                child: TextButton.icon(
                  onPressed: () => onCopy(text),
                  icon: const Icon(Icons.copy, size: 18),
                  label: const Text(DictateStrings.copy),
                ),
              ),
            ],
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
