// Talvio's design tokens for the phone, aligned with the desktop's Studio
// palette (docs/talvio-brand.md §5): colour, type, spacing, radii and
// motion. Everything the screens draw comes from here or from the theme
// built on it (theme.dart), so light and dark stay one system.

import 'package:flutter/material.dart';

/// The type families. Geist for everything read and operated, Geist Mono
/// for figures that must line up, Plus Jakarta Sans for the wordmark only.
abstract final class TalvioFonts {
  static const text = 'Geist';
  static const mono = 'GeistMono';
  static const display = 'PlusJakartaSans';
}

/// The one type scale. Line heights are absolute (height = line / size).
abstract final class TalvioType {
  static TextStyle _geist(
    double size,
    double line,
    FontWeight weight,
    double tracking,
  ) => TextStyle(
    fontFamily: TalvioFonts.text,
    fontSize: size,
    height: line / size,
    fontWeight: weight,
    letterSpacing: tracking,
    leadingDistribution: TextLeadingDistribution.even,
  );

  static TextStyle _mono(double size, double line) => TextStyle(
    fontFamily: TalvioFonts.mono,
    fontSize: size,
    height: line / size,
    fontWeight: FontWeight.w400,
    letterSpacing: 0,
    leadingDistribution: TextLeadingDistribution.even,
    fontFeatures: const [FontFeature.tabularFigures()],
  );

  /// The sign-in title only.
  static TextStyle get display => _geist(28, 34, FontWeight.w600, -0.6);

  /// Page-level empty-state and Ask/Do headings.
  static TextStyle get title1 => _geist(22, 28, FontWeight.w600, -0.4);

  /// The computer's name, sheet, dialog and app-bar titles.
  static TextStyle get title2 => _geist(17, 22, FontWeight.w600, -0.2);

  /// Chat, inputs, the composer, dictated text.
  static TextStyle get body => _geist(16, 24, FontWeight.w400, -0.1);
  static TextStyle get bodyStrong => _geist(16, 24, FontWeight.w600, -0.1);

  /// List-row titles, banner text.
  static TextStyle get rowTitle => _geist(15, 20, FontWeight.w500, -0.1);

  /// Secondary sentences inside cards.
  static TextStyle get rowBody => _geist(15, 20, FontWeight.w400, -0.1);

  /// Row subtitles, hints, captions, the status line.
  static TextStyle get footnote => _geist(13, 18, FontWeight.w400, 0);

  /// Section labels, compact buttons, chips.
  static TextStyle get label => _geist(13, 18, FontWeight.w500, 0);

  /// Labels of 44/48 buttons.
  static TextStyle get button => _geist(15, 20, FontWeight.w600, -0.1);

  /// Navigation labels, pills and badges.
  static TextStyle get caption => _geist(11, 13, FontWeight.w500, 0.1);

  /// The timer, sizes, the server's host, inline code.
  static TextStyle get mono => _mono(13, 18);

  /// Code blocks in answers.
  static TextStyle get monoBlock => _mono(13, 20);
}

/// Spacing on a 4-point grid.
abstract final class Space {
  static const s2 = 2.0;
  static const s4 = 4.0;
  static const s8 = 8.0;
  static const s12 = 12.0;
  static const s16 = 16.0;
  static const s20 = 20.0;
  static const s24 = 24.0;
  static const s32 = 32.0;
  static const s40 = 40.0;
  static const s48 = 48.0;

  /// The side margin of every page, every state, sheets included.
  static const gutter = 16.0;

  /// Content never grows wider than this on a wide screen.
  static const maxContent = 640.0;
}

/// Corner radii.
abstract final class Radii {
  /// Inline code, tooltips.
  static const xs = 6.0;

  /// 32 pt tiles and monograms.
  static const sm = 8.0;

  /// Wells, banners, code blocks, inputs, key caps, segmented track.
  static const md = 12.0;

  /// Grouped lists, cards, the live view.
  static const lg = 16.0;

  /// Bubbles, sheet tops, dialogs.
  static const xl = 20.0;

  /// Buttons, chips, the composer, pills.
  static const pill = 999.0;
}

/// Durations and curves. Short and decisive; no overshoot.
abstract final class Motion {
  static const press = Duration(milliseconds: 100);
  static const quick = Duration(milliseconds: 150);
  static const standard = Duration(milliseconds: 220);
  static const exit = Duration(milliseconds: 160);
  static const emphasized = Duration(milliseconds: 320);
  static const shimmer = Duration(milliseconds: 1600);

  static const pressCurve = Curves.easeOut;
  static const enter = Cubic(0.2, 0, 0, 1);
  static const leave = Cubic(0.3, 0, 1, 1);
  static const emphasizedCurve = Cubic(0.05, 0.7, 0.1, 1);
}

/// The colours Material's scheme has no slot for, as a theme extension so
/// widgets read them with `context.talvio`.
@immutable
class TalvioColors extends ThemeExtension<TalvioColors> {
  const TalvioColors({
    required this.canvas,
    required this.card,
    required this.fill,
    required this.fillStrong,
    required this.hairline,
    required this.border,
    required this.ink,
    required this.muted,
    required this.faint,
    required this.accent,
    required this.onAccent,
    required this.accentSoft,
    required this.coral,
    required this.success,
    required this.successSoft,
    required this.warning,
    required this.warningSoft,
    required this.danger,
    required this.dangerSoft,
    required this.onDanger,
    required this.live,
    required this.scrim,
    required this.pressed,
    required this.pressedOnAccent,
    required this.inverse,
    required this.onInverse,
  });

  /// Scaffold, top bar and bottom navigation: one colour, no bands.
  final Color canvas;

  /// Grouped lists, cards, the composer, sheets and dialogs.
  final Color card;

  /// User bubble, wells, segmented track, avatar, disabled fills.
  final Color fill;

  /// The pressed state of a [fill] surface.
  final Color fillStrong;

  /// Separators inside groups, the scrolled-under line.
  final Color hairline;

  /// Outline buttons, the composer and chips on light.
  final Color border;
  final Color ink;
  final Color muted;

  /// Timestamps and placeholders. Never on [fill] in light.
  final Color faint;

  /// The one primary action per view, links, focus, recording.
  final Color accent;
  final Color onAccent;

  /// A wash behind accent icons in banners only.
  final Color accentSoft;

  /// Graphics only: the waveform, the mark, the recording halo.
  final Color coral;
  final Color success;
  final Color successSoft;
  final Color warning;
  final Color warningSoft;
  final Color danger;
  final Color dangerSoft;
  final Color onDanger;

  /// The live badge and dot.
  final Color live;

  /// Behind sheets and dialogs.
  final Color scrim;

  /// The overlay on a pressed surface, and on a pressed accent fill.
  final Color pressed;
  final Color pressedOnAccent;

  /// Snack bars and tooltips.
  final Color inverse;
  final Color onInverse;

  /// The live picture's background and the full-screen viewer.
  static const mediaBg = Color(0xFF000000);

  /// Buttons and badges over video (with a 12 blur).
  static const glass = Color(0x73000000);

  /// The brand gradient: the mark, the Dictate mic and the profile avatar
  /// only.
  static const gradient = LinearGradient(
    begin: Alignment.topLeft,
    end: Alignment.bottomRight,
    colors: [Color(0xFFFF8F3F), Color(0xFFEE3A5C)],
  );

  bool get isDark => ink.computeLuminance() > 0.5;

  static const light = TalvioColors(
    canvas: Color(0xFFF2F0EC),
    card: Color(0xFFFFFFFF),
    fill: Color(0xFFEAE7E1),
    fillStrong: Color(0xFFDFDAD3),
    hairline: Color(0xFFE6E2DB),
    border: Color(0xFFD9D4CC),
    ink: Color(0xFF16151B),
    muted: Color(0xFF5F5C66),
    faint: Color(0xFF6C6872),
    accent: Color(0xFFC1381F),
    onAccent: Color(0xFFFFFFFF),
    accentSoft: Color(0xFFFBE5DF),
    coral: Color(0xFFF2543D),
    success: Color(0xFF1D7A43),
    successSoft: Color(0xFFE1F2E7),
    warning: Color(0xFF8F5300),
    warningSoft: Color(0xFFFBEFD9),
    danger: Color(0xFFB3261E),
    dangerSoft: Color(0xFFFBE6E3),
    onDanger: Color(0xFFFFFFFF),
    live: Color(0xFFD42F25),
    scrim: Color(0x52000000),
    pressed: Color(0x0F16151B),
    pressedOnAccent: Color(0x1A000000),
    inverse: Color(0xFF16151B),
    onInverse: Color(0xFFF4F3F6),
  );

  static const dark = TalvioColors(
    canvas: Color(0xFF0C0C10),
    card: Color(0xFF16161C),
    fill: Color(0xFF212129),
    fillStrong: Color(0xFF2A2A33),
    hairline: Color(0xFF24242C),
    border: Color(0xFF34343E),
    ink: Color(0xFFF4F3F6),
    muted: Color(0xFFA3A0AD),
    faint: Color(0xFF8A8794),
    accent: Color(0xFFFF6A52),
    onAccent: Color(0xFF0F0F13),
    accentSoft: Color(0xFF2E1915),
    coral: Color(0xFFFF6A52),
    success: Color(0xFF4CC77A),
    successSoft: Color(0xFF13291B),
    warning: Color(0xFFF0B04A),
    warningSoft: Color(0xFF2E2412),
    danger: Color(0xFFFF8A7F),
    dangerSoft: Color(0xFF33171A),
    onDanger: Color(0xFF0F0F13),
    live: Color(0xFFD42F25),
    scrim: Color(0x8F000000),
    pressed: Color(0x14F4F3F6),
    pressedOnAccent: Color(0x1F000000),
    inverse: Color(0xFFF4F3F6),
    onInverse: Color(0xFF16151B),
  );

  @override
  TalvioColors copyWith() => this;

  @override
  TalvioColors lerp(TalvioColors? other, double t) {
    if (other == null) return this;
    Color c(Color a, Color b) => Color.lerp(a, b, t)!;
    return TalvioColors(
      canvas: c(canvas, other.canvas),
      card: c(card, other.card),
      fill: c(fill, other.fill),
      fillStrong: c(fillStrong, other.fillStrong),
      hairline: c(hairline, other.hairline),
      border: c(border, other.border),
      ink: c(ink, other.ink),
      muted: c(muted, other.muted),
      faint: c(faint, other.faint),
      accent: c(accent, other.accent),
      onAccent: c(onAccent, other.onAccent),
      accentSoft: c(accentSoft, other.accentSoft),
      coral: c(coral, other.coral),
      success: c(success, other.success),
      successSoft: c(successSoft, other.successSoft),
      warning: c(warning, other.warning),
      warningSoft: c(warningSoft, other.warningSoft),
      danger: c(danger, other.danger),
      dangerSoft: c(dangerSoft, other.dangerSoft),
      onDanger: c(onDanger, other.onDanger),
      live: c(live, other.live),
      scrim: c(scrim, other.scrim),
      pressed: c(pressed, other.pressed),
      pressedOnAccent: c(pressedOnAccent, other.pressedOnAccent),
      inverse: c(inverse, other.inverse),
      onInverse: c(onInverse, other.onInverse),
    );
  }
}

extension TalvioTheme on BuildContext {
  /// Talvio's colours for the current theme.
  TalvioColors get talvio =>
      Theme.of(this).extension<TalvioColors>() ?? TalvioColors.light;

  TextTheme get text => Theme.of(this).textTheme;
}
