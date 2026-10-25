// The pieces of the Dictate page: where text goes, a request on its way,
// how it went, the recent dictations, the keys, and the window picker.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../core/app_services.dart';
import '../../core/relay_client.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
import '../screen/live_protocol.dart';
import '../screen/strings.dart';
import '../screen/watch_screen.dart' show screenLabel;
import 'computer_widgets.dart';
import 'dictate_controller.dart';
import 'recent_dictations.dart';
import 'remote_windows.dart';
import 'strings.dart';

/// Where text goes: the chosen window (or wherever the cursor is), and
/// whether it is pasted there or kept on the phone.
class TargetCard extends StatelessWidget {
  const TargetCard({
    super.key,
    required this.window,
    required this.computerName,
    required this.hasWindows,
    required this.deliver,
    required this.onChooseWindow,
    required this.onDeliver,
  });

  final RemoteWindow? window;
  final String computerName;
  final bool hasWindows;
  final Deliver deliver;
  final VoidCallback? onChooseWindow;
  final ValueChanged<Deliver>? onDeliver;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final (title, detail) = switch (window) {
      final window? => windowLabel(window),
      null => (
        DictateStrings.cursorWindow,
        DictateStrings.cursorWindowDetail(computerName),
      ),
    };
    final keep = deliver == Deliver.keep;
    return Surface(
      padding: const EdgeInsets.all(Space.xs),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (hasWindows && !keep)
            Semantics(
              button: true,
              enabled: onChooseWindow != null,
              label: DictateStrings.chooseWindow,
              value: detail == null ? title : '$title, $detail',
              // Excluding the row's own semantics drops its tap too.
              excludeSemantics: true,
              onTap: onChooseWindow,
              child: InkWell(
                onTap: onChooseWindow == null
                    ? null
                    : () {
                        Haptic.tick();
                        onChooseWindow!();
                      },
                borderRadius: BorderRadius.circular(Radii.md),
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(
                    Space.md,
                    Space.sm,
                    Space.sm,
                    Space.sm,
                  ),
                  child: Row(
                    children: [
                      IconTile(
                        window == null
                            ? Icons.ads_click_rounded
                            : Icons.web_asset_rounded,
                        tone: window == null ? Tone.neutral : Tone.accent,
                        size: 36,
                      ),
                      const SizedBox(width: Space.md),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            Text(
                              DictateStrings.goesTo,
                              style: context.text.labelSmall?.copyWith(
                                color: t.faint,
                              ),
                            ),
                            Text(
                              title,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: context.text.titleSmall,
                            ),
                            if (detail != null)
                              Text(
                                detail,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: context.text.bodySmall,
                              ),
                          ],
                        ),
                      ),
                      if (onChooseWindow != null)
                        Icon(Icons.unfold_more_rounded, color: t.muted),
                    ],
                  ),
                ),
              ),
            ),
          Padding(
            padding: const EdgeInsets.all(Space.xs),
            child: SegmentedPill<Deliver>(
              options: const [
                (
                  Deliver.paste,
                  DictateStrings.pasteOnComputer,
                  Icons.content_paste_go_rounded,
                ),
                (
                  Deliver.keep,
                  DictateStrings.keepOnPhone,
                  Icons.smartphone_rounded,
                ),
              ],
              selected: deliver,
              onChanged: onDeliver,
            ),
          ),
        ],
      ),
    );
  }
}

/// A dictation or typed text on its way: what the computer is doing, and
/// Cancel. While it asks its user to allow this phone, the approval card.
class SendingCard extends StatelessWidget {
  const SendingCard({
    super.key,
    required this.stage,
    required this.cancelling,
    required this.computerName,
    required this.onCancel,
  });

  final String? stage;
  final bool cancelling;
  final String computerName;
  final VoidCallback? onCancel;

  @override
  Widget build(BuildContext context) {
    if (stage == 'approval' && !cancelling) {
      return ApprovalCard(
        title: DictateStrings.approval(computerName),
        body: DictateStrings.approvalDetail,
        onCancel: onCancel,
        cancelLabel: DictateStrings.cancel,
      );
    }
    final text = cancelling
        ? DictateStrings.cancelling
        : stage == 'pasting'
        ? DictateStrings.pasting(computerName)
        : DictateStrings.transcribing(computerName);
    return ProgressCard(text: text, onCancel: onCancel);
  }
}

/// A request at work: a thin bar running along the top, what is happening,
/// and Cancel.
class ProgressCard extends StatelessWidget {
  const ProgressCard({
    super.key,
    required this.text,
    required this.onCancel,
    this.detail,
    this.cancelLabel = DictateStrings.cancel,
  });

  final String text;
  final String? detail;
  final VoidCallback? onCancel;
  final String cancelLabel;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Material(
      color: t.card,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.lg),
        side: BorderSide(color: t.hairline),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const LinearProgressIndicator(minHeight: 3),
          Padding(
            padding: const EdgeInsets.fromLTRB(
              Space.lg,
              Space.md,
              Space.sm,
              Space.md,
            ),
            child: Row(
              children: [
                const IconTile(
                  Icons.graphic_eq_rounded,
                  tone: Tone.accent,
                  size: 36,
                ),
                const SizedBox(width: Space.md),
                Expanded(
                  child: Semantics(
                    liveRegion: true,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(text, style: context.text.titleSmall),
                        if (detail case final detail?)
                          Text(detail, style: context.text.bodySmall),
                      ],
                    ),
                  ),
                ),
                TextButton(onPressed: onCancel, child: Text(cancelLabel)),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// How the last dictation or typed text went.
class OutcomeCard extends StatelessWidget {
  const OutcomeCard({
    super.key,
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
    final t = context.talvio;
    final error = outcome.error;
    final text = outcome.text;

    if (error != null) {
      return Notice(
        tone: error == 'cancelled' ? Tone.neutral : Tone.danger,
        message: DictateStrings.error(error, computerName),
        actions: [
          TextButton(
            onPressed: onDismiss,
            style: TextButton.styleFrom(foregroundColor: t.muted),
            child: const Text(DictateStrings.dismiss),
          ),
          if (onChooseWindow != null)
            TextButton(
              onPressed: onChooseWindow,
              child: const Text(DictateStrings.chooseWindow),
            ),
          if (onRetry != null)
            FilledButton(
              onPressed: () {
                Haptic.tap();
                onRetry!();
              },
              child: const Text(DictateStrings.tryAgain),
            ),
        ],
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
    final showText = !outcome.typed && text != null && text.isNotEmpty;
    return Surface(
      padding: const EdgeInsets.fromLTRB(
        Space.lg,
        Space.md,
        Space.xs,
        Space.sm,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              IconTile(
                outcome.pasted || outcome.typed
                    ? Icons.check_rounded
                    : Icons.notes_rounded,
                tone: outcome.pasted || outcome.typed
                    ? Tone.success
                    : Tone.accent,
                size: 28,
              ),
              const SizedBox(width: Space.sm),
              Expanded(
                child: Semantics(
                  liveRegion: true,
                  child: Text(title, style: context.text.titleSmall),
                ),
              ),
              IconButton(
                tooltip: DictateStrings.dismiss,
                icon: Icon(Icons.close_rounded, color: t.muted),
                onPressed: onDismiss,
              ),
            ],
          ),
          if (showText)
            Padding(
              padding: const EdgeInsets.fromLTRB(
                0,
                Space.xs,
                Space.md,
                Space.xs,
              ),
              child: SelectableText(text, style: context.text.bodyLarge),
            ),
          if (outcome.pasteOff)
            Padding(
              padding: const EdgeInsets.fromLTRB(
                0,
                Space.xs,
                Space.md,
                Space.xs,
              ),
              child: Text(
                DictateStrings.pasteOffNote(computerName),
                style: context.text.bodySmall?.copyWith(color: t.warning),
              ),
            ),
          if (onPressEnter != null || showText)
            OverflowBar(
              alignment: MainAxisAlignment.end,
              spacing: Space.sm,
              children: [
                if (onPressEnter case final onPressEnter?)
                  ActionChip(
                    avatar: Icon(
                      Icons.keyboard_return_rounded,
                      size: 18,
                      color: t.accent,
                    ),
                    label: const Text(DictateStrings.pressEnter),
                    onPressed: onPressEnter,
                  ),
                if (showText)
                  TextButton.icon(
                    onPressed: () => onCopy(text),
                    icon: const Icon(Icons.copy_rounded, size: 18),
                    label: const Text(DictateStrings.copy),
                  ),
              ],
            ),
        ],
      ),
    );
  }
}

/// The last dictations, on this phone, to copy or paste again.
class RecentDictationsList extends StatelessWidget {
  const RecentDictationsList({
    super.key,
    required this.items,
    required this.onCopy,
    required this.onOpen,
  });

  final List<RecentDictation> items;
  final ValueChanged<String> onCopy;
  final ValueChanged<RecentDictation> onOpen;

  @override
  Widget build(BuildContext context) {
    if (items.isEmpty) return const SizedBox.shrink();
    final t = context.talvio;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SectionHeader(
          DictateStrings.recent,
          trailing: Text(
            DictateStrings.historyNote,
            style: context.text.bodySmall,
          ),
        ),
        Surface(
          padding: EdgeInsets.zero,
          child: Column(
            children: [
              for (final (i, item) in items.indexed) ...[
                if (i > 0) Divider(indent: Space.lg, color: t.hairline),
                _RecentTile(
                  item,
                  onOpen: () => onOpen(item),
                  onCopy: () => onCopy(item.text),
                ),
              ],
            ],
          ),
        ),
      ],
    );
  }
}

class _RecentTile extends StatelessWidget {
  const _RecentTile(this.item, {required this.onOpen, required this.onCopy});

  final RecentDictation item;
  final VoidCallback onOpen;
  final VoidCallback onCopy;

  @override
  Widget build(BuildContext context) {
    final localizations = MaterialLocalizations.of(context);
    final now = DateTime.now();
    final today =
        item.at.year == now.year &&
        item.at.month == now.month &&
        item.at.day == now.day;
    return InkWell(
      onTap: onOpen,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(
          Space.lg,
          Space.md,
          Space.xs,
          Space.md,
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    item.text,
                    maxLines: 3,
                    overflow: TextOverflow.ellipsis,
                    style: context.text.bodyMedium,
                  ),
                  const SizedBox(height: Space.xxs),
                  Text(
                    today
                        ? localizations.formatTimeOfDay(
                            TimeOfDay.fromDateTime(item.at),
                          )
                        : localizations.formatShortMonthDay(item.at),
                    style: context.text.bodySmall?.copyWith(
                      color: context.talvio.faint,
                    ),
                  ),
                ],
              ),
            ),
            IconButton(
              tooltip: DictateStrings.copy,
              icon: Icon(
                Icons.copy_rounded,
                size: 20,
                color: context.talvio.muted,
              ),
              onPressed: onCopy,
            ),
          ],
        ),
      ),
    );
  }
}

/// The whole dictation, to select from, copy, or paste on the computer.
Future<void> openDictation(
  BuildContext context,
  DictateController controller,
  RecentDictation item,
) {
  final relay = AppServices.of(context).relay;
  final messenger = ScaffoldMessenger.of(context);
  return showModalBottomSheet<void>(
    context: context,
    isScrollControlled: true,
    builder: (sheet) {
      final computer = controller.computer;
      final canPaste =
          computer != null &&
          computer.online &&
          relay.status.value == RelayStatus.connected &&
          computer.capabilities.contains(DictateKind.text) &&
          controller.phase == DictatePhase.idle;
      return SafeArea(
        child: ConstrainedBox(
          constraints: BoxConstraints(
            maxHeight: MediaQuery.sizeOf(sheet).height * 0.8,
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Flexible(
                child: SingleChildScrollView(
                  padding: const EdgeInsets.fromLTRB(
                    Space.xxl,
                    0,
                    Space.xxl,
                    Space.sm,
                  ),
                  child: SelectableText(item.text, style: sheet.text.bodyLarge),
                ),
              ),
              Padding(
                padding: const EdgeInsets.fromLTRB(
                  Space.lg,
                  Space.sm,
                  Space.lg,
                  Space.sm,
                ),
                child: OverflowBar(
                  alignment: MainAxisAlignment.end,
                  spacing: Space.sm,
                  children: [
                    TextButton.icon(
                      onPressed: () {
                        Navigator.pop(sheet);
                        unawaited(
                          Clipboard.setData(ClipboardData(text: item.text)),
                        );
                        messenger
                          ..hideCurrentSnackBar()
                          ..showSnackBar(
                            const SnackBar(
                              content: Text(DictateStrings.copied),
                            ),
                          );
                      },
                      icon: const Icon(Icons.copy_rounded, size: 18),
                      label: const Text(DictateStrings.copy),
                    ),
                    if (canPaste)
                      FilledButton.icon(
                        onPressed: () {
                          Navigator.pop(sheet);
                          Haptic.tap();
                          // The page shows how it went.
                          unawaited(controller.sendText(item.text));
                        },
                        icon: const Icon(Icons.send_rounded, size: 18),
                        label: Text(DictateStrings.sendAgain(computer.name)),
                      ),
                  ],
                ),
              ),
            ],
          ),
        ),
      );
    },
  );
}

/// Above the text field: open the keys, and Enter, which is wanted most.
class KeysRow extends StatelessWidget {
  const KeysRow({
    super.key,
    required this.open,
    required this.onToggle,
    required this.onEnter,
  });

  final bool open;
  final VoidCallback onToggle;
  final VoidCallback? onEnter;

  @override
  Widget build(BuildContext context) {
    return Wrap(
      alignment: WrapAlignment.spaceBetween,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        FilterChip(
          selected: open,
          showCheckmark: false,
          avatar: Icon(
            open ? Icons.keyboard_hide_rounded : Icons.keyboard_rounded,
            size: 18,
          ),
          label: Text(open ? DictateStrings.hideKeys : DictateStrings.showKeys),
          onSelected: (_) {
            Haptic.tick();
            onToggle();
          },
        ),
        Tooltip(
          message: DictateStrings.keyEnter,
          child: ActionChip(
            avatar: const Icon(Icons.keyboard_return_rounded, size: 18),
            label: const Text(DictateStrings.keyEnter),
            onPressed: onEnter,
          ),
        ),
      ],
    );
  }
}

/// The computer's windows, to aim input at one of them.
class WindowPicker extends StatefulWidget {
  const WindowPicker(this.controller, {super.key, required this.onWatch});

  final DictateController controller;

  /// Opens live view of a window or screen; null when the computer has no
  /// live view, which hides every Watch action and the screens.
  final void Function(WatchTarget target, RemoteWindow? window)? onWatch;

  @override
  State<WindowPicker> createState() => _WindowPickerState();
}

class _WindowPickerState extends State<WindowPicker> {
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
    Haptic.tick();
    widget.controller.chooseWindow(window);
    Navigator.pop(context);
  }

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
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
              padding: const EdgeInsets.fromLTRB(Space.page, 0, Space.md, 0),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      DictateStrings.pickWindowTitle,
                      style: context.text.titleLarge,
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
                      icon: const Icon(Icons.refresh_rounded),
                    ),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(
                Space.page,
                0,
                Space.page,
                Space.sm,
              ),
              child: Text(
                DictateStrings.pickWindowBody(name),
                style: mutedText(context),
              ),
            ),
            ListTile(
              leading: const IconTile(Icons.ads_click_rounded, size: 40),
              title: const Text(DictateStrings.cursorWindow),
              subtitle: Text(DictateStrings.cursorWindowDetail(name)),
              trailing: c.window == null
                  ? Icon(Icons.check_circle_rounded, color: t.accent)
                  : null,
              onTap: () => _choose(null),
            ),
            const Divider(),
            if (error != null)
              Padding(
                padding: const EdgeInsets.fromLTRB(
                  Space.page,
                  Space.sm,
                  Space.page,
                  0,
                ),
                child: Notice(
                  tone: Tone.danger,
                  message: DictateStrings.error(error, name),
                ),
              )
            else if (windows == null)
              Padding(
                padding: const EdgeInsets.all(Space.xxl),
                child: Center(
                  child: Semantics(
                    label: DictateStrings.loadingWindows,
                    child: const CircularProgressIndicator(),
                  ),
                ),
              )
            else if (windows.isEmpty)
              Padding(
                padding: const EdgeInsets.fromLTRB(
                  Space.page,
                  Space.sm,
                  Space.page,
                  0,
                ),
                child: Text(
                  DictateStrings.noWindows(name),
                  style: mutedText(context),
                ),
              ),
            if (onWatch != null && _screens.isNotEmpty && error == null) ...[
              const SectionHeader(
                ScreenStrings.screens,
                padding: EdgeInsets.fromLTRB(
                  Space.page,
                  Space.lg,
                  Space.page,
                  Space.xs,
                ),
              ),
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
                const SectionHeader(
                  ScreenStrings.windows,
                  padding: EdgeInsets.fromLTRB(
                    Space.page,
                    Space.lg,
                    Space.page,
                    Space.xs,
                  ),
                ),
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
            const SizedBox(height: Space.lg),
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
    final t = context.talvio;
    final (title, detail) = windowLabel(window);
    return ListTile(
      leading: IconTile(
        Icons.web_asset_rounded,
        tone: selected ? Tone.accent : Tone.neutral,
        size: 40,
      ),
      title: Text(
        title,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: context.text.titleSmall,
      ),
      subtitle: detail == null
          ? null
          : Text(detail, maxLines: 2, overflow: TextOverflow.ellipsis),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (window.front) const Pill(DictateStrings.inFront),
          if (selected) ...[
            const SizedBox(width: Space.sm),
            Icon(Icons.check_circle_rounded, color: t.accent),
          ],
          if (onWatch case final onWatch?)
            IconButton(
              tooltip: ScreenStrings.watchItem(title),
              onPressed: onWatch,
              icon: const Icon(Icons.cast_rounded),
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
      leading: const IconTile(Icons.monitor_rounded, size: 40),
      title: Text(
        name,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: context.text.titleSmall,
      ),
      subtitle: Text(
        screen.primary ? '$size · ${ScreenStrings.primary}' : size,
      ),
      trailing: IconButton(
        tooltip: ScreenStrings.watchItem(name),
        onPressed: onWatch,
        icon: const Icon(Icons.cast_rounded),
      ),
      onTap: onWatch,
    );
  }
}
