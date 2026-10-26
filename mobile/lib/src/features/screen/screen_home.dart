import 'dart:async';

import 'package:flutter/material.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../core/relay_client.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
import '../../shell/feature_tab.dart';
import '../dictate/computer_widgets.dart';
import '../dictate/dictate_controller.dart';
import '../dictate/dictate_scope.dart';
import '../dictate/remote_windows.dart';
import '../dictate/strings.dart';
import 'live_controller.dart';
import 'live_dock.dart';
import 'live_protocol.dart';
import 'strings.dart';
import 'watch_controller.dart';
import 'watch_screen.dart' show screenLabel;

/// The computer's screens and open windows, each a tap away from the live
/// view at the top of the app.
final screenTab = FeatureTab(
  id: 'screen',
  label: ScreenStrings.homeTab,
  icon: AppIcons.screen,
  selectedIcon: AppIcons.screenSelected,
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

/// Lists the computer's screens and starts watching the main one in the
/// small view. Null when it started, else an error code.
Future<String?> watchMainScreen(BuildContext context) async {
  final dictate = DictateScope.of(context);
  final listed = await dictate.listWindows();
  if (listed.error case final error?) return error;
  final target = viewScreenTarget(listed.screens, listed.windows);
  if (target == null) return 'target_gone';
  if (!context.mounted) return null;
  openWatch(context, dictate, target);
  return null;
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

  void _watch(WatchTarget target, {RemoteWindow? window}) =>
      openWatch(context, _controller, target, window: window);

  @override
  Widget build(BuildContext context) {
    final relay = _services.relay;
    final live = LiveScope.maybeOf(context);
    return ListenableBuilder(
      listenable: Listenable.merge([
        _controller,
        relay.devices,
        relay.status,
        ?live,
      ]),
      builder: (context, _) {
        final computer = _controller.computer;
        final connected = relay.status.value == RelayStatus.connected;
        if (computer != null) _listWhenReady(computer, connected);
        return ComputerGate(
          controller: _controller,
          builder: (context, computer, connected) =>
              _buildBody(context, computer, connected && canWatch(computer)),
        );
      },
    );
  }

  Widget _buildBody(BuildContext context, Device computer, bool watchable) {
    final t = context.talvio;
    final listing = _controller.listing;
    final error = _error;
    final name = computer.name;
    final screens = [
      ...?listing?.screens.where((s) => s.primary),
      ...?listing?.screens.where((s) => !s.primary),
    ];
    final windows = listing?.windows ?? const <RemoteWindow>[];
    final watching = LiveScope.maybeOf(context)?.watch;
    final watchedId = watching != null && watching.phase != WatchPhase.ended
        ? watching.target.id
        : null;

    Future<void> refresh() => watchable ? _load() : Future.value();

    // Nothing to list: one empty state, still pulled to refresh.
    final Widget? empty;
    if (!computer.online) {
      empty = EmptyState(
        icon: DuoIcons.offline,
        title: DictateStrings.offlineTitle(name),
        body: ScreenStrings.offlineBody(name),
      );
    } else if (computer.capabilities.isEmpty) {
      // Online but not past its `hello` yet.
      empty = EmptyState(
        icon: DuoIcons.addComputer,
        title: ScreenStrings.waitingBody(name),
        busy: true,
      );
    } else if (!canWatch(computer)) {
      empty = computer.capabilities.contains(DictateKind.dictate)
          ? EmptyState(
              icon: DuoIcons.hidden,
              title: ScreenStrings.turnOnTitle(name),
              child: const _TurnOnSteps(),
            )
          : EmptyState(
              icon: DuoIcons.update,
              title: DictateStrings.updateTitle(name),
              body: ScreenStrings.updateBody(name),
            );
    } else {
      empty = null;
    }
    if (empty != null) {
      return RefreshIndicator(
        onRefresh: refresh,
        child: LayoutBuilder(
          builder: (context, constraints) => SingleChildScrollView(
            physics: const AlwaysScrollableScrollPhysics(),
            child: ConstrainedBox(
              constraints: BoxConstraints(minHeight: constraints.maxHeight),
              child: Center(child: empty),
            ),
          ),
        ),
      );
    }

    WatchTarget windowTarget(RemoteWindow window) => WatchTarget(
      id: window.id,
      label: windowLabel(window).$1,
      screen: false,
    );
    Widget liveDot() => StatusDot(color: t.live, blink: true);
    Widget trailing({required bool watched, bool front = false}) => watched
        ? const StatusPill(ScreenStrings.watching)
        : Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (front) ...[
                const StatusPill(ScreenStrings.inFront),
                const SizedBox(width: Space.s8),
              ],
              Icon(AppIcons.caretRight, size: 16, color: t.muted),
            ],
          );

    final children = <Widget>[
      if (error != null) ...[
        const SizedBox(height: Space.s16),
        InlineBanner(
          tone: Tone.danger,
          message: ScreenStrings.error(error, name),
        ),
      ],
      if (screens.isNotEmpty) ...[
        SectionLabel(ScreenStrings.screens, first: error == null),
        Group(
          children: [
            for (final screen in screens)
              ListRow(
                leading: const RowTile(AppIcons.display),
                title: screenLabel(screen),
                titleLeading: watchedId == screen.id ? liveDot() : null,
                subtitleWidget: Text.rich(
                  TextSpan(
                    children: [
                      if (screen.primary)
                        const TextSpan(text: '${ScreenStrings.primary} · '),
                      TextSpan(text: '${screen.width}', style: TalvioType.mono),
                      const TextSpan(text: ' × '),
                      TextSpan(
                        text: '${screen.height}',
                        style: TalvioType.mono,
                      ),
                    ],
                  ),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TalvioType.footnote.copyWith(color: t.muted),
                ),
                trailing: trailing(watched: watchedId == screen.id),
                enabled: watchable,
                semanticLabel: ScreenStrings.watchItem(screenLabel(screen)),
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
        ),
        const GroupFooter(ScreenStrings.heroBody),
      ],
      SectionLabel(
        ScreenStrings.openWindows,
        first: screens.isEmpty && error == null,
        trailing: _loading
            ? const SizedBox.square(
                dimension: 44,
                child: Center(child: Spinner(size: 18)),
              )
            : IconBtn(
                icon: AppIcons.refresh,
                tooltip: ScreenStrings.refresh,
                color: t.muted,
                onPressed: watchable ? _load : null,
              ),
      ),
      if (listing == null && error == null)
        Semantics(
          label: ScreenStrings.loadingWindows,
          child: const Group(
            children: [SkeletonRow(), SkeletonRow(), SkeletonRow()],
          ),
        )
      else if (windows.isEmpty)
        GroupFooter(ScreenStrings.noWindows(name))
      else
        Group(
          children: [
            for (final window in windows)
              ListRow(
                leading: Monogram(windowLabel(window).$1),
                title: windowLabel(window).$1,
                titleMaxLines: 1,
                titleLeading: watchedId == window.id ? liveDot() : null,
                subtitle: windowLabel(window).$2,
                subtitleMaxLines: 1,
                trailing: trailing(
                  watched: watchedId == window.id,
                  front: window.front,
                ),
                enabled: watchable,
                onTap: watchable
                    ? () => _watch(windowTarget(window), window: window)
                    : null,
              ),
          ],
        ),
      const SizedBox(height: Space.s24),
    ];

    return RefreshIndicator(
      onRefresh: refresh,
      child: ListView(
        physics: const AlwaysScrollableScrollPhysics(),
        children: children,
      ),
    );
  }
}

/// How to turn on seeing the screen, as numbered steps.
class _TurnOnSteps extends StatelessWidget {
  const _TurnOnSteps();

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Padding(
      padding: const EdgeInsets.only(top: Space.s16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (final (i, step) in ScreenStrings.turnOnSteps.indexed)
            Padding(
              padding: const EdgeInsets.only(bottom: Space.s12),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Container(
                    width: 22,
                    height: 22,
                    alignment: Alignment.center,
                    decoration: BoxDecoration(
                      color: t.fill,
                      shape: BoxShape.circle,
                    ),
                    child: Text(
                      '${i + 1}',
                      style: TalvioType.label.copyWith(
                        color: t.ink,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                  const SizedBox(width: Space.s12),
                  Expanded(
                    child: Text(
                      step,
                      style: TalvioType.rowBody.copyWith(color: t.ink),
                    ),
                  ),
                ],
              ),
            ),
          Text(
            ScreenStrings.turnOnBody,
            style: TalvioType.footnote.copyWith(color: t.muted),
          ),
        ],
      ),
    );
  }
}
