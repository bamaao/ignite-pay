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
import 'package:provider/provider.dart';

void main() {
  // The dashboard has an animated ConnectionDot that never settles,
  // so use pump(duration) instead of pumpAndSettle().
  Future<void> _pumpDashboard(WidgetTester tester) async {
    tester.view.physicalSize = const Size(800, 1400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(() {
      tester.view.resetPhysicalSize();
      tester.view.resetDevicePixelRatio();
    });

    // Reset the singleton so it is not in a disposed state from a prior test.
    DidcommService.resetInstance();

    await tester.pumpWidget(
      ChangeNotifierProvider(
        create: (_) => DidcommService(),
        child: const MaterialApp(home: IgnitePayDashboard()),
      ),
    );
    await tester.pump(const Duration(milliseconds: 100));
  }

  group('IgnitePayDashboard', () {
    testWidgets('renders header with Ignite Pay title', (tester) async {
      await _pumpDashboard(tester);
      expect(find.text('Ignite Pay'), findsOneWidget);
    });

    testWidgets('renders Mainnet badge', (tester) async {
      await _pumpDashboard(tester);
      expect(find.text('Mainnet'), findsOneWidget);
    });

    testWidgets('renders IDENTITY label', (tester) async {
      await _pumpDashboard(tester);
      expect(find.text('IDENTITY'), findsOneWidget);
    });

    testWidgets('renders Disconnected status', (tester) async {
      await _pumpDashboard(tester);
      expect(find.text('Disconnected'), findsOneWidget);
    });

    testWidgets('renders Vault and Policies nav cards', (tester) async {
      await _pumpDashboard(tester);
      expect(find.text('Vault'), findsOneWidget);
      expect(find.text('Policies'), findsOneWidget);
    });

    testWidgets('renders DAILY ALLOWANCE section', (tester) async {
      await _pumpDashboard(tester);
      expect(find.text('DAILY ALLOWANCE'), findsOneWidget);
      expect(find.text('Remaining'), findsOneWidget);
    });

    testWidgets('renders gauge labels', (tester) async {
      await _pumpDashboard(tester);
      expect(find.text('0.42 SOL'), findsOneWidget);
      expect(find.text('1.00 SOL'), findsOneWidget);
      expect(find.text('Spent'), findsOneWidget);
      expect(find.text('Limit'), findsOneWidget);
    });

    testWidgets('renders activity feed header', (tester) async {
      await _pumpDashboard(tester);
      expect(find.text('RECENT ACTIVITY'), findsOneWidget);
    });

    testWidgets('renders empty activity state when no messages',
        (tester) async {
      await _pumpDashboard(tester);
      expect(find.text('No recent activity'), findsOneWidget);
    });

    testWidgets('renders Scan nav card', (tester) async {
      await _pumpDashboard(tester);
      expect(find.text('Scan'), findsOneWidget);
    });

    testWidgets('renders Channels nav card', (tester) async {
      await _pumpDashboard(tester);
      expect(find.text('Channels'), findsOneWidget);
    });

    testWidgets('renders Authorize Payment button', (tester) async {
      await _pumpDashboard(tester);
      expect(find.text('Authorize Payment'), findsOneWidget);
    });
  });

  group('TrustScoreGauge', () {
    testWidgets('calculates remaining percentage correctly', (tester) async {
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(
          body: TrustScoreGauge(
            spent: 0.42,
            limit: 1.0,
            spentLabel: '0.42 SOL',
            limitLabel: '1.00 SOL',
          ),
        ),
      ));

      // remaining = (1.0 - 0.42) * 100 = 58%
      expect(find.textContaining('58%'), findsOneWidget);
    });

    testWidgets('shows 0% when spent equals limit', (tester) async {
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(
          body: TrustScoreGauge(
            spent: 1.0,
            limit: 1.0,
            spentLabel: '1.00 SOL',
            limitLabel: '1.00 SOL',
          ),
        ),
      ));

      expect(find.textContaining('0%'), findsOneWidget);
    });

    testWidgets('clamps remaining to 0 when over limit', (tester) async {
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(
          body: TrustScoreGauge(
            spent: 1.5,
            limit: 1.0,
            spentLabel: '1.50 SOL',
            limitLabel: '1.00 SOL',
          ),
        ),
      ));

      expect(find.textContaining('0%'), findsOneWidget);
    });

    testWidgets('shows 100% when nothing spent', (tester) async {
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(
          body: TrustScoreGauge(
            spent: 0.0,
            limit: 1.0,
            spentLabel: '0.00 SOL',
            limitLabel: '1.00 SOL',
          ),
        ),
      ));

      expect(find.textContaining('100%'), findsOneWidget);
    });
  });
}
