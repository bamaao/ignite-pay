import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ignite_pay_app/policy_screen.dart';

void main() {
  Future<void> _pumpPolicy(WidgetTester tester) async {
    tester.view.physicalSize = const Size(800, 1600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(() {
      tester.view.resetPhysicalSize();
      tester.view.resetDevicePixelRatio();
    });

    await tester.pumpWidget(const MaterialApp(home: PolicyArchitectScreen()));
    await tester.pump(const Duration(milliseconds: 100));
  }

  group('PolicyArchitectScreen', () {
    testWidgets('renders Policy Architect header', (tester) async {
      await _pumpPolicy(tester);
      expect(find.text('Policy Architect'), findsOneWidget);
      expect(find.text('Spending rules & whitelists'), findsOneWidget);
    });

    testWidgets('renders stats grid labels', (tester) async {
      await _pumpPolicy(tester);
      expect(find.text('MERCHANTS'), findsOneWidget);
      expect(find.text('AUTO-PAY'), findsOneWidget);
      expect(find.text('WEEKLY CAP'), findsOneWidget);
      expect(find.text('SPENT'), findsOneWidget);
    });

    testWidgets('renders correct merchant count', (tester) async {
      await _pumpPolicy(tester);
      expect(find.text('4'), findsOneWidget);
    });

    testWidgets('renders auto-pay count', (tester) async {
      await _pumpPolicy(tester);
      // ShopX (auto) + RPC Provider (auto) = 2
      expect(find.text('2'), findsOneWidget);
    });

    testWidgets('renders all 4 policy merchant names', (tester) async {
      await _pumpPolicy(tester);
      expect(find.text('ShopX Marketplace'), findsOneWidget);
      expect(find.text('DeFi Staking'), findsOneWidget);
      expect(find.text('NFT Mint'), findsOneWidget);
      expect(find.text('RPC Provider'), findsOneWidget);
    });

    testWidgets('renders all merchant domains', (tester) async {
      await _pumpPolicy(tester);
      expect(find.text('shopx.io'), findsOneWidget);
      expect(find.text('defistake.xyz'), findsOneWidget);
      expect(find.text('nftmint.pro'), findsOneWidget);
      expect(find.text('solrpc.dev'), findsOneWidget);
    });

    testWidgets('shows AUTO for auto-pay policies', (tester) async {
      await _pumpPolicy(tester);
      expect(find.text('AUTO'), findsWidgets);
    });

    testWidgets('shows MANUAL for non-auto-pay policies', (tester) async {
      await _pumpPolicy(tester);
      expect(find.text('MANUAL'), findsWidgets);
    });

    testWidgets('tapping a policy card expands detail section', (tester) async {
      await _pumpPolicy(tester);

      // Initially no detail fields visible
      expect(find.text('did:solana:7kPx...mN3q'), findsNothing);

      // Tap on first merchant name to expand
      await tester.tap(find.text('ShopX Marketplace'));
      await tester.pump(const Duration(milliseconds: 300));

      // DID should be visible in expanded state
      expect(find.text('did:solana:7kPx...mN3q'), findsOneWidget);
    });

    testWidgets('tapping expanded card collapses it', (tester) async {
      await _pumpPolicy(tester);

      // Expand
      await tester.tap(find.text('ShopX Marketplace'));
      await tester.pump(const Duration(milliseconds: 300));
      expect(find.text('did:solana:7kPx...mN3q'), findsOneWidget);

      // Collapse
      await tester.tap(find.text('ShopX Marketplace'));
      await tester.pump(const Duration(milliseconds: 300));
      expect(find.text('did:solana:7kPx...mN3q'), findsNothing);
    });

    testWidgets('back button pops navigator', (tester) async {
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

      // Find the back button by its icon (arrowLeft)
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
