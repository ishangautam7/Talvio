import 'package:flutter/material.dart';

import '../../core/api_client.dart';
import '../../core/app_services.dart';
import '../../core/config.dart';
import '../../core/models.dart';
import '../../core/session_store.dart';
import '../../core/strings.dart';
import '../../design/brand.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
import 'feedback_screen.dart';
import 'strings.dart';

/// Who is signed in, the account's devices, the server, signing out and
/// deleting the account (design spec §4.9). Opened from the avatar in the
/// top bar.
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
  var _scrolled = false;

  void _showError(ApiException error) =>
      showSnack(context, commonErrorText(error));

  Future<void> _remove(Device device) async {
    if (!await confirmDialog(
          context,
          title: AccountStrings.removeTitle(device.name),
          body: AccountStrings.removeBody,
          action: AccountStrings.remove,
          cancel: AccountStrings.cancel,
          destructive: true,
        ) ||
        !mounted) {
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
    if (!await confirmDialog(
          context,
          title: AccountStrings.deleteTitle,
          body: AccountStrings.deleteBody,
          action: AccountStrings.deleteAccount,
          cancel: AccountStrings.cancel,
          destructive: true,
        ) ||
        !mounted) {
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
    final ios = Theme.of(context).platform == TargetPlatform.iOS;

    return Scaffold(
      body: Column(
        children: [
          AnimatedContainer(
            duration: Motion.quick,
            decoration: BoxDecoration(
              color: t.canvas,
              border: Border(
                bottom: BorderSide(color: _scrolled ? t.hairline : t.canvas),
              ),
            ),
            child: SafeArea(
              bottom: false,
              child: SizedBox(
                height: 52,
                child: Padding(
                  padding: const EdgeInsetsDirectional.only(start: 4),
                  child: Row(
                    children: [
                      IconBtn(
                        icon: ios ? AppIcons.backIos : AppIcons.backAndroid,
                        size: IconSize.large,
                        tooltip: MaterialLocalizations.of(context)
                            .backButtonTooltip,
                        onPressed: () => Navigator.maybePop(context),
                      ),
                      const SizedBox(width: Space.s4),
                      Semantics(
                        header: true,
                        child: Text(
                          AccountStrings.title,
                          style: TalvioType.title2.copyWith(color: t.ink),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ),
          Expanded(
            child: NotificationListener<ScrollNotification>(
              onNotification: (n) {
                final scrolled = n.metrics.extentBefore > 0.5;
                if (scrolled != _scrolled) {
                  setState(() => _scrolled = scrolled);
                }
                return false;
              },
              child: ListenableBuilder(
                listenable: Listenable.merge([session, relay.devices]),
                builder: (context, _) {
                  final user = session.session?.user;
                  final devices = relay.devices.value;
                  final now = DateTime.now();
                  final url = session.session?.serverUrl;
                  return ListView(
                    padding: EdgeInsets.only(
                      top: Space.s8,
                      bottom: MediaQuery.paddingOf(context).bottom + 24,
                    ),
                    children: [
                      if (user != null) _Profile(user: user),
                      const SectionLabel(AccountStrings.devices, first: true),
                      Group(
                        children: [
                          if (devices == null)
                            ListRow(
                              leading: const Spinner(size: 18),
                              title: AccountStrings.connecting,
                              titleStyle: TalvioType.rowBody.copyWith(
                                color: t.muted,
                              ),
                            )
                          else
                            for (final device in devices)
                              ListRow(
                                leading: RowTile(
                                  platformIcon(device.platform),
                                  presence: device.online || device.current
                                      ? t.success
                                      : t.faint,
                                ),
                                title: device.name,
                                subtitle: device.current
                                    ? AccountStrings.thisPhone
                                    : device.online
                                    ? AccountStrings.online
                                    : AccountStrings.lastSeen(
                                        device.lastSeenAt,
                                        now,
                                      ),
                                trailing: device.current
                                    ? null
                                    : _removing.contains(device.id)
                                    ? const SizedBox.square(
                                        dimension: 44,
                                        child: Center(child: Spinner(size: 18)),
                                      )
                                    : IconBtn(
                                        tooltip: AccountStrings.removeTooltip(
                                          device.name,
                                        ),
                                        icon: AppIcons.remove,
                                        color: t.muted,
                                        onPressed: busy
                                            ? null
                                            : () => _remove(device),
                                      ),
                              ),
                        ],
                      ),
                      if (url != null) ...[
                        const SectionLabel(AccountStrings.server),
                        Group(
                          children: [
                            ListRow(
                              leading: const RowGlyph(AppIcons.server),
                              title: session.serverChoice == ServerChoice.pinned
                                  ? AccountStrings.developmentServer
                                  : AccountStrings.server,
                              subtitle: Uri.parse(url).authority,
                              subtitleStyle: TalvioType.mono.copyWith(
                                color: t.muted,
                              ),
                            ),
                          ],
                        ),
                        const GroupFooter(AccountStrings.serverNote),
                      ],
                      const SizedBox(height: Space.s24),
                      Group(
                        children: [
                          ListRow(
                            leading: RowGlyph(AppIcons.feedback, color: t.ink),
                            title: AccountStrings.sendFeedback,
                            semanticLabel: AccountStrings.sendFeedback,
                            enabled: !busy,
                            onTap: busy
                                ? null
                                : () => Navigator.of(context).push(
                                    MaterialPageRoute<void>(
                                      builder: (_) => const FeedbackScreen(),
                                    ),
                                  ),
                          ),
                        ],
                      ),
                      const SizedBox(height: Space.s24),
                      Group(
                        children: [
                          ListRow(
                            leading: RowGlyph(AppIcons.signOut, color: t.ink),
                            title: AccountStrings.signOut,
                            semanticLabel: AccountStrings.signOut,
                            enabled: !busy,
                            trailing: _signingOut
                                ? const Spinner(size: 18)
                                : null,
                            onTap: busy ? null : _signOut,
                          ),
                          ListRow(
                            leading: RowGlyph(AppIcons.delete, color: t.danger),
                            title: AccountStrings.deleteAccount,
                            titleStyle: TalvioType.rowTitle.copyWith(
                              color: t.danger,
                            ),
                            semanticLabel: AccountStrings.deleteAccount,
                            enabled: !busy,
                            trailing: _deleting
                                ? Spinner(size: 18, color: t.danger)
                                : null,
                            onTap: busy ? null : _deleteAccount,
                          ),
                        ],
                      ),
                      const SizedBox(height: Space.s32),
                      Center(
                        child: Column(
                          children: [
                            const LogoLockup(size: 20),
                            const SizedBox(height: Space.s8),
                            Text(
                              AccountStrings.version(appVersion),
                              style: TalvioType.footnote.copyWith(
                                color: t.muted,
                              ),
                            ),
                            const SizedBox(height: Space.s4),
                            Btn(
                              AccountStrings.licences,
                              kind: BtnKind.ghostNeutral,
                              compact: true,
                              foreground: t.muted,
                              onPressed: () => showLicensePage(
                                context: context,
                                applicationName: 'Talvio',
                                applicationVersion: appVersion,
                                applicationIcon: const Padding(
                                  padding: EdgeInsets.all(Space.s12),
                                  child: TalvioMark(size: 48),
                                ),
                              ),
                            ),
                          ],
                        ),
                      ),
                    ],
                  );
                },
              ),
            ),
          ),
        ],
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
    return Padding(
      padding: const EdgeInsets.fromLTRB(32, 8, 32, 8),
      child: Column(
        children: [
          Container(
            width: 64,
            height: 64,
            alignment: Alignment.center,
            decoration: const BoxDecoration(
              gradient: TalvioColors.gradient,
              shape: BoxShape.circle,
            ),
            child: Text(
              shown.characters.first.toUpperCase(),
              style: TalvioType.title1.copyWith(color: Colors.white),
            ),
          ),
          const SizedBox(height: Space.s12),
          Text(
            shown,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            textAlign: TextAlign.center,
            style: TalvioType.title2.copyWith(color: t.ink),
          ),
          if (shown != user.email) ...[
            const SizedBox(height: Space.s2),
            Text(
              user.email,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              textAlign: TextAlign.center,
              style: TalvioType.footnote.copyWith(color: t.muted),
            ),
          ],
        ],
      ),
    );
  }
}
