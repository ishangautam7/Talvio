// The computer this phone sends to, as every page shows it: the app bar's
// title and its chooser, the menu's header, and the states before there is
// one to send to.

import 'package:flutter/material.dart';

import '../../core/app_services.dart';
import '../../core/device_icon.dart';
import '../../core/models.dart';
import '../../core/relay_client.dart';
import '../../shell/strings.dart';
import 'dictate_controller.dart';
import 'dictate_scope.dart';
import 'remote_windows.dart';
import 'strings.dart';

TextStyle? mutedText(BuildContext context) => Theme.of(context)
    .textTheme
    .bodyMedium
    ?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant);

/// A window as two lines: the app, and its title (null when the app is all
/// there is to say).
(String, String?) windowLabel(RemoteWindow window) => switch (window) {
  RemoteWindow(app: '', title: '') => (DictateStrings.untitled, null),
  RemoteWindow(app: '', :final title) => (title, null),
  RemoteWindow(:final app, title: '') => (app, null),
  RemoteWindow(:final app, :final title) => (app, title),
};

/// Whether the user may switch computers now: there is more than one, and
/// no recording or request is holding on to this one.
bool canChooseComputer(DictateController controller) =>
    controller.computers.length > 1 && controller.phase == DictatePhase.idle;

/// The sheet that switches the computer to send to.
Future<void> chooseComputer(
  BuildContext context,
  DictateController controller,
) => showModalBottomSheet<void>(
  context: context,
  showDragHandle: true,
  builder: (context) => SafeArea(
    child: ListView(
      shrinkWrap: true,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(24, 0, 24, 8),
          child: Text(
            DictateStrings.chooseComputer,
            style: Theme.of(context).textTheme.titleMedium,
          ),
        ),
        for (final computer in controller.computers)
          ComputerTile(
            computer,
            selected: computer.id == controller.computer?.id,
            onTap: () {
              controller.choose(computer.id);
              Navigator.pop(context);
            },
          ),
      ],
    ),
  ),
);

/// An app bar's title: the computer, with its state, opening the chooser
/// when [onTap] is set.
class ComputerTitle extends StatelessWidget {
  const ComputerTitle(this.computer, {super.key, required this.onTap});

  final Device computer;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Semantics(
      button: onTap != null,
      label: DictateStrings.chooseComputer,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(8),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              DeviceIcon(computer),
              const SizedBox(width: 12),
              Flexible(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      computer.name,
                      overflow: TextOverflow.ellipsis,
                      style: theme.textTheme.titleMedium,
                    ),
                    Text(
                      computer.online
                          ? DictateStrings.online
                          : DictateStrings.offline,
                      style: theme.textTheme.bodySmall?.copyWith(
                        color: theme.colorScheme.onSurfaceVariant,
                      ),
                    ),
                  ],
                ),
              ),
              if (onTap != null) const Icon(Icons.arrow_drop_down),
            ],
          ),
        ),
      ),
    );
  }
}

/// The top of the menu: the app, who is signed in, and the computer.
class MenuHeader extends StatelessWidget {
  const MenuHeader({super.key});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final services = AppServices.of(context);
    final controller = DictateScope.of(context);
    return ListenableBuilder(
      listenable: Listenable.merge([
        controller,
        services.relay.devices,
        services.session,
      ]),
      builder: (context, _) {
        final user = services.session.session?.user;
        final computer = controller.computer;
        final choose = canChooseComputer(controller)
            ? () => chooseComputer(context, controller)
            : null;
        return Padding(
          padding: const EdgeInsets.fromLTRB(28, 20, 16, 12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                ShellStrings.appName,
                style: theme.textTheme.headlineSmall?.copyWith(
                  fontWeight: FontWeight.w800,
                  color: theme.colorScheme.primary,
                  letterSpacing: -0.5,
                ),
              ),
              if (user != null)
                Text(
                  user.email,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: mutedText(context),
                ),
              const SizedBox(height: 16),
              if (computer != null)
                Material(
                  color: theme.colorScheme.surfaceContainerHigh,
                  borderRadius: BorderRadius.circular(16),
                  clipBehavior: Clip.antiAlias,
                  child: Padding(
                    padding: const EdgeInsets.symmetric(vertical: 4),
                    child: Row(
                      children: [
                        Expanded(child: ComputerTitle(computer, onTap: choose)),
                      ],
                    ),
                  ),
                ),
            ],
          ),
        );
      },
    );
  }
}

class ConnectionBanner extends StatelessWidget {
  const ConnectionBanner({super.key});

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Theme.of(context).colorScheme.surfaceContainerHighest,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
        child: Row(
          children: [
            const SizedBox.square(
              dimension: 16,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Text(
                DictateStrings.connecting,
                style: Theme.of(context).textTheme.bodyMedium,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class ComputerTile extends StatelessWidget {
  const ComputerTile(
    this.computer, {
    super.key,
    required this.selected,
    required this.onTap,
  });

  final Device computer;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return ListTile(
      contentPadding: const EdgeInsets.symmetric(horizontal: 24),
      leading: DeviceIcon(computer),
      title: Text(computer.name),
      subtitle: Text(
        computer.online ? DictateStrings.online : DictateStrings.offline,
      ),
      trailing: selected ? const Icon(Icons.check) : null,
      onTap: onTap,
    );
  }
}

/// Several computers and no choice yet (or the chosen one left the account).
class PickComputer extends StatelessWidget {
  const PickComputer(this.computers, {super.key, required this.onPick});

  final List<Device> computers;
  final ValueChanged<String> onPick;

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.symmetric(vertical: 16),
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(24, 8, 24, 4),
          child: Text(
            DictateStrings.pickComputerTitle,
            style: Theme.of(context).textTheme.titleLarge,
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(24, 0, 24, 8),
          child: Text(
            DictateStrings.pickComputerBody,
            style: mutedText(context),
          ),
        ),
        for (final computer in computers)
          ComputerTile(
            computer,
            selected: false,
            onTap: () => onPick(computer.id),
          ),
      ],
    );
  }
}

class NoComputer extends StatelessWidget {
  const NoComputer({super.key, required this.email});

  final String email;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              Icons.computer_outlined,
              size: 48,
              color: theme.colorScheme.onSurfaceVariant,
            ),
            const SizedBox(height: 16),
            Text(
              DictateStrings.noComputerTitle,
              textAlign: TextAlign.center,
              style: theme.textTheme.titleLarge,
            ),
            const SizedBox(height: 8),
            Text(
              DictateStrings.noComputerBody(email),
              textAlign: TextAlign.center,
              style: mutedText(context),
            ),
          ],
        ),
      ),
    );
  }
}

/// The body every page shows until there is a computer: a spinner before
/// the device list arrives, the chooser when the user has to pick, how to
/// add one when there is none; else [build] with the computer. Above it, a
/// banner while the socket is not connected.
class ComputerGate extends StatelessWidget {
  const ComputerGate({
    super.key,
    required this.controller,
    required this.builder,
  });

  final DictateController controller;
  final Widget Function(BuildContext context, Device computer, bool connected)
  builder;

  @override
  Widget build(BuildContext context) {
    final services = AppServices.of(context);
    final relay = services.relay;
    final connected = relay.status.value == RelayStatus.connected;
    final known = relay.devices.value != null;
    final computers = controller.computers;
    return Column(
      children: [
        if (!connected) const ConnectionBanner(),
        Expanded(
          child: switch ((known, controller.computer)) {
            (false, _) => const Center(child: CircularProgressIndicator()),
            (true, null) when computers.isNotEmpty => PickComputer(
              computers,
              onPick: controller.choose,
            ),
            (true, null) => NoComputer(
              email: services.session.session?.user.email ?? '',
            ),
            (true, final Device computer) => builder(
              context,
              computer,
              connected,
            ),
          },
        ),
      ],
    );
  }
}
