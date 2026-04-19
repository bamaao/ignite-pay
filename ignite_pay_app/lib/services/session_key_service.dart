import 'dart:async';
import 'package:flutter/foundation.dart';
import 'package:ignite_pay_app/src/rust/api/simple.dart' as rust;
import 'package:ignite_pay_app/src/rust/api/session.dart' as session;
import 'package:ignite_pay_app/services/wallet_deep_link_service.dart';
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Signing method for session key registration.
enum SigningMethod {
  builtIn,
  deepLink,
  mwa,
}

/// Service managing session key lifecycle: creation, registration, query, revocation.
class SessionKeyService extends ChangeNotifier {
  static final SessionKeyService _instance = SessionKeyService._internal();
  factory SessionKeyService() => _instance;
  SessionKeyService._internal();

  String _storagePath = '';
  String _rpcUrl = 'https://api.devnet.solana.com';
  bool _isRegistering = false;
  List<session.SessionKeyEntry> _sessionKeys = [];
  session.SessionKeyEntry? _activeSessionKey;
  session.UnsignedRegisterTx? _pendingUnsignedTx;

  bool get isRegistering => _isRegistering;
  List<session.SessionKeyEntry> get sessionKeys => List.unmodifiable(_sessionKeys);
  session.SessionKeyEntry? get activeSessionKey => _activeSessionKey;
  session.UnsignedRegisterTx? get pendingUnsignedTx => _pendingUnsignedTx;

  /// Initialize the service with the storage path and RPC URL from preferences.
  Future<void> initialize() async {
    final dir = await getApplicationSupportDirectory();
    _storagePath = dir.path;
    final prefs = await SharedPreferences.getInstance();
    _rpcUrl = prefs.getString('solana_rpc_url') ?? 'https://api.devnet.solana.com';
    await loadAllKeys();
  }

  /// Load all session keys from local storage.
  Future<void> loadAllKeys() async {
    if (_storagePath.isEmpty) return;
    try {
      _sessionKeys = await rust.listSessionKeys(storagePath: _storagePath);
      _activeSessionKey = _sessionKeys.where((e) => e.status == 'active').firstOrNull;
      notifyListeners();
    } catch (e) {
      debugPrint('Failed to load session keys: $e');
    }
  }

  /// Check if an active session key exists.
  Future<session.SessionKeyEntry?> checkExistingKey() async {
    if (_storagePath.isEmpty) return null;
    try {
      _activeSessionKey = await rust.findActiveSessionKey(storagePath: _storagePath);
      notifyListeners();
      return _activeSessionKey;
    } catch (e) {
      debugPrint('Failed to check existing session key: $e');
      return null;
    }
  }

  /// Create and register a session key using the built-in DID-derived key.
  /// This is the simplest method — no external wallet needed.
  Future<session.SessionKeyInfo> createWithBuiltInKey({
    required int spendingLimit,
    required int durationSecs,
  }) async {
    _setRegistering(true);
    try {
      final info = await session.createAndRegisterSessionKey(
        storagePath: _storagePath,
        rpcUrl: _rpcUrl,
        ownerSecretKey: '', // Will use DID-derived key inside Rust
        targetProgram: '11111111111111111111111111111111',
        scopes: ['sol:transfer'],
        spendingLimit: BigInt.from(spendingLimit),
        durationSecs: durationSecs,
      );
      await loadAllKeys();
      return info;
    } catch (e) {
      debugPrint('Built-in key registration failed: $e');
      rethrow;
    } finally {
      _setRegistering(false);
    }
  }

  /// Create a session key using Deep Link (Phantom/Solflare) signing.
  /// Returns the wallet URL to open, or null on error.
  Future<String?> createWithDeepLink({
    required int spendingLimit,
    required int durationSecs,
  }) async {
    _setRegistering(true);
    try {
      _pendingUnsignedTx = await rust.buildUnsignedRegisterTx(
        storagePath: _storagePath,
        rpcUrl: _rpcUrl,
        spendingLimit: BigInt.from(spendingLimit),
        durationSecs: durationSecs,
      );
      notifyListeners();

      // Build the deep link URL for Phantom
      final walletUrl = WalletDeepLinkService().buildPhantomSignUrl(
        transactionB58: _pendingUnsignedTx!.unsignedTxB58,
        redirectScheme: 'ignitepay',
        redirectPath: 'onchain',
      );
      return walletUrl;
    } catch (e) {
      debugPrint('Deep link tx build failed: $e');
      _setRegistering(false);
      rethrow;
    }
  }

  /// Create a session key using MWA (Mobile Wallet Adapter) signing.
  Future<String?> createWithMwa({
    required int spendingLimit,
    required int durationSecs,
  }) async {
    _setRegistering(true);
    try {
      _pendingUnsignedTx = await rust.buildUnsignedRegisterTx(
        storagePath: _storagePath,
        rpcUrl: _rpcUrl,
        spendingLimit: BigInt.from(spendingLimit),
        durationSecs: durationSecs,
      );
      notifyListeners();
      // MWA signing is handled by the caller via WalletMwaService
      return _pendingUnsignedTx!.unsignedTxB58;
    } catch (e) {
      debugPrint('MWA tx build failed: $e');
      _setRegistering(false);
      rethrow;
    }
  }

  /// Complete registration after receiving an owner signature from an external wallet.
  Future<session.SessionKeyInfo> completeRegistration(String signature) async {
    if (_pendingUnsignedTx == null) {
      throw Exception('No pending unsigned transaction');
    }
    try {
      final info = await rust.completeRegisterWithSignature(
        storagePath: _storagePath,
        ephemeralPubkey: _pendingUnsignedTx!.ephemeralPubkey,
        ownerSignatureB58: signature,
        rpcUrl: _rpcUrl,
      );
      _pendingUnsignedTx = null;
      await loadAllKeys();
      return info;
    } catch (e) {
      debugPrint('Complete registration failed: $e');
      rethrow;
    } finally {
      _setRegistering(false);
    }
  }

  /// Revoke a session key on-chain.
  Future<String> revokeKey(String sessionPubkey) async {
    final txSig = await rust.revokeSessionKeyOnchain(
      storagePath: _storagePath,
      sessionPubkey: sessionPubkey,
      rpcUrl: _rpcUrl,
    );
    await loadAllKeys();
    return txSig;
  }

  /// Delete a session key from local storage only.
  Future<void> deleteLocalKey(String sessionPubkey) async {
    await rust.deleteSessionKeyLocal(
      storagePath: _storagePath,
      sessionPubkey: sessionPubkey,
    );
    await loadAllKeys();
  }

  void _setRegistering(bool value) {
    _isRegistering = value;
    notifyListeners();
  }
}
