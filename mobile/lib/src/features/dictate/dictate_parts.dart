// The pieces of the Dictate page (design spec §4.5): where text goes and
// the sheet that changes it, the recent dictations, and what the stage
// above the mic says.

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

/// Where text goes, as one tappable row: the window (or wherever the
/// cursor is), or this phone.
class DestinationRow extends StatelessWidget {
  const DestinationRow({
    super.key,
    required this.window,
    required this.computerName,
    required this.deliver,
    required this.onTap,
    this.hasWindows = true,
  });

  final RemoteWindow? window;
  final String computerName;
  final Deliver deliver;

  /// The computer lets this phone choose a window.
  final bool hasWindows;

  /// Null while a recording or request holds the destination.
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final (IconData icon, String title, String subtitle) = switch ((
      deliver,
      window,
    )) {
      (Deliver.keep, _) => (
        AppIcons.phone,
        DictateStrings.keepOnPhoneTitle,
        DictateStrings.textComesBack,
      ),
      (_, final window?) => (
        AppIcons.window,
        switch (windowLabel(window)) {
          (final app, final title?) => '$app — $title',
          (final app, null) => app,
        },
        computerName,
      ),
      (_, null) => (
        AppIcons.cursor,
        DictateStrings.cursorWindow,
        DictateStrings.pastesOn(computerName),
      ),
    };
    final t = context.talvio;
    return Padding(
      padding: const EdgeInsets.only(top: Space.s8),
      child: Group(
        children: [
          ListRow(
            leading: RowGlyph(icon),
            title: title,
            titleMaxLines: 1,
            subtitle: subtitle,
            subtitleMaxLines: 1,
            trailing: Icon(AppIcons.caretUpDown, size: 16, color: t.muted),
            enabled: onTap != null,
            semanticLabel: hasWindows
                ? DictateStrings.chooseWindow
                : DictateStrings.goesTo,
            semanticValue: '$title, $subtitle',
            // Always a button; disabled while a recording holds it.
            onTap: () {
              Haptic.tick();
              onTap?.call();
            },
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
    final localizations = MaterialLocalizations.of(context);
    final now = DateTime.now();
    String when(DateTime at) =>
        at.year == now.year && at.month == now.month && at.day == now.day
        ? localizations.formatTimeOfDay(TimeOfDay.fromDateTime(at))
        : localizations.formatShortMonthDay(at);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SectionLabel(DictateStrings.recent),
        Group(
          dividerInset: 16,
          children: [
            for (final item in items)
              ListRow(
                title: item.text,
                titleStyle: TalvioType.rowBody.copyWith(color: t.ink),
                subtitle: when(item.at),
                subtitleStyle: TalvioType.footnote.copyWith(color: t.faint),
                trailing: IconBtn(
                  icon: AppIcons.copy,
                  tooltip: DictateStrings.copy,
                  color: t.muted,
                  onPressed: () => onCopy(item.text),
                ),
                onTap: () => onOpen(item),
              ),
          ],
        ),
        const GroupFooter(DictateStrings.historyNote),
      ],
    );
  }
}

/// The whole dictation, to select from, copy, or paste on the computer.
Future<void> openDictation(
  BuildContext context,
  DictateController controller,
  RecentDictation item,
) => openText(context, controller, item.text);

/// [text] in a sheet, to select from, copy, or paste on the computer.
Future<void> openText(
  BuildContext context,
  DictateController controller,
  String text,
) {
  final relay = AppServices.of(context).relay;
  final messenger = ScaffoldMessenger.of(context);
  return showTalvioSheet<void>(
    context,
    title: DictateStrings.result,
    builder: (sheet) {
      final computer = controller.computer;
      final canPaste =
          computer != null &&
          computer.online &&
          relay.status.value == RelayStatus.connected &&
          computer.capabilities.contains(DictateKind.text) &&
          controller.phase == DictatePhase.idle;
      return Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Flexible(
            child: SingleChildScrollView(
              padding: const EdgeInsets.fromLTRB(16, 4, 16, 16),
              child: SelectableText(
                text,
                style: TalvioType.body.copyWith(color: sheet.talvio.ink),
              ),
            ),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
            child: Row(
              children: [
                Expanded(
                  child: Btn(
                    DictateStrings.copy,
                    kind: BtnKind.secondary,
                    compact: true,
                    icon: AppIcons.copy,
                    onPressed: () {
                      Navigator.pop(sheet);
                      unawaited(Clipboard.setData(ClipboardData(text: text)));
                      messenger
                        ..hideCurrentSnackBar()
                        ..showSnackBar(
                          const SnackBar(content: Text(DictateStrings.copied)),
                        );
                    },
                  ),
                ),
                if (canPaste) ...[
                  const SizedBox(width: Space.s8),
                  Expanded(
                    child: Btn(
                      DictateStrings.sendAgain(computer.name),
                      compact: true,
                      onPressed: () {
                        Navigator.pop(sheet);
                        Haptic.tap();
                        // The page shows how it went.
                        unawaited(controller.sendText(text));
                      },
                    ),
                  ),
                ],
              ],
            ),
          ),
        ],
      );
    },
  );
}

/// Text that came back to the phone: the text, Copy and Paste on.
class ReturnedCard extends StatelessWidget {
  const ReturnedCard({
    super.key,
    required this.outcome,
    required this.computerName,
    required this.onOpen,
    required this.onCopy,
    required this.onPaste,
    required this.onDismiss,
  });

  final DictateOutcome outcome;
  final String computerName;
  final VoidCallback onOpen;
  final VoidCallback onCopy;

  /// Null when it cannot be pasted now.
  final VoidCallback? onPaste;
  final VoidCallback onDismiss;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Stack(
      children: [
        Container(
          margin: const EdgeInsets.symmetric(horizontal: Space.gutter),
          padding: const EdgeInsetsDirectional.fromSTEB(16, 16, 16, 12),
          decoration: BoxDecoration(
            color: t.card,
            borderRadius: BorderRadius.circular(Radii.lg),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsetsDirectional.only(end: 28),
                child: GestureDetector(
                  onTap: onOpen,
                  child: Semantics(
                    liveRegion: true,
                    child: Text(
                      outcome.text ?? '',
                      maxLines: 6,
                      overflow: TextOverflow.ellipsis,
                      style: TalvioType.body.copyWith(color: t.ink),
                    ),
                  ),
                ),
              ),
              if (outcome.pasteOff) ...[
                const SizedBox(height: Space.s8),
                Text(
                  DictateStrings.pasteOffNote(computerName),
                  style: TalvioType.footnote.copyWith(color: t.muted),
                ),
              ],
              const SizedBox(height: Space.s12),
              Wrap(
                spacing: Space.s8,
                runSpacing: Space.s8,
                children: [
                  Btn(
                    DictateStrings.copy,
                    kind: BtnKind.tonal,
                    compact: true,
                    icon: AppIcons.copy,
                    onPressed: onCopy,
                  ),
                  if (onPaste != null)
                    Btn(
                      DictateStrings.sendAgain(computerName),
                      kind: BtnKind.tonal,
                      compact: true,
                      onPressed: onPaste,
                    ),
                ],
              ),
            ],
          ),
        ),
        PositionedDirectional(
          top: 4,
          end: Space.gutter + 4,
          child: IconBtn(
            icon: AppIcons.close,
            tooltip: DictateStrings.dismiss,
            color: t.muted,
            onPressed: onDismiss,
          ),
        ),
      ],
    );
  }
}

/// The sheet that says where text goes: pasted on the computer (where the
/// cursor is, or a window), or kept on the phone; and the screens to watch.
class WindowPicker extends StatefulWidget {
  const WindowPicker(this.controller, {super.key, required this.onWatch});

  final DictateController controller;

  /// Opens live view of a screen; null when the computer has no live view,
  /// which hides the screens.
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
    if (widget.controller.hasWindows) unawaited(_load());
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
    final c = widget.controller;
    return ListenableBuilder(
      listenable: c,
      builder: (context, _) {
        final t = context.talvio;
        final name = c.computer?.name ?? '';
        final windows = _windows;
        final error = _error;
        final onWatch = widget.onWatch;
        final paste = c.deliver == Deliver.paste;
        final check = Icon(AppIcons.check, size: 20, color: t.accent);
        return ListView(
          shrinkWrap: true,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
              child: SegmentedPill<Deliver>(
                options: const [
                  (Deliver.paste, DictateStrings.pasteOnComputer, null),
                  (Deliver.keep, DictateStrings.keepOnPhone, null),
                ],
                selected: c.deliver,
                onChanged: c.phase == DictatePhase.idle
                    ? (value) => c.deliver = value
                    : null,
              ),
            ),
            if (paste && c.hasWindows) ...[
              SectionLabel(
                ScreenStrings.windows,
                inset: 16,
                trailing: _loading
                    ? const SizedBox.square(
                        dimension: 44,
                        child: Center(child: Spinner(size: 18)),
                      )
                    : IconBtn(
                        icon: AppIcons.refresh,
                        tooltip: DictateStrings.refresh,
                        color: t.muted,
                        onPressed: _load,
                      ),
              ),
              SheetRows(
                children: [
                  ListRow(
                    leading: const RowGlyph(AppIcons.cursor),
                    title: DictateStrings.cursorWindow,
                    subtitle: DictateStrings.cursorWindowDetail(name),
                    trailing: c.window == null ? check : null,
                    selected: c.window == null,
                    onTap: () => _choose(null),
                  ),
                  if (error == null && windows == null)
                    ListRow(
                      leading: const Spinner(size: 18),
                      title: DictateStrings.loadingWindows,
                      titleStyle: TalvioType.rowBody.copyWith(color: t.muted),
                    ),
                  for (final window in windows ?? const <RemoteWindow>[])
                    _WindowRow(
                      window,
                      selected: c.window?.id == window.id,
                      onTap: () => _choose(window),
                    ),
                ],
              ),
              if (error != null)
                Padding(
                  padding: const EdgeInsets.only(top: Space.s8),
                  child: InlineBanner(
                    tone: Tone.danger,
                    message: DictateStrings.error(error, name),
                  ),
                )
              else if (windows != null && windows.isEmpty)
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
                  child: Text(
                    DictateStrings.noWindows(name),
                    style: TalvioType.footnote.copyWith(color: t.muted),
                  ),
                ),
              if (onWatch != null && _screens.isNotEmpty && error == null) ...[
                const SectionLabel(ScreenStrings.screens, inset: 16),
                SheetRows(
                  children: [
                    for (final screen in _screens)
                      ListRow(
                        leading: const RowTile(AppIcons.display),
                        title: screenLabel(screen),
                        subtitle: screen.primary
                            ? '${ScreenStrings.primary} · '
                                  '${ScreenStrings.screenSize(screen.width, screen.height)}'
                            : ScreenStrings.screenSize(
                                screen.width,
                                screen.height,
                              ),
                        trailing: Icon(
                          AppIcons.liveView,
                          size: 20,
                          color: t.muted,
                        ),
                        semanticLabel: ScreenStrings.watchItem(
                          screenLabel(screen),
                        ),
                        onTap: () => onWatch(
                          WatchTarget(
                            id: screen.id,
                            label: screenLabel(screen),
                            screen: true,
                          ),
                          null,
                        ),
                      ),
                  ],
                ),
              ],
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 12, 16, 8),
                child: Text(
                  DictateStrings.pickWindowBody(name),
                  style: TalvioType.footnote.copyWith(color: t.muted),
                ),
              ),
            ],
            if (!paste || !c.hasWindows)
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 12, 16, 8),
                child: Text(
                  paste
                      ? DictateStrings.hintPaste(name)
                      : DictateStrings.hintKeep,
                  style: TalvioType.footnote.copyWith(color: t.muted),
                ),
              ),
          ],
        );
      },
    );
  }
}

class _WindowRow extends StatelessWidget {
  const _WindowRow(this.window, {required this.selected, required this.onTap});

  final RemoteWindow window;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final (title, detail) = windowLabel(window);
    return ListRow(
      leading: Monogram(title),
      title: title,
      titleMaxLines: 1,
      subtitle: detail,
      subtitleMaxLines: 1,
      selected: selected,
      trailing: window.front || selected
          ? Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (window.front) const StatusPill(DictateStrings.inFront),
                if (window.front && selected) const SizedBox(width: Space.s8),
                if (selected) Icon(AppIcons.check, size: 20, color: t.accent),
              ],
            )
          : null,
      onTap: onTap,
    );
  }
}
