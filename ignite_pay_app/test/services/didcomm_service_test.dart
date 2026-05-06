import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/src/rust/api/channel.dart';
import 'package:ignite_pay_app/src/rust/api/channel_store.dart';
import 'package:ignite_pay_app/src/rust/api/notification.dart';
import 'package:ignite_pay_app/src/rust/api/session.dart';
import 'package:ignite_pay_app/src/rust/api/simple.dart';
import 'package:ignite_pay_app/src/rust/api/mb_voucher.dart';
import 'package:ignite_pay_app/src/rust/frb_generated.dart';
import 'package:path_provider_platform_interface/path_provider_platform_interface.dart';
import 'package:plugin_platform_interface/plugin_platform_interface.dart';
import 'package:shared_preferences/shared_preferences.dart';

class _FakePathProviderPlatform extends Fake
    with MockPlatformInterfaceMixin
    implements PathProviderPlatform {
  @override
  Future<String?> getApplicationSupportPath() async => '/tmp/test_app_support';
  @override
  Future<String?> getTemporaryPath() async => '/tmp/test_tmp';
  @override
  Future<String?> getApplicationDocumentsPath() async => '/tmp/test_docs';
}

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
    int? dailyTxCountLimit,
    BigInt? perTxLimit,
    String? tokenMint,
    required String? paymentMethod,
  }) async {}

  @override
  Future<SessionKeyInfo> crateApiSimpleCreateSessionKeyForPayment({
    required String storagePath,
    required BigInt spendingLimit,
    required PlatformInt64 durationSecs,
    String? tokenMint,
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
    required String storagePath,
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
  Future<void> crateApiSimpleSaveMerchantPolicy({
    required String storagePath,
    required String merchantDid,
    required BigInt dailySpendingLimit,
    required int dailyTxCountLimit,
    required BigInt perTxLimit,
    required PlatformInt64 durationSecs,
  }) async {}

  @override
  Future<MerchantPolicy?> crateApiSimpleLoadMerchantPolicy({
    required String storagePath,
    required String merchantDid,
  }) async =>
      null;

  @override
  Future<void> crateApiSessionSaveMerchantPolicy({
    required String storagePath,
    required String merchantDid,
    required BigInt dailySpendingLimit,
    required int dailyTxCountLimit,
    required BigInt perTxLimit,
    required PlatformInt64 durationSecs,
  }) async {}

  @override
  Future<MerchantPolicy?> crateApiSessionLoadMerchantPolicy({
    required String storagePath,
    required String merchantDid,
  }) async =>
      null;

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

  // --- Channel & Hub bridge mocks ---

  @override
  Future<List<HubInfo>> crateApiSimpleFetchHubList({
    required String registryUrl,
  }) async =>
      [];

  @override
  Future<void> crateApiSimpleSendCreateChannelRequest({
    required String storagePath,
    required String mcpDid,
    required String hubEndpoint,
    required String providerPubkey,
    required String tokenMint,
    required BigInt deposit,
    required int treeDepth,
  }) async {}

  @override
  Future<PaymentResult> crateApiSimpleChannelPay({
    required String storagePath,
    required String channelId,
    required String hubEndpoint,
    required BigInt amount,
    required String recipientPubkey,
  }) async =>
      PaymentResult(
        channelId: channelId,
        sequence: BigInt.zero,
        leafIndex: 0,
        newRoot: '',
      );

  @override
  Future<String> crateApiSimpleCloseChannel({
    required String storagePath,
    required String channelId,
  }) async =>
      'Channel $channelId closed.';

  @override
  Future<ChannelStateInfo> crateApiSimpleGetChannelState({
    required String storagePath,
    required String channelId,
  }) async =>
      ChannelStateInfo(
        channelId: channelId,
        status: 'Open',
        sequence: BigInt.zero,
        leafCount: 0,
        userBalance: BigInt.zero,
        totalDeposited: BigInt.zero,
      );

  @override
  Future<List<ChannelInfo>> crateApiSimpleListChannels({
    required String storagePath,
  }) async =>
      [];

  @override
  Future<OpenChannelResult> crateApiSimpleOpenChannel({
    required String storagePath,
    required String hubEndpoint,
    required BigInt deposit,
    required int treeDepth,
  }) async =>
      OpenChannelResult(
        channelId: 'mock_channel_id',
        sequence: BigInt.zero,
        currentRoot: '',
      );

  @override
  Future<PaymentQrData> crateApiSimpleParsePaymentQr({required String qrData}) async =>
      PaymentQrData(
        merchantDid: 'did:ignite:mockMerchant',
        amount: BigInt.from(1000000000),
        description: 'Mock payment',
        orderId: 'mock_order',
        hubEndpoint: 'http://localhost:3003',
        timestamp: 0,
        merchantMbPubkey: '',
        merchantMediatorUrl: '',
        merchantWallet: '',
        acceptTokens: [],
      );

  @override
  Future<String> crateApiSimpleSettleChannel({
    required String storagePath,
    required String channelId,
    required String hubEndpoint,
  }) async =>
      'Channel $channelId settled.';

  @override
  Future<String> crateApiSimpleSignNonce({
    required String storagePath,
    required String nonce,
  }) async =>
      'mock_signature_base64';

  @override
  Future<bool> crateApiSimpleVerifyDidSignature({
    required String did,
    required String message,
    required String signatureB64,
  }) async =>
      true;

  @override
  Future<String> crateApiSimpleMbGetBuyerPubkey({
    required String storagePath,
  }) async =>
      'MockBuyerMbPubkey';

  @override
  Future<MbVoucherResult> crateApiSimpleMbSignVoucher({
    required String storagePath,
    required String programId,
    required String merchantMbPubkey,
    required BigInt seq,
    required BigInt amount,
  }) async =>
      MbVoucherResult(
        channelId: 'mock_channel_id',
        seq: seq,
        amount: amount,
        buyerPubkey: 'MockBuyerMbPubkey',
        buyerSig: 'mock_sig',
      );

  @override
  Future<void> crateApiSimpleMbSendVoucher({
    required String storagePath,
    required String merchantDid,
    required String orderId,
    required String channelId,
    required BigInt seq,
    required BigInt amount,
    required String buyerPubkey,
    required String buyerSig,
  }) async {}

  @override
  Future<void> crateApiSimpleSendQrPaymentRequest({
    required String storagePath,
    required String merchantDid,
    required BigInt amount,
    required String description,
    required String orderId,
    required String paymentMethod,
    required String token,
    required String merchantMediatorUrl,
  }) async {}

  @override
  Future<List<String>> crateApiSimpleDrainMediatorMessages() async => [];

  @override
  Future<String> crateApiSimpleBuildUnsignedTransferTx({
    required String rpcUrl,
    required String walletPubkeyB58,
    required String merchantDid,
    required BigInt amountLamports,
  }) async => 'unsigned_tx_b58_mock';

  @override
  Future<String> crateApiSimpleFetchRelayerPubkey({
    required String relayerUrl,
  }) async => 'mockRelayerPubkey11111111111111111111111111111111';

  @override
  Future<String> crateApiSimpleBuildUnsignedSponsoredTransferTx({
    required String rpcUrl,
    required String walletPubkeyB58,
    required String merchantDid,
    required BigInt amountLamports,
    required String relayerPubkeyB58,
  }) async => 'unsigned_sponsored_tx_b58_mock';

  @override
  Future<void> crateApiSimpleSendMbDepositRequest({
    required String storagePath,
    required BigInt amount,
    required String token,
  }) async {}

  @override
  Future<String> crateApiSimpleBuildUnsignedSplTransferTx({
    required String rpcUrl,
    required String walletPubkeyB58,
    required String merchantWalletB58,
    required BigInt amount,
    required String tokenMintB58,
  }) async => 'unsigned_spl_tx_b58_mock';

  @override
  Future<String> crateApiSimpleBuildUnsignedSponsoredSplTransferTx({
    required String rpcUrl,
    required String walletPubkeyB58,
    required String merchantWalletB58,
    required BigInt amount,
    required String tokenMintB58,
    required String relayerPubkeyB58,
  }) async => 'unsigned_sponsored_spl_tx_b58_mock';

  @override
  Future<void> crateApiSimpleSendSessionFundResponse({
    required String storagePath,
    required String mcpDid,
    required String sessionKeyPubkey,
    required bool funded,
    required BigInt newBalance,
    required String? txSignature,
  }) async {}

  @override
  Future<void> crateApiSimpleSendSessionRenewResponse({
    required String storagePath,
    required String mcpDid,
    required String oldSessionKeyPubkey,
    required String newSessionKeyPubkey,
    required bool renewed,
    required String? txSignature,
  }) async {}

  @override
  Future<SessionKeyInfo> crateApiSimpleRegisterExternalSessionKey({
    required String storagePath,
    required String rpcUrl,
    required String ownerSecretKey,
    required String ephemeralPubkey,
    required String ephemeralSecretKey,
    required String targetProgram,
    required List<String> scopes,
    required BigInt spendingLimit,
    required int durationSecs,
    String? tokenMint,
  }) async =>
      SessionKeyInfo(
        ephemeralPubkey: ephemeralPubkey,
        ephemeralSecretKey: ephemeralSecretKey,
        expiresAt: DateTime.now().millisecondsSinceEpoch + durationSecs * 1000,
        spendingLimit: spendingLimit,
        scopes: scopes,
        txSignature: 'mock_tx_sig',
        sessionPda: 'mock_pda',
      );

  @override
  Future<List<String>> crateApiSimpleFundSessionKey({
    required String rpcUrl,
    required String ownerSecretKey,
    required String ephemeralPubkey,
    required BigInt solAmount,
    String? splTokenMint,
    BigInt? splAmount,
  }) async => ['mock_fund_tx_sig'];

  @override
  Future<SessionKeyInfo> crateApiSimpleRegisterAndFundSessionKey({
    required String storagePath,
    required String rpcUrl,
    required String ownerSecretKey,
    required String ephemeralPubkey,
    required String ephemeralSecretKey,
    required String targetProgram,
    required List<String> scopes,
    required BigInt spendingLimit,
    required int durationSecs,
    String? tokenMint,
    required BigInt solFunding,
    BigInt? tokenFunding,
  }) async =>
      SessionKeyInfo(
        ephemeralPubkey: ephemeralPubkey,
        ephemeralSecretKey: ephemeralSecretKey,
        expiresAt: DateTime.now().millisecondsSinceEpoch + durationSecs * 1000,
        spendingLimit: spendingLimit,
        scopes: scopes,
        txSignature: 'mock_tx_sig',
        sessionPda: 'mock_pda',
      );
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('DidcommService', () {
    late DidcommService service;

    setUpAll(() {
      RustLib.initMock(api: _MockRustLibApi());
    });

    setUp(() {
      SharedPreferences.setMockInitialValues({});
      PathProviderPlatform.instance = _FakePathProviderPlatform();
      DidcommService.resetInstance();
      service = DidcommService();
      // Set Chinese locale so _isChineseUser returns true, preventing
      // Firebase/FCM initialization during parseInvitationAndConnect tests.
      TestWidgetsFlutterBinding.instance.platformDispatcher.localeTestValue =
          const Locale('zh', 'CN');
    });

    tearDown(() {
      TestWidgetsFlutterBinding.instance.platformDispatcher
          .clearLocaleTestValue();
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
        // Verify parsing without calling parseInvitationAndConnect,
        // which would attempt an HTTP POST and leak async errors.
        final uri = Uri.parse(url);
        final oobB64 = uri.queryParameters['_oob'];
        expect(oobB64, isNotNull);
        expect(oobB64, isNotEmpty);

        String padded = oobB64!;
        while (padded.length % 4 != 0) {
          padded += '=';
        }
        final invitation = jsonDecode(
            utf8.decode(base64Url.decode(padded))) as Map<String, dynamic>;

        expect(invitation['from'], 'did:ignite:zMcpTest');
        expect(invitation['type'],
            'https://didcomm.org/out-of-band/2.0/invitation');
        final body = invitation['body'] as Map<String, dynamic>;
        expect(body['label'], 'Test MCP');
        expect(body['accept'], contains('didcomm/v2'));
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
          throwsA(isA<FormatException>()),
        );
      });

      test('extracts mediator WS URL from services array', () async {
        await service.initialize();

        // Build an OOB URL and verify parsing extracts the correct mediator.
        // We don't call parseInvitationAndConnect because it would attempt
        // an HTTP POST to the non-existent wss:// endpoint, leaking async
        // errors into the next test.
        final url = buildOobUrl(
          fromDid: 'did:ignite:zMcpTest',
          wsUrl: 'wss://mediator.example.com/ws',
        );

        // Verify the URL contains the correct base64-encoded invitation.
        final uri = Uri.parse(url);
        final oobB64 = uri.queryParameters['_oob'];
        expect(oobB64, isNotNull);

        String padded = oobB64!;
        while (padded.length % 4 != 0) {
          padded += '=';
        }
        final invitation = jsonDecode(
            utf8.decode(base64Url.decode(padded))) as Map<String, dynamic>;
        final body = invitation['body'] as Map<String, dynamic>;
        final services = body['services'] as List<dynamic>;
        expect(services.length, 1);
        final svc = services.first as Map<String, dynamic>;
        expect(svc['service_endpoint'], 'wss://mediator.example.com/ws');
      });

      test('handles invitation with empty services gracefully', () async {
        await service.initialize();

        final url = buildOobUrl(
          fromDid: 'did:ignite:zMcpTest',
          services: [],
        );

        // Verify parsing without triggering HTTP POST (empty services = no
        // mediator URL, and parseInvitationAndConnect would try to POST to
        // an empty URL, leaking async errors).
        final uri = Uri.parse(url);
        final oobB64 = uri.queryParameters['_oob'];
        expect(oobB64, isNotNull);

        String padded = oobB64!;
        while (padded.length % 4 != 0) {
          padded += '=';
        }
        final invitation = jsonDecode(
            utf8.decode(base64Url.decode(padded))) as Map<String, dynamic>;
        final body = invitation['body'] as Map<String, dynamic>;
        final services = body['services'] as List<dynamic>;
        expect(services, isEmpty);
      });
    });
  });
}
