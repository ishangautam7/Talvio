// Talvio's design tokens for the phone (docs/talvio-brand.md §5): colour,
// type, spacing, radii and motion. Everything the screens draw comes from
// here or from the theme built on it (theme.dart), so light and dark, and
// the desktop app's palette, stay one system.

import 'package:flutter/material.dart';

/// The two type families. Inter for everything read and operated, Plus
/// Jakarta Sans for display (page heroes, the wordmark, big numbers).
abstract final class TalvioFonts {
  static const text = 'Inter';
  static const display = 'PlusJakartaSans';
}

/// Spacing on a 4-point grid.
abstract final class Space {
  static const xxs = 2.0;
  static const xs = 4.0;
  static const sm = 8.0;
  static const md = 12.0;
  static const lg = 16.0;
  static const xl = 20.0;
  static const xxl = 24.0;
  static const xxxl = 32.0;

  /// The side margin of every page.
  static const page = 20.0;
}

/// Corner radii.
abstract final class Radii {
  static const sm = 10.0;
  static const md = 14.0;
  static const lg = 20.0;
  static const xl = 28.0;
  static const pill = 999.0;
}

/// Durations and curves. Short and decisive: the UI answers a tap at once,
/// and only state changes (recording, live, a result arriving) animate.
abstract final class Motion {
  static const fast = Duration(milliseconds: 140);
  static const medium = Duration(milliseconds: 240);
  static const slow = Duration(milliseconds: 380);
  static const emphasized = Curves.easeOutCubic;
  static const standard = Curves.easeInOutCubic;
}

/// The colours Material's scheme has no slot for, as a theme extension so
/// widgets read them with `context.talvio`.
@immutable
class TalvioColors extends ThemeExtension<TalvioColors> {
  const TalvioColors({
    required this.canvas,
    required this.chrome,
    required this.card,
    required this.raised,
    required this.sunken,
    required this.ink,
    required this.muted,
    required this.faint,
    required this.hairline,
    required this.accent,
    required this.onAccent,
    required this.coral,
    required this.accentSoft,
    required this.success,
    required this.successSoft,
    required this.warning,
    required this.warningSoft,
    required this.danger,
    required this.dangerSoft,
    required this.live,
    required this.userBubble,
    required this.onUserBubble,
  });

  /// The page background (warm paper / ink navy).
  final Color canvas;

  /// One step from the canvas: the navigation bar, the header's pill.
  final Color chrome;

  /// Cards and the composer.
  final Color card;

  /// A card on a card, a pressed row.
  final Color raised;

  /// Wells inside a card: inputs, key caps, quiet chips.
  final Color sunken;
  final Color ink;
  final Color muted;

  /// Text that only labels (timestamps, counters).
  final Color faint;
  final Color hairline;

  /// Coral for text, links and filled buttons (text-safe).
  final Color accent;
  final Color onAccent;

  /// The brand coral, for graphics only on light.
  final Color coral;

  /// A coral wash behind accent icons and the selected tab.
  final Color accentSoft;
  final Color success;
  final Color successSoft;
  final Color warning;
  final Color warningSoft;
  final Color danger;
  final Color dangerSoft;

  /// The red of a live picture's dot.
  final Color live;
  final Color userBubble;
  final Color onUserBubble;

  /// The fill of a round button that cannot be pressed now: a well on
  /// light, a step up on dark (a well there reads as a hole).
  Color get disabledFill => ink.computeLuminance() > 0.5 ? raised : sunken;

  /// The brand gradient: the icon's tile, the mic.
  static const gradient = LinearGradient(
    begin: Alignment.topLeft,
    end: Alignment.bottomRight,
    colors: [Color(0xFFFF8F3F), Color(0xFFEE3A5C)],
  );

  /// The ink tile: dark surfaces that stay dark in both themes (the screen
  /// hero, live view).
  static const inkTile = LinearGradient(
    begin: Alignment.topLeft,
    end: Alignment.bottomRight,
    colors: [Color(0xFF262B6B), Color(0xFF11122E)],
  );

  static const light = TalvioColors(
    canvas: Color(0xFFFBF7F3),
    chrome: Color(0xFFF6F1EB),
    card: Color(0xFFFFFFFF),
    raised: Color(0xFFF7F2ED),
    sunken: Color(0xFFF3EDE7),
    ink: Color(0xFF1B1D33),
    muted: Color(0xFF5B5E78),
    faint: Color(0xFF8A8CA3),
    hairline: Color(0xFFEAE3DC),
    accent: Color(0xFFC2402A),
    onAccent: Color(0xFFFFFFFF),
    coral: Color(0xFFF2543D),
    accentSoft: Color(0xFFFCE6E1),
    success: Color(0xFF16803F),
    successSoft: Color(0xFFE2F4E8),
    warning: Color(0xFF9A5B00),
    warningSoft: Color(0xFFFDF0D9),
    danger: Color(0xFFB3261E),
    dangerSoft: Color(0xFFFBE7E5),
    live: Color(0xFFE5372A),
    userBubble: Color(0xFF1B1D33),
    onUserBubble: Color(0xFFF7F5FB),
  );

  static const dark = TalvioColors(
    canvas: Color(0xFF101122),
    chrome: Color(0xFF14152A),
    card: Color(0xFF17192F),
    raised: Color(0xFF1E2140),
    sunken: Color(0xFF0C0D1C),
    ink: Color(0xFFF2F1F7),
    muted: Color(0xFFA3A5BF),
    faint: Color(0xFF7D809C),
    hairline: Color(0xFF262944),
    accent: Color(0xFFFF6A52),
    onAccent: Color(0xFF101122),
    coral: Color(0xFFFF6A52),
    accentSoft: Color(0xFF3A1F2A),
    success: Color(0xFF4CCB7E),
    successSoft: Color(0xFF15301F),
    warning: Color(0xFFF2B54B),
    warningSoft: Color(0xFF33280F),
    danger: Color(0xFFFF8A7F),
    dangerSoft: Color(0xFF3A1A1E),
    live: Color(0xFFFF4D40),
    userBubble: Color(0xFF262A52),
    onUserBubble: Color(0xFFF2F1F7),
  );

  @override
  TalvioColors copyWith() => this;

  @override
  TalvioColors lerp(TalvioColors? other, double t) {
    if (other == null) return this;
    Color c(Color a, Color b) => Color.lerp(a, b, t)!;
    return TalvioColors(
      canvas: c(canvas, other.canvas),
      chrome: c(chrome, other.chrome),
      card: c(card, other.card),
      raised: c(raised, other.raised),
      sunken: c(sunken, other.sunken),
      ink: c(ink, other.ink),
      muted: c(muted, other.muted),
      faint: c(faint, other.faint),
      hairline: c(hairline, other.hairline),
      accent: c(accent, other.accent),
      onAccent: c(onAccent, other.onAccent),
      coral: c(coral, other.coral),
      accentSoft: c(accentSoft, other.accentSoft),
      success: c(success, other.success),
      successSoft: c(successSoft, other.successSoft),
      warning: c(warning, other.warning),
      warningSoft: c(warningSoft, other.warningSoft),
      danger: c(danger, other.danger),
      dangerSoft: c(dangerSoft, other.dangerSoft),
      live: c(live, other.live),
      userBubble: c(userBubble, other.userBubble),
      onUserBubble: c(onUserBubble, other.onUserBubble),
    );
  }
}

extension TalvioTheme on BuildContext {
  /// Talvio's colours for the current theme.
  TalvioColors get talvio =>
      Theme.of(this).extension<TalvioColors>() ?? TalvioColors.light;

  TextTheme get text => Theme.of(this).textTheme;
}
