import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ignite_pay_app/challenge_screen.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';

void main() {
  group('showX402Challenge', () {
    // Use a large viewport to avoid overflow in the challenge overlay
    Future<void> _setupChallenge(WidgetTester tester, {AuthRequest? request}) async {
      tester.view.physicalSize = const Size(800, 1200);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(() {
        tester.view.resetPhysicalSize();
        tester.view.resetDevicePixelRatio();
      });

      await tester.pumpWidget(MaterialApp(
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () => showX402Challenge(context, request: request),
              child: const Text('Open'),
            ),
          ),
        ),
      ));

      await tester.tap(find.text('Open'));
      // First pump processes the tap, second pump advances the transition
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));
    }

    testWidgets('opens challenge overlay and renders content', (tester) async {
      await _setupChallenge(tester);
      expect(find.text('X402 Challenge'), findsOneWidget);
      expect(find.text('PAYMENT REQUEST'), findsOneWidget);
      expect(find.text('SOL'), findsWidgets); // SOL appears in amount + policy inputs
    });

    testWidgets('renders default values with no request', (tester) async {
      await _setupChallenge(tester);
      expect(find.textContaining('shopx'), findsOneWidget);
      expect(find.text('Payment for services'), findsOneWidget);
    });

    testWidgets('renders actual request data', (tester) async {
      final request = AuthRequest(
        paymentId: 'pay_test_001',
        merchantDid: 'did:solana:7kPxRgQmN3qTvYwB8fLzCjKeXhDsUmVp',
        amount: 2000000000,
        description: 'Test purchase',
      );

      await _setupChallenge(tester, request: request);
      expect(find.text('2'), findsOneWidget); // Amount as integer (2 SOL)
      expect(find.text('Test purchase'), findsOneWidget);
    });

    testWidgets('decline button pops with declined', (tester) async {
      tester.view.physicalSize = const Size(800, 1200);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(() {
        tester.view.resetPhysicalSize();
        tester.view.resetDevicePixelRatio();
      });

      String? result;
      await tester.pumpWidget(MaterialApp(
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () async {
                result = await showX402Challenge<String>(context);
              },
              child: const Text('Open'),
            ),
          ),
        ),
      ));

      await tester.tap(find.text('Open'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));

      await tester.tap(find.text('Decline & Block'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));

      expect(result, 'declined');
    });

    testWidgets('renders authorization policy card', (tester) async {
      await _setupChallenge(tester);
      expect(find.text('AUTHORIZATION POLICY'), findsOneWidget);
      expect(find.text('Daily Limit'), findsOneWidget);
      expect(find.text('Daily Tx Count'), findsOneWidget);
      expect(find.text('Per-Tx Limit'), findsOneWidget);
      expect(find.text('Duration'), findsOneWidget);
    });

    testWidgets('renders approve button', (tester) async {
      await _setupChallenge(tester);
      expect(find.text('APPROVE'), findsOneWidget);
    });

    testWidgets('renders decline button', (tester) async {
      await _setupChallenge(tester);
      expect(find.text('Decline & Block'), findsOneWidget);
    });

    testWidgets('renders list action selector', (tester) async {
      await _setupChallenge(tester);
      expect(find.text('LIST ACTION'), findsOneWidget);
      expect(find.text('This time only'), findsOneWidget);
      expect(find.text('Whitelist'), findsOneWidget);
      expect(find.text('Blacklist'), findsOneWidget);
      expect(find.text('Remove WL'), findsOneWidget);
      expect(find.text('Remove BL'), findsOneWidget);
    });

    testWidgets('shows label input when Whitelist selected', (tester) async {
      await _setupChallenge(tester);

      expect(find.text('Label (e.g. "ShopX Marketplace")'), findsNothing);

      await tester.tap(find.text('Whitelist'));
      await tester.pump(const Duration(milliseconds: 100));

      expect(find.text('Label (e.g. "ShopX Marketplace")'), findsOneWidget);
      expect(find.text('Max amount (lamports, optional)'), findsOneWidget);
    });

    testWidgets('shows only label input when Blacklist selected',
        (tester) async {
      await _setupChallenge(tester);

      await tester.tap(find.text('Blacklist'));
      await tester.pump(const Duration(milliseconds: 100));

      expect(find.text('Label (e.g. "ShopX Marketplace")'), findsOneWidget);
      expect(find.text('Max amount (lamports, optional)'), findsNothing);
    });
  });
}
