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

import 'dart:async';
import 'dart:convert';
import 'dart:math';
import 'dart:typed_data';

import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:url_launcher/url_launcher.dart';

import 'package:ignite_pay_app/services/app_log_service.dart';
import 'package:ignite_pay_app/services/wallet_service.dart';
import 'package:ignite_pay_app/src/rust/api/phantom_crypto.dart' as crypto;

// ---------------------------------------------------------------------------
// Phantom deep link constants
// ---------------------------------------------------------------------------

const _kPhantomConnectBase = 'phantom://v1/connect';
const _kPhantomSignAndSendBase = 'phantom://v1/signAndSendTransaction';
const _kPhantomSignOnlyBase = 'phantom://v1/signTransaction';
const _kAppUrl = 'https://ignitepay.app';
const _kCluster = 'devnet';
const _kRedirectScheme = 'ignitepay';
const _kConnectPath = 'phantom/connect';
const _kSignPath = 'phantom/sign';
const _kSignOnlyPath = 'phantom/signonly';

// SharedPreferences keys
const _kPrefsPublicKey = 'phantom_dapp_public_key';
const _kPrefsSecretKey = 'phantom_dapp_secret_key';
const _kPrefsWalletPubkey = 'phantom_wallet_public_key';
const _kPrefsSession = 'phantom_session_token';
const _kPrefsSharedSecret = 'phantom_shared_secret';

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Manages the Phantom wallet deep link encryption session.
///
/// Crypto operations (Ed25519 keypair, X25519 key exchange, NaCl box
/// encrypt/decrypt) are delegated to Rust via flutter_rust_bridge.
/// See `rust/src/api/phantom_crypto.rs` for the Rust implementations.
class PhantomWalletService extends ChangeNotifier implements WalletService {
  static final PhantomWalletService _instance =
      PhantomWalletService._internal();
  factory PhantomWalletService() => _instance;
  PhantomWalletService._internal();

  // ── Session state ─────────────────────────────────────────────────────

  /// dApp Ed25519 keypair (base64, no padding).
  String? _dappPublicKeyB64;
  String? _dappSecretKeyB64;

  /// Phantom wallet public key (base58).
  String? _walletPublicKey;

  /// Session token returned by Phantom on connect.
  String? _sessionToken;

  /// X25519 shared secret (base64, no padding), derived from
  /// dApp secret key + Phantom encryption public key.
  String? _sharedSecretB64;

  bool _loaded = false;

  // ── Completers for deep link callbacks ─────────────────────────────────

  Completer<bool>? _connectCompleter;
  Completer<String?>? _signCompleter;
  Completer<String?>? _signOnlyCompleter;

  // ── Deep link listener ────────────────────────────────────────────────

  // Deep link routing is handled centrally by main.dart's _handleDeepLink,
  // which calls handleConnectCallback / handleSignCallback / handleSignOnlyCallback
  // directly. No separate listener is needed here.

  // ── Public API ────────────────────────────────────────────────────────

  /// Whether the service has an active Phantom session.
  @override
  bool get isConnected =>
      _walletPublicKey != null &&
      _sessionToken != null &&
      _sharedSecretB64 != null;

  /// The Phantom wallet public key in base58, or null if not connected.
  @override
  String? get walletPublicKey => _walletPublicKey;

  /// Initialize by loading any persisted session from SharedPreferences.
  @override
  Future<void> loadSession() async {
    if (_loaded) return;
    final prefs = await SharedPreferences.getInstance();
    _dappPublicKeyB64 = prefs.getString(_kPrefsPublicKey);
    _dappSecretKeyB64 = prefs.getString(_kPrefsSecretKey);
    _walletPublicKey = prefs.getString(_kPrefsWalletPubkey);
    _sessionToken = prefs.getString(_kPrefsSession);
    _sharedSecretB64 = prefs.getString(_kPrefsSharedSecret);
    _loaded = true;

    if (isConnected) {
      AppLogService().info('Phantom', 'Loaded session: wallet=$_walletPublicKey');
    }
  }

  /// Connect to Phantom wallet.
  ///
  /// Generates a dApp keypair (if not already present), builds the connect
  /// deep link, opens Phantom, and waits for the redirect callback.
  /// Returns `true` on success, `false` on failure.
  @override
  Future<bool> connect() async {
    await loadSession();

    // Generate a new dApp keypair for every connect attempt.
    try {
      final keypair = await crypto.phantomGenerateKeypair();
      _dappPublicKeyB64 = keypair.publicKeyB64;
      _dappSecretKeyB64 = keypair.secretKeyB64;
    } catch (e) {
      AppLogService().error('Phantom', 'Failed to generate keypair: $e');
      return false;
    }

    final redirect = Uri.encodeFull('$_kRedirectScheme://$_kConnectPath');
    final url = Uri.parse(_kPhantomConnectBase).replace(queryParameters: {
      'dapp_encryption_public_key': _b64ToB58(_dappPublicKeyB64!),
      'app_url': _kAppUrl,
      'cluster': _kCluster,
      'redirect_link': redirect,
    });

    // Set up completer — main.dart's deep link handler will call handleConnectCallback
    _connectCompleter = Completer<bool>();

    final launched = await _launchUrl(url);
    if (!launched) {
      AppLogService().error('Phantom', 'Could not launch Phantom connect URL');
      return false;
    }

    // Wait for the redirect callback with a timeout.
    try {
      final result = await _connectCompleter!.future
          .timeout(const Duration(minutes: 3));
      return result;
    } on TimeoutException {
      AppLogService().error('Phantom', 'Connect timed out');
      return false;
    } catch (e) {
      AppLogService().error('Phantom', 'Connect failed: $e');
      return false;
    }
  }

  /// Disconnect the Phantom session and clear persisted state.
  @override
  Future<void> disconnect() async {
    _dappPublicKeyB64 = null;
    _dappSecretKeyB64 = null;
    _walletPublicKey = null;
    _sessionToken = null;
    _sharedSecretB64 = null;
    _loaded = false;

    final prefs = await SharedPreferences.getInstance();
    await prefs.remove(_kPrefsPublicKey);
    await prefs.remove(_kPrefsSecretKey);
    await prefs.remove(_kPrefsWalletPubkey);
    await prefs.remove(_kPrefsSession);
    await prefs.remove(_kPrefsSharedSecret);

    notifyListeners();
    AppLogService().info('Phantom', 'Disconnected');
  }

  /// Sign and send a transaction through Phantom.
  ///
  /// [transactionB58] is the base58-encoded unsigned transaction.
  /// Returns the transaction signature (base58), or null if the user declined
  /// or an error occurred.
  @override
  Future<String?> signAndSendTransaction(String transactionB58) async {
    await loadSession();

    if (!isConnected) {
      AppLogService().error('Phantom', 'Not connected — call connect() first');
      return null;
    }

    // 1. Encrypt the transaction payload.
    final nonceBytes = _randomBytes(24);
    final nonceB64 = _b64Encode(nonceBytes);

    String payloadB64;
    try {
      payloadB64 = await crypto.phantomEncrypt(
        sharedSecretB64: _sharedSecretB64!,
        nonceB64: nonceB64,
        plaintextB64: _b64Encode(utf8.encode(transactionB58)),
      );
    } catch (e) {
      AppLogService().error('Phantom', 'Encrypt failed: $e');
      return null;
    }

    // 2. Build the sign deep link (Phantom expects base58-encoded fields).
    final redirect = Uri.encodeFull('$_kRedirectScheme://$_kSignPath');
    final url = Uri.parse(_kPhantomSignAndSendBase).replace(queryParameters: {
      'dapp_encryption_public_key': _b64ToB58(_dappPublicKeyB64!),
      'nonce': _b64ToB58(nonceB64),
      'payload': _b64ToB58(payloadB64),
      'session': _sessionToken,
      'redirect_link': redirect,
      'cluster': _kCluster,
    });

    // 3. Launch Phantom and wait for callback.
    _signCompleter = Completer<String?>();

    final launched = await _launchUrl(url);
    if (!launched) {
      AppLogService().error('Phantom', 'Could not launch Phantom sign URL');
      return null;
    }

    try {
      final result = await _signCompleter!.future
          .timeout(const Duration(minutes: 3));
      return result;
    } on TimeoutException {
      AppLogService().error('Phantom', 'Sign timed out');
      return null;
    } catch (e) {
      AppLogService().error('Phantom', 'Sign failed: $e');
      return null;
    }
  }

  /// Sign a transaction through Phantom (sign only, do not broadcast).
  ///
  /// [transactionB58] is the base58-encoded unsigned transaction.
  /// Returns the fully signed transaction (base58), or null if the user
  /// declined or an error occurred.
  @override
  Future<String?> signTransaction(String transactionB58) async {
    await loadSession();

    if (!isConnected) {
      AppLogService().error('Phantom', 'Not connected — call connect() first');
      return null;
    }

    // 1. Encrypt the transaction payload.
    final nonceBytes = _randomBytes(24);
    final nonceB64 = _b64Encode(nonceBytes);

    String payloadB64;
    try {
      payloadB64 = await crypto.phantomEncrypt(
        sharedSecretB64: _sharedSecretB64!,
        nonceB64: nonceB64,
        plaintextB64: _b64Encode(utf8.encode(transactionB58)),
      );
    } catch (e) {
      AppLogService().error('Phantom', 'Encrypt failed: $e');
      return null;
    }

    // 2. Build the sign-only deep link (Phantom expects base58-encoded fields).
    final redirect = Uri.encodeFull('$_kRedirectScheme://$_kSignOnlyPath');
    final url = Uri.parse(_kPhantomSignOnlyBase).replace(queryParameters: {
      'dapp_encryption_public_key': _b64ToB58(_dappPublicKeyB64!),
      'nonce': _b64ToB58(nonceB64),
      'payload': _b64ToB58(payloadB64),
      'session': _sessionToken,
      'redirect_link': redirect,
      'cluster': _kCluster,
    });

    // 3. Launch Phantom and wait for callback.
    _signOnlyCompleter = Completer<String?>();

    final launched = await _launchUrl(url);
    if (!launched) {
      AppLogService().error('Phantom', 'Could not launch Phantom signTransaction URL');
      return null;
    }

    try {
      final result = await _signOnlyCompleter!.future
          .timeout(const Duration(minutes: 3));
      return result;
    } on TimeoutException {
      AppLogService().error('Phantom', 'SignTransaction timed out');
      return null;
    } catch (e) {
      AppLogService().error('Phantom', 'SignTransaction failed: $e');
      return null;
    }
  }

  // ── Deep link callback handling ───────────────────────────────────────

  /// Called from main.dart's deep link handler when a `phantom/connect`
  /// callback URI is received.
  void handleConnectCallback(Uri uri) {
    AppLogService().info('Phantom', 'Connect callback received');

    // Guard against duplicate callback processing
    if (_connectCompleter == null || _connectCompleter!.isCompleted) {
      AppLogService().info('Phantom', 'Ignoring duplicate connect callback');
      return;
    }

    // Check for error response.
    final errorCode = uri.queryParameters['errorCode'];
    if (errorCode != null) {
      final errorMessage =
          uri.queryParameters['errorMessage'] ?? 'Unknown error';
      AppLogService().error('Phantom', 'Connect error: $errorMessage');
      _completeConnect(false);
      return;
    }

    final phantomEncPubKey = uri.queryParameters['phantom_encryption_public_key'];
    final dataB58 = uri.queryParameters['data'];
    final nonceB58 = uri.queryParameters['nonce'];

    if (phantomEncPubKey == null || dataB58 == null || nonceB58 == null) {
      AppLogService().error('Phantom', 'Connect callback missing parameters');
      _completeConnect(false);
      return;
    }

    AppLogService().info('Phantom', 'dapp_sec=${_dappSecretKeyB64?.substring(0, 8)}..., phantom_enc_pub_b58=${phantomEncPubKey.substring(0, 8)}..., dapp_pub=${_dappPublicKeyB64?.substring(0, 8)}...');

    // Phantom returns base58-encoded values; convert to base64url for Rust crypto.
    final phantomEncPubKeyB64 = _b58ToB64(phantomEncPubKey);
    final dataB64 = _b58ToB64(dataB58);
    final nonceB64 = _b58ToB64(nonceB58);

    AppLogService().info('Phantom', 'phantom_enc_pub_b64_len=${phantomEncPubKeyB64.length}, data_b64_len=${dataB64.length}, nonce_b64_len=${nonceB64.length}');

    // Compute shared secret.
    _computeSharedSecret(phantomEncPubKeyB64).then((_) async {
      if (_sharedSecretB64 == null) {
        AppLogService().error('Phantom', 'Shared secret is null after computation');
        _completeConnect(false);
        return;
      }

      AppLogService().info('Phantom', 'shared_secret=${_sharedSecretB64!.substring(0, 8)}..., trying decrypt...');

      // Decrypt the data payload.
      try {
        final decryptedB64 = await crypto.phantomDecrypt(
          sharedSecretB64: _sharedSecretB64!,
          nonceB64: nonceB64,
          ciphertextB64: dataB64,
        );
        final decryptedBytes = _b64Decode(decryptedB64);
        final decryptedStr = utf8.decode(decryptedBytes);
        final json = jsonDecode(decryptedStr) as Map<String, dynamic>;

        _walletPublicKey = json['public_key'] as String?;
        _sessionToken = json['session'] as String?;

        if (_walletPublicKey == null || _sessionToken == null) {
          AppLogService().error(
              'Phantom', 'Decrypted payload missing public_key or session');
          _completeConnect(false);
          return;
        }

        // Persist session.
        await _persistSession();

        AppLogService().info(
            'Phantom', 'Connected: wallet=$_walletPublicKey');
        _completeConnect(true);
      } catch (e) {
        AppLogService().error('Phantom', 'Decrypt failed: $e');
        _completeConnect(false);
      }
    }).catchError((e) {
      AppLogService().error('Phantom', 'Shared secret failed: $e');
      _completeConnect(false);
    });
  }

  /// Called from main.dart's deep link handler when a `phantom/sign`
  /// callback URI is received.
  void handleSignCallback(Uri uri) {
    AppLogService().info('Phantom', 'Sign callback received');

    // Check for error response.
    final errorCode = uri.queryParameters['errorCode'];
    if (errorCode != null) {
      final errorMessage =
          uri.queryParameters['errorMessage'] ?? 'Unknown error';
      AppLogService().error('Phantom', 'Sign error: $errorMessage');
      _completeSign(null);
      return;
    }

    final dataB58 = uri.queryParameters['data'];
    final nonceB58 = uri.queryParameters['nonce'];
    final signatureB58 = uri.queryParameters['signature'];

    if (dataB58 == null || nonceB58 == null) {
      AppLogService().error('Phantom', 'Sign callback missing data/nonce');
      _completeSign(null);
      return;
    }

    // Phantom returns base58-encoded values; convert to base64url for Rust crypto.
    final dataB64 = _b58ToB64(dataB58);
    final nonceB64 = _b58ToB64(nonceB58);

    // Decrypt the response data.
    () async {
      try {
        final decryptedB64 = await crypto.phantomDecrypt(
          sharedSecretB64: _sharedSecretB64!,
          nonceB64: nonceB64,
          ciphertextB64: dataB64,
        );
        final decryptedBytes = _b64Decode(decryptedB64);
        final decryptedStr = utf8.decode(decryptedBytes);
        final json = jsonDecode(decryptedStr) as Map<String, dynamic>;

        // The signature is in the decrypted payload as well as in the URL param.
        final sig =
            json['signature'] as String? ?? signatureB58;
        AppLogService().info('Phantom', 'Transaction signed: $sig');
        _completeSign(sig);
      } catch (e) {
        // If decryption fails, fall back to the URL signature param.
        if (signatureB58 != null) {
          AppLogService().info(
              'Phantom', 'Decrypt failed, using URL signature: $signatureB58');
          _completeSign(signatureB58);
        } else {
          AppLogService().error('Phantom', 'Sign decrypt failed: $e');
          _completeSign(null);
        }
      }
    }();
  }

  /// Called from main.dart's deep link handler when a `phantom/signonly`
  /// callback URI is received (from signTransaction).
  void handleSignOnlyCallback(Uri uri) {
    AppLogService().info('Phantom', 'SignOnly callback received');

    // Check for error response.
    final errorCode = uri.queryParameters['errorCode'];
    if (errorCode != null) {
      final errorMessage =
          uri.queryParameters['errorMessage'] ?? 'Unknown error';
      AppLogService().error('Phantom', 'SignOnly error: $errorMessage');
      _completeSignOnly(null);
      return;
    }

    final dataB58 = uri.queryParameters['data'];
    final nonceB58 = uri.queryParameters['nonce'];

    if (dataB58 == null || nonceB58 == null) {
      AppLogService().error('Phantom', 'SignOnly callback missing data/nonce');
      _completeSignOnly(null);
      return;
    }

    // Phantom returns base58-encoded values; convert to base64url for Rust crypto.
    final dataB64 = _b58ToB64(dataB58);
    final nonceB64 = _b58ToB64(nonceB58);

    // Decrypt the response data.
    () async {
      try {
        final decryptedB64 = await crypto.phantomDecrypt(
          sharedSecretB64: _sharedSecretB64!,
          nonceB64: nonceB64,
          ciphertextB64: dataB64,
        );
        final decryptedBytes = _b64Decode(decryptedB64);
        final decryptedStr = utf8.decode(decryptedBytes);
        final json = jsonDecode(decryptedStr) as Map<String, dynamic>;

        // signTransaction returns the signed transaction in base58.
        final signedTx = json['transaction'] as String?;
        if (signedTx != null) {
          AppLogService().info('Phantom', 'Transaction signed (signOnly): ${signedTx.substring(0, 20)}...');
          _completeSignOnly(signedTx);
        } else {
          AppLogService().error('Phantom', 'SignOnly response missing transaction field');
          _completeSignOnly(null);
        }
      } catch (e) {
        AppLogService().error('Phantom', 'SignOnly decrypt failed: $e');
        _completeSignOnly(null);
      }
    }();
  }

  // ── Internal helpers ──────────────────────────────────────────────────

  Future<void> _computeSharedSecret(String phantomEncPubKeyB64 /* already base64url */) async {
    if (_dappSecretKeyB64 == null) {
      AppLogService().error('Phantom', 'No dApp secret key for shared secret');
      return;
    }
    try {
      _sharedSecretB64 = await crypto.phantomSharedSecret(
        mySecretKeyB64: _dappSecretKeyB64!,
        theirPublicKeyB64: phantomEncPubKeyB64,
      );
    } catch (e) {
      AppLogService().error('Phantom', 'Shared secret computation failed: $e');
      _sharedSecretB64 = null;
    }
  }

  void _completeConnect(bool success) {
    notifyListeners();
    if (_connectCompleter != null && !_connectCompleter!.isCompleted) {
      _connectCompleter!.complete(success);
    }
  }

  void _completeSign(String? signature) {
    if (_signCompleter != null && !_signCompleter!.isCompleted) {
      _signCompleter!.complete(signature);
    }
  }

  void _completeSignOnly(String? signedTx) {
    if (_signOnlyCompleter != null && !_signOnlyCompleter!.isCompleted) {
      _signOnlyCompleter!.complete(signedTx);
    }
  }

  Future<void> _persistSession() async {
    final prefs = await SharedPreferences.getInstance();
    if (_dappPublicKeyB64 != null) {
      await prefs.setString(_kPrefsPublicKey, _dappPublicKeyB64!);
    }
    if (_dappSecretKeyB64 != null) {
      await prefs.setString(_kPrefsSecretKey, _dappSecretKeyB64!);
    }
    if (_walletPublicKey != null) {
      await prefs.setString(_kPrefsWalletPubkey, _walletPublicKey!);
    }
    if (_sessionToken != null) {
      await prefs.setString(_kPrefsSession, _sessionToken!);
    }
    if (_sharedSecretB64 != null) {
      await prefs.setString(_kPrefsSharedSecret, _sharedSecretB64!);
    }
  }

  Future<bool> _launchUrl(Uri url) async {
    try {
      return launchUrl(url, mode: LaunchMode.externalApplication);
    } catch (e) {
      AppLogService().error('Phantom', 'Launch URL failed: $e');
      return false;
    }
  }

  // ── Encoding helpers ──────────────────────────────────────────────────

  /// Base64url encode without padding.
  static String _b64Encode(List<int> bytes) {
    return base64Url.encode(bytes).replaceAll('=', '');
  }

  /// Base64url decode (adds padding automatically).
  static Uint8List _b64Decode(String b64) {
    var s = b64;
    while (s.length % 4 != 0) {
      s += '=';
    }
    return base64Url.decode(s);
  }

  /// Generate [n] cryptographically secure random bytes.
  static Uint8List _randomBytes(int n) {
    final rng = Random.secure();
    return Uint8List.fromList(List.generate(n, (_) => rng.nextInt(256)));
  }

  // ── Base58 helpers (Bitcoin alphabet, used by Phantom/Solana) ─────────

  static const _b58Alphabet =
      '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

  /// Encode bytes to base58 (Bitcoin alphabet).
  static String _b58Encode(List<int> input) {
    final bytes = Uint8List.fromList(input);
    int zeros = 0;
    while (zeros < bytes.length && bytes[zeros] == 0) {
      zeros++;
    }
    // Use a growable list to avoid any buffer sizing issues
    List<int> encoded = [];
    for (int i = zeros; i < bytes.length; i++) {
      int carry = bytes[i];
      for (int j = 0; j < encoded.length; j++) {
        carry += encoded[j] * 256;
        encoded[j] = carry % 58;
        carry ~/= 58;
      }
      while (carry > 0) {
        encoded.add(carry % 58);
        carry ~/= 58;
      }
    }
    // encoded is little-endian, need to reverse
    final sb = StringBuffer();
    for (int i = 0; i < zeros; i++) {
      sb.write('1');
    }
    for (int i = encoded.length - 1; i >= 0; i--) {
      sb.write(_b58Alphabet[encoded[i]]);
    }
    return sb.toString();
  }

  /// Decode a base58 string to bytes.
  static Uint8List _b58Decode(String input) {
    int zeros = 0;
    while (zeros < input.length && input[zeros] == '1') {
      zeros++;
    }
    // Use a growable list to avoid any buffer sizing issues
    List<int> bytes = [];
    for (int i = zeros; i < input.length; i++) {
      int carry = _b58Alphabet.indexOf(input[i]);
      if (carry < 0) {
        throw FormatException('Invalid base58 character: ${input[i]}');
      }
      for (int j = 0; j < bytes.length; j++) {
        carry += bytes[j] * 58;
        bytes[j] = carry & 0xFF;
        carry >>= 8;
      }
      while (carry > 0) {
        bytes.add(carry & 0xFF);
        carry >>= 8;
      }
    }
    // bytes is little-endian, need to reverse to big-endian
    final result = BytesBuilder();
    for (int i = 0; i < zeros; i++) {
      result.addByte(0);
    }
    for (int i = bytes.length - 1; i >= 0; i--) {
      result.addByte(bytes[i]);
    }
    return result.toBytes();
  }

  /// Convert a base64url-encoded string to base58.
  static String _b64ToB58(String b64) {
    return _b58Encode(_b64Decode(b64));
  }

  /// Convert a base58-encoded string to base64url (no padding).
  static String _b58ToB64(String b58) {
    return _b64Encode(_b58Decode(b58));
  }
}
