import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:wakelock_plus/wakelock_plus.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../theme.dart';
import '../dictate/dictate_controller.dart';
import '../dictate/remote_windows.dart';
import '../dictate/strings.dart';
import 'live_protocol.dart';
import 'rtc_link.dart';
import 'strings.dart';
import 'view_geometry.dart';
import 'watch_controller.dart';
import 'webrtc_link.dart';

/// A screen as the picker and the viewer name it.
String screenLabel(RemoteScreen screen) =>
    screen.name.isEmpty ? ScreenStrings.untitledScreen : screen.name;

/// The route of live view: a window or screen of [computer], to watch.
Route<void> watchRoute({
  required Device computer,
  required WatchTarget target,
  RtcLinkFactory? linkFactory,
}) => MaterialPageRoute(
  builder: (_) => WatchScreen(
    computer: computer,
    target: target,
    linkFactory: linkFactory ?? WebRtcLink.create,
  ),
);

/// Opens live view of [target] on [dictate]'s computer. The watched window
/// is also where the Send page's dictation and typing go next: [window], or
/// for a screen wherever the keyboard is. Completes when the viewer closes.
Future<void> openWatch(
  BuildContext context,
  DictateController dictate,
  WatchTarget target, {
  RemoteWindow? window,
}) async {
  final computer = dictate.computer;
  if (computer == null) return;
  dictate.chooseWindow(window);
  await Navigator.of(
    context,
  ).push(watchRoute(computer: computer, target: target));
}

/// Live view, to watch only: pinch and drag move around the picture, and
/// nothing on the computer is clicked, scrolled or typed into from here.
class WatchScreen extends StatefulWidget {
  const WatchScreen({
    super.key,
    required this.computer,
    required this.target,
    required this.linkFactory,
  });

  final Device computer;
  final WatchTarget target;
  final RtcLinkFactory linkFactory;

  @override
  State<WatchScreen> createState() => _WatchScreenState();
}

class _WatchScreenState extends State<WatchScreen>
    with SingleTickerProviderStateMixin {
  late final WatchController _watch;
  late final AppLifecycleListener _lifecycle;
  final _transform = TransformationController();
  late final AnimationController _move = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 280),
  );
  Animation<Matrix4>? _moveTo;

  /// Keep the text cursor in view. Any pan or zoom by hand turns it off.
  var _follow = true;

  /// The picture fills the view, cropped, rather than fits in it whole.
  var _filled = false;

  /// The orientation the viewer is held in; null to follow the phone.
  Orientation? _held;
  Size _viewport = Size.zero;
  Caret? _followed;

  /// Where the view was when the pan or zoom in progress began.
  Matrix4? _gestureFrom;

  @override
  void initState() {
    super.initState();
    _watch = WatchController(
      relay: AppServices.of(context).relay,
      computer: widget.computer,
      target: widget.target,
      linkFactory: widget.linkFactory,
    )..addListener(_onWatch);
    _move.addListener(() {
      if (_moveTo case final to?) _transform.value = to.value;
    });
    _lifecycle = AppLifecycleListener(
      onResume: () => _watch.setForeground(true),
      onPause: () => _watch.setForeground(false),
    );
    _keepAwake(true);
    _watch.start();
  }

  @override
  void dispose() {
    _keepAwake(false);
    if (_held != null) _hold(null);
    _lifecycle.dispose();
    _watch
      ..removeListener(_onWatch)
      ..dispose();
    _move.dispose();
    _transform.dispose();
    super.dispose();
  }

  /// The screen stays on while watching. Best effort: without the plugin
  /// (tests) it simply does not.
  static void _keepAwake(bool on) => unawaited(
    Future.sync(
      () => on ? WakelockPlus.enable() : WakelockPlus.disable(),
    ).catchError((Object _) {}),
  );

  String get _name => widget.computer.name;

  /// Holds the viewer sideways or upright; null lets the phone decide again.
  /// Best effort, like the wake lock.
  static void _hold(Orientation? orientation) => unawaited(
    Future.sync(
      () => SystemChrome.setPreferredOrientations(switch (orientation) {
        Orientation.landscape => const [
          DeviceOrientation.landscapeLeft,
          DeviceOrientation.landscapeRight,
        ],
        Orientation.portrait => const [DeviceOrientation.portraitUp],
        null => const [],
      }),
    ).catchError((Object _) {}),
  );

  void _rotate(Orientation now) {
    final next = now == Orientation.portrait
        ? Orientation.landscape
        : Orientation.portrait;
    setState(() => _held = next);
    _hold(next);
  }

  /// Fills the view with the picture, or shows all of it again. Either is a
  /// view chosen by hand, so it stops Follow.
  void _toggleFill() {
    final fill = !_filled;
    setState(() {
      _filled = fill;
      _follow = false;
    });
    _moveView(fill ? fillTransform(_picture, _viewport) : Matrix4.identity());
  }

  void _moveView(Matrix4 to) {
    _moveTo = Matrix4Tween(
      begin: _transform.value.clone(),
      end: to,
    ).animate(CurvedAnimation(parent: _move, curve: Curves.easeOutCubic));
    unawaited(_move.forward(from: 0));
  }

  /// The picture's size: the decoded frames', else the window's as the
  /// computer described it, else the viewport's (no letterbox).
  Size get _picture {
    if (_watch.link?.videoSize.value case final size?) return size;
    if (_watch.watched case WatchedTarget(
      :final width,
      :final height,
    ) when width > 0 && height > 0) {
      return Size(width.toDouble(), height.toDouble());
    }
    return _viewport;
  }

  void _onWatch() {
    if (_watch.caret != _followed) {
      _followed = _watch.caret;
      _applyFollow();
    }
  }

  /// Moves the view to the caret, if Follow is on and there is one.
  void _applyFollow({bool animate = true}) {
    final caret = _watch.caret;
    if (!_follow || caret == null || _viewport.isEmpty) return;
    final to = followTransform(
      current: _transform.value,
      caret: caret,
      viewport: _viewport,
      picture: _picture,
    );
    if (to == _transform.value) return;
    if (_filled) setState(() => _filled = false);
    if (!animate) {
      _move.stop();
      _transform.value = to;
      return;
    }
    _moveView(to);
  }

  void _setFollow(bool follow) {
    setState(() => _follow = follow);
    if (follow) _applyFollow();
  }

  void _onInteractionStart(ScaleStartDetails details) {
    _move.stop();
    _gestureFrom = _transform.value.clone();
  }

  void _onInteractionEnd(ScaleEndDetails details) {
    final from = _gestureFrom;
    _gestureFrom = null;
    if (from == null || from == _transform.value) return;
    if (_follow) setState(() => _follow = false);
    final zoomed =
        (from.getMaxScaleOnAxis() - _transform.value.getMaxScaleOnAxis())
            .abs() >
        0.01;
    if (_filled && zoomed) setState(() => _filled = false);
  }

  @override
  Widget build(BuildContext context) {
    return Theme(
      data: buildTheme(Brightness.dark),
      child: Builder(
        builder: (context) => ListenableBuilder(
          listenable: _watch,
          builder: (context, _) => Scaffold(
            backgroundColor: Colors.black,
            body: MediaQuery.orientationOf(context) == Orientation.landscape
                ? Row(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Expanded(child: _buildStage(context)),
                      _buildSide(context),
                    ],
                  )
                : Column(
                    children: [
                      Expanded(child: _buildStage(context)),
                      _buildBar(context),
                    ],
                  ),
          ),
        ),
      ),
    );
  }

  Widget _buildStage(BuildContext context) {
    final w = _watch;
    final link = w.link;
    return Stack(
      children: [
        Positioned.fill(
          child: LayoutBuilder(
            builder: (context, constraints) {
              final viewport = constraints.biggest;
              if (viewport != _viewport) {
                _viewport = viewport;
                // Rotated or resized: put the caret back in view.
                WidgetsBinding.instance.addPostFrameCallback(
                  (_) => _applyFollow(animate: false),
                );
              }
              return InteractiveViewer(
                transformationController: _transform,
                minScale: 1,
                maxScale: 8,
                onInteractionStart: _onInteractionStart,
                onInteractionEnd: _onInteractionEnd,
                child: SizedBox.fromSize(
                  size: viewport,
                  child: link == null || !w.foreground
                      ? const ColoredBox(color: Colors.black)
                      : ValueListenableBuilder(
                          valueListenable: link.videoSize,
                          builder: (context, size, child) {
                            // The first frame sets the letterbox.
                            if (size != null) {
                              WidgetsBinding.instance.addPostFrameCallback(
                                (_) => _applyFollow(animate: false),
                              );
                            }
                            return child!;
                          },
                          child: Semantics(
                            label: ScreenStrings.watchItem(w.title),
                            image: true,
                            child: link.buildVideo(),
                          ),
                        ),
                ),
              );
            },
          ),
        ),
        Positioned(
          left: 0,
          right: 0,
          top: 0,
          child: _TopBar(
            title: w.title,
            computerName: _name,
            status: _statusText(),
            live: w.phase == WatchPhase.live && !w.reconnecting,
            follow: _follow,
            onFollow: w.caret == null ? null : () => _setFollow(!_follow),
            onClose: () => Navigator.maybePop(context),
            filled: _filled,
            // Only a picture that leaves bars can fill the view.
            onFill:
                _filled ||
                    (link != null &&
                        !_viewport.isEmpty &&
                        fillScale(_picture, _viewport) >= 1.05)
                ? _toggleFill
                : null,
            sideways:
                MediaQuery.orientationOf(context) == Orientation.landscape,
            onRotate: () => _rotate(MediaQuery.orientationOf(context)),
            notice: switch (w.watched?.state) {
              'minimized' => ScreenStrings.minimized,
              'covered' => ScreenStrings.covered,
              _ => null,
            },
          ),
        ),
        if (w.phase == WatchPhase.ended)
          Center(child: _endedCard(context))
        else if (w.phase != WatchPhase.live)
          Center(child: _waiting(context)),
      ],
    );
  }

  String _statusText() {
    final w = _watch;
    return switch (w.phase) {
      WatchPhase.starting => ScreenStrings.starting,
      WatchPhase.approval => ScreenStrings.approval,
      WatchPhase.connecting => ScreenStrings.connecting,
      WatchPhase.live when w.reconnecting => ScreenStrings.reconnecting,
      WatchPhase.live => ScreenStrings.live,
      WatchPhase.ended => ScreenStrings.ended,
    };
  }

  Widget _waiting(BuildContext context) {
    final theme = Theme.of(context);
    final approval = _watch.phase == WatchPhase.approval;
    return Padding(
      padding: const EdgeInsets.all(32),
      child: Semantics(
        liveRegion: true,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const CircularProgressIndicator(),
            const SizedBox(height: 16),
            Text(
              approval ? DictateStrings.approval(_name) : _statusText(),
              textAlign: TextAlign.center,
              style: theme.textTheme.titleMedium,
            ),
            if (approval)
              Text(
                ScreenStrings.approvalDetail,
                textAlign: TextAlign.center,
                style: theme.textTheme.bodyMedium?.copyWith(
                  color: theme.colorScheme.onSurfaceVariant,
                ),
              ),
          ],
        ),
      ),
    );
  }

  Widget _endedCard(BuildContext context) {
    final theme = Theme.of(context);
    final error = _watch.endError;
    return Padding(
      padding: const EdgeInsets.all(24),
      child: Card.filled(
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 16, 8, 8),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Semantics(
                liveRegion: true,
                child: Text(
                  error == null
                      ? ScreenStrings.stoppedByYou
                      : ScreenStrings.error(error, _name),
                  style: theme.textTheme.bodyLarge,
                ),
              ),
              OverflowBar(
                alignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                    onPressed: () => Navigator.maybePop(context),
                    child: const Text(ScreenStrings.close),
                  ),
                  if (error != 'target_gone')
                    TextButton(
                      onPressed: _watch.start,
                      child: const Text(ScreenStrings.watchAgain),
                    ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// Sound on or off.
  Widget _sound() {
    final w = _watch;
    return IconButton(
      tooltip: w.soundOn ? ScreenStrings.soundOff : ScreenStrings.soundOn,
      isSelected: w.soundOn,
      onPressed: w.phase == WatchPhase.ended
          ? null
          : () => w.setSound(!w.soundOn),
      icon: const Icon(Icons.volume_off_outlined),
      selectedIcon: const Icon(Icons.volume_up),
    );
  }

  void _close() => unawaited(Navigator.maybePop(context));

  /// Portrait: one slim row under the picture, sound and Stop.
  Widget _buildBar(BuildContext context) {
    return Material(
      color: Theme.of(context).colorScheme.surface,
      child: SafeArea(
        top: false,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(8, 4, 8, 4),
          child: Row(
            children: [
              _sound(),
              const Spacer(),
              FilledButton.tonalIcon(
                onPressed: _close,
                icon: const Icon(Icons.stop_rounded),
                label: const Text(ScreenStrings.stop),
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// Landscape: the same two buttons in a slim column beside the picture.
  Widget _buildSide(BuildContext context) {
    return Material(
      color: Theme.of(context).colorScheme.surface,
      child: SafeArea(
        left: false,
        child: SizedBox(
          width: 64,
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(8),
            child: Column(
              children: [
                _sound(),
                const SizedBox(height: 8),
                IconButton.filledTonal(
                  tooltip: ScreenStrings.stop,
                  onPressed: _close,
                  icon: const Icon(Icons.stop_rounded),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _TopBar extends StatelessWidget {
  const _TopBar({
    required this.title,
    required this.computerName,
    required this.status,
    required this.live,
    required this.follow,
    required this.onFollow,
    required this.onClose,
    required this.notice,
    required this.filled,
    required this.onFill,
    required this.sideways,
    required this.onRotate,
  });

  final String title;
  final String computerName;
  final String status;
  final bool live;
  final bool follow;

  /// Null while there is no text cursor to follow.
  final VoidCallback? onFollow;
  final VoidCallback onClose;
  final String? notice;

  /// The picture fills the view; [onFill] toggles it, null while there are
  /// no bars to fill.
  final bool filled;
  final VoidCallback? onFill;

  /// The viewer is on its side; [onRotate] turns it.
  final bool sideways;
  final VoidCallback onRotate;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return DecoratedBox(
      decoration: const BoxDecoration(
        gradient: LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [Color(0xCC000000), Color(0x00000000)],
        ),
      ),
      child: SafeArea(
        bottom: false,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(4, 4, 8, 12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: [
              Row(
                children: [
                  IconButton(
                    tooltip: ScreenStrings.stopWatching,
                    onPressed: onClose,
                    icon: const Icon(Icons.close),
                  ),
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
                        Row(
                          children: [
                            Icon(
                              Icons.circle,
                              size: 8,
                              color: live
                                  ? theme.colorScheme.primary
                                  : theme.colorScheme.onSurfaceVariant,
                            ),
                            const SizedBox(width: 6),
                            Semantics(
                              liveRegion: true,
                              child: Text(
                                status,
                                style: theme.textTheme.bodySmall?.copyWith(
                                  color: live
                                      ? theme.colorScheme.primary
                                      : theme.colorScheme.onSurfaceVariant,
                                  fontWeight: FontWeight.w600,
                                ),
                              ),
                            ),
                            Flexible(
                              child: Text(
                                ' · $computerName',
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: theme.textTheme.bodySmall?.copyWith(
                                  color: theme.colorScheme.onSurfaceVariant,
                                ),
                              ),
                            ),
                          ],
                        ),
                      ],
                    ),
                  ),
                  if (onFill case final onFill?)
                    IconButton(
                      tooltip: filled ? ScreenStrings.fit : ScreenStrings.fill,
                      isSelected: filled,
                      onPressed: onFill,
                      icon: const Icon(Icons.fit_screen_outlined),
                      selectedIcon: const Icon(Icons.fit_screen),
                    ),
                  IconButton(
                    tooltip: sideways
                        ? ScreenStrings.unrotate
                        : ScreenStrings.rotate,
                    onPressed: onRotate,
                    icon: Icon(
                      sideways
                          ? Icons.stay_current_portrait
                          : Icons.stay_current_landscape,
                    ),
                  ),
                  if (onFollow != null)
                    IconButton(
                      tooltip: follow
                          ? ScreenStrings.following
                          : ScreenStrings.follow,
                      isSelected: follow,
                      onPressed: onFollow,
                      icon: const Icon(Icons.center_focus_weak),
                      selectedIcon: const Icon(Icons.center_focus_strong),
                    ),
                ],
              ),
              if (notice case final notice?)
                Padding(
                  padding: const EdgeInsets.fromLTRB(12, 4, 12, 0),
                  child: Text(notice, style: theme.textTheme.bodySmall),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
