import 'package:flutter/material.dart';

import '../../core/api_client.dart';
import '../../core/app_services.dart';
import '../../core/config.dart';
import '../../core/models.dart';
import '../../core/strings.dart';
import '../../design/brand.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
import '../dictate/computer_widgets.dart' show ComputerAvatar;
import 'strings.dart';

/// Who is signed in, the account's devices, the server, signing out and
/// deleting the account. Opened from the avatar in the top bar.
class AccountScreen extends StatefulWidget {
  const AccountScreen({super.key});

  @override
  State<AccountScreen> createState() => _AccountScreenState();
}

class _AccountScreenState extends State<AccountScreen> {
  late final AppServices _services = AppServices.of(context);
  final _removing = <String>{};
  var _signingOut = false;
  var _deleting = false;

  void _showError(ApiException error) {
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(commonErrorText(error))));
  }

  Future<bool> _confirm({
    required String title,
    required String body,
    required String action,
    bool destructive = false,
  }) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(title),
        content: Text(body),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            style: TextButton.styleFrom(foregroundColor: context.talvio.muted),
            child: const Text(AccountStrings.cancel),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, true),
            style: destructive
                ? TextButton.styleFrom(foregroundColor: context.talvio.danger)
                : null,
            child: Text(action),
          ),
        ],
      ),
    );
    return confirmed == true && mounted;
  }

  Future<void> _remove(Device device) async {
    if (!await _confirm(
      title: AccountStrings.removeTitle(device.name),
      body: AccountStrings.removeBody,
      action: AccountStrings.remove,
    )) {
      return;
    }
    setState(() => _removing.add(device.id));
    try {
      // The `devices` message that follows takes it off the list.
      await _services.api.removeDevice(device.id);
    } on ApiException catch (e) {
      // not_found: it was removed already.
      if (e.code != 'not_found' && mounted) _showError(e);
    } finally {
      if (mounted) setState(() => _removing.remove(device.id));
    }
  }

  Future<void> _signOut() async {
    Haptic.tick();
    setState(() => _signingOut = true);
    try {
      await _services.api.logOut();
    } on ApiException {
      // Signed out here regardless. If the server never heard, this phone
      // stays in the device list until it is removed from another device.
    }
    await _services.session.clear();
  }

  Future<void> _deleteAccount() async {
    if (!await _confirm(
      title: AccountStrings.deleteTitle,
      body: AccountStrings.deleteBody,
      action: AccountStrings.deleteAccount,
      destructive: true,
    )) {
      return;
    }
    setState(() => _deleting = true);
    try {
      await _services.api.deleteAccount();
      await _services.session.clear();
    } on ApiException catch (e) {
      if (mounted) {
        setState(() => _deleting = false);
        _showError(e);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final session = _services.session;
    final relay = _services.relay;
    final busy = _signingOut || _deleting;

    return Scaffold(
      appBar: AppBar(title: const Text(AccountStrings.title)),
      body: ListenableBuilder(
        listenable: Listenable.merge([session, relay.devices]),
        builder: (context, _) {
          final user = session.session?.user;
          final devices = relay.devices.value;
          final now = DateTime.now();
          return ListView(
            padding: const EdgeInsets.fromLTRB(
              Space.lg,
              Space.sm,
              Space.lg,
              Space.xxxl,
            ),
            children: [
              if (user != null) _Profile(user: user),
              const SectionHeader(AccountStrings.devices),
              if (devices == null)
                const Notice(
                  busy: true,
                  tone: Tone.neutral,
                  message: AccountStrings.connecting,
                )
              else
                Surface(
                  padding: EdgeInsets.zero,
                  child: Column(
                    children: [
                      for (final (i, device) in devices.indexed) ...[
                        if (i > 0) Divider(indent: 72, color: t.hairline),
                        ListTile(
                          minVerticalPadding: 10,
                          contentPadding: const EdgeInsets.only(
                            left: Space.lg,
                            right: Space.xs,
                          ),
                          leading: ComputerAvatar(device, size: 40),
                          title: Text(
                            device.name,
                            style: context.text.titleSmall,
                          ),
                          subtitle: Text(
                            device.current
                                ? AccountStrings.thisPhone
                                : device.online
                                ? AccountStrings.online
                                : AccountStrings.lastSeen(
                                    device.lastSeenAt,
                                    now,
                                  ),
                          ),
                          trailing: device.current
                              ? const Pill(
                                  AccountStrings.thisPhoneShort,
                                  tone: Tone.accent,
                                )
                              : _removing.contains(device.id)
                              ? const Padding(
                                  padding: EdgeInsets.all(12),
                                  child: ButtonSpinner(),
                                )
                              : IconButton(
                                  tooltip: AccountStrings.removeTooltip(
                                    device.name,
                                  ),
                                  icon: Icon(
                                    Icons.remove_circle_outline_rounded,
                                    color: t.muted,
                                  ),
                                  onPressed: busy
                                      ? null
                                      : () => _remove(device),
                                ),
                        ),
                      ],
                    ],
                  ),
                ),
              const SectionHeader(AccountStrings.server),
              if (session.session?.serverUrl case final url?)
                Surface(
                  padding: EdgeInsets.zero,
                  child: ListTile(
                    leading: const IconTile(Icons.dns_outlined, size: 36),
                    title: Text(
                      serverText(session.serverChoice, url),
                      style: context.text.bodyMedium,
                    ),
                    subtitle: const Text(AccountStrings.serverNote),
                  ),
                ),
              const SizedBox(height: Space.xxl),
              OutlinedButton.icon(
                onPressed: busy ? null : _signOut,
                style: OutlinedButton.styleFrom(
                  minimumSize: const Size.fromHeight(52),
                ),
                icon: _signingOut
                    ? const SizedBox.shrink()
                    : const Icon(Icons.logout_rounded),
                label: _signingOut
                    ? const ButtonSpinner()
                    : const Text(AccountStrings.signOut),
              ),
              const SizedBox(height: Space.sm),
              TextButton(
                onPressed: busy ? null : _deleteAccount,
                style: TextButton.styleFrom(
                  foregroundColor: t.danger,
                  minimumSize: const Size.fromHeight(48),
                ),
                child: _deleting
                    ? const ButtonSpinner()
                    : const Text(AccountStrings.deleteAccount),
              ),
              const SizedBox(height: Space.xxl),
              Center(
                child: Column(
                  children: [
                    const LogoLockup(size: 22),
                    const SizedBox(height: Space.sm),
                    Text(
                      AccountStrings.version(appVersion),
                      style: context.text.bodySmall?.copyWith(color: t.faint),
                    ),
                    TextButton(
                      onPressed: () => showLicensePage(
                        context: context,
                        applicationName: 'Talvio',
                        applicationVersion: appVersion,
                        applicationIcon: const Padding(
                          padding: EdgeInsets.all(Space.md),
                          child: TalvioMark(size: 48),
                        ),
                      ),
                      style: TextButton.styleFrom(foregroundColor: t.muted),
                      child: const Text(AccountStrings.licences),
                    ),
                  ],
                ),
              ),
            ],
          );
        },
      ),
    );
  }
}

class _Profile extends StatelessWidget {
  const _Profile({required this.user});

  final User user;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final name = user.name?.trim();
    final shown = name == null || name.isEmpty ? user.email : name;
    return Surface(
      padding: const EdgeInsets.all(Space.lg),
      child: Row(
        children: [
          Container(
            width: 52,
            height: 52,
            alignment: Alignment.center,
            decoration: const BoxDecoration(
              gradient: TalvioColors.gradient,
              shape: BoxShape.circle,
            ),
            child: Text(
              shown.characters.first.toUpperCase(),
              style: context.text.headlineSmall?.copyWith(color: Colors.white),
            ),
          ),
          const SizedBox(width: Space.lg),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  shown,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.titleLarge,
                ),
                if (shown != user.email)
                  Text(
                    user.email,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: context.text.bodyMedium?.copyWith(color: t.muted),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
