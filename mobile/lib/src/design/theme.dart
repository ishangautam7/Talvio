// The Material theme built from Talvio's tokens. Screens style themselves
// through it (and `context.talvio`), never with colours of their own.

import 'package:flutter/cupertino.dart' show CupertinoPageTransitionsBuilder;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'tokens.dart';

/// The status and navigation bars for [brightness]: transparent status bar,
/// the navigation bar in the canvas colour, edge to edge.
SystemUiOverlayStyle systemBars(Brightness brightness) {
  final t = brightness == Brightness.light
      ? TalvioColors.light
      : TalvioColors.dark;
  final light = brightness == Brightness.light;
  return (light ? SystemUiOverlayStyle.dark : SystemUiOverlayStyle.light)
      .copyWith(
        statusBarColor: Colors.transparent,
        systemNavigationBarColor: t.canvas,
        systemNavigationBarDividerColor: t.canvas,
        systemNavigationBarIconBrightness: light
            ? Brightness.dark
            : Brightness.light,
        systemNavigationBarContrastEnforced: false,
        systemStatusBarContrastEnforced: false,
      );
}

ThemeData buildTheme(Brightness brightness) {
  final light = brightness == Brightness.light;
  final t = light ? TalvioColors.light : TalvioColors.dark;

  final scheme = ColorScheme(
    brightness: brightness,
    primary: t.accent,
    onPrimary: t.onAccent,
    primaryContainer: t.accentSoft,
    onPrimaryContainer: t.ink,
    secondary: t.ink,
    onSecondary: t.canvas,
    secondaryContainer: t.fill,
    onSecondaryContainer: t.ink,
    tertiary: t.ink,
    onTertiary: t.canvas,
    error: t.danger,
    onError: t.onDanger,
    errorContainer: t.dangerSoft,
    onErrorContainer: t.ink,
    surface: t.canvas,
    onSurface: t.ink,
    onSurfaceVariant: t.muted,
    surfaceDim: t.canvas,
    surfaceBright: t.card,
    surfaceContainerLowest: t.card,
    surfaceContainerLow: t.card,
    surfaceContainer: t.card,
    surfaceContainerHigh: t.fill,
    surfaceContainerHighest: t.fillStrong,
    outline: t.border,
    outlineVariant: t.hairline,
    shadow: Colors.black,
    scrim: Colors.black,
    inverseSurface: t.inverse,
    onInverseSurface: t.onInverse,
    inversePrimary: light ? const Color(0xFFFF8F7C) : const Color(0xFFC1381F),
    surfaceTint: Colors.transparent,
  );

  final text = _textTheme(t);
  const pill = StadiumBorder();
  final pressed = WidgetStateProperty.resolveWith<Color?>(
    (s) => s.contains(WidgetState.pressed) ? t.pressed : null,
  );
  final pressedOnAccent = WidgetStateProperty.resolveWith<Color?>(
    (s) => s.contains(WidgetState.pressed) ? t.pressedOnAccent : null,
  );
  final focusRing = WidgetStateProperty.resolveWith<BorderSide?>(
    (s) => s.contains(WidgetState.focused)
        ? BorderSide(color: t.accent, width: 2)
        : null,
  );
  OutlineInputBorder well([Color? color, double width = 1.5]) =>
      OutlineInputBorder(
        borderRadius: BorderRadius.circular(Radii.md),
        borderSide: color == null
            ? BorderSide.none
            : BorderSide(color: color, width: width),
      );

  return ThemeData(
    useMaterial3: true,
    brightness: brightness,
    colorScheme: scheme,
    fontFamily: TalvioFonts.text,
    textTheme: text,
    primaryTextTheme: text,
    scaffoldBackgroundColor: t.canvas,
    canvasColor: t.canvas,
    splashFactory: NoSplash.splashFactory,
    splashColor: Colors.transparent,
    highlightColor: t.pressed,
    hoverColor: t.pressed,
    focusColor: t.pressed,
    visualDensity: VisualDensity.standard,
    materialTapTargetSize: MaterialTapTargetSize.padded,
    extensions: [t],
    dividerTheme: DividerThemeData(color: t.hairline, thickness: 1, space: 1),
    iconTheme: IconThemeData(color: t.ink, size: 20),
    appBarTheme: AppBarTheme(
      backgroundColor: t.canvas,
      foregroundColor: t.ink,
      elevation: 0,
      scrolledUnderElevation: 0,
      centerTitle: false,
      toolbarHeight: 52,
      titleSpacing: 4,
      titleTextStyle: text.titleMedium,
      systemOverlayStyle: systemBars(brightness),
    ),
    cardTheme: CardThemeData(
      color: t.card,
      elevation: 0,
      margin: EdgeInsets.zero,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.lg),
      ),
    ),
    // Primary: the one accent-filled action of a view.
    filledButtonTheme: FilledButtonThemeData(
      style: ButtonStyle(
        backgroundColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.disabled) ? t.fill : t.accent,
        ),
        foregroundColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.disabled)
              ? t.muted.withValues(alpha: 0.6)
              : t.onAccent,
        ),
        overlayColor: pressedOnAccent,
        elevation: const WidgetStatePropertyAll(0),
        minimumSize: const WidgetStatePropertyAll(Size(64, 48)),
        padding: const WidgetStatePropertyAll(
          EdgeInsets.symmetric(horizontal: 20),
        ),
        textStyle: WidgetStatePropertyAll(TalvioType.button),
        iconSize: const WidgetStatePropertyAll(20),
        shape: const WidgetStatePropertyAll(pill),
        side: focusRing,
        splashFactory: NoSplash.splashFactory,
      ),
    ),
    // Secondary: transparent with a border.
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: ButtonStyle(
        foregroundColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.disabled)
              ? t.ink.withValues(alpha: 0.38)
              : t.ink,
        ),
        overlayColor: pressed,
        minimumSize: const WidgetStatePropertyAll(Size(64, 48)),
        padding: const WidgetStatePropertyAll(
          EdgeInsets.symmetric(horizontal: 20),
        ),
        textStyle: WidgetStatePropertyAll(TalvioType.button),
        iconSize: const WidgetStatePropertyAll(20),
        shape: const WidgetStatePropertyAll(pill),
        side: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.focused)
              ? BorderSide(color: t.accent, width: 2)
              : BorderSide(
                  color: s.contains(WidgetState.disabled)
                      ? t.border.withValues(alpha: 0.5)
                      : t.border,
                ),
        ),
        splashFactory: NoSplash.splashFactory,
      ),
    ),
    // Ghost: text only, accent by default.
    textButtonTheme: TextButtonThemeData(
      style: ButtonStyle(
        foregroundColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.disabled)
              ? t.accent.withValues(alpha: 0.38)
              : t.accent,
        ),
        overlayColor: pressed,
        minimumSize: const WidgetStatePropertyAll(Size(64, 44)),
        padding: const WidgetStatePropertyAll(
          EdgeInsets.symmetric(horizontal: 12),
        ),
        textStyle: WidgetStatePropertyAll(TalvioType.button),
        iconSize: const WidgetStatePropertyAll(20),
        shape: const WidgetStatePropertyAll(pill),
        side: focusRing,
        splashFactory: NoSplash.splashFactory,
      ),
    ),
    iconButtonTheme: IconButtonThemeData(
      style: ButtonStyle(
        foregroundColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.disabled)
              ? t.ink.withValues(alpha: 0.38)
              : t.ink,
        ),
        overlayColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.pressed) ? t.fill : null,
        ),
        iconSize: const WidgetStatePropertyAll(20),
        minimumSize: const WidgetStatePropertyAll(Size.square(44)),
        fixedSize: const WidgetStatePropertyAll(Size.square(44)),
        padding: const WidgetStatePropertyAll(EdgeInsets.zero),
        shape: const WidgetStatePropertyAll(CircleBorder()),
        side: focusRing,
        splashFactory: NoSplash.splashFactory,
      ),
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: t.fill,
      hintStyle: TalvioType.body.copyWith(color: t.muted),
      labelStyle: TalvioType.body.copyWith(color: t.muted),
      floatingLabelBehavior: FloatingLabelBehavior.never,
      helperStyle: TalvioType.footnote.copyWith(color: t.muted),
      errorStyle: TalvioType.footnote.copyWith(color: t.danger),
      errorMaxLines: 3,
      helperMaxLines: 2,
      contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      border: well(),
      enabledBorder: well(),
      disabledBorder: well(),
      focusedBorder: well(t.accent),
      errorBorder: well(t.danger),
      focusedErrorBorder: well(t.danger),
      suffixIconColor: t.muted,
    ),
    listTileTheme: ListTileThemeData(
      iconColor: t.muted,
      textColor: t.ink,
      titleTextStyle: text.titleSmall,
      subtitleTextStyle: text.bodySmall,
      contentPadding: const EdgeInsets.symmetric(horizontal: Space.gutter),
    ),
    bottomSheetTheme: BottomSheetThemeData(
      backgroundColor: t.card,
      surfaceTintColor: Colors.transparent,
      modalBackgroundColor: t.card,
      modalBarrierColor: t.scrim,
      showDragHandle: true,
      dragHandleColor: t.border,
      dragHandleSize: const Size(36, 4),
      elevation: 0,
      modalElevation: 0,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(Radii.xl)),
      ),
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: t.card,
      surfaceTintColor: Colors.transparent,
      barrierColor: t.scrim,
      elevation: 0,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.xl),
      ),
      titleTextStyle: text.titleMedium,
      contentTextStyle: text.bodyMedium?.copyWith(color: t.muted),
      actionsPadding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
    ),
    snackBarTheme: SnackBarThemeData(
      behavior: SnackBarBehavior.floating,
      backgroundColor: t.inverse,
      elevation: 0,
      contentTextStyle: TalvioType.rowBody.copyWith(color: t.onInverse),
      actionTextColor: light ? const Color(0xFFFF8F7C) : t.accent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.md),
      ),
      insetPadding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
    ),
    progressIndicatorTheme: ProgressIndicatorThemeData(
      color: t.accent,
      linearTrackColor: t.fill,
      circularTrackColor: Colors.transparent,
      strokeWidth: 2,
    ),
    tooltipTheme: TooltipThemeData(
      decoration: BoxDecoration(
        color: t.inverse,
        borderRadius: BorderRadius.circular(Radii.xs),
      ),
      textStyle: TalvioType.footnote.copyWith(color: t.onInverse),
    ),
    popupMenuTheme: PopupMenuThemeData(
      color: t.card,
      surfaceTintColor: Colors.transparent,
      elevation: 2,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.md),
      ),
      textStyle: text.titleSmall,
    ),
    scrollbarTheme: ScrollbarThemeData(
      thumbColor: WidgetStatePropertyAll(t.border),
    ),
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
  TextStyle ink(TextStyle s) => s.copyWith(color: t.ink);
  return TextTheme(
    // Off-scale Material slots alias onto the scale.
    displayLarge: ink(TalvioType.title1),
    displayMedium: ink(TalvioType.title1),
    displaySmall: ink(TalvioType.display),
    headlineLarge: ink(TalvioType.title1),
    headlineMedium: ink(TalvioType.title1),
    headlineSmall: ink(TalvioType.title1),
    titleLarge: ink(TalvioType.title2),
    titleMedium: ink(TalvioType.title2),
    titleSmall: ink(TalvioType.rowTitle),
    bodyLarge: ink(TalvioType.body),
    bodyMedium: ink(TalvioType.rowBody),
    bodySmall: TalvioType.footnote.copyWith(color: t.muted),
    labelLarge: ink(TalvioType.button),
    labelMedium: ink(TalvioType.label),
    labelSmall: ink(TalvioType.caption),
  );
}
