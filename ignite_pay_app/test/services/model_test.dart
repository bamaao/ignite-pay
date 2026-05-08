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

import 'package:flutter_test/flutter_test.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/services/mediator_api.dart';

void main() {
  // ===========================================================================
  // AuthRequest
  // ===========================================================================
  group('AuthRequest', () {
    test('constructs with required fields', () {
      final req = AuthRequest(
        paymentId: 'pay_123',
        merchantDid: 'did:solana:abc',
        amount: 500000000,
        description: 'Test payment',
      );
      expect(req.paymentId, 'pay_123');
      expect(req.merchantDid, 'did:solana:abc');
      expect(req.amount, 500000000);
      expect(req.description, 'Test payment');
    });

    test('allows empty strings', () {
      final req = AuthRequest(
        paymentId: '',
        merchantDid: '',
        amount: 0,
        description: '',
      );
      expect(req.paymentId, isEmpty);
      expect(req.amount, 0);
    });
  });

  // ===========================================================================
  // AuthResponseData
  // ===========================================================================
  group('AuthResponseData', () {
    test('constructs with required fields', () {
      final resp = AuthResponseData(
        paymentId: 'pay_123',
        authorized: true,
        listAction: 'none',
      );
      expect(resp.paymentId, 'pay_123');
      expect(resp.authorized, true);
      expect(resp.listAction, 'none');
      expect(resp.listLabel, isNull);
      expect(resp.listMaxAmount, isNull);
    });

    test('constructs with optional fields', () {
      final resp = AuthResponseData(
        paymentId: 'pay_456',
        authorized: false,
        listAction: 'add_whitelist',
        listLabel: 'ShopX',
        listMaxAmount: 1000000000,
      );
      expect(resp.listLabel, 'ShopX');
      expect(resp.listMaxAmount, 1000000000);
    });

    test('all list actions are valid strings', () {
      const actions = [
        'none',
        'add_whitelist',
        'add_blacklist',
        'remove_whitelist',
        'remove_blacklist',
      ];
      for (final action in actions) {
        final resp = AuthResponseData(
          paymentId: '',
          authorized: true,
          listAction: action,
        );
        expect(resp.listAction, action);
      }
    });
  });

  // ===========================================================================
  // DecryptedMsg
  // ===========================================================================
  group('DecryptedMsg', () {
    test('constructs with required fields only', () {
      final msg = DecryptedMsg(
        msgType: 'payment-auth-request',
        rawBody: '{}',
      );
      expect(msg.msgType, 'payment-auth-request');
      expect(msg.rawBody, '{}');
      expect(msg.paymentId, isNull);
      expect(msg.merchantDid, isNull);
      expect(msg.amount, isNull);
      expect(msg.description, isNull);
      expect(msg.listCid, isNull);
      expect(msg.listType, isNull);
      expect(msg.label, isNull);
    });

    test('constructs with all fields', () {
      final msg = DecryptedMsg(
        msgType: 'list-sync-notification',
        paymentId: 'pay_789',
        merchantDid: 'did:ignite:zMerchant',
        amount: 250000000,
        description: 'Subscription',
        rawBody: '{"test": true}',
        listCid: 'QmXyz123',
        listType: 'whitelist',
        label: 'ShopX Marketplace',
      );
      expect(msg.paymentId, 'pay_789');
      expect(msg.merchantDid, 'did:ignite:zMerchant');
      expect(msg.amount, 250000000);
      expect(msg.description, 'Subscription');
      expect(msg.listCid, 'QmXyz123');
      expect(msg.listType, 'whitelist');
      expect(msg.label, 'ShopX Marketplace');
    });
  });

  // ===========================================================================
  // DidcommMessage
  // ===========================================================================
  group('DidcommMessage', () {
    test('fromJson parses all fields correctly', () {
      final json = {
        'msg_id': 'msg_001',
        'jwe_envelope': 'eyJhbGciOiJFQ0RILUVTK...',
        'created_at': 1712640000,
      };
      final msg = DidcommMessage.fromJson(json);
      expect(msg.msgId, 'msg_001');
      expect(msg.jweEnvelope, 'eyJhbGciOiJFQ0RILUVTK...');
      expect(msg.createdAt, 1712640000);
    });

    test('fromJson handles extra fields gracefully', () {
      final json = {
        'msg_id': 'msg_002',
        'jwe_envelope': 'abc',
        'created_at': 123,
        'extra_field': 'ignored',
      };
      final msg = DidcommMessage.fromJson(json);
      expect(msg.msgId, 'msg_002');
    });

    test('fromJson throws on missing required field', () {
      final json = {'msg_id': 'msg_003'};
      expect(() => DidcommMessage.fromJson(json), throwsA(isA<TypeError>()));
    });

    test('fromJson throws on wrong type', () {
      final json = {
        'msg_id': 12345, // wrong type
        'jwe_envelope': 'abc',
        'created_at': 123,
      };
      expect(() => DidcommMessage.fromJson(json), throwsA(isA<TypeError>()));
    });
  });
}
