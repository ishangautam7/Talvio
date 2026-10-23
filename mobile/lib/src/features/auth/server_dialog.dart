import 'package:flutter/material.dart';

import '../../core/config.dart';
import 'strings.dart';

/// What the Server setting was changed to: the [custom] URL typed, or
/// Automatic when it is null.
typedef ServerSetting = ({String? custom});

/// Asks for the account server's address. Pops a [ServerSetting], or null
/// when cancelled.
class ServerDialog extends StatefulWidget {
  const ServerDialog({super.key, this.current});

  /// The Custom server in use; null in Automatic mode.
  final String? current;

  @override
  State<ServerDialog> createState() => _ServerDialogState();
}

class _ServerDialogState extends State<ServerDialog> {
  final _formKey = GlobalKey<FormState>();
  late final _controller = TextEditingController(text: widget.current ?? '');

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _save() {
    if (!_formKey.currentState!.validate()) return;
    Navigator.pop<ServerSetting>(context, (
      custom: normalizeServerUrl(_controller.text),
    ));
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text(AuthStrings.serverTitle),
      content: Form(
        key: _formKey,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Text(AuthStrings.serverHelp),
            const SizedBox(height: 16),
            TextFormField(
              controller: _controller,
              autofocus: true,
              keyboardType: TextInputType.url,
              autocorrect: false,
              enableSuggestions: false,
              textInputAction: TextInputAction.done,
              decoration: const InputDecoration(
                labelText: AuthStrings.serverLabel,
                hintText: AuthStrings.serverHint,
              ),
              validator: (value) => normalizeServerUrl(value ?? '') == null
                  ? AuthStrings.serverInvalid
                  : null,
              onFieldSubmitted: (_) => _save(),
            ),
          ],
        ),
      ),
      actions: [
        if (widget.current != null)
          TextButton(
            onPressed: () =>
                Navigator.pop<ServerSetting>(context, (custom: null)),
            child: const Text(AuthStrings.useAutomatic),
          ),
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text(AuthStrings.cancel),
        ),
        TextButton(onPressed: _save, child: const Text(AuthStrings.save)),
      ],
    );
  }
}
