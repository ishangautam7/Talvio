import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../core/app_services.dart';
import '../../core/relay_client.dart';
import '../../shell/feature_tab.dart';
import '../../shell/shell_scope.dart';
import 'computer_widgets.dart';
import 'dictate_controller.dart';
import 'dictate_scope.dart';
import 'dictate_screen.dart' show sendTabId;
import 'recent_dictations.dart';
import 'strings.dart';

/// The recent dictations, to read and copy again.
final historyTab = FeatureTab(
  id: 'history',
  label: DictateStrings.historyTab,
  icon: Icons.history,
  selectedIcon: Icons.history,
  builder: (_) => const HistoryScreen(),
);

class HistoryScreen extends StatelessWidget {
  const HistoryScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final controller = DictateScope.of(context);
    final shell = ShellScope.maybeOf(context);
    return Scaffold(
      appBar: AppBar(
        leading: shellLeading(context),
        title: const Text(DictateStrings.historyTitle),
      ),
      body: ListenableBuilder(
        listenable: controller,
        builder: (context, _) {
          final items = controller.recents.items;
          if (items.isEmpty) {
            return _Empty(
              onDictate: shell?.available.contains(sendTabId) ?? false
                  ? () => shell!.select(sendTabId)
                  : null,
            );
          }
          return ListView.separated(
            padding: const EdgeInsets.symmetric(vertical: 8),
            itemCount: items.length + 1,
            separatorBuilder: (_, i) => i == 0
                ? const SizedBox.shrink()
                : const Divider(indent: 16, endIndent: 16, height: 1),
            itemBuilder: (context, i) => i == 0
                ? Padding(
                    padding: const EdgeInsets.fromLTRB(16, 8, 16, 8),
                    child: Text(
                      DictateStrings.historyNote,
                      style: mutedText(context),
                    ),
                  )
                : _HistoryTile(
                    items[i - 1],
                    onOpen: () => _open(context, controller, items[i - 1]),
                  ),
          );
        },
      ),
    );
  }

  static void _copy(BuildContext context, String text) {
    unawaited(Clipboard.setData(ClipboardData(text: text)));
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(const SnackBar(content: Text(DictateStrings.copied)));
  }

  /// The whole dictation, to select from, copy, or paste on the computer.
  static Future<void> _open(
    BuildContext context,
    DictateController controller,
    RecentDictation item,
  ) {
    final relay = AppServices.of(context).relay;
    final shell = ShellScope.maybeOf(context);
    return showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      builder: (sheet) {
        final computer = controller.computer;
        final canPaste =
            computer != null &&
            computer.online &&
            relay.status.value == RelayStatus.connected &&
            computer.capabilities.contains(DictateKind.text) &&
            controller.phase == DictatePhase.idle &&
            (shell?.available.contains(sendTabId) ?? false);
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
                    padding: const EdgeInsets.fromLTRB(24, 0, 24, 8),
                    child: SelectableText(
                      item.text,
                      style: Theme.of(sheet).textTheme.bodyLarge,
                    ),
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 8, 16, 8),
                  child: OverflowBar(
                    alignment: MainAxisAlignment.end,
                    spacing: 8,
                    children: [
                      TextButton.icon(
                        onPressed: () {
                          Navigator.pop(sheet);
                          _copy(context, item.text);
                        },
                        icon: const Icon(Icons.copy, size: 18),
                        label: const Text(DictateStrings.copy),
                      ),
                      if (canPaste)
                        FilledButton.icon(
                          onPressed: () {
                            Navigator.pop(sheet);
                            // The Send page shows how it went.
                            shell!.select(sendTabId);
                            unawaited(controller.sendText(item.text));
                          },
                          icon: const Icon(Icons.send, size: 18),
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
}

class _HistoryTile extends StatelessWidget {
  const _HistoryTile(this.item, {required this.onOpen});

  final RecentDictation item;
  final VoidCallback onOpen;

  @override
  Widget build(BuildContext context) {
    final localizations = MaterialLocalizations.of(context);
    final now = DateTime.now();
    final today =
        item.at.year == now.year &&
        item.at.month == now.month &&
        item.at.day == now.day;
    return ListTile(
      contentPadding: const EdgeInsets.only(left: 16, right: 4),
      title: Text(item.text, maxLines: 3, overflow: TextOverflow.ellipsis),
      subtitle: Text(
        today
            ? localizations.formatTimeOfDay(TimeOfDay.fromDateTime(item.at))
            : localizations.formatShortMonthDay(item.at),
      ),
      onTap: onOpen,
      trailing: IconButton(
        tooltip: DictateStrings.copy,
        icon: const Icon(Icons.copy),
        onPressed: () => HistoryScreen._copy(context, item.text),
      ),
    );
  }
}

class _Empty extends StatelessWidget {
  const _Empty({required this.onDictate});

  final VoidCallback? onDictate;

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
              Icons.history,
              size: 48,
              color: theme.colorScheme.onSurfaceVariant,
            ),
            const SizedBox(height: 16),
            Text(
              DictateStrings.historyEmptyTitle,
              textAlign: TextAlign.center,
              style: theme.textTheme.titleLarge,
            ),
            const SizedBox(height: 8),
            Text(
              DictateStrings.historyEmptyBody,
              textAlign: TextAlign.center,
              style: mutedText(context),
            ),
            if (onDictate != null) ...[
              const SizedBox(height: 24),
              FilledButton.icon(
                onPressed: onDictate,
                icon: const Icon(Icons.mic),
                label: const Text(DictateStrings.dictateNow),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
