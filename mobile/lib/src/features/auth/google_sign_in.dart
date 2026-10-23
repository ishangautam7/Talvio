import 'package:flutter/services.dart';
import 'package:flutter_web_auth_2/flutter_web_auth_2.dart';

import '../../core/api_client.dart';
import '../../core/config.dart';
import '../../core/models.dart';
import '../../core/pkce.dart';

/// The phone's redirect target (PROTOCOL.md, "Google sign-in").
const googleRedirectUri = 'talvio://auth/callback';

/// Google sign-in through an auth session (Custom Tabs on Android,
/// ASWebAuthenticationSession on iOS). Null when the person backed out.
Future<AuthResponse?> signInWithGoogle(ApiClient api) async {
  final pkce = Pkce.generate();
  final String callback;
  try {
    callback = await FlutterWebAuth2.authenticate(
      url: api
          .googleStartUri(
            redirectUri: googleRedirectUri,
            challenge: pkce.challenge,
          )
          .toString(),
      callbackUrlScheme: 'talvio',
    );
  } on PlatformException catch (e) {
    if (e.code == 'CANCELED') return null;
    throw const ApiException('google_failed');
  }

  final params = Uri.parse(callback).queryParameters;
  final error = params['error'];
  final code = params['code'];
  if (error == 'access_denied') return null;
  if (error != null || code == null || code.isEmpty) {
    throw ApiException(error ?? 'google_failed');
  }
  return api.exchange(
    code: code,
    codeVerifier: pkce.verifier,
    device: thisDevice(),
  );
}
