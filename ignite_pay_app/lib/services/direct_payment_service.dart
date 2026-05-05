import 'dart:async';
import 'package:dio/dio.dart';
import 'package:flutter/foundation.dart';
import 'package:ignite_pay_app/src/rust/api/simple.dart' as bridge;
import 'package:ignite_pay_app/services/wallet_deep_link_service.dart';

/// Result of a direct wallet payment.
class DirectPaymentResult {
  final bool success;
  final String? signature;
  final String? error;

  const DirectPaymentResult.success({this.signature})
      : success = true,
        error = null;
  const DirectPaymentResult.failure({required this.error})
      : success = false,
        signature = null;
}

/// Orchestrates the three-step direct wallet payment flow:
///   1. connectWallet() — opens wallet connect deep link
///   2. handleConnectCallback() — receives wallet pubkey
///   3. executePayment() — builds tx, opens sign deep link, awaits callback
class DirectPaymentService extends ChangeNotifier {
  static final DirectPaymentService _instance = DirectPaymentService._internal();
  factory DirectPaymentService() => _instance;
  DirectPaymentService._internal();

  // ── State ────────────────────────────────────────────────────────────

  /// Which wallet type was selected ('phantom' or 'solflare').
  String? _walletType;
  String? get walletType => _walletType;

  /// Wallet public key in base58, set after connect callback.
  String? _walletPubkey;
  String? get walletPubkey => _walletPubkey;

  /// Whether we're waiting for a connect callback.
  bool _isConnecting = false;
  bool get isConnecting => _isConnecting;

  /// Whether a payment is in progress.
  bool _isPaying = false;
  bool get isPaying => _isPaying;

  /// Completer bridging the async deep link callback into the executePayment Future.
  Completer<DirectPaymentResult>? _paymentCompleter;

  /// Completer bridging the connect deep link callback.
  Completer<String>? _connectCompleter;

  final _deepLink = WalletDeepLinkService();

  // ── Connect ──────────────────────────────────────────────────────────

  /// Open wallet connect deep link. Returns a Future that resolves with the
  /// wallet's base58 public key when the deep link callback arrives.
  Future<String> connectWallet(String walletType) async {
    _walletType = walletType;
    _walletPubkey = null;
    _isConnecting = true;
    _connectCompleter = Completer<String>();
    notifyListeners();

    final String url;
    if (walletType == 'phantom') {
      url = _deepLink.buildPhantomConnectUrl(
        redirectScheme: 'ignitepay',
        redirectPath: 'wallet_connect',
      );
    } else {
      url = _deepLink.buildSolflareConnectUrl(
        redirectScheme: 'ignitepay',
        redirectPath: 'wallet_connect',
      );
    }

    await _deepLink.openWalletUrl(url);
    return _connectCompleter!.future;
  }

  /// Called from the deep link handler when the wallet returns the public key.
  void handleConnectCallback(String publicKey) {
    _walletPubkey = publicKey;
    _isConnecting = false;
    notifyListeners();
    if (_connectCompleter != null && !_connectCompleter!.isCompleted) {
      _connectCompleter!.complete(publicKey);
    }
  }

  // ── Pay ──────────────────────────────────────────────────────────────

  /// Build unsigned tx, open sign deep link, and await the result.
  Future<DirectPaymentResult> executePayment({
    required String rpcUrl,
    required String merchantDid,
    required int amountLamports,
    String token = 'SOL',
    String tokenMint = '',
    String? merchantWallet,
  }) async {
    if (_walletPubkey == null) {
      return DirectPaymentResult.failure(error: 'Wallet not connected');
    }

    _isPaying = true;
    _paymentCompleter = Completer<DirectPaymentResult>();
    notifyListeners();

    try {
      final String unsignedTx;
      if (token != 'SOL' && tokenMint.isNotEmpty) {
        // SPL Token transfer
        final merchantAddr = merchantWallet ?? merchantDid;
        unsignedTx = await bridge.buildUnsignedSplTransferTx(
          rpcUrl: rpcUrl,
          walletPubkeyB58: _walletPubkey!,
          merchantWalletB58: merchantAddr,
          amount: BigInt.from(amountLamports),
          tokenMintB58: tokenMint,
        );
      } else {
        // SOL transfer
        unsignedTx = await bridge.buildUnsignedTransferTx(
          rpcUrl: rpcUrl,
          walletPubkeyB58: _walletPubkey!,
          merchantDid: merchantDid,
          amountLamports: BigInt.from(amountLamports),
        );
      }

      // Open wallet sign-and-send deep link
      final String url;
      if (_walletType == 'phantom') {
        url = _deepLink.buildPhantomSignUrl(
          transactionB58: unsignedTx,
          redirectScheme: 'ignitepay',
          redirectPath: 'direct_pay',
        );
      } else {
        url = _deepLink.buildSolflareSignUrl(
          transactionB58: unsignedTx,
          redirectScheme: 'ignitepay',
          redirectPath: 'direct_pay',
        );
      }

      await _deepLink.openWalletUrl(url);

      // Wait for the deep link callback
      final result = await _paymentCompleter!.future;
      return result;
    } catch (e) {
      _isPaying = false;
      notifyListeners();
      return DirectPaymentResult.failure(error: e.toString());
    } finally {
      _isPaying = false;
      notifyListeners();
    }
  }

  /// Called from the deep link handler on payment callback.
  void handlePaymentCallback({String? signature, String? errorCode}) {
    _isPaying = false;
    notifyListeners();

    if (_paymentCompleter == null || _paymentCompleter!.isCompleted) return;

    if (signature != null) {
      _paymentCompleter!.complete(DirectPaymentResult.success(signature: signature));
    } else {
      _paymentCompleter!.complete(DirectPaymentResult.failure(
        error: errorCode ?? 'Payment rejected by wallet',
      ));
    }
  }

  // ── Reset ────────────────────────────────────────────────────────────

  /// Clear all state (called on dispose or before a new flow).
  void reset() {
    _walletType = null;
    _walletPubkey = null;
    _isConnecting = false;
    _isPaying = false;
    if (_connectCompleter != null && !_connectCompleter!.isCompleted) {
      _connectCompleter!.completeError('reset');
    }
    if (_paymentCompleter != null && !_paymentCompleter!.isCompleted) {
      _paymentCompleter!.completeError('reset');
    }
    if (_sponsoredSignCompleter != null && !_sponsoredSignCompleter!.isCompleted) {
      _sponsoredSignCompleter!.completeError('reset');
    }
    _connectCompleter = null;
    _paymentCompleter = null;
    _sponsoredSignCompleter = null;
    notifyListeners();
  }

  // ── Sponsored Payment ────────────────────────────────────────────────

  /// Completer bridging the signTransaction deep link callback.
  Completer<String>? _sponsoredSignCompleter;

  /// Execute a sponsored payment: build tx → wallet signTransaction → relayer broadcast.
  Future<DirectPaymentResult> executeSponsoredPayment({
    required String rpcUrl,
    required String merchantDid,
    required int amountLamports,
    required String relayerUrl,
    String token = 'SOL',
    String tokenMint = '',
    String? merchantWallet,
  }) async {
    if (_walletPubkey == null) {
      return DirectPaymentResult.failure(error: 'Wallet not connected');
    }

    _isPaying = true;
    _sponsoredSignCompleter = Completer<String>();
    notifyListeners();

    try {
      // 1. Fetch relayer pubkey
      final relayerPubkey = await bridge.fetchRelayerPubkey(
        relayerUrl: relayerUrl,
      );

      // 2. Build unsigned sponsored tx
      final String unsignedTx;
      if (token != 'SOL' && tokenMint.isNotEmpty) {
        // SPL Token sponsored transfer
        final merchantAddr = merchantWallet ?? merchantDid;
        unsignedTx = await bridge.buildUnsignedSponsoredSplTransferTx(
          rpcUrl: rpcUrl,
          walletPubkeyB58: _walletPubkey!,
          merchantWalletB58: merchantAddr,
          amount: BigInt.from(amountLamports),
          tokenMintB58: tokenMint,
          relayerPubkeyB58: relayerPubkey,
        );
      } else {
        // SOL sponsored transfer
        unsignedTx = await bridge.buildUnsignedSponsoredTransferTx(
          rpcUrl: rpcUrl,
          walletPubkeyB58: _walletPubkey!,
          merchantDid: merchantDid,
          amountLamports: BigInt.from(amountLamports),
          relayerPubkeyB58: relayerPubkey,
        );
      }

      // 3. Open wallet signTransaction deep link
      final String url;
      if (_walletType == 'phantom') {
        url = _deepLink.buildPhantomSignTransactionUrl(
          transactionB58: unsignedTx,
          redirectScheme: 'ignitepay',
          redirectPath: 'sponsored_sign',
        );
      } else {
        url = _deepLink.buildSolflareSignTransactionUrl(
          transactionB58: unsignedTx,
          redirectScheme: 'ignitepay',
          redirectPath: 'sponsored_sign',
        );
      }

      await _deepLink.openWalletUrl(url);

      // 4. Wait for signTransaction callback with the signed tx
      final signedTx = await _sponsoredSignCompleter!.future;

      // 5. Send to relayer for fee-payer signature and broadcast
      final sponsorUrl = relayerUrl.replaceAll(RegExp(r'/sponsor$'), '').replaceAll(RegExp(r'/$'), '');
      final dio = Dio();
      final response = await dio.post(
        '$sponsorUrl/sponsor',
        data: {'transaction': signedTx},
      );

      if (response.statusCode != 200) {
        return DirectPaymentResult.failure(
          error: 'Relayer error: ${response.statusCode} ${response.data}',
        );
      }

      final result = response.data;
      final signature = result['signature'] as String?;

      if (signature == null) {
        return DirectPaymentResult.failure(error: 'No signature in relayer response');
      }

      return DirectPaymentResult.success(signature: signature);
    } catch (e) {
      return DirectPaymentResult.failure(error: e.toString());
    } finally {
      _isPaying = false;
      notifyListeners();
    }
  }

  /// Called from the deep link handler when the wallet returns a signed transaction.
  void handleSponsoredSignCallback(String signedTransaction) {
    if (_sponsoredSignCompleter != null && !_sponsoredSignCompleter!.isCompleted) {
      _sponsoredSignCompleter!.complete(signedTransaction);
    }
  }
}
