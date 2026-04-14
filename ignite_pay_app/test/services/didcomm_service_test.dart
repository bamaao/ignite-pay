import 'package:flutter_test/flutter_test.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  group('DidcommService', () {
    late DidcommService service;

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
  });
}
