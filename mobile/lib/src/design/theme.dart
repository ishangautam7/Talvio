// The Material theme built from Talvio's tokens. Screens style themselves
// through it (and `context.talvio`), never with colours of their own.

import 'package:flutter/cupertino.dart' show CupertinoPageTransitionsBuilder;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'tokens.dart';

ThemeData buildTheme(Brightness brightness) {
  final light = brightness == Brightness.light;
  final t = light ? TalvioColors.light : TalvioColors.dark;

  final scheme = ColorScheme(
    brightness: brightness,
    primary: t.accent,
    onPrimary: t.onAccent,
    primaryContainer: t.accentSoft,
    onPrimaryContainer: light
        ? const Color(0xFF7A1E10)
        : const Color(0xFFFFD9D2),
    secondary: t.ink,
    onSecondary: t.canvas,
    secondaryContainer: t.sunken,
    onSecondaryContainer: t.ink,
    tertiary: light ? const Color(0xFF3B3F8F) : const Color(0xFFB9BCFF),
    onTertiary: light ? Colors.white : const Color(0xFF14164A),
    error: t.danger,
    onError: light ? Colors.white : const Color(0xFF3A0B0B),
    errorContainer: t.dangerSoft,
    onErrorContainer: light ? const Color(0xFF7A1410) : const Color(0xFFFFDAD6),
    surface: t.canvas,
    onSurface: t.ink,
    onSurfaceVariant: t.muted,
    surfaceDim: t.chrome,
    surfaceBright: t.card,
    surfaceContainerLowest: t.card,
    surfaceContainerLow: t.card,
    surfaceContainer: t.chrome,
    surfaceContainerHigh: t.raised,
    surfaceContainerHighest: t.sunken,
    outline: light ? const Color(0xFFD6CDC4) : const Color(0xFF3A3E62),
    outlineVariant: t.hairline,
    shadow: Colors.black,
    scrim: Colors.black,
    inverseSurface: light ? const Color(0xFF1B1D33) : const Color(0xFFF2F1F7),
    onInverseSurface: light ? const Color(0xFFF2F1F7) : const Color(0xFF1B1D33),
    inversePrimary: light ? const Color(0xFFFF6A52) : const Color(0xFFC2402A),
    surfaceTint: Colors.transparent,
  );

  final text = _textTheme(t);
  final stadium = RoundedRectangleBorder(
    borderRadius: BorderRadius.circular(Radii.md),
  );
  final hairline = BorderSide(color: t.hairline);

  return ThemeData(
    useMaterial3: true,
    brightness: brightness,
    colorScheme: scheme,
    fontFamily: TalvioFonts.text,
    textTheme: text,
    scaffoldBackgroundColor: t.canvas,
    canvasColor: t.canvas,
    splashFactory: InkSparkle.splashFactory,
    visualDensity: VisualDensity.standard,
    materialTapTargetSize: MaterialTapTargetSize.padded,
    extensions: [t],
    dividerTheme: DividerThemeData(color: t.hairline, thickness: 1, space: 1),
    iconTheme: IconThemeData(color: t.ink, size: 22),
    appBarTheme: AppBarTheme(
      backgroundColor: t.canvas,
      foregroundColor: t.ink,
      elevation: 0,
      scrolledUnderElevation: 0,
      centerTitle: false,
      titleTextStyle: text.titleLarge,
      systemOverlayStyle: light
          ? SystemUiOverlayStyle.dark.copyWith(
              statusBarColor: Colors.transparent,
            )
          : SystemUiOverlayStyle.light.copyWith(
              statusBarColor: Colors.transparent,
            ),
    ),
    cardTheme: CardThemeData(
      color: t.card,
      elevation: 0,
      margin: EdgeInsets.zero,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.lg),
        side: hairline,
      ),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        backgroundColor: t.accent,
        foregroundColor: t.onAccent,
        disabledBackgroundColor: light ? t.sunken : t.raised,
        disabledForegroundColor: t.faint,
        minimumSize: const Size(64, 48),
        padding: const EdgeInsets.symmetric(horizontal: 20),
        textStyle: text.labelLarge,
        shape: stadium,
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        foregroundColor: t.ink,
        minimumSize: const Size(64, 48),
        padding: const EdgeInsets.symmetric(horizontal: 18),
        textStyle: text.labelLarge,
        side: BorderSide(color: scheme.outline),
        shape: stadium,
      ),
    ),
    textButtonTheme: TextButtonThemeData(
      style: TextButton.styleFrom(
        foregroundColor: t.accent,
        minimumSize: const Size(48, 44),
        padding: const EdgeInsets.symmetric(horizontal: 14),
        textStyle: text.labelLarge,
        shape: stadium,
      ),
    ),
    iconButtonTheme: IconButtonThemeData(
      style: IconButton.styleFrom(foregroundColor: t.ink),
    ),
    segmentedButtonTheme: SegmentedButtonThemeData(
      style: ButtonStyle(
        textStyle: WidgetStatePropertyAll(text.labelLarge),
        side: WidgetStatePropertyAll(BorderSide(color: t.hairline)),
        shape: WidgetStatePropertyAll(stadium),
        minimumSize: const WidgetStatePropertyAll(Size(48, 44)),
        backgroundColor: WidgetStateProperty.resolveWith(
          (states) => states.contains(WidgetState.selected) ? t.ink : t.card,
        ),
        foregroundColor: WidgetStateProperty.resolveWith(
          (states) => states.contains(WidgetState.disabled)
              ? t.faint
              : states.contains(WidgetState.selected)
              ? t.canvas
              : t.ink,
        ),
      ),
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: t.card,
      hintStyle: text.bodyLarge?.copyWith(color: t.faint),
      labelStyle: text.bodyLarge?.copyWith(color: t.muted),
      floatingLabelStyle: text.bodyMedium?.copyWith(color: t.accent),
      helperStyle: text.bodySmall?.copyWith(color: t.muted),
      contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 16),
      border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(Radii.md),
        borderSide: BorderSide(color: scheme.outline),
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(Radii.md),
        borderSide: BorderSide(color: scheme.outline),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(Radii.md),
        borderSide: BorderSide(color: t.accent, width: 1.6),
      ),
      disabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(Radii.md),
        borderSide: BorderSide(color: t.hairline),
      ),
      errorBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(Radii.md),
        borderSide: BorderSide(color: t.danger),
      ),
      focusedErrorBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(Radii.md),
        borderSide: BorderSide(color: t.danger, width: 1.6),
      ),
    ),
    chipTheme: ChipThemeData(
      backgroundColor: t.card,
      side: hairline,
      labelStyle: text.labelLarge?.copyWith(color: t.ink),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.pill),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
      iconTheme: IconThemeData(color: t.muted, size: 18),
    ),
    listTileTheme: ListTileThemeData(
      iconColor: t.muted,
      textColor: t.ink,
      titleTextStyle: text.titleMedium,
      subtitleTextStyle: text.bodyMedium?.copyWith(color: t.muted),
      contentPadding: const EdgeInsets.symmetric(horizontal: Space.page),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.md),
      ),
    ),
    navigationBarTheme: NavigationBarThemeData(
      backgroundColor: t.chrome,
      surfaceTintColor: Colors.transparent,
      elevation: 0,
      height: 68,
      indicatorColor: t.accentSoft,
      indicatorShape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.pill),
      ),
      labelBehavior: NavigationDestinationLabelBehavior.alwaysShow,
      labelTextStyle: WidgetStateProperty.resolveWith(
        (states) => text.labelMedium?.copyWith(
          color: states.contains(WidgetState.selected) ? t.ink : t.muted,
          fontWeight: states.contains(WidgetState.selected)
              ? FontWeight.w700
              : FontWeight.w500,
        ),
      ),
      iconTheme: WidgetStateProperty.resolveWith(
        (states) => IconThemeData(
          size: 24,
          color: states.contains(WidgetState.selected) ? t.accent : t.muted,
        ),
      ),
    ),
    navigationRailTheme: NavigationRailThemeData(
      backgroundColor: t.chrome,
      indicatorColor: t.accentSoft,
      selectedIconTheme: IconThemeData(color: t.accent),
      unselectedIconTheme: IconThemeData(color: t.muted),
      selectedLabelTextStyle: text.labelMedium?.copyWith(
        color: t.ink,
        fontWeight: FontWeight.w700,
      ),
      unselectedLabelTextStyle: text.labelMedium?.copyWith(color: t.muted),
    ),
    bottomSheetTheme: BottomSheetThemeData(
      backgroundColor: t.card,
      surfaceTintColor: Colors.transparent,
      modalBackgroundColor: t.card,
      showDragHandle: true,
      dragHandleColor: t.hairline,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(Radii.xl)),
      ),
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: t.card,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.xl),
      ),
      titleTextStyle: text.titleLarge,
      contentTextStyle: text.bodyLarge?.copyWith(color: t.muted),
    ),
    snackBarTheme: SnackBarThemeData(
      behavior: SnackBarBehavior.floating,
      backgroundColor: scheme.inverseSurface,
      contentTextStyle: text.bodyMedium?.copyWith(
        color: scheme.onInverseSurface,
      ),
      actionTextColor: light ? const Color(0xFFFF8F7C) : t.accent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.md),
      ),
      insetPadding: const EdgeInsets.fromLTRB(12, 0, 12, 12),
    ),
    progressIndicatorTheme: ProgressIndicatorThemeData(
      color: t.accent,
      linearTrackColor: t.sunken,
      circularTrackColor: Colors.transparent,
    ),
    tooltipTheme: TooltipThemeData(
      decoration: BoxDecoration(
        color: scheme.inverseSurface,
        borderRadius: BorderRadius.circular(Radii.sm),
      ),
      textStyle: text.bodySmall?.copyWith(color: scheme.onInverseSurface),
    ),
    popupMenuTheme: PopupMenuThemeData(
      color: t.card,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.md),
        side: hairline,
      ),
      textStyle: text.bodyLarge,
    ),
    badgeTheme: BadgeThemeData(backgroundColor: t.success),
    textSelectionTheme: TextSelectionThemeData(
      cursorColor: t.accent,
      selectionColor: t.accent.withValues(alpha: 0.25),
      selectionHandleColor: t.accent,
    ),
    pageTransitionsTheme: const PageTransitionsTheme(
      builders: {
        TargetPlatform.android: FadeForwardsPageTransitionsBuilder(),
        TargetPlatform.iOS: CupertinoPageTransitionsBuilder(),
      },
    ),
  );
}

TextTheme _textTheme(TalvioColors t) {
  TextStyle display(double size, FontWeight weight, double height, double ls) =>
      TextStyle(
        fontFamily: TalvioFonts.display,
        fontSize: size,
        fontWeight: weight,
        height: height,
        letterSpacing: ls,
        color: t.ink,
      );
  TextStyle inter(double size, FontWeight weight, double height, [Color? c]) =>
      TextStyle(
        fontFamily: TalvioFonts.text,
        fontSize: size,
        fontWeight: weight,
        height: height,
        letterSpacing: 0,
        color: c ?? t.ink,
      );
  return TextTheme(
    displayLarge: display(44, FontWeight.w800, 1.08, -1.2),
    displayMedium: display(36, FontWeight.w800, 1.1, -0.9),
    displaySmall: display(30, FontWeight.w800, 1.14, -0.6),
    headlineLarge: display(28, FontWeight.w800, 1.18, -0.6),
    headlineMedium: display(24, FontWeight.w800, 1.2, -0.4),
    headlineSmall: display(21, FontWeight.w700, 1.24, -0.3),
    titleLarge: display(18, FontWeight.w700, 1.3, -0.2),
    titleMedium: inter(16, FontWeight.w600, 1.35),
    titleSmall: inter(14, FontWeight.w600, 1.35),
    bodyLarge: inter(16, FontWeight.w400, 1.5),
    bodyMedium: inter(14.5, FontWeight.w400, 1.45),
    bodySmall: inter(12.5, FontWeight.w400, 1.4, t.muted),
    labelLarge: inter(14.5, FontWeight.w600, 1.25),
    labelMedium: inter(12.5, FontWeight.w600, 1.25),
    labelSmall: inter(11, FontWeight.w600, 1.2),
  );
}
