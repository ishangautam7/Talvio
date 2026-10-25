// The small live view at the top of every page while a watch runs: the
// picture with its state, sound, a button to make it full screen and Stop.
// Collapsed it is one line.

import 'dart:async';

import 'package:flutter/material.dart';

import '../../design/components.dart';
import '../../design/tokens.dart';
import '../assistant/assistant_scope.dart';
import '../dictate/dictate_controller.dart';
import '../dictate/remote_windows.dart';
import 'live_protocol.dart';
import 'live_controller.dart';
import 'strings.dart';
import 'watch_controller.dart';
import 'watch_screen.dart';

/// What the watch is doing, in a word or two.
String watchStatus(WatchController w) => switch (w.phase) {
  WatchPhase.starting => ScreenStrings.starting,
  WatchPhase.approval => ScreenStrings.approval,
  WatchPhase.connecting => ScreenStrings.connecting,
  WatchPhase.live when w.reconnecting => ScreenStrings.reconnecting,
  WatchPhase.live => ScreenStrings.live,
  WatchPhase.ended => ScreenStrings.ended,
};

/// Opens the full-screen viewer of the running watch.
Future<void> openFullScreen(BuildContext context, LiveController live) async {
  final watch = live.watch;
  if (watch == null || live.fullScreen) return;
  live.setFullScreen(true);
  try {
    await Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => WatchScreen(
          watch: watch,
          live: live,
          assistant: AssistantScope.maybeOf(context),
        ),
      ),
    );
  } finally {
    live.setFullScreen(false);
  }
}

class LiveDock extends StatelessWidget {
  const LiveDock({super.key});

  @override
  Widget build(BuildContext context) {
    final live = LiveScope.maybeOf(context);
    if (live == null) return const SizedBox.shrink();
    return ListenableBuilder(
      listenable: live,
      builder: (context, _) {
        final watch = live.watch;
        return AnimatedSize(
          duration: Motion.medium,
          curve: Motion.emphasized,
          alignment: Alignment.topCenter,
          child: watch == null
              ? const SizedBox(width: double.infinity)
              : Padding(
                  padding: const EdgeInsets.fromLTRB(
                    Space.lg,
                    Space.xs,
                    Space.lg,
                    Space.sm,
                  ),
                  child: _Dock(live: live, watch: watch),
                ),
        );
      },
    );
  }
}

class _Dock extends StatelessWidget {
  const _Dock({required this.live, required this.watch});

  final LiveController live;
  final WatchController watch;

  @override
  Widget build(BuildContext context) {
    final w = watch;
    final ended = w.phase == WatchPhase.ended;
    final showPicture = live.expanded && !live.fullScreen;
    final height = MediaQuery.sizeOf(context).height;
    return Semantics(
      container: true,
      label: ScreenStrings.watchItem(w.title),
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: const Color(0xFF0B0C18),
          borderRadius: BorderRadius.circular(Radii.lg),
          boxShadow: [
            BoxShadow(
              color: Colors.black.withValues(alpha: 0.18),
              blurRadius: 20,
              offset: const Offset(0, 8),
            ),
          ],
        ),
        child: ClipRRect(
          borderRadius: BorderRadius.circular(Radii.lg),
          child: Theme(
            data: Theme.of(context).copyWith(
              iconButtonTheme: IconButtonThemeData(
                style: IconButton.styleFrom(foregroundColor: Colors.white),
              ),
            ),
            child: showPicture
                ? ConstrainedBox(
                    constraints: BoxConstraints(maxHeight: height * 0.3),
                    child: _Aspect(
                      watch: w,
                      child: Stack(
                        fit: StackFit.expand,
                        children: [
                          GestureDetector(
                            onTap: ended
                                ? null
                                : () =>
                                      unawaited(openFullScreen(context, live)),
                            child: _Picture(watch: w),
                          ),
                          _Overlay(live: live, watch: w),
                          if (ended)
                            _Ended(live: live, watch: w)
                          else if (w.phase != WatchPhase.live)
                            _Waiting(watch: w),
                        ],
                      ),
                    ),
                  )
                : _Bar(live: live, watch: w),
          ),
        ),
      ),
    );
  }
}

/// The picture's shape: the frames', else the window's as the computer
/// described it, else 16:9.
class _Aspect extends StatelessWidget {
  const _Aspect({required this.watch, required this.child});

  final WatchController watch;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    double from(Size? size) {
      if (size != null && size.width > 0 && size.height > 0) {
        return (size.width / size.height).clamp(0.6, 2.4);
      }
      if (watch.watched case final target?
          when target.width > 0 && target.height > 0) {
        return (target.width / target.height).clamp(0.6, 2.4);
      }
      return 16 / 9;
    }

    final link = watch.link;
    if (link == null) return AspectRatio(aspectRatio: from(null), child: child);
    return ValueListenableBuilder(
      valueListenable: link.videoSize,
      builder: (context, size, child) =>
          AspectRatio(aspectRatio: from(size), child: child),
      child: child,
    );
  }
}

/// Starts watching [target] on [dictate]'s computer, in the small view. The
/// watched window is also where dictation and typing go next: [window], or
/// for a screen wherever the keyboard is.
void openWatch(
  BuildContext context,
  DictateController dictate,
  WatchTarget target, {
  RemoteWindow? window,
}) {
  final computer = dictate.computer;
  final live = LiveScope.maybeOf(context);
  if (computer == null || live == null) return;
  Haptic.tap();
  dictate.chooseWindow(window);
  live.start(computer, target);
}

class _Picture extends StatelessWidget {
  const _Picture({required this.watch});

  final WatchController watch;

  @override
  Widget build(BuildContext context) {
    final link = watch.link;
    if (link == null || !watch.foreground) {
      return const ColoredBox(color: Color(0xFF0B0C18));
    }
    return ColoredBox(color: Colors.black, child: link.buildVideo());
  }
}

/// The picture's top edge: state and title; its buttons.
class _Overlay extends StatelessWidget {
  const _Overlay({required this.live, required this.watch});

  final LiveController live;
  final WatchController watch;

  @override
  Widget build(BuildContext context) {
    final w = watch;
    final isLive = w.phase == WatchPhase.live && !w.reconnecting;
    return Column(
      children: [
        DecoratedBox(
          decoration: const BoxDecoration(
            gradient: LinearGradient(
              begin: Alignment.topCenter,
              end: Alignment.bottomCenter,
              colors: [Color(0xB3000000), Color(0x00000000)],
            ),
          ),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(Space.md, Space.xs, 0, Space.lg),
            child: Row(
              children: [
                _LiveBadge(text: watchStatus(w), live: isLive),
                const SizedBox(width: Space.sm),
                Expanded(
                  child: Text(
                    w.title,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: context.text.labelMedium?.copyWith(
                      color: Colors.white.withValues(alpha: 0.9),
                    ),
                  ),
                ),
                IconButton(
                  tooltip: ScreenStrings.collapse,
                  visualDensity: VisualDensity.compact,
                  onPressed: () => live.setExpanded(false),
                  icon: const Icon(Icons.expand_less_rounded),
                ),
                IconButton(
                  tooltip: ScreenStrings.stopWatching,
                  visualDensity: VisualDensity.compact,
                  onPressed: () {
                    Haptic.tick();
                    live.close();
                  },
                  icon: const Icon(Icons.close_rounded),
                ),
              ],
            ),
          ),
        ),
        const Spacer(),
        if (w.phase != WatchPhase.ended)
          Padding(
            padding: const EdgeInsets.all(Space.xs),
            child: Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                _GlassButton(
                  tooltip: w.soundOn
                      ? ScreenStrings.soundOff
                      : ScreenStrings.soundOn,
                  icon: w.soundOn
                      ? Icons.volume_up_rounded
                      : Icons.volume_off_rounded,
                  onPressed: () => w.setSound(!w.soundOn),
                ),
                const SizedBox(width: Space.xs),
                _GlassButton(
                  tooltip: ScreenStrings.fullScreen,
                  icon: Icons.open_in_full_rounded,
                  onPressed: () => unawaited(openFullScreen(context, live)),
                ),
              ],
            ),
          ),
      ],
    );
  }
}

class _GlassButton extends StatelessWidget {
  const _GlassButton({
    required this.tooltip,
    required this.icon,
    required this.onPressed,
  });

  final String tooltip;
  final IconData icon;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) => IconButton(
    tooltip: tooltip,
    onPressed: onPressed,
    style: IconButton.styleFrom(
      backgroundColor: Colors.black.withValues(alpha: 0.45),
      foregroundColor: Colors.white,
      minimumSize: const Size.square(40),
      fixedSize: const Size.square(40),
    ),
    iconSize: 20,
    icon: Icon(icon),
  );
}

class _LiveBadge extends StatelessWidget {
  const _LiveBadge({required this.text, required this.live});

  final String text;
  final bool live;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Semantics(
      liveRegion: true,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: live ? t.live : Colors.white.withValues(alpha: 0.16),
          borderRadius: BorderRadius.circular(Radii.pill),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (live) ...[
                const StatusDot(color: Colors.white, size: 6),
                const SizedBox(width: 5),
              ],
              Text(
                live ? text.toUpperCase() : text,
                style: context.text.labelSmall?.copyWith(
                  color: Colors.white,
                  letterSpacing: live ? 0.8 : 0,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _Waiting extends StatelessWidget {
  const _Waiting({required this.watch});

  final WatchController watch;

  @override
  Widget build(BuildContext context) {
    final approval = watch.phase == WatchPhase.approval;
    return IgnorePointer(
      child: Center(
        child: Padding(
          padding: const EdgeInsets.all(Space.lg),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const SizedBox.square(
                dimension: 26,
                child: CircularProgressIndicator(
                  strokeWidth: 2.5,
                  color: Colors.white,
                ),
              ),
              const SizedBox(height: Space.md),
              Text(
                approval ? ScreenStrings.approval : watchStatus(watch),
                textAlign: TextAlign.center,
                style: context.text.titleSmall?.copyWith(color: Colors.white),
              ),
              if (approval)
                Text(
                  ScreenStrings.approvalDetail,
                  textAlign: TextAlign.center,
                  style: context.text.bodySmall?.copyWith(
                    color: Colors.white.withValues(alpha: 0.7),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

class _Ended extends StatelessWidget {
  const _Ended({required this.live, required this.watch});

  final LiveController live;
  final WatchController watch;

  @override
  Widget build(BuildContext context) {
    final error = watch.endError;
    final name = watch.computer.name;
    return ColoredBox(
      color: const Color(0xE60B0C18),
      child: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(Space.lg),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Semantics(
                liveRegion: true,
                child: Text(
                  error == null
                      ? ScreenStrings.stoppedByYou
                      : ScreenStrings.error(error, name),
                  textAlign: TextAlign.center,
                  style: context.text.bodyMedium?.copyWith(color: Colors.white),
                ),
              ),
              const SizedBox(height: Space.sm),
              Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  TextButton(
                    onPressed: live.close,
                    style: TextButton.styleFrom(
                      foregroundColor: Colors.white70,
                    ),
                    child: const Text(ScreenStrings.close),
                  ),
                  if (error != 'target_gone')
                    FilledButton(
                      onPressed: () {
                        Haptic.tap();
                        watch.start();
                      },
                      style: FilledButton.styleFrom(
                        minimumSize: const Size(64, 40),
                      ),
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
}

/// Collapsed: one line with the state, sound, bigger and Stop. In full
/// screen it stands in for the picture.
class _Bar extends StatelessWidget {
  const _Bar({required this.live, required this.watch});

  final LiveController live;
  final WatchController watch;

  @override
  Widget build(BuildContext context) {
    final w = watch;
    final isLive = w.phase == WatchPhase.live && !w.reconnecting;
    return SizedBox(
      height: 56,
      child: Row(
        children: [
          const SizedBox(width: Space.md),
          _LiveBadge(text: watchStatus(w), live: isLive),
          const SizedBox(width: Space.sm),
          Expanded(
            child: Text(
              live.fullScreen ? ScreenStrings.inFullScreen : w.title,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: context.text.labelLarge?.copyWith(color: Colors.white),
            ),
          ),
          if (!live.fullScreen && w.phase != WatchPhase.ended)
            IconButton(
              tooltip: w.soundOn
                  ? ScreenStrings.soundOff
                  : ScreenStrings.soundOn,
              onPressed: () => w.setSound(!w.soundOn),
              icon: Icon(
                w.soundOn ? Icons.volume_up_rounded : Icons.volume_off_rounded,
              ),
            ),
          if (!live.fullScreen)
            IconButton(
              tooltip: ScreenStrings.expand,
              onPressed: () => live.setExpanded(true),
              icon: const Icon(Icons.expand_more_rounded),
            ),
          IconButton(
            tooltip: ScreenStrings.stopWatching,
            onPressed: () {
              Haptic.tick();
              live.close();
            },
            icon: const Icon(Icons.close_rounded),
          ),
          const SizedBox(width: Space.xs),
        ],
      ),
    );
  }
}
