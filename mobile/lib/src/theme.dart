import 'package:flutter/material.dart';

// The desktop app's accent (src/App.css --color-accent), light and dark.
const _tealLight = Color(0xFF007E70);
const _tealDark = Color(0xFF5CD6BE);

ThemeData buildTheme(Brightness brightness) {
  final light = brightness == Brightness.light;
  final brand = light ? _tealLight : _tealDark;
  final scheme = ColorScheme.fromSeed(
    seedColor: brand,
    brightness: brightness,
  ).copyWith(primary: brand);
  return ThemeData(
    colorScheme: scheme,
    inputDecorationTheme: const InputDecorationTheme(
      border: OutlineInputBorder(),
    ),
  );
}
