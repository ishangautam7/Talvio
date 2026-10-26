// The building blocks every screen is made of (design spec §3): grouped
// lists and their rows, banners, buttons, chips, pills, empty states, the
// request card, and the few animations that say "working".

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'icons.dart';
import 'tokens.dart';

export 'icons.dart';

/// Looping decoration (live dots, the shimmer, blinking, the halo). Widget
/// tests turn it off so `pumpAndSettle` settles; spinners that say
/// "working" are separate and always run.
bool ambientMotion = true;

/// Movement (scale, slide) is allowed: the system has not asked for less.
bool motionAllowed(BuildContext context) =>
    !(MediaQuery.maybeDisableAnimationsOf(context) ?? false);

/// Looping decoration is allowed.
bool loopsAllowed(BuildContext context) =>
    ambientMotion && motionAllowed(context);

/// Haptics, in one place so every screen uses the same four.
abstract final class Haptic {
  /// A selection: a tab, a segment, a chip, a picker row.
  static void tick() => unawaited(HapticFeedback.selectionClick());

  /// Something starts or is sent: recording, a question, a suggestion.
  static void tap() => unawaited(HapticFeedback.lightImpact());

  /// Something firm: stop recording, Do it, Stop watching.
  static void firm() => unawaited(HapticFeedback.mediumImpact());

  /// A request ended in an error banner.
  static void error() => unawaited(HapticFeedback.heavyImpact());
}

/// The semantic tone of a banner, a status glyph or a pill.
enum Tone { neutral, accent, success, warning, danger }

extension ToneColors on Tone {
  /// The tone's glyph colour.
  Color fg(TalvioColors t) => switch (this) {
    Tone.neutral => t.muted,
    Tone.accent => t.accent,
    Tone.success => t.success,
    Tone.warning => t.warning,
    Tone.danger => t.danger,
  };

  /// The tone's soft fill (banners).
  Color bg(TalvioColors t) => switch (this) {
    Tone.neutral => t.isDark ? t.fill : t.card,
    Tone.accent => t.accentSoft,
    Tone.success => t.successSoft,
    Tone.warning => t.warningSoft,
    Tone.danger => t.dangerSoft,
  };
}

/// Shrinks its child a touch while pressed (0.97, 100 ms). The tap itself
/// is the child's.
class Pressable extends StatefulWidget {
  const Pressable({
    super.key,
    required this.child,
    this.enabled = true,
    this.scale = 0.97,
  });

  final Widget child;
  final bool enabled;
  final double scale;

  @override
  State<Pressable> createState() => _PressableState();
}

class _PressableState extends State<Pressable> {
  var _down = false;

  void _set(bool down) {
    if (widget.enabled && down != _down) setState(() => _down = down);
  }

  @override
  Widget build(BuildContext context) {
    if (!motionAllowed(context)) return widget.child;
    return Listener(
      onPointerDown: (_) => _set(true),
      onPointerUp: (_) => _set(false),
      onPointerCancel: (_) => _set(false),
      child: AnimatedScale(
        scale: _down ? widget.scale : 1,
        duration: Motion.press,
        curve: Motion.pressCurve,
        child: widget.child,
      ),
    );
  }
}

/// A card: the card colour on the canvas, rounded, no border, no shadow.
/// With [onTap] it is a button that dips a little when pressed.
class Surface extends StatelessWidget {
  const Surface({
    super.key,
    required this.child,
    this.padding = const EdgeInsets.all(Space.s16),
    this.onTap,
    this.color,
    this.borderColor,
    this.radius = Radii.lg,
  });

  final Widget child;
  final EdgeInsetsGeometry padding;
  final VoidCallback? onTap;
  final Color? color;

  /// None by default: cards separate from the canvas by fill alone.
  final Color? borderColor;
  final double radius;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final shape = RoundedRectangleBorder(
      borderRadius: BorderRadius.circular(radius),
      side: borderColor == null
          ? BorderSide.none
          : BorderSide(color: borderColor!),
    );
    final body = Padding(padding: padding, child: child);
    final material = Material(
      color: color ?? t.card,
      shape: shape,
      clipBehavior: Clip.antiAlias,
      child: onTap == null ? body : InkWell(onTap: onTap, child: body),
    );
    if (onTap == null) return material;
    return Pressable(child: material);
  }
}

/// Rows in one inset group (iOS Settings style): the card colour, radius
/// 16, separators inset to the rows' text.
class Group extends StatelessWidget {
  const Group({
    super.key,
    required this.children,
    this.dividerInset = 60,
    this.margin = const EdgeInsets.symmetric(horizontal: Space.gutter),
  });

  final List<Widget> children;

  /// Where separators start: 16 + the leading's 32 + 12 for rows with a
  /// leading, 16 for rows without.
  final double dividerInset;
  final EdgeInsetsGeometry margin;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Padding(
      padding: margin,
      child: Material(
        color: t.card,
        borderRadius: BorderRadius.circular(Radii.lg),
        clipBehavior: Clip.antiAlias,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            for (final (i, child) in children.indexed) ...[
              if (i > 0)
                Padding(
                  padding: EdgeInsetsDirectional.only(start: dividerInset),
                  child: Divider(height: 1, thickness: 1, color: t.hairline),
                ),
              child,
            ],
          ],
        ),
      ),
    );
  }
}

/// One row of a [Group] or a sheet: leading, title and subtitle, trailing.
class ListRow extends StatelessWidget {
  const ListRow({
    super.key,
    required this.title,
    this.subtitle,
    this.subtitleWidget,
    this.titleLeading,
    this.leading,
    this.trailing,
    this.onTap,
    this.enabled = true,
    this.titleStyle,
    this.subtitleStyle,
    this.titleMaxLines = 2,
    this.subtitleMaxLines = 2,
    this.semanticLabel,
    this.semanticValue,
    this.selected = false,
  });

  final String title;
  final String? subtitle;

  /// In place of [subtitle] (a size set in mono).
  final Widget? subtitleWidget;

  /// Before the title, on its line (the live dot).
  final Widget? titleLeading;
  final Widget? leading;
  final Widget? trailing;
  final VoidCallback? onTap;

  /// False dims the row to 38% and takes its tap away.
  final bool enabled;
  final TextStyle? titleStyle;
  final TextStyle? subtitleStyle;
  final int titleMaxLines;
  final int subtitleMaxLines;

  /// What a screen reader says instead of the title and subtitle.
  final String? semanticLabel;
  final String? semanticValue;
  final bool selected;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final trailing = this.trailing;
    // An icon button's glyph sits where a plain trailing glyph would.
    final endPad = trailing is IconBtn ? Space.s4 : Space.s16;
    final row = ConstrainedBox(
      constraints: BoxConstraints(
        minHeight: subtitle == null && subtitleWidget == null ? 52 : 64,
      ),
      child: Padding(
        padding: EdgeInsetsDirectional.fromSTEB(
          Space.s16,
          Space.s12,
          endPad,
          Space.s12,
        ),
        child: Row(
          children: [
            if (leading case final leading?) ...[
              SizedBox(width: 32, child: Center(child: leading)),
              const SizedBox(width: Space.s12),
            ],
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  Row(
                    children: [
                      if (titleLeading case final titleLeading?) ...[
                        titleLeading,
                        const SizedBox(width: 6),
                      ],
                      Flexible(
                        child: Text(
                          title,
                          maxLines: titleMaxLines,
                          overflow: TextOverflow.ellipsis,
                          style:
                              titleStyle ??
                              TalvioType.rowTitle.copyWith(color: t.ink),
                        ),
                      ),
                    ],
                  ),
                  if (subtitleWidget case final subtitleWidget?) ...[
                    const SizedBox(height: Space.s2),
                    subtitleWidget,
                  ] else if (subtitle case final subtitle?) ...[
                    const SizedBox(height: Space.s2),
                    Text(
                      subtitle,
                      maxLines: subtitleMaxLines,
                      overflow: TextOverflow.ellipsis,
                      style:
                          subtitleStyle ??
                          TalvioType.footnote.copyWith(color: t.muted),
                    ),
                  ],
                ],
              ),
            ),
            if (trailing != null) ...[
              const SizedBox(width: Space.s12),
              trailing,
            ],
          ],
        ),
      ),
    );
    final tappable = onTap != null && enabled;
    Widget body = tappable
        ? InkWell(onTap: onTap, highlightColor: t.pressed, child: row)
        : row;
    if (semanticLabel != null) {
      body = Semantics(
        button: onTap != null,
        enabled: tappable,
        selected: selected,
        label: semanticLabel,
        value: semanticValue,
        onTap: tappable ? onTap : null,
        excludeSemantics: true,
        child: body,
      );
    }
    return Opacity(opacity: enabled ? 1 : 0.38, child: body);
  }
}

/// A row's leading glyph: 20, muted (or a status colour).
class RowGlyph extends StatelessWidget {
  const RowGlyph(this.icon, {super.key, this.color});

  final IconData icon;
  final Color? color;

  @override
  Widget build(BuildContext context) =>
      Icon(icon, size: IconSize.regular, color: color ?? context.talvio.muted);
}

/// A 32 tile on the fill colour with an ink glyph (devices, screens), and
/// an optional presence dot at its bottom-end corner.
class RowTile extends StatelessWidget {
  const RowTile(this.icon, {super.key, this.presence});

  final IconData icon;

  /// The presence dot's colour; null for none.
  final Color? presence;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final tile = Container(
      width: 32,
      height: 32,
      decoration: BoxDecoration(
        color: t.fill,
        borderRadius: BorderRadius.circular(Radii.sm),
      ),
      alignment: Alignment.center,
      child: Icon(icon, size: IconSize.regular, color: t.ink),
    );
    if (presence == null) return tile;
    return SizedBox.square(
      dimension: 36,
      child: Stack(
        children: [
          tile,
          PositionedDirectional(
            end: 0,
            bottom: 0,
            child: Container(
              width: 14,
              height: 14,
              decoration: BoxDecoration(
                color: presence,
                shape: BoxShape.circle,
                border: Border.all(color: t.card, width: 2),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// An app's first letter on a neutral 32 tile.
class Monogram extends StatelessWidget {
  const Monogram(this.name, {super.key});

  final String name;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final letter = name.trim().isEmpty
        ? '?'
        : name.trim().characters.first.toUpperCase();
    return Container(
      width: 32,
      height: 32,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        color: t.fill,
        borderRadius: BorderRadius.circular(Radii.sm),
      ),
      child: Text(
        letter,
        style: TalvioType.label.copyWith(
          color: t.muted,
          fontWeight: FontWeight.w600,
        ),
      ),
    );
  }
}

/// A group's label: 13/500 muted, aligned with the rows' text.
class SectionLabel extends StatelessWidget {
  const SectionLabel(
    this.title, {
    super.key,
    this.trailing,
    this.first = false,
    this.inset = 32,
  });

  final String title;

  /// An icon button at the group's end (Refresh).
  final Widget? trailing;

  /// From the screen's edge: 32 over a group, 16 in a sheet.
  final double inset;

  /// The first label on a page: 16 above instead of 24.
  final bool first;

  @override
  Widget build(BuildContext context) {
    final top = first ? Space.s16 : Space.s24;
    final label = Semantics(
      header: true,
      child: Text(
        title,
        style: TalvioType.label.copyWith(color: context.talvio.muted),
      ),
    );
    if (trailing == null) {
      return Padding(
        padding: EdgeInsetsDirectional.fromSTEB(inset, top, inset, Space.s8),
        child: label,
      );
    }
    return Padding(
      padding: EdgeInsetsDirectional.fromSTEB(inset, top - 12, inset - 12, 0),
      child: Row(
        children: [
          Expanded(child: label),
          trailing!,
        ],
      ),
    );
  }
}

/// A note under a group.
class GroupFooter extends StatelessWidget {
  const GroupFooter(this.text, {super.key});

  final String text;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsetsDirectional.fromSTEB(32, Space.s8, 32, 0),
    child: Text(
      text,
      style: TalvioType.footnote.copyWith(color: context.talvio.muted),
    ),
  );
}

/// The 44×44 icon button: a 20 glyph, a 36 circle while pressed, 38%
/// opacity when it cannot be pressed.
class IconBtn extends StatefulWidget {
  const IconBtn({
    super.key,
    required this.icon,
    required this.tooltip,
    required this.onPressed,
    this.color,
    this.size = IconSize.regular,
    this.selected = false,
    this.semanticValue,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback? onPressed;
  final Color? color;
  final double size;
  final bool selected;
  final String? semanticValue;

  @override
  State<IconBtn> createState() => _IconBtnState();
}

class _IconBtnState extends State<IconBtn> {
  var _down = false;

  void _set(bool down) {
    if (widget.onPressed != null && down != _down) {
      setState(() => _down = down);
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final enabled = widget.onPressed != null;
    final glyph = Icon(
      widget.icon,
      size: widget.size,
      color: widget.color ?? t.ink,
    );
    return Semantics(
      button: true,
      enabled: enabled,
      selected: widget.selected,
      label: widget.tooltip,
      value: widget.semanticValue,
      onTap: widget.onPressed,
      excludeSemantics: true,
      child: Tooltip(
        message: widget.tooltip,
        excludeFromSemantics: true,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTapDown: (_) => _set(true),
          onTapUp: (_) => _set(false),
          onTapCancel: () => _set(false),
          onTap: widget.onPressed,
          child: SizedBox.square(
            dimension: 44,
            child: Center(
              child: AnimatedContainer(
                duration: Motion.press,
                width: 36,
                height: 36,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: _down ? t.fill : Colors.transparent,
                ),
                alignment: Alignment.center,
                child: Opacity(opacity: enabled ? 1 : 0.38, child: glyph),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// The button variants of §3.7.
enum BtnKind {
  /// The one accent-filled action of a view.
  primary,

  /// Transparent with a border.
  secondary,

  /// A neutral fill, inside cards.
  tonal,

  /// Text only, in the accent.
  ghost,

  /// Text only, in ink.
  ghostNeutral,

  /// Text only, in the danger colour.
  ghostDanger,

  /// Filled with the danger colour.
  destructive,
}

/// A button of [kind]: pill-shaped, 48 (40 compact) tall, a spinner while
/// [loading], dips a little when pressed.
class Btn extends StatelessWidget {
  const Btn(
    this.label, {
    super.key,
    required this.onPressed,
    this.kind = BtnKind.primary,
    this.icon,
    this.leading,
    this.compact = false,
    this.loading = false,
    this.expand = false,
    this.foreground,
    this.borderColor,
    this.height,
  });

  final String label;
  final VoidCallback? onPressed;
  final BtnKind kind;

  /// Overrides the variant's height (44 in cards).
  final double? height;
  final IconData? icon;

  /// In place of [icon] (the Google mark).
  final Widget? leading;
  final bool compact;
  final bool loading;

  /// Fills the width it is given.
  final bool expand;

  /// Overrides the label colour (white on the live view).
  final Color? foreground;
  final Color? borderColor;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final height =
        this.height ??
        switch (kind) {
          BtnKind.tonal ||
          BtnKind.ghost ||
          BtnKind.ghostNeutral => compact ? 36.0 : 44.0,
          BtnKind.ghostDanger => compact ? 36.0 : 44.0,
          _ => compact ? 40.0 : 48.0,
        };
    final padX = switch (kind) {
      BtnKind.tonal => 16.0,
      BtnKind.ghost || BtnKind.ghostNeutral || BtnKind.ghostDanger => 12.0,
      _ => compact ? 16.0 : 20.0,
    };
    final textStyle = compact
        ? TalvioType.label.copyWith(fontWeight: FontWeight.w600)
        : TalvioType.button;
    final fg = foreground;
    final base = ButtonStyle(
      minimumSize: WidgetStatePropertyAll(
        Size(expand ? double.infinity : 64, height),
      ),
      padding: WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: padX)),
      textStyle: WidgetStatePropertyAll(textStyle),
      tapTargetSize: MaterialTapTargetSize.padded,
    );
    ButtonStyle filled(Color bg, Color on, {Color? pressed}) => base.copyWith(
      backgroundColor: WidgetStateProperty.resolveWith(
        (s) => s.contains(WidgetState.disabled) && !loading ? t.fill : bg,
      ),
      foregroundColor: WidgetStateProperty.resolveWith(
        (s) => s.contains(WidgetState.disabled) && !loading
            ? t.muted.withValues(alpha: 0.6)
            : on,
      ),
      overlayColor: WidgetStateProperty.resolveWith(
        (s) => s.contains(WidgetState.pressed) ? (pressed ?? t.pressed) : null,
      ),
    );
    ButtonStyle text(Color color) => base.copyWith(
      foregroundColor: WidgetStateProperty.resolveWith(
        (s) => s.contains(WidgetState.disabled)
            ? color.withValues(alpha: 0.38)
            : color,
      ),
    );
    final spinnerColor = switch (kind) {
      BtnKind.primary => t.onAccent,
      BtnKind.destructive => t.onDanger,
      BtnKind.ghost => t.accent,
      BtnKind.ghostDanger => t.danger,
      _ => fg ?? t.ink,
    };
    final Widget content = loading
        ? ButtonSpinner(color: spinnerColor)
        : Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (leading case final leading?) ...[
                leading,
                const SizedBox(width: Space.s8),
              ] else if (icon case final icon?) ...[
                Icon(icon, size: compact ? 16 : 20),
                const SizedBox(width: Space.s8),
              ],
              Flexible(
                child: Text(
                  label,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  textAlign: TextAlign.center,
                ),
              ),
            ],
          );
    // A loading button keeps its look and ignores taps.
    final press = loading ? () {} : onPressed;
    final Widget button = switch (kind) {
      BtnKind.primary => FilledButton(
        onPressed: press,
        style: filled(t.accent, t.onAccent, pressed: t.pressedOnAccent),
        child: content,
      ),
      BtnKind.destructive => FilledButton(
        onPressed: press,
        style: filled(t.danger, t.onDanger, pressed: t.pressedOnAccent),
        child: content,
      ),
      BtnKind.tonal => FilledButton(
        onPressed: press,
        style: filled(t.fill, fg ?? t.ink, pressed: t.pressed),
        child: content,
      ),
      BtnKind.secondary => OutlinedButton(
        onPressed: press,
        style: base.copyWith(
          foregroundColor: WidgetStateProperty.resolveWith(
            (s) => s.contains(WidgetState.disabled)
                ? (fg ?? t.ink).withValues(alpha: 0.38)
                : fg ?? t.ink,
          ),
          side: WidgetStateProperty.resolveWith(
            (s) => BorderSide(
              color: (borderColor ?? t.border).withValues(
                alpha: s.contains(WidgetState.disabled) ? 0.5 : 1,
              ),
            ),
          ),
        ),
        child: content,
      ),
      BtnKind.ghost => TextButton(
        onPressed: press,
        style: text(fg ?? t.accent),
        child: content,
      ),
      BtnKind.ghostNeutral => TextButton(
        onPressed: press,
        style: text(fg ?? t.ink),
        child: content,
      ),
      BtnKind.ghostDanger => TextButton(
        onPressed: press,
        style: text(t.danger),
        child: content,
      ),
    };
    return Pressable(enabled: onPressed != null && !loading, child: button);
  }
}

/// A small round dot: online, offline, connecting, live.
class StatusDot extends StatelessWidget {
  const StatusDot({
    super.key,
    required this.color,
    this.size = 6,
    this.pulse = false,
    this.blink = false,
  });

  final Color color;
  final double size;

  /// A soft ring that breathes around the dot (connecting).
  final bool pulse;

  /// The dot itself fades 1 ↔ 0.5 (live).
  final bool blink;

  @override
  Widget build(BuildContext context) {
    final dot = Container(
      width: size,
      height: size,
      decoration: BoxDecoration(color: color, shape: BoxShape.circle),
    );
    if (blink) {
      return Breathing(
        period: const Duration(milliseconds: 1200),
        builder: (v) => Opacity(opacity: 1 - 0.5 * v, child: dot),
      );
    }
    if (!pulse) return dot;
    return Breathing(
      builder: (v) => SizedBox.square(
        dimension: size * 2.2,
        child: Stack(
          alignment: Alignment.center,
          children: [
            Container(
              width: size * (1 + 1.2 * v),
              height: size * (1 + 1.2 * v),
              decoration: BoxDecoration(
                color: color.withValues(alpha: 0.35 * (1 - v)),
                shape: BoxShape.circle,
              ),
            ),
            dot,
          ],
        ),
      ),
    );
  }
}

/// What a pill says about its subject.
enum PillKind { neutral, live, approval, offline }

/// A short label on a 22 pill, sentence case.
class StatusPill extends StatelessWidget {
  const StatusPill(this.label, {super.key, this.kind = PillKind.neutral});

  final String label;
  final PillKind kind;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final (Color bg, Color fg) = switch (kind) {
      PillKind.neutral || PillKind.offline => (t.fill, t.muted),
      PillKind.live => (t.live, Colors.white),
      PillKind.approval => (t.warningSoft, t.warning),
    };
    return Container(
      constraints: const BoxConstraints(minHeight: 22),
      padding: const EdgeInsets.symmetric(horizontal: Space.s8),
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(Radii.pill),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (kind == PillKind.live) ...[
            StatusDot(color: Colors.white, blink: true),
            const SizedBox(width: Space.s4),
          ] else if (kind == PillKind.offline) ...[
            StatusDot(color: t.faint),
            const SizedBox(width: Space.s4),
          ],
          Flexible(
            child: Text(
              label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TalvioType.caption.copyWith(color: fg),
            ),
          ),
        ],
      ),
    );
  }
}

/// A message in the flow of a page: errors, offline computers, notes. A
/// glyph in the tone's colour, the text in ink, an optional action.
class InlineBanner extends StatelessWidget {
  const InlineBanner({
    super.key,
    required this.message,
    this.tone = Tone.neutral,
    this.icon,
    this.action,
    this.onClose,
    this.closeLabel = 'Dismiss',
    this.busy = false,
    this.live = true,
    this.margin = const EdgeInsets.symmetric(horizontal: Space.gutter),
  });

  final String message;
  final Tone tone;
  final IconData? icon;

  /// A trailing compact button ("Try again").
  final Widget? action;

  /// A close button at the end.
  final VoidCallback? onClose;
  final String closeLabel;

  /// A 14 spinner in place of the icon.
  final bool busy;

  /// Read out by screen readers when it appears.
  final bool live;
  final EdgeInsetsGeometry margin;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final fg = tone.fg(t);
    return Padding(
      padding: margin,
      child: Container(
        constraints: const BoxConstraints(minHeight: 44),
        padding: EdgeInsetsDirectional.fromSTEB(
          Space.s12,
          action == null && onClose == null ? Space.s12 : Space.s4,
          Space.s8,
          action == null && onClose == null ? Space.s12 : Space.s4,
        ),
        decoration: BoxDecoration(
          color: tone.bg(t),
          borderRadius: BorderRadius.circular(Radii.md),
        ),
        child: LayoutBuilder(
          builder: (context, constraints) {
            // A long message, or a narrow screen, puts the action under it.
            final stacked =
                action != null &&
                (constraints.maxWidth < 340 || message.length > 70);
            final glyph = SizedBox.square(
              dimension: 20,
              child: busy
                  ? Center(child: Spinner(size: 14, color: fg))
                  : Icon(icon ?? _icon, size: IconSize.regular, color: fg),
            );
            final text = Semantics(
              liveRegion: live,
              child: Text(
                message,
                style: TalvioType.rowBody.copyWith(color: t.ink),
              ),
            );
            final close = onClose == null
                ? null
                : IconBtn(
                    icon: AppIcons.close,
                    tooltip: closeLabel,
                    color: t.muted,
                    onPressed: onClose,
                  );
            if (stacked) {
              return Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Padding(padding: const EdgeInsets.only(top: 8), child: glyph),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Padding(
                          padding: EdgeInsetsDirectional.only(
                            top: 8,
                            end: close == null ? 4 : 0,
                          ),
                          child: text,
                        ),
                        Transform.translate(
                          offset: Offset(
                            Directionality.of(context) == TextDirection.rtl
                                ? 12
                                : -12,
                            0,
                          ),
                          child: action,
                        ),
                      ],
                    ),
                  ),
                  ?close,
                ],
              );
            }
            return Row(
              children: [
                glyph,
                const SizedBox(width: 10),
                Expanded(child: text),
                if (action != null) ...[
                  const SizedBox(width: Space.s4),
                  action!,
                ],
                ?close,
              ],
            );
          },
        ),
      ),
    );
  }

  IconData get _icon => switch (tone) {
    Tone.danger => AppIcons.error,
    Tone.warning => AppIcons.warning,
    Tone.success => AppIcons.done,
    _ => AppIcons.info,
  };
}

/// A page with nothing to show yet: a 40 duotone glyph, a heading, a line
/// or two, and what to do about it.
class EmptyState extends StatelessWidget {
  const EmptyState({
    super.key,
    required this.icon,
    required this.title,
    this.body,
    this.action,
    this.busy = false,
    this.child,
  });

  final DuoIcon icon;
  final String title;
  final String? body;
  final Widget? action;

  /// Waiting rather than empty: a spinner in place of the glyph.
  final bool busy;

  /// Under the body (numbered steps).
  final Widget? child;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.symmetric(horizontal: 32, vertical: 24),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 320),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              SizedBox.square(
                dimension: IconSize.empty,
                child: busy
                    ? Center(child: Spinner(size: 24, color: t.muted))
                    : DuoGlyph(icon, size: IconSize.empty, color: t.muted),
              ),
              const SizedBox(height: Space.s16),
              Semantics(
                liveRegion: true,
                child: Text(
                  title,
                  textAlign: TextAlign.center,
                  style: TalvioType.title1.copyWith(color: t.ink),
                ),
              ),
              if (body case final body?) ...[
                const SizedBox(height: Space.s8),
                Text(
                  body,
                  textAlign: TextAlign.center,
                  maxLines: 4,
                  overflow: TextOverflow.ellipsis,
                  style: TalvioType.rowBody.copyWith(color: t.muted),
                ),
              ],
              ?child,
              if (action case final action?) ...[
                const SizedBox(height: Space.s24),
                action,
              ],
            ],
          ),
        ),
      ),
    );
  }
}

/// A circular spinner of [size] (stroke 2).
class Spinner extends StatelessWidget {
  const Spinner({super.key, this.size = 20, this.color, this.semanticLabel});

  final double size;
  final Color? color;
  final String? semanticLabel;

  @override
  Widget build(BuildContext context) => SizedBox.square(
    dimension: size,
    child: _Turning(
      child: CircularProgressIndicator(
        value: 0.7,
        strokeWidth: 2,
        strokeCap: StrokeCap.round,
        color: color ?? context.talvio.accent,
        semanticsLabel: semanticLabel,
      ),
    ),
  );
}

/// The progress indicator inside a button whose action is running.
class ButtonSpinner extends StatelessWidget {
  const ButtonSpinner({super.key, this.color});

  final Color? color;

  @override
  Widget build(BuildContext context) => SizedBox.square(
    dimension: 18,
    child: _Turning(
      child: CircularProgressIndicator(
        value: 0.7,
        strokeWidth: 2,
        strokeCap: StrokeCap.round,
        color: color,
      ),
    ),
  );
}

/// Turns its child once a second: a spinner that reads as one in every
/// frame (Material's indeterminate arc shrinks to a dash at times).
class _Turning extends StatefulWidget {
  const _Turning({required this.child});

  final Widget child;

  @override
  State<_Turning> createState() => _TurningState();
}

class _TurningState extends State<_Turning>
    with SingleTickerProviderStateMixin {
  late final _c = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 900),
  )..repeat();

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) =>
      RotationTransition(turns: _c, child: widget.child);
}

/// A request in flight (§3.11): a spinner, or a raised hand while the
/// computer asks its user to allow this phone; what is happening; Cancel.
class RequestCard extends StatefulWidget {
  const RequestCard({
    super.key,
    required this.title,
    this.detail,
    this.approval = false,
    this.onCancel,
    this.cancelLabel = 'Cancel',
    this.margin = const EdgeInsets.symmetric(horizontal: Space.gutter),
  });

  final String title;
  final String? detail;

  /// Waiting for the user at the computer to allow this phone.
  final bool approval;
  final VoidCallback? onCancel;
  final String cancelLabel;
  final EdgeInsetsGeometry margin;

  @override
  State<RequestCard> createState() => _RequestCardState();
}

class _RequestCardState extends State<RequestCard> {
  @override
  void initState() {
    super.initState();
    Haptic.tick();
  }

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Padding(
      padding: widget.margin,
      child: Container(
        padding: const EdgeInsetsDirectional.fromSTEB(16, 12, 4, 12),
        constraints: const BoxConstraints(minHeight: 64),
        decoration: BoxDecoration(
          color: t.card,
          borderRadius: BorderRadius.circular(Radii.lg),
        ),
        child: Row(
          children: [
            SizedBox.square(
              dimension: 20,
              child: widget.approval
                  ? Icon(AppIcons.waiting, size: 20, color: t.warning)
                  : const Spinner(),
            ),
            const SizedBox(width: Space.s12),
            Expanded(
              child: Semantics(
                liveRegion: true,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    AnimatedSwitcher(
                      duration: Motion.quick,
                      child: Text(
                        widget.title,
                        key: ValueKey(widget.title),
                        style: TalvioType.rowTitle.copyWith(color: t.ink),
                      ),
                    ),
                    if (widget.detail case final detail?) ...[
                      const SizedBox(height: Space.s2),
                      Text(
                        detail,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: TalvioType.footnote.copyWith(color: t.muted),
                      ),
                    ],
                  ],
                ),
              ),
            ),
            const SizedBox(width: Space.s12),
            Btn(
              widget.cancelLabel,
              kind: BtnKind.ghostNeutral,
              compact: true,
              onPressed: widget.onCancel,
            ),
          ],
        ),
      ),
    );
  }
}

/// Two or three choices on a track, the chosen one raised.
class SegmentedPill<T> extends StatelessWidget {
  const SegmentedPill({
    super.key,
    required this.options,
    required this.selected,
    required this.onChanged,
    this.height = 36,
  });

  /// Each choice with its label (and an optional icon).
  final List<(T value, String label, IconData? icon)> options;
  final T selected;

  /// Null: the control shows the choice but cannot change it now.
  final ValueChanged<T>? onChanged;
  final double height;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final enabled = onChanged != null;
    final index = options.indexWhere((o) => o.$1 == selected);
    final rtl = Directionality.of(context) == TextDirection.rtl;
    return Opacity(
      opacity: enabled ? 1 : 0.38,
      child: Container(
        constraints: BoxConstraints(minHeight: height),
        padding: const EdgeInsets.all(2),
        decoration: BoxDecoration(
          color: t.fill,
          borderRadius: BorderRadius.circular(Radii.md),
        ),
        child: LayoutBuilder(
          builder: (context, constraints) {
            final width = constraints.maxWidth / options.length;
            return Stack(
              children: [
                if (index >= 0)
                  AnimatedPositioned(
                    duration: Motion.standard,
                    curve: Motion.enter,
                    left: width * (rtl ? options.length - 1 - index : index),
                    top: 0,
                    bottom: 0,
                    width: width,
                    child: Container(
                      decoration: BoxDecoration(
                        color: t.isDark ? t.fillStrong : t.card,
                        borderRadius: BorderRadius.circular(10),
                        boxShadow: t.isDark
                            ? null
                            : const [
                                BoxShadow(
                                  color: Color(0x14000000),
                                  blurRadius: 2,
                                  offset: Offset(0, 1),
                                ),
                                BoxShadow(
                                  color: Color(0x0A000000),
                                  spreadRadius: 0.5,
                                ),
                              ],
                      ),
                    ),
                  ),
                Row(
                  children: [
                    for (final (value, label, icon) in options)
                      Expanded(
                        child: Semantics(
                          button: true,
                          selected: value == selected,
                          enabled: enabled,
                          child: GestureDetector(
                            behavior: HitTestBehavior.opaque,
                            onTap: !enabled || value == selected
                                ? null
                                : () {
                                    Haptic.tick();
                                    onChanged!(value);
                                  },
                            child: ConstrainedBox(
                              constraints: BoxConstraints(
                                minHeight: height - 4,
                              ),
                              child: Center(
                                child: Padding(
                                  padding: const EdgeInsets.symmetric(
                                    horizontal: 8,
                                    vertical: 4,
                                  ),
                                  child: Row(
                                    mainAxisSize: MainAxisSize.min,
                                    children: [
                                      if (icon != null) ...[
                                        Icon(
                                          icon,
                                          size: 16,
                                          color: value == selected
                                              ? t.ink
                                              : t.muted,
                                        ),
                                        const SizedBox(width: 6),
                                      ],
                                      Flexible(
                                        child: Text(
                                          label,
                                          maxLines: 1,
                                          overflow: TextOverflow.ellipsis,
                                          style: TalvioType.label.copyWith(
                                            fontWeight: FontWeight.w600,
                                            color: value == selected
                                                ? t.ink
                                                : t.muted,
                                          ),
                                        ),
                                      ),
                                    ],
                                  ),
                                ),
                              ),
                            ),
                          ),
                        ),
                      ),
                  ],
                ),
              ],
            );
          },
        ),
      ),
    );
  }
}

/// A chip (§3.6): Do's suggestions, a question's choices. 36 tall in a 44
/// tap target.
class TalvioChip extends StatefulWidget {
  const TalvioChip({
    super.key,
    required this.label,
    required this.onPressed,
    this.icon,
    this.selected = false,
  });

  final String label;
  final VoidCallback? onPressed;
  final IconData? icon;
  final bool selected;

  @override
  State<TalvioChip> createState() => _TalvioChipState();
}

class _TalvioChipState extends State<TalvioChip> {
  var _down = false;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final enabled = widget.onPressed != null;
    final selected = widget.selected;
    final bg = selected
        ? t.ink
        : _down
        ? t.fillStrong
        : t.isDark
        ? t.fill
        : t.card;
    final fg = selected ? t.canvas : t.ink;
    return Semantics(
      button: true,
      enabled: enabled,
      selected: selected,
      label: widget.label,
      onTap: widget.onPressed,
      excludeSemantics: true,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTapDown: enabled ? (_) => setState(() => _down = true) : null,
        onTapUp: enabled ? (_) => setState(() => _down = false) : null,
        onTapCancel: enabled ? () => setState(() => _down = false) : null,
        onTap: widget.onPressed == null
            ? null
            : () {
                Haptic.tick();
                widget.onPressed!();
              },
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 4),
          child: Opacity(
            opacity: enabled ? 1 : 0.38,
            child: AnimatedContainer(
              duration: Motion.press,
              constraints: const BoxConstraints(minHeight: 36),
              padding: EdgeInsetsDirectional.fromSTEB(
                widget.icon == null ? 14 : 12,
                0,
                14,
                0,
              ),
              decoration: BoxDecoration(
                color: bg,
                borderRadius: BorderRadius.circular(Radii.pill),
                border: t.isDark || selected
                    ? null
                    : Border.all(color: t.border),
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  if (widget.icon case final icon?) ...[
                    Icon(icon, size: 16, color: selected ? fg : t.muted),
                    const SizedBox(width: 6),
                  ],
                  Flexible(
                    child: Text(
                      widget.label,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TalvioType.label.copyWith(color: fg),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Fades and lifts its child in once, when it first appears: a new answer,
/// a result, a banner.
class Appear extends StatelessWidget {
  const Appear({super.key, required this.child, this.offset = 8});

  final Widget child;

  /// How far below its place it starts.
  final double offset;

  @override
  Widget build(BuildContext context) {
    if (!motionAllowed(context)) return child;
    return TweenAnimationBuilder<double>(
      tween: Tween(begin: 0, end: 1),
      duration: Motion.standard,
      curve: Motion.enter,
      builder: (context, t, child) => Opacity(
        opacity: t,
        child: Transform.translate(
          offset: Offset(0, offset * (1 - t)),
          child: child,
        ),
      ),
      child: child,
    );
  }
}

/// A status line with a light passing over it, left to right: "Thinking…".
class ShimmerText extends StatelessWidget {
  const ShimmerText(this.text, {super.key, this.style});

  final String text;
  final TextStyle? style;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final label = Text(
      text,
      style: (style ?? TalvioType.rowBody).copyWith(color: t.muted),
    );
    if (!loopsAllowed(context)) return label;
    return Looping(
      period: Motion.shimmer,
      builder: (v) => ShaderMask(
        blendMode: BlendMode.srcATop,
        shaderCallback: (rect) {
          final x = -0.4 + 1.8 * v;
          return LinearGradient(
            colors: [t.muted, t.ink, t.muted],
            stops: [
              (x - 0.2).clamp(0.0, 1.0),
              x.clamp(0.0, 1.0),
              (x + 0.2).clamp(0.0, 1.0),
            ],
          ).createShader(rect);
        },
        child: label,
      ),
    );
  }
}

/// A 6 pt ink dot that blinks at the end of text still streaming in.
class BlinkDot extends StatelessWidget {
  const BlinkDot({super.key});

  @override
  Widget build(BuildContext context) {
    final dot = Container(
      width: 6,
      height: 6,
      decoration: BoxDecoration(
        color: context.talvio.ink,
        shape: BoxShape.circle,
      ),
    );
    return ExcludeSemantics(
      child: Breathing(
        period: const Duration(milliseconds: 800),
        builder: (v) => Opacity(opacity: 1 - 0.7 * v, child: dot),
      ),
    );
  }
}

/// Grey bars that shimmer while a list loads.
class SkeletonRow extends StatelessWidget {
  const SkeletonRow({super.key});

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    Widget bar(double w) => Container(
      width: w,
      height: 10,
      decoration: BoxDecoration(
        color: t.fill,
        borderRadius: BorderRadius.circular(5),
      ),
    );
    final row = Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 16),
      child: Row(
        children: [
          Container(
            width: 32,
            height: 32,
            decoration: BoxDecoration(
              color: t.fill,
              borderRadius: BorderRadius.circular(Radii.sm),
            ),
          ),
          const SizedBox(width: 12),
          Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [bar(120), const SizedBox(height: 8), bar(180)],
          ),
        ],
      ),
    );
    return ExcludeSemantics(
      child: Breathing(
        period: Motion.shimmer,
        builder: (v) => Opacity(opacity: 1 - 0.45 * v, child: row),
      ),
    );
  }
}

/// A child rebuilt with a value going 0 → 1 → 0, forever (still at 0 when
/// looping decoration is off).
class Breathing extends StatefulWidget {
  const Breathing({
    super.key,
    required this.builder,
    this.period = const Duration(milliseconds: 1400),
  });

  final Widget Function(double t) builder;
  final Duration period;

  @override
  State<Breathing> createState() => _BreathingState();
}

class _BreathingState extends State<Breathing>
    with SingleTickerProviderStateMixin {
  late final _c = AnimationController(
    vsync: this,
    duration: Duration(milliseconds: widget.period.inMilliseconds ~/ 2),
  );
  var _started = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final run = loopsAllowed(context);
    if (run && !_started) {
      _started = true;
      unawaited(_c.repeat(reverse: true));
    } else if (!run && _started) {
      _started = false;
      _c
        ..stop()
        ..value = 0;
    }
  }

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: _c,
    builder: (context, _) =>
        widget.builder(Curves.easeInOut.transform(_c.value)),
  );
}

/// A child rebuilt with a value going 0 → 1, forever.
class Looping extends StatefulWidget {
  const Looping({super.key, required this.builder, required this.period});

  final Widget Function(double t) builder;
  final Duration period;

  @override
  State<Looping> createState() => _LoopingState();
}

class _LoopingState extends State<Looping> with SingleTickerProviderStateMixin {
  late final _c = AnimationController(vsync: this, duration: widget.period);

  @override
  void initState() {
    super.initState();
    if (ambientMotion) unawaited(_c.repeat());
  }

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: _c,
    builder: (context, _) => widget.builder(_c.value),
  );
}

/// A bottom sheet in the app's style (§3.15): a title, an optional
/// subtitle, then [builder]'s rows, scrolling past 85% of the screen.
Future<T?> showTalvioSheet<T>(
  BuildContext context, {
  required String title,
  String? subtitle,
  Widget? titleTrailing,
  required WidgetBuilder builder,
}) => showModalBottomSheet<T>(
  context: context,
  isScrollControlled: true,
  constraints: BoxConstraints(
    maxHeight: MediaQuery.sizeOf(context).height * 0.85,
  ),
  builder: (sheet) => SheetBody(
    title: title,
    subtitle: subtitle,
    titleTrailing: titleTrailing,
    child: builder(sheet),
  ),
);

/// A sheet's title and its content, safe-area aware.
class SheetBody extends StatelessWidget {
  const SheetBody({
    super.key,
    required this.title,
    this.subtitle,
    this.titleTrailing,
    required this.child,
  });

  final String title;
  final String? subtitle;
  final Widget? titleTrailing;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Padding(
      padding: EdgeInsets.only(
        bottom: MediaQuery.paddingOf(context).bottom + Space.s8,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: EdgeInsetsDirectional.fromSTEB(
              16,
              0,
              titleTrailing == null ? 16 : 4,
              8,
            ),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Semantics(
                        header: true,
                        child: Text(
                          title,
                          style: TalvioType.title2.copyWith(color: t.ink),
                        ),
                      ),
                      if (subtitle case final subtitle?) ...[
                        const SizedBox(height: Space.s4),
                        Text(
                          subtitle,
                          style: TalvioType.footnote.copyWith(color: t.muted),
                        ),
                      ],
                    ],
                  ),
                ),
                ?titleTrailing,
              ],
            ),
          ),
          Flexible(child: child),
        ],
      ),
    );
  }
}

/// A sheet's rows, separated as in a group but without its card.
class SheetRows extends StatelessWidget {
  const SheetRows({super.key, required this.children, this.dividerInset = 60});

  final List<Widget> children;
  final double dividerInset;

  @override
  Widget build(BuildContext context) => Column(
    mainAxisSize: MainAxisSize.min,
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      for (final (i, child) in children.indexed) ...[
        if (i > 0)
          Padding(
            padding: EdgeInsetsDirectional.only(start: dividerInset),
            child: Divider(height: 1, color: context.talvio.hairline),
          ),
        child,
      ],
    ],
  );
}

/// A confirm dialog (§3.15): title, body, Cancel and the action. True when
/// the action was chosen.
Future<bool> confirmDialog(
  BuildContext context, {
  required String title,
  required String body,
  required String action,
  required String cancel,
  bool destructive = false,
}) async {
  final confirmed = await showDialog<bool>(
    context: context,
    builder: (context) {
      final t = context.talvio;
      return Dialog(
        insetPadding: const EdgeInsets.all(24),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 340),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(24, 24, 16, 16),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsetsDirectional.only(end: 8),
                  child: Text(
                    title,
                    style: TalvioType.title2.copyWith(color: t.ink),
                  ),
                ),
                const SizedBox(height: Space.s8),
                Padding(
                  padding: const EdgeInsetsDirectional.only(end: 8),
                  child: Text(
                    body,
                    style: TalvioType.rowBody.copyWith(color: t.muted),
                  ),
                ),
                const SizedBox(height: Space.s16),
                Wrap(
                  alignment: WrapAlignment.end,
                  spacing: Space.s8,
                  children: [
                    Btn(
                      cancel,
                      kind: BtnKind.ghostNeutral,
                      onPressed: () => Navigator.pop(context, false),
                    ),
                    Btn(
                      action,
                      kind: destructive ? BtnKind.ghostDanger : BtnKind.ghost,
                      onPressed: () => Navigator.pop(context, true),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      );
    },
  );
  return confirmed == true;
}

/// Shows a snack bar with [message], replacing any on screen.
void showSnack(BuildContext context, String message, {SnackBarAction? action}) {
  ScaffoldMessenger.maybeOf(context)
    ?..hideCurrentSnackBar()
    ..showSnackBar(SnackBar(content: Text(message), action: action));
}
