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
  icon: Icons.cast_outlined,
  selectedIcon: Icons.cast_rounded,
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
    final screens = listing?.screens ?? const <RemoteScreen>[];
    final windows = listing?.windows ?? const <RemoteWindow>[];
    final target = viewScreenTarget(screens, windows);
    final mainScreen =
        screens.where((s) => s.primary).firstOrNull ?? screens.firstOrNull;
    final watching = LiveScope.maybeOf(context)?.watch;
    final watchedId = watching != null && watching.phase != WatchPhase.ended
        ? watching.target.id
        : null;

    final Widget state;
    if (!computer.online) {
      state = Notice(
        tone: Tone.neutral,
        icon: Icons.power_settings_new_rounded,
        message: ScreenStrings.offlineBody(name),
      );
    } else if (computer.capabilities.isEmpty) {
      // Online but not past its `hello` yet.
      state = Notice(busy: true, message: ScreenStrings.waitingBody(name));
    } else if (!canWatch(computer)) {
      state = computer.capabilities.contains(DictateKind.dictate)
          ? _TurnOn(name: name)
          : Notice(
              tone: Tone.warning,
              icon: Icons.system_update_alt_rounded,
              message: ScreenStrings.updateBody(name),
            );
    } else {
      state = _Hero(
        name: name,
        detail: mainScreen == null
            ? null
            : ScreenStrings.viewScreenDetail(
                screenLabel(mainScreen),
                ScreenStrings.screenSize(mainScreen.width, mainScreen.height),
              ),
        onView: watchable && target != null ? () => _watch(target) : null,
        watching: watchedId != null && watchedId == target?.id,
      );
    }

    final children = <Widget>[
      state,
      if (canWatch(computer)) ...[
        SectionHeader(
          ScreenStrings.openWindows,
          trailing: _loading
              ? const Padding(
                  padding: EdgeInsets.all(12),
                  child: ButtonSpinner(),
                )
              : IconButton(
                  tooltip: ScreenStrings.refresh,
                  onPressed: watchable ? _load : null,
                  icon: Icon(Icons.refresh_rounded, color: t.muted),
                ),
        ),
        if (error != null)
          Notice(tone: Tone.danger, message: ScreenStrings.error(error, name))
        else if (listing == null)
          Padding(
            padding: const EdgeInsets.all(Space.xxl),
            child: Column(
              children: [
                const CircularProgressIndicator(),
                const SizedBox(height: Space.md),
                Text(ScreenStrings.loadingWindows, style: mutedText(context)),
              ],
            ),
          )
        else if (windows.isEmpty)
          Notice(tone: Tone.neutral, message: ScreenStrings.noWindows(name))
        else
          _Group(
            children: [
              for (final window in windows)
                _TargetRow(
                  leading: _AppMonogram(windowLabel(window).$1),
                  title: windowLabel(window).$1,
                  subtitle: windowLabel(window).$2,
                  front: window.front,
                  watching: watchedId == window.id,
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
            ],
          ),
        if (screens.length > 1) ...[
          const SectionHeader(ScreenStrings.otherScreens),
          _Group(
            children: [
              for (final screen in screens)
                _TargetRow(
                  leading: const IconTile(Icons.monitor_rounded, size: 40),
                  title: screenLabel(screen),
                  subtitle: screen.primary
                      ? '${ScreenStrings.screenSize(screen.width, screen.height)}'
                            ' · ${ScreenStrings.primary}'
                      : ScreenStrings.screenSize(screen.width, screen.height),
                  front: false,
                  watching: watchedId == screen.id,
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
        ],
      ],
      const SizedBox(height: Space.xxl),
    ];

    return RefreshIndicator(
      onRefresh: watchable ? _load : () async {},
      child: ListView(
        padding: const EdgeInsets.fromLTRB(Space.lg, Space.xs, Space.lg, 0),
        physics: const AlwaysScrollableScrollPhysics(),
        children: children,
      ),
    );
  }
}

/// The dark card at the top: see the screen.
class _Hero extends StatelessWidget {
  const _Hero({
    required this.name,
    required this.detail,
    required this.onView,
    required this.watching,
  });

  final String name;
  final String? detail;
  final VoidCallback? onView;
  final bool watching;

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: BoxDecoration(
        gradient: TalvioColors.inkTile,
        borderRadius: BorderRadius.circular(Radii.xl),
      ),
      padding: const EdgeInsets.all(Space.xl),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                child: Text(
                  ScreenStrings.heroTitle(name),
                  style: context.text.headlineSmall?.copyWith(
                    color: Colors.white,
                  ),
                ),
              ),
              const SizedBox(width: Space.md),
              const _MonitorArt(),
            ],
          ),
          const SizedBox(height: Space.sm),
          Text(
            ScreenStrings.heroBody,
            style: context.text.bodyMedium?.copyWith(
              color: Colors.white.withValues(alpha: 0.72),
            ),
          ),
          const SizedBox(height: Space.xl),
          SizedBox(
            width: double.infinity,
            child: FilledButton.icon(
              onPressed: onView,
              style: FilledButton.styleFrom(
                backgroundColor: Colors.white,
                foregroundColor: const Color(0xFF1B1D33),
                disabledBackgroundColor: Colors.white.withValues(alpha: 0.18),
                disabledForegroundColor: Colors.white.withValues(alpha: 0.5),
                minimumSize: const Size.fromHeight(52),
              ),
              icon: Icon(
                watching ? Icons.cast_connected_rounded : Icons.cast_rounded,
              ),
              label: Text(
                watching ? ScreenStrings.watchingNow : ScreenStrings.viewScreen,
              ),
            ),
          ),
          if (detail case final detail?) ...[
            const SizedBox(height: Space.sm),
            Center(
              child: Text(
                detail,
                style: context.text.bodySmall?.copyWith(
                  color: Colors.white.withValues(alpha: 0.6),
                ),
              ),
            ),
          ],
        ],
      ),
    );
  }
}

/// A small monitor with the brand's waveform on it.
class _MonitorArt extends StatelessWidget {
  const _MonitorArt();

  @override
  Widget build(BuildContext context) => ExcludeSemantics(
    child: SizedBox(
      width: 64,
      height: 52,
      child: CustomPaint(painter: _MonitorPainter()),
    ),
  );
}

class _MonitorPainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final screen = Rect.fromLTWH(0, 0, size.width, size.height * 0.74);
    canvas.drawRRect(
      RRect.fromRectAndRadius(screen, const Radius.circular(8)),
      Paint()..shader = TalvioColors.gradient.createShader(screen),
    );
    final stand = Paint()..color = Colors.white.withValues(alpha: 0.5);
    canvas.drawRRect(
      RRect.fromRectAndRadius(
        Rect.fromLTWH(
          size.width * 0.44,
          screen.bottom,
          size.width * 0.12,
          size.height * 0.14,
        ),
        const Radius.circular(2),
      ),
      stand,
    );
    canvas.drawRRect(
      RRect.fromRectAndRadius(
        Rect.fromLTWH(
          size.width * 0.3,
          size.height * 0.88,
          size.width * 0.4,
          size.height * 0.08,
        ),
        const Radius.circular(3),
      ),
      stand,
    );
    final bar = Paint()..color = Colors.white;
    const heights = [0.22, 0.4, 0.62, 0.4, 0.22];
    final w = size.width * 0.07;
    for (var i = 0; i < 5; i++) {
      final h = screen.height * heights[i];
      final x = size.width * (0.24 + i * 0.13);
      canvas.drawRRect(
        RRect.fromRectAndRadius(
          Rect.fromLTWH(x - w / 2, screen.center.dy - h / 2, w, h),
          Radius.circular(w / 2),
        ),
        bar,
      );
    }
  }

  @override
  bool shouldRepaint(_MonitorPainter oldDelegate) => false;
}

/// How to turn on seeing the screen, as numbered steps.
class _TurnOn extends StatelessWidget {
  const _TurnOn({required this.name});

  final String name;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Surface(
      padding: const EdgeInsets.all(Space.xl),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const IconTile(
            Icons.visibility_off_outlined,
            tone: Tone.accent,
            size: 44,
          ),
          const SizedBox(height: Space.lg),
          Text(ScreenStrings.turnOnTitle(name), style: context.text.titleLarge),
          const SizedBox(height: Space.md),
          for (final (i, step) in ScreenStrings.turnOnSteps.indexed)
            Padding(
              padding: const EdgeInsets.only(bottom: Space.sm),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Container(
                    width: 22,
                    height: 22,
                    alignment: Alignment.center,
                    decoration: BoxDecoration(
                      color: t.sunken,
                      shape: BoxShape.circle,
                    ),
                    child: Text('${i + 1}', style: context.text.labelSmall),
                  ),
                  const SizedBox(width: Space.md),
                  Expanded(child: Text(step, style: context.text.bodyMedium)),
                ],
              ),
            ),
          Text(ScreenStrings.turnOnBody, style: context.text.bodySmall),
        ],
      ),
    );
  }
}

/// Rows in one card, with hairlines between them.
class _Group extends StatelessWidget {
  const _Group({required this.children});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) => Surface(
    padding: EdgeInsets.zero,
    child: Column(
      children: [
        for (final (i, child) in children.indexed) ...[
          if (i > 0) Divider(indent: 68, color: context.talvio.hairline),
          child,
        ],
      ],
    ),
  );
}

/// An app's first letter on a tile, in a colour of its own.
class _AppMonogram extends StatelessWidget {
  const _AppMonogram(this.app);

  final String app;

  static const _hues = [14.0, 32.0, 200.0, 255.0, 160.0, 330.0, 280.0];

  @override
  Widget build(BuildContext context) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    final hue = _hues[app.hashCode.abs() % _hues.length];
    final bg = HSLColor.fromAHSL(1, hue, 0.55, dark ? 0.24 : 0.92).toColor();
    final fg = HSLColor.fromAHSL(1, hue, 0.6, dark ? 0.8 : 0.36).toColor();
    final letter = app.trim().isEmpty
        ? '?'
        : app.trim().characters.first.toUpperCase();
    return Container(
      width: 40,
      height: 40,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Text(
        letter,
        style: context.text.titleMedium?.copyWith(
          color: fg,
          fontWeight: FontWeight.w700,
        ),
      ),
    );
  }
}

/// A window or screen; tapping it watches it.
class _TargetRow extends StatelessWidget {
  const _TargetRow({
    required this.leading,
    required this.title,
    required this.subtitle,
    required this.front,
    required this.watching,
    required this.onTap,
  });

  final Widget leading;
  final String title;
  final String? subtitle;
  final bool front;
  final bool watching;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return ListTile(
      enabled: onTap != null,
      contentPadding: const EdgeInsets.symmetric(
        horizontal: Space.lg,
        vertical: 2,
      ),
      leading: leading,
      title: Text(
        title,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: context.text.titleSmall,
      ),
      subtitle: subtitle == null
          ? null
          : Text(subtitle!, maxLines: 1, overflow: TextOverflow.ellipsis),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (watching)
            Pill(ScreenStrings.live, tone: Tone.danger, dot: t.live)
          else if (front)
            const Pill(ScreenStrings.inFront),
          const SizedBox(width: Space.xs),
          Icon(Icons.chevron_right_rounded, color: t.faint),
        ],
      ),
      onTap: onTap == null
          ? null
          : () {
              Haptic.tick();
              onTap!();
            },
    );
  }
}
