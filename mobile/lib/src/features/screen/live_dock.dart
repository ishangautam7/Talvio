// The small live view at the top of every page while a watch runs (design
// spec §3.14): the picture with its state, sound, full screen and close.
// Collapsed it is one line.

import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import '../../design/components.dart';
import '../../design/tokens.dart';
import '../assistant/assistant_scope.dart';
import '../dictate/dictate_controller.dart';
import '../dictate/remote_windows.dart';
import 'live_controller.dart';
import 'live_protocol.dart';
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
          duration: Motion.emphasized,
          curve: Motion.emphasizedCurve,
          alignment: Alignment.topCenter,
          child: watch == null
              ? const SizedBox(width: double.infinity)
              : Padding(
                  padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
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
    final showPicture = live.expanded && !live.fullScreen;
    final height = MediaQuery.sizeOf(context).height;
    return Semantics(
      container: true,
      label: ScreenStrings.watchItem(w.title),
      child: DecoratedBox(
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(Radii.lg),
          boxShadow: showPicture
              ? Depth.raised(context.talvio)
              : Depth.card(context.talvio),
        ),
        child: ClipRRect(
          borderRadius: BorderRadius.circular(Radii.lg),
          child: showPicture
              ? ColoredBox(
                  color: TalvioColors.mediaBg,
                  child: ConstrainedBox(
                    constraints: BoxConstraints(
                      maxHeight: (height * 0.32).clamp(0, 280),
                    ),
                    child: Center(
                      heightFactor: 1,
                      child: _Aspect(
                        watch: w,
                        child: _Picture(live: live, watch: w),
                      ),
                    ),
                  ),
                )
              : _Bar(live: live, watch: w),
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

/// The picture, its state, and the controls over it, which hide after a
/// few seconds without a touch and come back on a tap.
class _Picture extends StatefulWidget {
  const _Picture({required this.live, required this.watch});

  final LiveController live;
  final WatchController watch;

  @override
  State<_Picture> createState() => _PictureState();
}

class _PictureState extends State<_Picture>
    with SingleTickerProviderStateMixin {
  var _shown = true;

  /// Counts down the 3 s the controls stay without a touch.
  late final _idle = AnimationController(
    vsync: this,
    duration: const Duration(seconds: 3),
  )..addStatusListener(_onIdle);

  @override
  void initState() {
    super.initState();
    _restart();
  }

  @override
  void dispose() {
    _idle.dispose();
    super.dispose();
  }

  void _onIdle(AnimationStatus status) {
    final live = widget.watch.phase == WatchPhase.live;
    if (status == AnimationStatus.completed && live && mounted) {
      setState(() => _shown = false);
    }
  }

  /// Shows the controls and starts their countdown again.
  void _restart() {
    if (!_shown) setState(() => _shown = true);
    // Tests and reduced motion keep them up.
    if (ambientMotion) unawaited(_idle.forward(from: 0));
  }

  void _toggle() {
    if (_shown) {
      _idle.stop();
      setState(() => _shown = false);
    } else {
      _restart();
    }
  }

  @override
  Widget build(BuildContext context) {
    final w = widget.watch;
    final live = widget.live;
    final link = w.link;
    final ended = w.phase == WatchPhase.ended;
    final waiting = !ended && w.phase != WatchPhase.live;
    final shown = _shown || ended || waiting;
    return Stack(
      fit: StackFit.expand,
      children: [
        GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: _toggle,
          child: link == null || !w.foreground
              ? const ColoredBox(color: TalvioColors.mediaBg)
              : ColoredBox(
                  color: TalvioColors.mediaBg,
                  child: link.buildVideo(),
                ),
        ),
        if (ended)
          _Ended(live: live, watch: w)
        else if (waiting)
          _Waiting(watch: w),
        IgnorePointer(
          ignoring: !shown,
          child: AnimatedOpacity(
            duration: Motion.quick,
            opacity: shown ? 1 : 0,
            child: Listener(
              onPointerDown: (_) => _restart(),
              behavior: HitTestBehavior.translucent,
              child: _Overlay(live: live, watch: w),
            ),
          ),
        ),
      ],
    );
  }
}

/// The controls over the picture: the state and title at the top with
/// collapse and close; sound and full screen at the bottom.
class _Overlay extends StatelessWidget {
  const _Overlay({required this.live, required this.watch});

  final LiveController live;
  final WatchController watch;

  @override
  Widget build(BuildContext context) {
    final w = watch;
    final isLive = w.phase == WatchPhase.live && !w.reconnecting;
    return Stack(
      fit: StackFit.expand,
      children: [
        const Positioned(
          left: 0,
          right: 0,
          top: 0,
          height: 56,
          child: IgnorePointer(
            child: DecoratedBox(
              decoration: BoxDecoration(
                gradient: LinearGradient(
                  begin: Alignment.topCenter,
                  end: Alignment.bottomCenter,
                  colors: [Color(0x80000000), Color(0x00000000)],
                ),
              ),
            ),
          ),
        ),
        PositionedDirectional(
          start: 12,
          top: 10,
          end: 88,
          child: Row(
            children: [
              LiveBadge(text: watchStatus(w), live: isLive),
              const SizedBox(width: Space.s8),
              Expanded(
                child: Text(
                  w.title,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TalvioType.label.copyWith(color: Colors.white),
                ),
              ),
            ],
          ),
        ),
        PositionedDirectional(
          end: 4,
          top: 0,
          child: Row(
            children: [
              GlassButton(
                tooltip: ScreenStrings.collapse,
                icon: AppIcons.caretUp,
                onPressed: () => live.setExpanded(false),
              ),
              GlassButton(
                tooltip: ScreenStrings.stopWatching,
                icon: AppIcons.close,
                onPressed: () {
                  Haptic.tick();
                  live.close();
                },
              ),
            ],
          ),
        ),
        if (w.phase != WatchPhase.ended)
          PositionedDirectional(
            end: 4,
            bottom: 0,
            child: Row(
              children: [
                GlassButton(
                  tooltip: w.soundOn
                      ? ScreenStrings.soundOff
                      : ScreenStrings.soundOn,
                  icon: w.soundOn ? AppIcons.soundOn : AppIcons.soundOff,
                  onPressed: () => w.setSound(!w.soundOn),
                ),
                GlassButton(
                  tooltip: ScreenStrings.fullScreen,
                  icon: AppIcons.fullScreen,
                  onPressed: () => unawaited(openFullScreen(context, live)),
                ),
              ],
            ),
          ),
      ],
    );
  }
}

/// A 32 glass circle over video, in a 44 tap target.
class GlassButton extends StatelessWidget {
  const GlassButton({
    super.key,
    required this.tooltip,
    required this.icon,
    required this.onPressed,
  });

  final String tooltip;
  final IconData icon;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) => Semantics(
    button: true,
    label: tooltip,
    enabled: onPressed != null,
    onTap: onPressed,
    excludeSemantics: true,
    child: Tooltip(
      message: tooltip,
      excludeFromSemantics: true,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: onPressed,
        child: SizedBox.square(
          dimension: 44,
          child: Center(
            child: ClipOval(
              child: BackdropFilter(
                filter: ui.ImageFilter.blur(sigmaX: 12, sigmaY: 12),
                child: Container(
                  width: 32,
                  height: 32,
                  color: TalvioColors.glass,
                  alignment: Alignment.center,
                  child: Icon(icon, size: 20, color: Colors.white),
                ),
              ),
            ),
          ),
        ),
      ),
    ),
  );
}

/// "Live" on the live red, or the state on glass while it is not live yet.
class LiveBadge extends StatelessWidget {
  const LiveBadge({super.key, required this.text, required this.live});

  final String text;
  final bool live;

  @override
  Widget build(BuildContext context) => Semantics(
    liveRegion: true,
    child: live
        ? StatusPill(text, kind: PillKind.live)
        : Container(
            constraints: const BoxConstraints(minHeight: 22),
            padding: const EdgeInsets.symmetric(horizontal: Space.s8),
            decoration: BoxDecoration(
              color: TalvioColors.glass,
              borderRadius: BorderRadius.circular(Radii.pill),
            ),
            alignment: Alignment.center,
            child: Text(
              text,
              style: TalvioType.caption.copyWith(color: Colors.white),
            ),
          ),
  );
}

class _Waiting extends StatelessWidget {
  const _Waiting({required this.watch});

  final WatchController watch;

  @override
  Widget build(BuildContext context) {
    final approval = watch.phase == WatchPhase.approval;
    return IgnorePointer(
      child: ColoredBox(
        color: TalvioColors.mediaBg,
        child: Center(
          child: Padding(
            padding: const EdgeInsets.all(Space.s16),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                SizedBox.square(
                  dimension: 20,
                  child: approval
                      ? const Icon(
                          AppIcons.waiting,
                          size: 20,
                          color: Colors.white,
                        )
                      : const Spinner(color: Colors.white),
                ),
                const SizedBox(height: Space.s8),
                Text(
                  approval ? ScreenStrings.approval : watchStatus(watch),
                  textAlign: TextAlign.center,
                  style: TalvioType.rowTitle.copyWith(color: Colors.white),
                ),
                if (approval) ...[
                  const SizedBox(height: Space.s4),
                  Text(
                    ScreenStrings.approvalDetail,
                    textAlign: TextAlign.center,
                    style: TalvioType.footnote.copyWith(
                      color: Colors.white.withValues(alpha: 0.7),
                    ),
                  ),
                ],
              ],
            ),
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
      color: TalvioColors.mediaBg,
      child: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.fromLTRB(16, 44, 16, 12),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Semantics(
                liveRegion: true,
                child: Text(
                  ScreenStrings.ended,
                  textAlign: TextAlign.center,
                  style: TalvioType.rowTitle.copyWith(color: Colors.white),
                ),
              ),
              const SizedBox(height: Space.s4),
              Text(
                error == null
                    ? ScreenStrings.stoppedByYou
                    : ScreenStrings.error(error, name),
                textAlign: TextAlign.center,
                style: TalvioType.footnote.copyWith(
                  color: Colors.white.withValues(alpha: 0.7),
                ),
              ),
              const SizedBox(height: Space.s12),
              Wrap(
                alignment: WrapAlignment.center,
                spacing: Space.s8,
                children: [
                  Btn(
                    ScreenStrings.close,
                    kind: BtnKind.ghostNeutral,
                    compact: true,
                    foreground: Colors.white,
                    onPressed: live.close,
                  ),
                  if (error != 'target_gone')
                    Btn(
                      ScreenStrings.watchAgain,
                      kind: BtnKind.secondary,
                      compact: true,
                      foreground: Colors.white,
                      borderColor: Colors.white,
                      onPressed: () {
                        Haptic.tap();
                        watch.start();
                      },
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

/// Collapsed: one line with the state, sound, bigger and close. In full
/// screen it stands in for the picture.
class _Bar extends StatelessWidget {
  const _Bar({required this.live, required this.watch});

  final LiveController live;
  final WatchController watch;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final w = watch;
    final isLive = w.phase == WatchPhase.live && !w.reconnecting;
    return Container(
      color: t.card,
      constraints: const BoxConstraints(minHeight: 48),
      padding: const EdgeInsetsDirectional.only(start: 12, end: 4),
      child: Row(
        children: [
          StatusDot(color: isLive ? t.live : t.faint, size: 8, blink: isLive),
          const SizedBox(width: Space.s8),
          Expanded(
            child: Semantics(
              liveRegion: true,
              child: Text(
                live.fullScreen
                    ? ScreenStrings.inFullScreen
                    : isLive
                    ? w.title
                    : '${watchStatus(w)} · ${w.title}',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TalvioType.rowTitle.copyWith(color: t.ink),
              ),
            ),
          ),
          if (!live.fullScreen && w.phase != WatchPhase.ended)
            IconBtn(
              tooltip: w.soundOn
                  ? ScreenStrings.soundOff
                  : ScreenStrings.soundOn,
              onPressed: () => w.setSound(!w.soundOn),
              icon: w.soundOn ? AppIcons.soundOn : AppIcons.soundOff,
            ),
          if (!live.fullScreen)
            IconBtn(
              tooltip: ScreenStrings.expand,
              onPressed: () => live.setExpanded(true),
              icon: AppIcons.caretDown,
            ),
          IconBtn(
            tooltip: ScreenStrings.stopWatching,
            onPressed: () {
              Haptic.tick();
              live.close();
            },
            icon: AppIcons.close,
          ),
        ],
      ),
    );
  }
}
