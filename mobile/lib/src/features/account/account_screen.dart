import 'package:flutter/material.dart';

import '../../core/api_client.dart';
import '../../core/app_services.dart';
import '../../core/device_icon.dart';
import '../../core/models.dart';
import '../../core/strings.dart';
import '../../core/widgets.dart';
import '../../shell/feature_tab.dart';
import '../../shell/shell_scope.dart';
import 'strings.dart';

final accountTab = FeatureTab(
  id: 'account',
  label: AccountStrings.tab,
  icon: Icons.person_outline,
  selectedIcon: Icons.person,
  secondary: true,
  builder: (_) => const AccountScreen(),
);

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
            child: const Text(AccountStrings.cancel),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, true),
            style: destructive
                ? TextButton.styleFrom(
                    foregroundColor: Theme.of(context).colorScheme.error,
                  )
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
    final theme = Theme.of(context);
    final session = _services.session;
    final relay = _services.relay;
    final busy = _signingOut || _deleting;

    return Scaffold(
      appBar: AppBar(
        leading: shellLeading(context),
        title: const Text(AccountStrings.title),
      ),
      body: ListenableBuilder(
        listenable: Listenable.merge([session, relay.devices]),
        builder: (context, _) {
          final user = session.session?.user;
          final devices = relay.devices.value;
          final now = DateTime.now();
          return ListView(
            children: [
              if (user != null)
                ListTile(
                  leading: CircleAvatar(
                    child: Text(
                      (user.name ?? user.email).characters.first.toUpperCase(),
                    ),
                  ),
                  title: Text(user.name ?? user.email),
                  subtitle: user.name == null ? null : Text(user.email),
                ),
              SectionTitle(AccountStrings.devices),
              if (devices == null)
                const ListTile(
                  leading: SizedBox.square(
                    dimension: 24,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  ),
                  title: Text(AccountStrings.connecting),
                )
              else
                for (final device in devices)
                  ListTile(
                    leading: DeviceIcon(device),
                    title: Text(device.name),
                    subtitle: Text(
                      device.current
                          ? AccountStrings.thisPhone
                          : device.online
                          ? AccountStrings.online
                          : AccountStrings.lastSeen(device.lastSeenAt, now),
                    ),
                    trailing: device.current
                        ? null
                        : _removing.contains(device.id)
                        ? const SizedBox.square(
                            dimension: 24,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : IconButton(
                            tooltip: AccountStrings.removeTooltip(device.name),
                            icon: const Icon(Icons.remove_circle_outline),
                            onPressed: busy ? null : () => _remove(device),
                          ),
                  ),
              SectionTitle(AccountStrings.server),
              if (session.session?.serverUrl case final url?)
                ListTile(
                  leading: const Icon(Icons.dns_outlined),
                  title: Text(serverText(session.serverChoice, url)),
                ),
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 24, 16, 24),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    OutlinedButton(
                      onPressed: busy ? null : _signOut,
                      style: OutlinedButton.styleFrom(
                        minimumSize: const Size.fromHeight(48),
                      ),
                      child: _signingOut
                          ? const ButtonSpinner()
                          : const Text(AccountStrings.signOut),
                    ),
                    const SizedBox(height: 8),
                    TextButton(
                      onPressed: busy ? null : _deleteAccount,
                      style: TextButton.styleFrom(
                        foregroundColor: theme.colorScheme.error,
                        minimumSize: const Size.fromHeight(48),
                      ),
                      child: _deleting
                          ? const ButtonSpinner()
                          : const Text(AccountStrings.deleteAccount),
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
