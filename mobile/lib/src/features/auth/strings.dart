import '../../core/api_client.dart';
import '../../core/strings.dart';

abstract final class AuthStrings {
  static const signInTitle = 'Sign in to Talvio';
  static const createTitle = 'Create your account';
  static const subtitle =
      'Use the same account on this phone and your computer.';
  static const signInTab = 'Sign in';
  static const createTab = 'Create account';
  static const name = 'Name (optional)';
  static const email = 'Email';
  static const password = 'Password';
  static const passwordHelp = 'At least 8 characters';
  static const showPassword = 'Show password';
  static const hidePassword = 'Hide password';
  static const signIn = 'Sign in';
  static const createAccount = 'Create account';
  static const or = 'or';
  static const google = 'Continue with Google';
  static const serverLooking = 'Looking for the server…';
  static const serverNotFound = "Can't find the server";
  static String serverProblem(String detail) => 'Reason: $detail';
  static const tryAgain = 'Try again';

  static const emailMissing = 'Enter your email.';
  static const emailInvalid = 'Enter a valid email address.';
  static const passwordMissing = 'Enter your password.';
  static const passwordShort = 'Use at least 8 characters.';
  static const passwordLong = 'Use 256 characters or fewer.';
  static const nameLong = 'Use 64 characters or fewer.';

  static String error(ApiException error) => switch (error.code) {
    'invalid_email' => emailInvalid,
    'weak_password' => 'Use at least 8 characters for your password.',
    'invalid_credentials' =>
      "That email and password don't match. If you signed up with Google, "
          'use Continue with Google.',
    'email_taken' =>
      'There is already an account with this email. Sign in instead.',
    'invalid_code' => 'Google sign-in took too long. Try again.',
    'google_unavailable' => "This server doesn't offer Google sign-in.",
    'google_failed' => "Google sign-in didn't work. Try again.",
    'account_conflict' =>
      'The account with this email is linked to a different Google account. '
          'Sign in with that one instead.',
    _ => commonErrorText(error),
  };
}
