import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:talvio_mobile/src/shell/feature_tab.dart';

import '../helpers.dart';

FeatureTab tab({String? requires}) => FeatureTab(
  id: 'x',
  label: 'X',
  icon: Icons.star_outline,
  selectedIcon: Icons.star,
  requires: requires,
  builder: (_) => const SizedBox(),
);

void main() {
  test('a tab with no requirement always shows', () {
    expect(tab().availableWith([]), isTrue);
  });

  test('a capability tab hides only when online computers lack it', () {
    final agent = tab(requires: 'agent.prompt');
    expect(agent.availableWith([]), isTrue, reason: 'nothing known yet');
    expect(
      agent.availableWith([device(online: false)]),
      isTrue,
      reason: 'offline computers advertise nothing',
    );
    expect(agent.availableWith([device()]), isFalse);
    expect(
      agent.availableWith([device(capabilities: [])]),
      isTrue,
      reason: 'online but not past its hello yet',
    );
    expect(
      agent.availableWith([device(), device(id: 'b', capabilities: [])]),
      isTrue,
    );
    expect(
      agent.availableWith([
        device(),
        device(id: 'b', capabilities: ['dictate', 'agent.prompt']),
      ]),
      isTrue,
    );
    expect(
      agent.availableWith([
        device(id: 'phone', kind: 'mobile', capabilities: ['agent.prompt']),
        device(),
      ]),
      isFalse,
      reason: 'only computers count',
    );
  });
}
