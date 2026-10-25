import 'package:flutter/material.dart';

import '../design/components.dart';

export '../design/components.dart' show ButtonSpinner;

/// A list section's heading.
class SectionTitle extends StatelessWidget {
  const SectionTitle(this.text, {super.key, this.trailing});

  final String text;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) => SectionHeader(text, trailing: trailing);
}
