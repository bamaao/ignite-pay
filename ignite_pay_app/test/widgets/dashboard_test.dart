// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ignite_pay_app/main.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/services/session_key_service.dart';
import 'package:provider/provider.dart';

void main() {
  // The dashboard has an animated ConnectionDot that never settles,
  // so use pump(duration) instead of pumpAndSettle().
  Future<void> pumpDashboard(WidgetTester tester) async {
    tester.view.physicalSize = const Size(800, 1400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(() {
      tester.view.resetPhysicalSize();
      tester.view.resetDevicePixelRatio();
    });

    // Reset the singleton so it is not in a disposed state from a prior test.
    DidcommService.resetInstance();

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider(create: (_) => DidcommService()),
          ChangeNotifierProvider<SessionKeyService>.value(
            value: SessionKeyService(),
          ),
        ],
        child: const MaterialApp(home: IgnitePayDashboard()),
      ),
    );
    await tester.pump(const Duration(milliseconds: 100));
  }

  group('IgnitePayDashboard', () {
    testWidgets('renders header with Ignite Pay title', (tester) async {
      await pumpDashboard(tester);
      expect(find.text('Ignite Pay'), findsOneWidget);
    });

    testWidgets('renders Devnet badge', (tester) async {
      await pumpDashboard(tester);
      expect(find.text('Devnet'), findsOneWidget);
    });

    testWidgets('renders IDENTITY label', (tester) async {
      await pumpDashboard(tester);
      expect(find.text('IDENTITY'), findsOneWidget);
    });

    testWidgets('renders Disconnected status', (tester) async {
      await pumpDashboard(tester);
      expect(find.text('Disconnected'), findsOneWidget);
    });

    testWidgets('renders Vault and Policies nav cards', (tester) async {
      await pumpDashboard(tester);
      expect(find.text('Vault'), findsOneWidget);
      expect(find.text('Policies'), findsOneWidget);
    });

    testWidgets('renders session key balance card', (tester) async {
      await pumpDashboard(tester);
      // No active session key — shows empty state
      expect(find.text('No active session key'), findsOneWidget);
    });

    testWidgets('renders recent payments section', (tester) async {
      await pumpDashboard(tester);
      expect(find.text('RECENT PAYMENTS'), findsOneWidget);
    });

    testWidgets('renders empty payments state when no records',
        (tester) async {
      await pumpDashboard(tester);
      expect(find.text('No payment records yet'), findsOneWidget);
    });

    testWidgets('renders Scan nav card', (tester) async {
      await pumpDashboard(tester);
      expect(find.text('Scan'), findsOneWidget);
    });

    testWidgets('renders Authorize Payment button', (tester) async {
      await pumpDashboard(tester);
      expect(find.text('Authorize Payment'), findsOneWidget);
    });
  });
}
