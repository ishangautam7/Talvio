import 'dart:async';

import 'package:flutter/material.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../core/relay_client.dart';
import '../../core/widgets.dart';
import '../../shell/feature_tab.dart';
import '../../shell/shell_scope.dart';
import '../dictate/computer_widgets.dart';
import '../dictate/dictate_controller.dart';
import '../dictate/dictate_scope.dart';
import '../dictate/dictate_screen.dart' show sendTabId;
import '../dictate/remote_windows.dart';
import 'live_protocol.dart';
import 'strings.dart';
import 'watch_screen.dart';

/// Home: the chosen computer, a button to see its screen, and its open
/// windows, each a tap away from live view.
final screenTab = FeatureTab(
  id: 'screen',
  label: ScreenStrings.homeTab,
  icon: Icons.desktop_windows_outlined,
  selectedIcon: Icons.desktop_windows,
  builder: (_) => const ScreenHome(),
);

/// Whether [computer] can be watched from this phone now.
bool canWatch(Device computer) =>
    computer.online &&
    computer.capabilities.contains(WindowsKind.capability) &&
    computer.capabilities.contains(ScreenKind.capability);

/// What "View screen" opens: the main screen, else any screen, else the
/// window in front, else any window.
WatchTarget? viewScreenTarget(
  List<RemoteScreen> screens,
  List<RemoteWindow> windows,
) {
  final screen =
      screens.where((s) => s.primary).firstOrNull ?? screens.firstOrNull;
  if (screen != null) {
    return WatchTarget(id: screen.id, label: screenLabel(screen), screen: true);
  }
  final window =
      windows.where((w) => w.front).firstOrNull ?? windows.firstOrNull;
  if (window == null) return null;
  return WatchTarget(
    id: window.id,
    label: windowLabel(window).$1,
    screen: false,
  );
}

class ScreenHome extends StatefulWidget {
  const ScreenHome({super.key});

  @override
  State<ScreenHome> createState() => _ScreenHomeState();
}

class _ScreenHomeState extends State<ScreenHome> {
  late final AppServices _services = AppServices.of(context);
  late final DictateController _controller = DictateScope.of(context);

  /// The computer the list was last asked of, while it stays watchable; a
  /// change (another computer, or this one coming back online) lists again.
  String? _listedFor;
  var _loading = false;
  String? _error;

  Future<void> _load() async {
    if (_loading) return;
    setState(() => _loading = true);
    final listed = await _controller.listWindows();
    if (!mounted) return;
    setState(() {
      _loading = false;
      _error = listed.error;
    });
  }

  /// Lists the windows once a computer can be watched, without waiting for
  /// a pull.
  void _listWhenReady(Device computer, bool connected) {
    final key = connected && canWatch(computer) ? computer.id : null;
    if (key == _listedFor) return;
    _listedFor = key;
    if (key == null) {
      _error = null;
      return;
    }
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) unawaited(_load());
    });
  }

  Future<void> _watch(WatchTarget target, {RemoteWindow? window}) async {
    await openWatch(context, _controller, target, window: window);
    // Windows open and close while watching; ids may have moved on too.
    if (mounted) unawaited(_load());
  }

  @override
  Widget build(BuildContext context) {
    final relay = _services.relay;
    return ListenableBuilder(
      listenable: Listenable.merge([_controller, relay.devices, relay.status]),
      builder: (context, _) {
        final computer = _controller.computer;
        final connected = relay.status.value == RelayStatus.connected;
        if (computer != null) _listWhenReady(computer, connected);
        final shell = ShellScope.maybeOf(context);
        final hasSend = shell?.available.contains(sendTabId) ?? false;
        final watchable = computer != null && connected && canWatch(computer);
        return Scaffold(
          appBar: AppBar(
            leading: shellLeading(context),
            // The card below names the computer; the bar only switches
            // between several.
            title: computer == null || _controller.computers.length < 2
                ? const Text(ScreenStrings.homeTab)
                : ComputerTitle(
                    computer,
                    onTap: canChooseComputer(_controller)
                        ? () => chooseComputer(context, _controller)
                        : null,
                  ),
            actions: [
              if (watchable)
                _loading
                    ? const Padding(
                        padding: EdgeInsets.all(14),
                        child: ButtonSpinner(),
                      )
                    : IconButton(
                        tooltip: ScreenStrings.refresh,
                        onPressed: _load,
                        icon: const Icon(Icons.refresh),
                      ),
            ],
          ),
          floatingActionButton: hasSend && computer != null && computer.online
              ? FloatingActionButton.extended(
                  onPressed: () => shell!.select(sendTabId),
                  icon: const Icon(Icons.mic),
                  label: const Text(ScreenStrings.dictateOrType),
                )
              : null,
          body: ComputerGate(
            controller: _controller,
            builder: (context, computer, connected) => _buildBody(
              context,
              computer,
              watchable,
              onSend: hasSend ? () => shell!.select(sendTabId) : null,
            ),
          ),
        );
      },
    );
  }

  Widget _buildBody(
    BuildContext context,
    Device computer,
    bool watchable, {
    required VoidCallback? onSend,
  }) {
    final theme = Theme.of(context);
    final listing = _controller.listing;
    final error = _error;
    final name = computer.name;
    final screens = listing?.screens ?? const <RemoteScreen>[];
    final windows = listing?.windows ?? const <RemoteWindow>[];
    final target = viewScreenTarget(screens, windows);
    final mainScreen =
        screens.where((s) => s.primary).firstOrNull ?? screens.firstOrNull;

    final Widget state;
    if (!computer.online) {
      state = _CardText(ScreenStrings.offlineBody(name));
    } else if (computer.capabilities.isEmpty) {
      // Online but not past its `hello` yet.
      state = Row(
        children: [
          const ButtonSpinner(),
          const SizedBox(width: 12),
          Expanded(child: _CardText(ScreenStrings.waitingBody(name))),
        ],
      );
    } else if (!canWatch(computer)) {
      state = Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            ScreenStrings.turnOnTitle(name),
            style: theme.textTheme.titleMedium,
          ),
          const SizedBox(height: 4),
          _CardText(
            computer.capabilities.contains(DictateKind.dictate)
                ? ScreenStrings.turnOnBody
                : ScreenStrings.updateBody(name),
          ),
          if (onSend != null) ...[
            const SizedBox(height: 16),
            OutlinedButton.icon(
              onPressed: onSend,
              style: OutlinedButton.styleFrom(
                minimumSize: const Size.fromHeight(48),
              ),
              icon: const Icon(Icons.mic),
              label: const Text(ScreenStrings.sendInstead),
            ),
          ],
        ],
      );
    } else {
      state = Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          FilledButton.icon(
            onPressed: watchable && target != null
                ? () => _watch(target)
                : null,
            style: FilledButton.styleFrom(
              minimumSize: const Size.fromHeight(56),
              textStyle: theme.textTheme.titleMedium,
            ),
            icon: const Icon(Icons.visibility_outlined),
            label: const Text(ScreenStrings.viewScreen),
          ),
          if (mainScreen != null) ...[
            const SizedBox(height: 8),
            Text(
              ScreenStrings.viewScreenDetail(
                screenLabel(mainScreen),
                ScreenStrings.screenSize(mainScreen.width, mainScreen.height),
              ),
              textAlign: TextAlign.center,
              style: theme.textTheme.bodySmall?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ),
          ],
        ],
      );
    }

    final children = <Widget>[
      Padding(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
        child: _ComputerCard(computer: computer, child: state),
      ),
      if (canWatch(computer)) ...[
        if (error != null)
          Padding(
            padding: const EdgeInsets.fromLTRB(24, 16, 24, 0),
            child: Semantics(
              liveRegion: true,
              child: Text(
                ScreenStrings.error(error, name),
                style: theme.textTheme.bodyMedium?.copyWith(
                  color: theme.colorScheme.error,
                ),
              ),
            ),
          )
        else if (listing == null)
          Padding(
            padding: const EdgeInsets.all(32),
            child: Column(
              children: [
                const CircularProgressIndicator(),
                const SizedBox(height: 12),
                Text(ScreenStrings.loadingWindows, style: mutedText(context)),
              ],
            ),
          )
        else ...[
          const SectionTitle(ScreenStrings.openWindows),
          if (windows.isEmpty)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Text(
                ScreenStrings.noWindows(name),
                style: mutedText(context),
              ),
            ),
          for (final window in windows)
            _TargetTile(
              icon: Icons.web_asset,
              title: windowLabel(window).$1,
              subtitle: windowLabel(window).$2,
              front: window.front,
              onTap: watchable
                  ? () => _watch(
                      WatchTarget(
                        id: window.id,
                        label: windowLabel(window).$1,
                        screen: false,
                      ),
                      window: window,
                    )
                  : null,
            ),
          if (screens.length > 1) ...[
            const SectionTitle(ScreenStrings.otherScreens),
            for (final screen in screens)
              _TargetTile(
                icon: Icons.desktop_windows_outlined,
                title: screenLabel(screen),
                subtitle: screen.primary
                    ? '${ScreenStrings.screenSize(screen.width, screen.height)}'
                          ' · ${ScreenStrings.primary}'
                    : ScreenStrings.screenSize(screen.width, screen.height),
                front: false,
                onTap: watchable
                    ? () => _watch(
                        WatchTarget(
                          id: screen.id,
                          label: screenLabel(screen),
                          screen: true,
                        ),
                      )
                    : null,
              ),
          ],
        ],
      ],
      // Room for the floating button over the last row.
      const SizedBox(height: 96),
    ];

    return RefreshIndicator(
      onRefresh: watchable ? _load : () async {},
      child: ListView(
        physics: const AlwaysScrollableScrollPhysics(),
        children: children,
      ),
    );
  }
}

class _CardText extends StatelessWidget {
  const _CardText(this.text);

  final String text;

  @override
  Widget build(BuildContext context) => Text(text, style: mutedText(context));
}

/// The computer, large, with what can be done with it now.
class _ComputerCard extends StatelessWidget {
  const _ComputerCard({required this.computer, required this.child});

  final Device computer;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    return Card.filled(
      color: scheme.surfaceContainerLow,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(24)),
      child: Padding(
        padding: const EdgeInsets.all(20),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                DecoratedBox(
                  decoration: BoxDecoration(
                    color: scheme.primaryContainer,
                    borderRadius: BorderRadius.circular(16),
                  ),
                  child: SizedBox.square(
                    dimension: 56,
                    child: Icon(
                      switch (computer.platform) {
                        'macos' => Icons.laptop_mac,
                        'linux' => Icons.computer,
                        _ => Icons.desktop_windows,
                      },
                      size: 28,
                      color: scheme.onPrimaryContainer,
                    ),
                  ),
                ),
                const SizedBox(width: 16),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        computer.name,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.titleLarge,
                      ),
                      const SizedBox(height: 2),
                      Row(
                        children: [
                          Icon(
                            Icons.circle,
                            size: 10,
                            color: computer.online
                                ? scheme.primary
                                : scheme.outline,
                          ),
                          const SizedBox(width: 6),
                          Flexible(
                            child: Text(
                              computer.online
                                  ? ScreenStrings.online(computer.platform)
                                  : ScreenStrings.offline,
                              style: theme.textTheme.bodyMedium?.copyWith(
                                color: scheme.onSurfaceVariant,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ],
                  ),
                ),
              ],
            ),
            const SizedBox(height: 20),
            child,
          ],
        ),
      ),
    );
  }
}

/// A window or screen; tapping it watches it.
class _TargetTile extends StatelessWidget {
  const _TargetTile({
    required this.icon,
    required this.title,
    required this.subtitle,
    required this.front,
    required this.onTap,
  });

  final IconData icon;
  final String title;
  final String? subtitle;
  final bool front;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    return ListTile(
      enabled: onTap != null,
      leading: DecoratedBox(
        decoration: BoxDecoration(
          color: scheme.surfaceContainerHighest,
          borderRadius: BorderRadius.circular(12),
        ),
        child: SizedBox.square(
          dimension: 40,
          child: Icon(icon, color: scheme.onSurfaceVariant),
        ),
      ),
      title: Text(
        title,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: const TextStyle(fontWeight: FontWeight.w600),
      ),
      subtitle: subtitle == null
          ? null
          : Text(subtitle!, maxLines: 1, overflow: TextOverflow.ellipsis),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (front)
            DecoratedBox(
              decoration: BoxDecoration(
                color: scheme.secondaryContainer,
                borderRadius: BorderRadius.circular(8),
              ),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                child: Text(
                  ScreenStrings.inFront,
                  style: theme.textTheme.labelSmall?.copyWith(
                    color: scheme.onSecondaryContainer,
                  ),
                ),
              ),
            ),
          const SizedBox(width: 4),
          const Icon(Icons.chevron_right),
        ],
      ),
      onTap: onTap,
    );
  }
}
