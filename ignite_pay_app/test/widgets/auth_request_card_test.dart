import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/widgets/auth_request_card.dart';

void main() {
  group('AuthRequestCard', () {
    Widget _buildCard({
      required AuthRequest request,
      VoidCallback? onApprove,
      VoidCallback? onReject,
    }) {
      return MaterialApp(
        home: Scaffold(
          body: AuthRequestCard(
            request: request,
            onApprove: onApprove,
            onReject: onReject,
          ),
        ),
      );
    }

    testWidgets('renders header text', (tester) async {
      final request = AuthRequest(
        paymentId: 'pay_1',
        merchantDid: 'did:solana:short',
        amount: 1000000000,
        description: 'Test',
      );

      await tester.pumpWidget(_buildCard(request: request));
      expect(find.text('AUTHORIZATION REQUIRED'), findsOneWidget);
    });

    testWidgets('displays amount in SOL', (tester) async {
      final request = AuthRequest(
        paymentId: 'pay_1',
        merchantDid: 'did:solana:short',
        amount: 1000000000, // 1 SOL
        description: 'Test',
      );

      await tester.pumpWidget(_buildCard(request: request));
      expect(find.text('1.00 SOL'), findsOneWidget);
    });

    testWidgets('displays 0.5 SOL correctly', (tester) async {
      final request = AuthRequest(
        paymentId: 'pay_2',
        merchantDid: 'did:solana:short',
        amount: 500000000, // 0.5 SOL
        description: '',
      );

      await tester.pumpWidget(_buildCard(request: request));
      expect(find.text('0.50 SOL'), findsOneWidget);
    });

    testWidgets('displays 0 SOL for zero amount', (tester) async {
      final request = AuthRequest(
        paymentId: 'pay_3',
        merchantDid: 'did:solana:short',
        amount: 0,
        description: '',
      );

      await tester.pumpWidget(_buildCard(request: request));
      expect(find.text('0.00 SOL'), findsOneWidget);
    });

    testWidgets('displays full DID when <= 30 characters', (tester) async {
      const did = 'did:solana:abc1234567890';
      final request = AuthRequest(
        paymentId: 'pay_4',
        merchantDid: did,
        amount: 100,
        description: '',
      );

      await tester.pumpWidget(_buildCard(request: request));
      expect(find.text(did), findsOneWidget);
    });

    testWidgets('truncates DID when > 30 characters', (tester) async {
      const did = 'did:solana:7kPxRgQmN3qTvYwB8fLzCjKeXhDsUmVpRo';
      final request = AuthRequest(
        paymentId: 'pay_5',
        merchantDid: did,
        amount: 100,
        description: '',
      );

      await tester.pumpWidget(_buildCard(request: request));
      // Should show first 24 chars + "..."
      expect(find.text('${did.substring(0, 24)}...'), findsOneWidget);
    });

    testWidgets('shows description when non-empty', (tester) async {
      final request = AuthRequest(
        paymentId: 'pay_6',
        merchantDid: 'did:solana:short',
        amount: 100,
        description: 'Payment for coffee',
      );

      await tester.pumpWidget(_buildCard(request: request));
      expect(find.text('Payment for coffee'), findsOneWidget);
      expect(find.text('Description'), findsOneWidget);
    });

    testWidgets('hides description when empty', (tester) async {
      final request = AuthRequest(
        paymentId: 'pay_7',
        merchantDid: 'did:solana:short',
        amount: 100,
        description: '',
      );

      await tester.pumpWidget(_buildCard(request: request));
      expect(find.text('Description'), findsNothing);
    });

    testWidgets('fires onApprove callback on Approve tap', (tester) async {
      var approved = false;
      final request = AuthRequest(
        paymentId: 'pay_8',
        merchantDid: 'did:solana:short',
        amount: 100,
        description: '',
      );

      await tester.pumpWidget(_buildCard(
        request: request,
        onApprove: () => approved = true,
      ));

      await tester.tap(find.text('Approve'));
      expect(approved, isTrue);
    });

    testWidgets('fires onReject callback on Decline tap', (tester) async {
      var rejected = false;
      final request = AuthRequest(
        paymentId: 'pay_9',
        merchantDid: 'did:solana:short',
        amount: 100,
        description: '',
      );

      await tester.pumpWidget(_buildCard(
        request: request,
        onReject: () => rejected = true,
      ));

      await tester.tap(find.text('Decline'));
      expect(rejected, isTrue);
    });

    testWidgets('does not crash when callbacks are null', (tester) async {
      final request = AuthRequest(
        paymentId: 'pay_10',
        merchantDid: 'did:solana:short',
        amount: 100,
        description: '',
      );

      await tester.pumpWidget(_buildCard(request: request));
      // Tapping should not throw
      await tester.tap(find.text('Approve'));
      await tester.tap(find.text('Decline'));
    });
  });
}
