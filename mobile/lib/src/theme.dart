import 'package:flutter/material.dart';

// Talvio's colours (docs/talvio-brand.md §5): the coral accent, the ink text
// and the warm paper canvas, light and dark. The full redesign comes later;
// this only puts the brand on Material's generated scheme.
const _coralLight = Color(0xFFF2543D);
const _coralDark = Color(0xFFFF6A52);
const _inkLight = Color(0xFF1B1D33);
const _inkDark = Color(0xFFF2F1F7);
const _canvasLight = Color(0xFFFBF7F3);
const _canvasDark = Color(0xFF101122);

ThemeData buildTheme(Brightness brightness) {
  final light = brightness == Brightness.light;
  final brand = light ? _coralLight : _coralDark;
  final scheme = ColorScheme.fromSeed(
    seedColor: brand,
    brightness: brightness,
  ).copyWith(
    primary: brand,
    surface: light ? _canvasLight : _canvasDark,
    onSurface: light ? _inkLight : _inkDark,
  );
  return ThemeData(
    colorScheme: scheme,
    scaffoldBackgroundColor: scheme.surface,
    inputDecorationTheme: const InputDecorationTheme(
      border: OutlineInputBorder(),
    ),
  );
}
