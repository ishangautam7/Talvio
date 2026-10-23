import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:zynveliq_mobile/src/core/config.dart';

void main() {
  test('normalizeServerUrl keeps http(s) base URLs in one form', () {
    expect(
      normalizeServerUrl(' https://sync.example.com/ '),
      'https://sync.example.com',
    );
    expect(
      normalizeServerUrl('http://192.168.1.10:8787'),
      'http://192.168.1.10:8787',
    );
    expect(
      normalizeServerUrl('https://example.com/zynveliq//'),
      'https://example.com/zynveliq',
    );
    expect(normalizeServerUrl('sync.example.com'), 'https://sync.example.com');
    expect(
      normalizeServerUrl('sync.example.com:8787'),
      'https://sync.example.com:8787',
    );
  });

  test('normalizeServerUrl takes https only in release builds', () {
    expect(
      normalizeServerUrl('https://sync.example.com', release: true),
      'https://sync.example.com',
    );
    expect(
      normalizeServerUrl('sync.example.com', release: true),
      'https://sync.example.com',
    );
    for (final input in [
      'http://192.168.1.10:8787',
      'http://10.0.2.2:8787',
      'http://sync.example.com',
    ]) {
      expect(normalizeServerUrl(input, release: true), isNull, reason: input);
    }
  });

  test('normalizeServerUrl rejects anything that is not a base URL', () {
    for (final input in [
      '',
      '   ',
      'ftp://sync.example.com',
      'https://',
      'https://user:pw@sync.example.com',
      'https://sync.example.com/?a=1',
      'https://sync.example.com/#x',
    ]) {
      expect(normalizeServerUrl(input), isNull, reason: input);
    }
  });

  test('appVersion matches pubspec.yaml', () {
    final version = RegExp(
      r'^version: ([^+\s]+)',
      multiLine: true,
    ).firstMatch(File('pubspec.yaml').readAsStringSync())!.group(1);
    expect(appVersion, version);
  });
}
