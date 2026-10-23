import 'package:flutter_test/flutter_test.dart';
import 'package:talvio_mobile/src/core/pkce.dart';

void main() {
  test('S256 challenge matches RFC 7636 appendix B', () {
    expect(
      Pkce.challengeFor('dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk'),
      'E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM',
    );
  });

  test('verifier is 32 random bytes, base64url without padding', () {
    final a = Pkce.generate();
    final b = Pkce.generate();
    expect(a.verifier, matches(RegExp(r'^[A-Za-z0-9_-]{43}$')));
    expect(a.challenge, matches(RegExp(r'^[A-Za-z0-9_-]{43}$')));
    expect(a.challenge, Pkce.challengeFor(a.verifier));
    expect(a.verifier, isNot(b.verifier));
  });
}
