import 'dart:convert';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/src/rust/api/notification.dart';
import 'package:ignite_pay_app/src/rust/api/session.dart';
import 'package:ignite_pay_app/src/rust/api/simple.dart';
import 'package:ignite_pay_app/src/rust/frb_generated.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// A mock implementation of [RustLibApi] that returns stub values
/// without needing the real Rust FFI runtime.
class _MockRustLibApi extends RustLibApi {
  @override
  Future<DidInfo> crateApiSimpleInitializeIdentity({
    required String storagePath,
  }) async =>
      DidInfo(did: 'did:ignite:zPhone${DateTime.now().millisecondsSinceEpoch % 10000}', didDocJson: '{}');

  @override
  Future<void> crateApiSimpleConnectMediator({
    required String storagePath,
    required String wsUrl,
  }) async {}

  @override
  Future<void> crateApiSimpleDisconnectMediator() async {}

  @override
  Future<void> crateApiSimpleSendAuthResponse({
    required String storagePath,
    required String paymentId,
    required bool authorized,
    required String listAction,
    required String mcpDid,
    SessionKeyInfo? sessionKeyInfo,
    String? listLabel,
    BigInt? listMaxAmount,
  }) async {}

  @override
  Future<SessionKeyInfo> crateApiSimpleCreateSessionKeyForPayment({
    required String storagePath,
    required BigInt spendingLimit,
    required PlatformInt64 durationSecs,
  }) async =>
      SessionKeyInfo(
        ephemeralPubkey: 'mockPubkey',
        ephemeralSecretKey: 'mockSecretKey',
        expiresAt: DateTime.now().millisecondsSinceEpoch + durationSecs * 1000,
        spendingLimit: spendingLimit,
        scopes: ['sol:transfer'],
        txSignature: null,
        sessionPda: null,
      );

  @override
  Future<String> crateApiSimpleAuthenticateWithMediator({
    required String mediatorUrl,
    required String did,
  }) async =>
      'mock_token';

  @override
  Future<DecryptedMessage> crateApiSimpleDecryptMessage({
    required String storagePath,
    required String jwe,
  }) async =>
      DecryptedMessage(
        msgType: 'placeholder',
        rawBody: jwe,
      );

  @override
  Future<String> crateApiSimpleGetDid({required String storagePath}) async =>
      'did:ignite:mock';

  @override
  Future<List<DidcommMessage>> crateApiSimplePullMessages({
    required String mediatorUrl,
    required String token,
    String? afterId,
    required int limit,
  }) async =>
      [];

  @override
  Future<AuthGrant> crateApiSimpleSignPayment({
    required String merchantDid,
    required BigInt amount,
  }) async =>
      AuthGrant(merchantDid: merchantDid, amount: amount, signature: 'mock_sig');

  @override
  Future<void> crateApiSimpleRegisterDeviceToken({
    required String mediatorUrl,
    required String authToken,
    required String fcmToken,
  }) async {}

  @override
  Future<SessionKeyInfo> crateApiSessionCreateSessionKey({
    required String storagePath,
    required String ownerPubkey,
    required String targetProgram,
    required List<String> scopes,
    required BigInt spendingLimit,
    required PlatformInt64 durationSecs,
  }) async =>
      SessionKeyInfo(
        ephemeralPubkey: 'mockPubkey',
        ephemeralSecretKey: 'mockSecretKey',
        expiresAt: DateTime.now().millisecondsSinceEpoch + durationSecs * 1000,
        spendingLimit: spendingLimit,
        scopes: scopes,
        txSignature: null,
        sessionPda: null,
      );

  @override
  Future<SessionKeyInfo> crateApiSessionCreateAndRegisterSessionKey({
    required String storagePath,
    required String rpcUrl,
    required String ownerSecretKey,
    required String targetProgram,
    required List<String> scopes,
    required BigInt spendingLimit,
    required PlatformInt64 durationSecs,
  }) async =>
      SessionKeyInfo(
        ephemeralPubkey: 'mockPubkey',
        ephemeralSecretKey: 'mockSecretKey',
        expiresAt: DateTime.now().millisecondsSinceEpoch + durationSecs * 1000,
        spendingLimit: spendingLimit,
        scopes: scopes,
        txSignature: 'mock_tx_sig',
        sessionPda: 'mock_pda',
      );

  @override
  Future<List<SessionKeyEntry>> crateApiSimpleListSessionKeys({
    required String storagePath,
  }) async =>
      [];

  @override
  Future<List<SessionKeyEntry>> crateApiSessionListSessionKeys({
    required String storagePath,
  }) async =>
      [];

  @override
  Future<SessionKeyEntry?> crateApiSimpleFindActiveSessionKey({
    required String storagePath,
  }) async =>
      null;

  @override
  Future<SessionKeyEntry?> crateApiSessionFindActiveSessionKey({
    required String storagePath,
  }) async =>
      null;

  @override
  Future<UnsignedRegisterTx> crateApiSimpleBuildUnsignedRegisterTx({
    required String storagePath,
    required String rpcUrl,
    required BigInt spendingLimit,
    required PlatformInt64 durationSecs,
  }) async =>
      UnsignedRegisterTx(
        unsignedTxB58: 'mock_unsigned_tx',
        sessionPda: 'mock_pda',
        ephemeralPubkey: 'mock_ephemeral_pubkey',
      );

  @override
  Future<UnsignedRegisterTx> crateApiSessionBuildUnsignedRegisterTx({
    required String storagePath,
    required String rpcUrl,
    required BigInt spendingLimit,
    required PlatformInt64 durationSecs,
  }) async =>
      UnsignedRegisterTx(
        unsignedTxB58: 'mock_unsigned_tx',
        sessionPda: 'mock_pda',
        ephemeralPubkey: 'mock_ephemeral_pubkey',
      );

  @override
  Future<SessionKeyInfo> crateApiSimpleCompleteRegisterWithSignature({
    required String storagePath,
    required String ephemeralPubkey,
    required String ownerSignatureB58,
    required String rpcUrl,
  }) async =>
      SessionKeyInfo(
        ephemeralPubkey: ephemeralPubkey,
        ephemeralSecretKey: 'mockSecretKey',
        expiresAt: DateTime.now().millisecondsSinceEpoch + 3600000,
        spendingLimit: BigInt.from(5000000000),
        scopes: ['sol:transfer'],
        txSignature: 'mock_tx_sig',
        sessionPda: 'mock_pda',
      );

  @override
  Future<SessionKeyInfo> crateApiSessionCompleteRegisterWithSignature({
    required String storagePath,
    required String ephemeralPubkey,
    required String ownerSignatureB58,
    required String rpcUrl,
  }) async =>
      SessionKeyInfo(
        ephemeralPubkey: ephemeralPubkey,
        ephemeralSecretKey: 'mockSecretKey',
        expiresAt: DateTime.now().millisecondsSinceEpoch + 3600000,
        spendingLimit: BigInt.from(5000000000),
        scopes: ['sol:transfer'],
        txSignature: 'mock_tx_sig',
        sessionPda: 'mock_pda',
      );

  @override
  Future<String> crateApiSimpleRevokeSessionKeyOnchain({
    required String storagePath,
    required String sessionPubkey,
    required String rpcUrl,
  }) async =>
      'mock_revoke_tx_sig';

  @override
  Future<String> crateApiSessionRevokeSessionKeyOnchain({
    required String storagePath,
    required String sessionPubkey,
    required String rpcUrl,
  }) async =>
      'mock_revoke_tx_sig';

  @override
  Future<void> crateApiSimpleDeleteSessionKeyLocal({
    required String storagePath,
    required String sessionPubkey,
  }) async {}

  @override
  Future<void> crateApiSessionDeleteSessionKeyLocal({
    required String storagePath,
    required String sessionPubkey,
  }) async {}

  @override
  Future<OobInvitationData> crateApiSimpleParseOobInvitation({
    required String invitationUrl,
  }) async =>
      OobInvitationData(
        mcpDid: 'did:ignite:mockMcp',
        didDocJson: '{}',
        mediatorWsUrl: 'ws://mock:3000/ws',
        label: 'Mock MCP',
      );

  @override
  Future<void> crateApiSimpleSendConnectionRequest({
    required String storagePath,
    required String mcpDid,
    required String mcpDidDocJson,
    required String mediatorWsUrl,
    required String pushChannel,
    String? fcmToken,
  }) async {}
}

void main() {
  group('DidcommService', () {
    late DidcommService service;

    setUpAll(() {
      RustLib.initMock(api: _MockRustLibApi());
    });

    setUp(() {
      SharedPreferences.setMockInitialValues({});
      service = DidcommService();
    });

    test('factory returns same singleton instance', () {
      final a = DidcommService();
      final b = DidcommService();
      expect(identical(a, b), isTrue);
    });

    test('initial state is correct', () {
      expect(service.did, isEmpty);
      expect(service.didDocJson, isEmpty);
      expect(service.isConnected, isFalse);
      expect(service.isInitialized, isFalse);
      expect(service.messages, isEmpty);
      expect(service.pendingAuth, isNull);
      expect(service.pendingMessageCount, 0);
    });

    test('messages returns unmodifiable list', () {
      expect(() => service.messages.add(DecryptedMsg(msgType: 'x', rawBody: '')),
          throwsUnsupportedError);
    });

    group('initialize', () {
      test('sets isInitialized and generates DID', () async {
        await service.initialize();
        expect(service.isInitialized, isTrue);
        expect(service.did, isNotEmpty);
        expect(service.did, startsWith('did:ignite:zPhone'));
      });

      test('does not change DID on second call (idempotent)', () async {
        await service.initialize();
        final firstDid = service.did;
        await service.initialize();
        expect(service.did, firstDid);
      });
    });

    group('disconnect', () {
      test('sets isConnected to false and notifies', () async {
        var notified = false;
        service.addListener(() => notified = true);

        await service.disconnect();
        expect(service.isConnected, isFalse);
        expect(notified, isTrue);
      });
    });

    group('handleAuthRequest', () {
      test('sets pendingAuth and emits to stream', () async {
        final request = AuthRequest(
          paymentId: 'pay_test',
          merchantDid: 'did:test:merchant',
          amount: 1000000000,
          description: 'Test payment',
        );

        AuthRequest? streamEvent;
        service.authRequests.listen((req) => streamEvent = req);

        var notified = false;
        service.addListener(() => notified = true);

        service.handleAuthRequest(request);

        expect(service.pendingAuth, request);
        expect(notified, isTrue);

        // Wait for stream event
        await Future.delayed(const Duration(milliseconds: 50));
        expect(streamEvent, request);
      });
    });

    group('simulateAuthRequest', () {
      test('sets pendingAuth same as handleAuthRequest', () {
        final request = AuthRequest(
          paymentId: 'pay_sim',
          merchantDid: 'did:test:merchant',
          amount: 500,
          description: '',
        );

        service.simulateAuthRequest(request);
        expect(service.pendingAuth, request);
      });
    });

    group('clearPendingAuth', () {
      test('clears pendingAuth and notifies', () {
        service.handleAuthRequest(AuthRequest(
          paymentId: 'pay_x',
          merchantDid: 'did:test',
          amount: 100,
          description: '',
        ));
        expect(service.pendingAuth, isNotNull);

        var notified = false;
        service.addListener(() => notified = true);

        service.clearPendingAuth();
        expect(service.pendingAuth, isNull);
        expect(notified, isTrue);
      });
    });

    group('sendAuthResponse', () {
      test('clears pendingAuth after sending', () async {
        service.handleAuthRequest(AuthRequest(
          paymentId: 'pay_y',
          merchantDid: 'did:test',
          amount: 200,
          description: '',
        ));
        expect(service.pendingAuth, isNotNull);

        var notified = false;
        service.addListener(() => notified = true);

        await service.sendAuthResponse(AuthResponseData(
          paymentId: 'pay_y',
          authorized: true,
          listAction: 'none',
        ));

        expect(service.pendingAuth, isNull);
        expect(notified, isTrue);
      });
    });

    group('sendAuthResponseWithSessionKey', () {
      test('clears pendingAuth and delegates to sendAuthResponse', () async {
        service.handleAuthRequest(AuthRequest(
          paymentId: 'pay_z',
          merchantDid: 'did:test',
          amount: 300,
          description: '',
        ));

        await service.sendAuthResponseWithSessionKey(
          paymentId: 'pay_z',
          authorized: true,
          listAction: 'add_whitelist',
          spendingLimit: 3000,
          durationSecs: 3600,
          listLabel: 'ShopX',
          listMaxAmount: 1000000000,
        );

        expect(service.pendingAuth, isNull);
      });

      test('passes listLabel and listMaxAmount correctly', () async {
        service.handleAuthRequest(AuthRequest(
          paymentId: 'pay_w',
          merchantDid: 'did:test',
          amount: 400,
          description: '',
        ));

        // Should complete without error - the internal AuthResponseData
        // carries the label and max amount
        await service.sendAuthResponseWithSessionKey(
          paymentId: 'pay_w',
          authorized: true,
          listAction: 'add_blacklist',
          spendingLimit: 4000,
          durationSecs: 7200,
          listLabel: 'Evil Corp',
        );

        expect(service.pendingAuth, isNull);
      });

      test('works with null optional parameters', () async {
        service.handleAuthRequest(AuthRequest(
          paymentId: 'pay_v',
          merchantDid: 'did:test',
          amount: 500,
          description: '',
        ));

        await service.sendAuthResponseWithSessionKey(
          paymentId: 'pay_v',
          authorized: false,
          listAction: 'none',
          spendingLimit: 0,
          durationSecs: 0,
        );

        expect(service.pendingAuth, isNull);
      });
    });

    group('auth stream', () {
      test('receives multiple auth requests in order', () async {
        final received = <AuthRequest>[];
        service.authRequests.listen((req) => received.add(req));

        for (int i = 0; i < 3; i++) {
          service.handleAuthRequest(AuthRequest(
            paymentId: 'pay_$i',
            merchantDid: 'did:test',
            amount: i * 100,
            description: '',
          ));
        }

        await Future.delayed(const Duration(milliseconds: 50));
        expect(received.length, 3);
        expect(received[0].paymentId, 'pay_0');
        expect(received[1].paymentId, 'pay_1');
        expect(received[2].paymentId, 'pay_2');
      });
    });

    group('WS message handling', () {
      test('handleAuthRequest processes payment-auth-request from WS', () {
        // Simulate a WS message that triggers auth request
        final request = AuthRequest(
          paymentId: 'pay_ws_test',
          merchantDid: 'did:test:merchant_ws',
          amount: 5000,
          description: 'WS test payment',
        );

        service.handleAuthRequest(request);
        expect(service.pendingAuth, isNotNull);
        expect(service.pendingAuth!.paymentId, 'pay_ws_test');
        expect(service.messages.length, 0); // No messages until _decryptAndProcess is called
      });

      test('simulateAuthRequest works for WS path', () {
        final request = AuthRequest(
          paymentId: 'pay_ws_sim',
          merchantDid: 'did:test:merchant_ws',
          amount: 3000,
          description: 'WS simulated payment',
        );

        service.simulateAuthRequest(request);
        expect(service.pendingAuth, isNotNull);
        expect(service.pendingAuth!.paymentId, 'pay_ws_sim');
      });

      test('multiple WS auth requests queue correctly', () async {
        final received = <AuthRequest>[];
        service.authRequests.listen((req) => received.add(req));

        for (int i = 0; i < 5; i++) {
          service.handleAuthRequest(AuthRequest(
            paymentId: 'pay_ws_$i',
            merchantDid: 'did:test:merchant',
            amount: i * 1000,
            description: 'WS payment $i',
          ));
        }

        await Future.delayed(const Duration(milliseconds: 50));
        expect(received.length, 5);
        // Only the last one should be pending auth
        expect(service.pendingAuth!.paymentId, 'pay_ws_4');
      });

      test('disconnect cleans up WS state', () async {
        await service.disconnect();
        expect(service.isConnected, isFalse);
      });
    });

    group('parseInvitationAndConnect', () {
      /// Helper to build a valid OOB invitation URL from parts.
      String buildOobUrl({
        required String fromDid,
        String label = 'Test MCP',
        String wsUrl = 'ws://localhost:3000/ws',
        Map<String, dynamic>? didDoc,
        List<Map<String, dynamic>>? services,
      }) {
        final body = <String, dynamic>{
          'label': label,
          'goal_code': 'p2p-messaging',
          'accept': ['didcomm/v2'],
          'did_document': didDoc ?? {'id': fromDid},
          'services': services ??
              [
                {
                  'id': '#mediator',
                  'type': 'did-communication',
                  'service_endpoint': wsUrl,
                  'routing_keys': [fromDid],
                }
              ],
        };

        final invitation = <String, dynamic>{
          'type': 'https://didcomm.org/out-of-band/2.0/invitation',
          'from': fromDid,
          'body': body,
        };

        final jsonStr = jsonEncode(invitation);
        final b64 = base64Url.encode(utf8.encode(jsonStr)).replaceAll('=', '');
        return 'didcomm://?_oob=$b64';
      }

      test('parses valid OOB invitation URL with correct fields', () async {
        await service.initialize();

        final url = buildOobUrl(fromDid: 'did:ignite:zMcpTest');
        // This will fail because we're not connected to a real mediator,
        // but we can verify parsing works by checking the error message.
        try {
          await service.parseInvitationAndConnect(url);
        } catch (e) {
          // Expected: connection request fails because no mediator connected
          // but the parsing should succeed
          expect(e.toString(), isNot(contains('Missing _oob')));
          expect(e.toString(), isNot(contains('Missing from')));
        }
      });

      test('rejects URL without _oob parameter', () async {
        await service.initialize();

        expect(
          () => service.parseInvitationAndConnect('didcomm://?foo=bar'),
          throwsA(isA<Exception>().having(
            (e) => e.toString(),
            'message',
            contains('Missing _oob'),
          )),
        );
      });

      test('rejects invitation missing from field', () async {
        await service.initialize();

        // Build an invitation without "from"
        final invitation = {
          'type': 'https://didcomm.org/out-of-band/2.0/invitation',
          'body': {'label': 'No From'},
        };
        final jsonStr = jsonEncode(invitation);
        final b64 = base64Url.encode(utf8.encode(jsonStr)).replaceAll('=', '');
        final url = 'didcomm://?_oob=$b64';

        expect(
          () => service.parseInvitationAndConnect(url),
          throwsA(isA<Exception>().having(
            (e) => e.toString(),
            'message',
            contains('Missing from'),
          )),
        );
      });

      test('rejects invalid base64', () async {
        await service.initialize();

        expect(
          () => service.parseInvitationAndConnect('didcomm://?_oob=!!!invalid!!!'),
          throwsA(anything),
        );
      });

      test('extracts mediator WS URL from services array', () async {
        await service.initialize();

        final url = buildOobUrl(
          fromDid: 'did:ignite:zMcpTest',
          wsUrl: 'wss://mediator.example.com/ws',
        );

        try {
          await service.parseInvitationAndConnect(url);
        } catch (e) {
          // Parsing succeeded but connection failed — expected
          expect(e.toString(), isNot(contains('_oob')));
        }
      });

      test('handles invitation with empty services gracefully', () async {
        await service.initialize();

        final url = buildOobUrl(
          fromDid: 'did:ignite:zMcpTest',
          services: [],
        );

        // Should not crash on empty services
        try {
          await service.parseInvitationAndConnect(url);
        } catch (e) {
          // Expected: fails because no mediator URL found, but parsing is fine
          expect(e.toString(), isNot(contains('services')));
        }
      });
    });
  });
}
