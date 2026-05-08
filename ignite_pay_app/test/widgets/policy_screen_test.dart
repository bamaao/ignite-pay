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

import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ignite_pay_app/policy_screen.dart';
import 'package:path_provider_platform_interface/path_provider_platform_interface.dart';
import 'package:plugin_platform_interface/plugin_platform_interface.dart';
import 'package:shared_preferences/shared_preferences.dart';

class FakePathProviderPlatform extends Fake
    with MockPlatformInterfaceMixin
    implements PathProviderPlatform {
  @override
  Future<String?> getApplicationSupportPath() async => '/tmp/test_app_support';

  @override
  Future<String?> getTemporaryPath() async => '/tmp/test_tmp';

  @override
  Future<String?> getApplicationDocumentsPath() async => '/tmp/test_docs';
}

/// Wraps [testWidgets] to suppress google_fonts fire-and-forget async errors
/// that occur during PolicyArchitectScreen rebuilds.
void _fontSafeTestWidgets(String description, WidgetTesterCallback body) {
  testWidgets(description, (tester) async {
    // The test binding creates its own zone. We need to override how it
    // reports uncaught async errors to tolerate google_fonts font loading.
    // Simply run the test body normally — errors will be caught by the
    // test framework. We use a Zone to prevent the specific google_fonts
    // errors from propagating.
    Zone? innerZone;
    innerZone = Zone.current.fork(
      specification: ZoneSpecification(
        handleUncaughtError: (self, parent, zone, error, stackTrace) {
          final msg = error.toString();
          if (msg.contains('Failed to load font') ||
              msg.contains('google_fonts')) {
            return;
          }
          parent.handleUncaughtError(zone, error, stackTrace);
        },
      ),
    );
    await innerZone.run(() => body(tester));
  });
}

void main() {
  Future<void> _pumpPolicy(WidgetTester tester) async {
    SharedPreferences.setMockInitialValues({});
    PathProviderPlatform.instance = FakePathProviderPlatform();
    tester.view.physicalSize = const Size(800, 1600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(() {
      tester.view.resetPhysicalSize();
      tester.view.resetDevicePixelRatio();
    });

    await tester.pumpWidget(const MaterialApp(home: PolicyArchitectScreen()));
    await tester.pump(const Duration(milliseconds: 100));
    await tester.pump(const Duration(milliseconds: 100));
  }

  group('PolicyArchitectScreen', () {
    _fontSafeTestWidgets('renders Policy Architect header', (tester) async {
      await _pumpPolicy(tester);
      expect(find.text('Policy Architect'), findsOneWidget);
      expect(find.text('Spending rules & whitelists'), findsOneWidget);
    });

    _fontSafeTestWidgets('renders empty state when no policies', (tester) async {
      await _pumpPolicy(tester);
      expect(find.text('No merchant policies yet'), findsOneWidget);
    });

    _fontSafeTestWidgets('renders empty state subtitle', (tester) async {
      await _pumpPolicy(tester);
      expect(
          find.textContaining('Policies will appear here'), findsOneWidget);
    });

    _fontSafeTestWidgets('back button pops navigator', (tester) async {
      SharedPreferences.setMockInitialValues({});
      PathProviderPlatform.instance = FakePathProviderPlatform();
      tester.view.physicalSize = const Size(800, 1600);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(() {
        tester.view.resetPhysicalSize();
        tester.view.resetDevicePixelRatio();
      });

      await tester.pumpWidget(MaterialApp(
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () => openPolicyArchitect(context),
              child: const Text('Open'),
            ),
          ),
        ),
      ));

      await tester.tap(find.text('Open'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));

      expect(find.text('Policy Architect'), findsOneWidget);

      final backBtn = find.byIcon(Icons.arrow_back);
      if (backBtn.evaluate().isNotEmpty) {
        await tester.tap(backBtn.first);
      } else {
        await tester.tapAt(const Offset(30, 50));
      }
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));

      expect(find.text('Policy Architect'), findsNothing);
    });
  });
}
