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
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/widgets/message_list.dart';

void main() {
  group('MessageList', () {
    Widget buildList({
      required List<DecryptedMsg> messages,
      ValueChanged<DecryptedMsg>? onMessageTap,
    }) {
      return MaterialApp(
        home: Scaffold(
          body: MessageList(
            messages: messages,
            onMessageTap: onMessageTap,
          ),
        ),
      );
    }

    testWidgets('shows empty state when no messages', (tester) async {
      await tester.pumpWidget(buildList(messages: []));
      expect(find.text('No messages yet'), findsOneWidget);
    });

    testWidgets('shows message count', (tester) async {
      final msgs = [
        DecryptedMsg(msgType: 'test', rawBody: '{}'),
        DecryptedMsg(msgType: 'test', rawBody: '{}'),
        DecryptedMsg(msgType: 'test', rawBody: '{}'),
      ];

      await tester.pumpWidget(buildList(messages: msgs));
      expect(find.text('3'), findsOneWidget);
    });

    testWidgets('shows "MESSAGES" header when messages exist', (tester) async {
      final msgs = [DecryptedMsg(msgType: 'test', rawBody: '{}')];
      await tester.pumpWidget(buildList(messages: msgs));
      expect(find.text('MESSAGES'), findsOneWidget);
    });

    testWidgets('renders Payment Request for auth message type',
        (tester) async {
      final msgs = [
        DecryptedMsg(
          msgType: 'payment-auth-request/v1',
          paymentId: 'pay_123456789012',
          rawBody: '{}',
        ),
      ];

      await tester.pumpWidget(buildList(messages: msgs));
      expect(find.text('Payment Request'), findsOneWidget);
    });

    testWidgets('renders List Sync for list-sync message type', (tester) async {
      final msgs = [
        DecryptedMsg(msgType: 'list-sync-notification/v1', rawBody: '{}'),
      ];

      await tester.pumpWidget(buildList(messages: msgs));
      expect(find.text('List Sync'), findsOneWidget);
    });

    testWidgets('renders generic Message for unknown type', (tester) async {
      final msgs = [
        DecryptedMsg(msgType: 'unknown-type', rawBody: '{}'),
      ];

      await tester.pumpWidget(buildList(messages: msgs));
      expect(find.text('Message'), findsOneWidget);
    });

    testWidgets('shows payment ID subtitle when available', (tester) async {
      final msgs = [
        DecryptedMsg(
          msgType: 'payment-auth-request/v1',
          paymentId: 'pay_abcdefghij1234567890',
          rawBody: '{}',
        ),
      ];

      await tester.pumpWidget(buildList(messages: msgs));

      // First 12 chars: "pay_abcdefgh" + "..."
      expect(find.textContaining('Payment: pay_abcdefgh'), findsOneWidget);
    });

    testWidgets('shows msgType segment as subtitle when no paymentId',
        (tester) async {
      final msgs = [
        DecryptedMsg(msgType: 'some/namespace/action', rawBody: '{}'),
      ];

      await tester.pumpWidget(buildList(messages: msgs));
      expect(find.text('action'), findsOneWidget);
    });

    testWidgets('fires onMessageTap with correct message', (tester) async {
      DecryptedMsg? tappedMsg;
      final msg = DecryptedMsg(msgType: 'test', rawBody: '{}');
      final msgs = [msg];

      await tester.pumpWidget(buildList(
        messages: msgs,
        onMessageTap: (m) => tappedMsg = m,
      ));

      // Tap the message tile (find by type since tiles are containers)
      await tester.tap(find.text('Message'));
      expect(tappedMsg, msg);
    });

    testWidgets('renders multiple messages in order', (tester) async {
      final msgs = [
        DecryptedMsg(
          msgType: 'payment-auth-request/v1',
          rawBody: '{}',
        ),
        DecryptedMsg(
          msgType: 'list-sync-notification/v1',
          rawBody: '{}',
        ),
        DecryptedMsg(msgType: 'unknown', rawBody: '{}'),
      ];

      await tester.pumpWidget(buildList(messages: msgs));
      expect(find.text('Payment Request'), findsOneWidget);
      expect(find.text('List Sync'), findsOneWidget);
      expect(find.text('Message'), findsOneWidget);
    });

    testWidgets('short payment ID shown in full', (tester) async {
      final msgs = [
        DecryptedMsg(
          msgType: 'payment-auth-request/v1',
          paymentId: 'short',
          rawBody: '{}',
        ),
      ];

      await tester.pumpWidget(buildList(messages: msgs));
      expect(find.textContaining('Payment: short'), findsOneWidget);
    });
  });
}
