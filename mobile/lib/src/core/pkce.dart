import 'dart:convert';
import 'dart:math';

import 'package:crypto/crypto.dart';

/// A PKCE pair (RFC 7636, S256): the verifier stays on the phone, the
/// challenge goes to the server, so only this phone can redeem the login code.
class Pkce {
  Pkce._(this.verifier) : challenge = challengeFor(verifier);

  /// 32 random bytes, base64url without padding.
  factory Pkce.generate() {
    final random = Random.secure();
    return Pkce._(
      _base64Url(List<int>.generate(32, (_) => random.nextInt(256))),
    );
  }

  final String verifier;
  final String challenge;

  static String challengeFor(String verifier) =>
      _base64Url(sha256.convert(ascii.encode(verifier)).bytes);

  static String _base64Url(List<int> bytes) =>
      base64Url.encode(bytes).replaceAll('=', '');
}
