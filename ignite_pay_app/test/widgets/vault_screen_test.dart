import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ignite_pay_app/vault_screen.dart';

void main() {
  // Use a tall viewport to avoid overflow from the vault screen's scroll content
  Future<void> _pumpVault(WidgetTester tester) async {
    tester.view.physicalSize = const Size(800, 1600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(() {
      tester.view.resetPhysicalSize();
      tester.view.resetDevicePixelRatio();
    });

    await tester.pumpWidget(const MaterialApp(home: VaultIdentityScreen()));
    await tester.pump(const Duration(milliseconds: 100));
  }

  group('VaultIdentityScreen', () {
    testWidgets('renders Vault & Identity header', (tester) async {
      await _pumpVault(tester);
      expect(find.text('Vault & Identity'), findsOneWidget);
      expect(find.text('Key management & credentials'), findsOneWidget);
    });

    testWidgets('renders DECENTRALIZED IDENTITY label', (tester) async {
      await _pumpVault(tester);
      expect(find.text('DECENTRALIZED IDENTITY'), findsOneWidget);
    });

    testWidgets('renders key metadata chips', (tester) async {
      await _pumpVault(tester);
      expect(find.text('Ed25519'), findsOneWidget);
      expect(find.text('Mainnet'), findsOneWidget);
      expect(find.text('Active'), findsOneWidget);
    });

    testWidgets('renders VAULT section label', (tester) async {
      await _pumpVault(tester);
      expect(find.text('VAULT'), findsOneWidget);
    });

    testWidgets('renders Secret Phrase tile', (tester) async {
      await _pumpVault(tester);
      expect(find.text('Back up Secret Phrase'), findsOneWidget);
      expect(find.text('12-word recovery phrase'), findsOneWidget);
    });

    testWidgets('renders Mediator Endpoint tile', (tester) async {
      await _pumpVault(tester);
      expect(find.text('Mediator Endpoint'), findsOneWidget);
      expect(find.text('WebSocket relay for DIDComm'), findsOneWidget);
    });

    testWidgets('renders Audit Log tile', (tester) async {
      await _pumpVault(tester);
      expect(find.text('Signature Audit Logs'), findsOneWidget);
      expect(find.text('3 events this week'), findsOneWidget);
    });

    testWidgets('renders Danger Zone tile', (tester) async {
      await _pumpVault(tester);
      expect(find.text('Erase Key Material'), findsOneWidget);
      expect(find.text('Permanently delete local keys'), findsOneWidget);
    });

    testWidgets('tapping secret phrase reveals words', (tester) async {
      await _pumpVault(tester);

      expect(find.text('NEVER SHARE THESE WORDS'), findsNothing);
      expect(find.text('orbit'), findsNothing);

      await tester.tap(find.text('Back up Secret Phrase'));
      await tester.pump(const Duration(milliseconds: 100));

      expect(find.text('NEVER SHARE THESE WORDS'), findsOneWidget);
      expect(find.text('orbit'), findsOneWidget);
      expect(find.text('glacier'), findsOneWidget);
      expect(find.text('prism'), findsOneWidget);
    });

    testWidgets('tapping secret phrase again hides words', (tester) async {
      await _pumpVault(tester);

      await tester.tap(find.text('Back up Secret Phrase'));
      await tester.pump(const Duration(milliseconds: 100));
      expect(find.text('orbit'), findsOneWidget);

      await tester.tap(find.text('Back up Secret Phrase'));
      await tester.pump(const Duration(milliseconds: 100));
      expect(find.text('orbit'), findsNothing);
      expect(find.text('NEVER SHARE THESE WORDS'), findsNothing);
    });

    testWidgets('shows HW Protected badge', (tester) async {
      await _pumpVault(tester);
      expect(find.text('HW Protected'), findsOneWidget);
    });

    testWidgets('shows default DID placeholder when not initialized',
        (tester) async {
      await _pumpVault(tester);
      expect(find.textContaining('did:ignite:zInitializing'), findsOneWidget);
    });
  });

  group('openVaultIdentity navigation', () {
    testWidgets('navigates to VaultIdentityScreen', (tester) async {
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
              onPressed: () => openVaultIdentity(context),
              child: const Text('Open Vault'),
            ),
          ),
        ),
      ));

      await tester.tap(find.text('Open Vault'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));

      expect(find.text('Vault & Identity'), findsOneWidget);
    });
  });

  group('Audit Logs', () {
    Future<void> _openAuditLogs(WidgetTester tester) async {
      await _pumpVault(tester);

      // Scroll to make the audit log tile visible, then tap it
      final auditTile = find.text('Signature Audit Logs');
      await tester.ensureVisible(auditTile);
      await tester.pump(const Duration(milliseconds: 50));
      await tester.tap(auditTile);

      // Pump through the slide transition animation
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
    }

    testWidgets('navigates to audit logs page', (tester) async {
      await _openAuditLogs(tester);
      expect(find.text('CRYPTOGRAPHIC PROOF OF AUTHORIZATION'), findsOneWidget);
    });

    testWidgets('renders audit log entries', (tester) async {
      await _openAuditLogs(tester);
      // 3 hardcoded entries: ShopX, DeFi, System
      expect(find.text('ShopX Marketplace'), findsOneWidget);
      expect(find.text('DeFi Staking'), findsOneWidget);
      expect(find.text('System'), findsOneWidget);
    });

    testWidgets('renders audit log entry details', (tester) async {
      await _openAuditLogs(tester);
      expect(find.textContaining('sign_payment'), findsWidgets);
      expect(find.textContaining('key_derive'), findsOneWidget);
      expect(find.textContaining('0.12 SOL'), findsOneWidget);
      expect(find.textContaining('0.30 SOL'), findsOneWidget);
    });

    testWidgets('renders audit log status badges', (tester) async {
      await _openAuditLogs(tester);
      expect(find.text('CONFIRMED'), findsWidgets);
      expect(find.text('PENDING'), findsOneWidget);
    });
  });
}
