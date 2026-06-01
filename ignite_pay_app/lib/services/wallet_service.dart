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

import 'dart:io' show Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import 'package:ignite_pay_app/services/native_deep_link_wallet_service.dart';
import 'package:ignite_pay_app/services/native_wallet_config.dart';
import 'package:ignite_pay_app/services/reown_wallet_service.dart';

/// True on Android/iOS where native wallet deep links work.
bool get supportsNativeWalletDeepLink =>
    !kIsWeb && (Platform.isAndroid || Platform.isIOS);

/// Default wallet: Phantom deep link on mobile, WalletConnect elsewhere.
WalletService createDefaultWalletService() {
  if (supportsNativeWalletDeepLink) {
    return NativeDeepLinkWalletService.forWallet(NativeWalletId.phantom);
  }
  return ReownWalletService();
}

/// Init Reown if needed, then connect when no session exists.
Future<void> ensureWalletConnected(
  WalletService wallet,
  BuildContext context,
) async {
  if (wallet is ReownWalletService) {
    await wallet.init(context);
  }
  await wallet.loadSession();
  if (!wallet.isConnected) {
    final connected = await wallet.connect();
    if (!connected) {
      final detail = wallet.lastError;
      final suffix = (detail != null && detail.isNotEmpty) ? ': $detail' : '';
      throw StateError(
        'Failed to connect to ${wallet.walletDisplayName}$suffix',
      );
    }
  }
}

/// Abstract interface for wallet connections.
///
/// Implementations: [PhantomWalletService] (deep link), [ReownWalletService]
/// (WalletConnect v2 via reown_appkit).
abstract class WalletService extends ChangeNotifier {
  /// Human-readable wallet name for UI.
  String get walletDisplayName;

  /// Last wallet-level error for diagnostics.
  String? get lastError;

  /// The wallet public key in base58, or null if not connected.
  String? get walletPublicKey;

  /// Whether the service has an active wallet session.
  bool get isConnected;

  /// Connect to the wallet. Returns `true` on success.
  Future<bool> connect();

  /// Disconnect and clear session state.
  Future<void> disconnect();

  /// Load any persisted session.
  Future<void> loadSession();

  /// Sign transaction without broadcasting. Returns signed tx base58.
  Future<String?> signTransaction(String transactionB58);

  /// Sign and broadcast transaction. Returns tx signature base58.
  Future<String?> signAndSendTransaction(String transactionB58);
}
