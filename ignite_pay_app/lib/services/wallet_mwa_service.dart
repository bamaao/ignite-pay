import 'dart:typed_data';
import 'package:flutter/foundation.dart';

/// Service for Mobile Wallet Adapter (MWA) integration.
///
/// MWA is Android-only and requires the `solana_mobile_wallet_adapter` package.
/// This is a stub implementation that can be filled in when the MWA dependency
/// is added and native Android setup is complete.
class WalletMwaService {
  static final WalletMwaService _instance = WalletMwaService._internal();
  factory WalletMwaService() => _instance;
  WalletMwaService._internal();

  bool get isAvailable => false; // MWA not yet integrated

  /// Authorize the wallet and get a public key.
  /// Returns the base58-encoded public key, or null if declined/unavailable.
  Future<String?> authorize() async {
    debugPrint('MWA: authorize() called but MWA is not integrated yet');
    return null;
  }

  /// Sign and send a transaction via MWA.
  /// Returns the transaction signature, or null if declined/unavailable.
  Future<String?> signAndSendTransaction(Uint8List transactionBytes) async {
    debugPrint('MWA: signAndSendTransaction() called but MWA is not integrated yet');
    return null;
  }

  /// Sign a transaction without sending via MWA.
  /// Returns the signature bytes, or null if declined/unavailable.
  Future<Uint8List?> signTransaction(Uint8List transactionBytes) async {
    debugPrint('MWA: signTransaction() called but MWA is not integrated yet');
    return null;
  }
}
