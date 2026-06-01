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

import 'package:bs58/bs58.dart' as bs58;
import 'package:shared_preferences/shared_preferences.dart';
import 'package:url_launcher/url_launcher.dart';

import 'package:ignite_pay_app/services/app_log_service.dart';
import 'package:ignite_pay_app/services/native_wallet_config.dart';
import 'package:ignite_pay_app/services/wallet_service.dart';
import 'package:ignite_pay_app/src/rust/api/phantom_crypto.dart' as crypto;

const _kAppUrl = 'https://ignitepay.app';
const _kCluster = 'devnet';
const _kRedirectScheme = 'ignitepay';

/// Encrypted deep-link wallet connection (Phantom-compatible protocol).
///
/// One cached instance per [NativeWalletId]; each wallet keeps its own session.
class NativeDeepLinkWalletService extends WalletService {
  final NativeWalletConfig config;

  static final Map<NativeWalletId, NativeDeepLinkWalletService> _instances = {};

  factory NativeDeepLinkWalletService.forWallet(NativeWalletId id) {
    return _instances.putIfAbsent(
      id,
      () => NativeDeepLinkWalletService.withConfig(NativeWalletConfigs.byId(id)),
    );
  }

  NativeDeepLinkWalletService.withConfig(this.config) {
    _instances[config.id] = this;
  }

  String? _dappPublicKeyB64;
  String? _dappSecretKeyB64;
  String? _walletPublicKey;
  String? _sessionToken;
  String? _sharedSecretB64;
  bool _loaded = false;

  Completer<bool>? _connectCompleter;
  Completer<String?>? _signCompleter;
  Completer<String?>? _signOnlyCompleter;
  final List<MapEntry<String, String>> _connectKeyCandidates = [];

  String get _logTag => config.displayName;
  String? _lastError;
  String get _kPrefsPublicKey => '${config.prefsPrefix}dapp_public_key';
  String get _kPrefsSecretKey => '${config.prefsPrefix}dapp_secret_key';
  String get _kPrefsWalletPubkey => '${config.prefsPrefix}wallet_public_key';
  String get _kPrefsSession => '${config.prefsPrefix}session_token';
  String get _kPrefsSharedSecret => '${config.prefsPrefix}shared_secret';

  @override
  String get walletDisplayName => config.displayName;

  @override
  String? get lastError => _lastError;

  String? _extractWalletError(Map<String, String> params) {
    String? raw = params['errorMessage'] ??
        params['error_message'] ??
        params['error'] ??
        params['errorCode'] ??
        params['error_code'];
    if ((raw == null || raw.isEmpty) &&
        (params['status'] == 'error' || params['status'] == 'rejected')) {
      raw = params['message'];
    }
    if (raw == null || raw.isEmpty) return null;

    // Some wallets return JSON-encoded error payload in `error`.
    try {
      final decoded = jsonDecode(raw);
      if (decoded is Map<String, dynamic>) {
        final nested = decoded['errorMessage'] ??
            decoded['message'] ??
            decoded['reason'] ??
            decoded['code'];
        if (nested != null) return nested.toString();
      }
    } catch (_) {}
    return raw;
  }

  @override
  bool get isConnected =>
      _walletPublicKey != null &&
      _sessionToken != null &&
      _sharedSecretB64 != null;

  @override
  String? get walletPublicKey => _walletPublicKey;

  /// Route ignitepay://{wallet}/connect|sign|signonly callbacks.
  /// Returns true if the path was handled.
  static bool routeCallback(String path, Uri uri) {
    final normalized = path.replaceFirst(RegExp(r'^/+'), '').replaceAll(RegExp(r'/+$'), '');
    for (final cfg in NativeWalletConfigs.mobileWallets) {
      final svc = NativeDeepLinkWalletService.forWallet(cfg.id);
      if (normalized == cfg.connectRedirectPath) {
        svc.handleConnectCallback(uri);
        return true;
      }
      if (normalized == cfg.signRedirectPath) {
        svc.handleSignCallback(uri);
        return true;
      }
      if (normalized == cfg.signOnlyRedirectPath) {
        svc.handleSignOnlyCallback(uri);
        return true;
      }
    }
    return false;
  }

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
      AppLogService().info(_logTag, 'Loaded session: wallet=$_walletPublicKey');
    }
  }

  @override
  Future<bool> connect() async {
    if (isConnected) {
      await disconnect();
    }

    await loadSession();
    _connectKeyCandidates.clear();
    if (_dappPublicKeyB64 != null && _dappSecretKeyB64 != null) {
      _connectKeyCandidates.add(
        MapEntry(_dappPublicKeyB64!, _dappSecretKeyB64!),
      );
    }

    try {
      final keypair = await crypto.phantomGenerateKeypair();
      _dappPublicKeyB64 = keypair.publicKeyB64;
      _dappSecretKeyB64 = keypair.secretKeyB64;
      _connectKeyCandidates.insert(
        0,
        MapEntry(_dappPublicKeyB64!, _dappSecretKeyB64!),
      );
      _lastError = null;
    } catch (e) {
      _lastError = 'Failed to initialize wallet encryption keypair';
      AppLogService().error(_logTag, 'Failed to generate keypair: $e');
      return false;
    }

    final redirect = _buildRedirectLink(config.connectRedirectPath);
    final url = Uri.parse(config.connectUrl).replace(queryParameters: {
      'dapp_encryption_public_key': _b64ToB58(_dappPublicKeyB64!),
      'app_url': _kAppUrl,
      'cluster': _kCluster,
      'redirect_link': redirect,
    });

    _connectCompleter = Completer<bool>();

    final launched = await _launchUrl(url);
    if (!launched) {
      _lastError = 'Could not open ${config.displayName} app';
      AppLogService().error(_logTag, 'Could not launch connect URL');
      return false;
    }

    try {
      return await _connectCompleter!.future
          .timeout(const Duration(minutes: 3));
    } on TimeoutException {
      _connectKeyCandidates.clear();
      _lastError = 'Connection timed out waiting for ${config.displayName} callback';
      AppLogService().error(_logTag, 'Connect timed out');
      return false;
    } catch (e) {
      _connectKeyCandidates.clear();
      _lastError = 'Connect failed: $e';
      AppLogService().error(_logTag, 'Connect failed: $e');
      return false;
    }
  }

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
    AppLogService().info(_logTag, 'Disconnected');
  }

  @override
  Future<String?> signAndSendTransaction(String transactionB58) async {
    await loadSession();
    if (!isConnected) {
      _lastError = 'Wallet not connected';
      AppLogService().error(_logTag, 'Not connected — call connect() first');
      return null;
    }

    final nonceBytes = _randomBytes(24);
    final nonceB64 = _b64Encode(nonceBytes);

    String payloadB64;
    try {
      final payloadJson = jsonEncode({
        'transaction': transactionB58,
        'session': _sessionToken,
      });
      payloadB64 = await crypto.phantomEncrypt(
        sharedSecretB64: _sharedSecretB64!,
        nonceB64: nonceB64,
        plaintextB64: _b64Encode(utf8.encode(payloadJson)),
      );
    } catch (e) {
      _lastError = 'Failed to encrypt transaction payload';
      AppLogService().error(_logTag, 'Encrypt failed: $e');
      return null;
    }

    final redirect = _buildRedirectLink(config.signRedirectPath);
    final url = Uri.parse(config.signAndSendUrl).replace(queryParameters: {
      'dapp_encryption_public_key': _b64ToB58(_dappPublicKeyB64!),
      'nonce': _b64ToB58(nonceB64),
      'payload': _b64ToB58(payloadB64),
      'redirect_link': redirect,
      'cluster': _kCluster,
    });

    _signCompleter = Completer<String?>();

    final launched = await _launchUrl(url);
    if (!launched) {
      _lastError = 'Could not launch ${config.displayName} for signing';
      AppLogService().error(_logTag, 'Could not launch sign URL');
      return null;
    }

    try {
      return await _signCompleter!.future.timeout(const Duration(minutes: 3));
    } on TimeoutException {
      _lastError = 'Wallet sign request timed out';
      AppLogService().error(_logTag, 'Sign timed out');
      return null;
    } catch (e) {
      _lastError = 'Wallet sign failed: $e';
      AppLogService().error(_logTag, 'Sign failed: $e');
      return null;
    }
  }

  @override
  Future<String?> signTransaction(String transactionB58) async {
    await loadSession();
    if (!isConnected) {
      _lastError = 'Wallet not connected';
      AppLogService().error(_logTag, 'Not connected — call connect() first');
      return null;
    }

    final nonceBytes = _randomBytes(24);
    final nonceB64 = _b64Encode(nonceBytes);

    String payloadB64;
    try {
      final payloadJson = jsonEncode({
        'transaction': transactionB58,
        'session': _sessionToken,
      });
      payloadB64 = await crypto.phantomEncrypt(
        sharedSecretB64: _sharedSecretB64!,
        nonceB64: nonceB64,
        plaintextB64: _b64Encode(utf8.encode(payloadJson)),
      );
    } catch (e) {
      _lastError = 'Failed to encrypt transaction payload';
      AppLogService().error(_logTag, 'Encrypt failed: $e');
      return null;
    }

    final redirect = _buildRedirectLink(config.signOnlyRedirectPath);
    final url = Uri.parse(config.signOnlyUrl).replace(queryParameters: {
      'dapp_encryption_public_key': _b64ToB58(_dappPublicKeyB64!),
      'nonce': _b64ToB58(nonceB64),
      'payload': _b64ToB58(payloadB64),
      'redirect_link': redirect,
      'cluster': _kCluster,
    });

    _signOnlyCompleter = Completer<String?>();

    final launched = await _launchUrl(url);
    if (!launched) {
      _lastError = 'Could not launch ${config.displayName} for signTransaction';
      AppLogService().error(_logTag, 'Could not launch signTransaction URL');
      return null;
    }

    try {
      return await _signOnlyCompleter!.future
          .timeout(const Duration(minutes: 3));
    } on TimeoutException {
      _lastError = 'Wallet signTransaction request timed out';
      AppLogService().error(_logTag, 'SignTransaction timed out');
      return null;
    } catch (e) {
      _lastError = 'Wallet signTransaction failed: $e';
      AppLogService().error(_logTag, 'SignTransaction failed: $e');
      return null;
    }
  }

  void handleConnectCallback(Uri uri) {
    AppLogService().info(_logTag, 'Connect callback received');
    final params = _allParams(uri);

    if (_connectCompleter == null || _connectCompleter!.isCompleted) {
      AppLogService().info(_logTag, 'Ignoring duplicate connect callback');
      return;
    }

    final errorCode = params['errorCode'];
    if (errorCode != null) {
      final errorMessage = params['errorMessage'] ?? 'Unknown error';
      _lastError = 'Wallet returned error: $errorMessage';
      AppLogService().error(_logTag, 'Connect error: $errorMessage');
      _completeConnect(false);
      return;
    }

    final walletEncPubKey = params[config.encryptionPublicKeyParam] ??
        params['phantom_encryption_public_key'] ??
        params['solflare_encryption_public_key'] ??
        params['wallet_encryption_public_key'];
    final dataB58 = params['data'];
    final nonceB58 = params['nonce'];

    if (walletEncPubKey == null || dataB58 == null || nonceB58 == null) {
      final missing = <String>[
        if (walletEncPubKey == null) 'wallet_encryption_public_key',
        if (dataB58 == null) 'data',
        if (nonceB58 == null) 'nonce',
      ];
      _lastError = 'Wallet callback missing fields: ${missing.join(', ')}';
      AppLogService().error(
        _logTag,
        'Connect callback missing parameters: ${missing.join(', ')}; uri=$uri',
      );
      _completeConnect(false);
      return;
    }

    final walletEncPubKeyCandidates = <String>[];
    final nonceCandidates = <String>[];
    final dataCandidates = <String>[];
    try {
      walletEncPubKeyCandidates.addAll(
        _walletValueToB64CandidatesWithExpectedLen(
          walletEncPubKey,
          expectedLen: 32,
        ),
      );
      nonceCandidates.addAll(
        _walletValueToB64CandidatesWithExpectedLen(
          nonceB58,
          expectedLen: 24,
        ),
      );
      dataCandidates.addAll(_ciphertextB64Candidates(dataB58));
    } catch (e) {
      _lastError = 'Unsupported callback encoding from wallet';
      AppLogService().error(
        _logTag,
        'Failed to decode callback fields; uri=$uri; error=$e',
      );
      _completeConnect(false);
      return;
    }

    () async {
      final candidates = _connectKeyCandidates.isNotEmpty
          ? List<MapEntry<String, String>>.from(_connectKeyCandidates)
          : (_dappPublicKeyB64 != null && _dappSecretKeyB64 != null)
              ? [MapEntry(_dappPublicKeyB64!, _dappSecretKeyB64!)]
              : const <MapEntry<String, String>>[];
      if (candidates.isEmpty) {
        _lastError = 'No dApp keypair available for wallet callback';
        AppLogService().error(_logTag, _lastError!);
        _completeConnect(false);
        return;
      }

      Object? lastErr;
      for (final pair in candidates) {
        for (final walletEncPubKeyB64 in walletEncPubKeyCandidates) {
          String shared;
          try {
            shared = await crypto.phantomSharedSecret(
              mySecretKeyB64: pair.value,
              theirPublicKeyB64: walletEncPubKeyB64,
            );
          } catch (e) {
            lastErr = e;
            continue;
          }
          for (final nonceB64 in nonceCandidates) {
            for (final dataB64 in dataCandidates) {
              try {
                final decryptedB64 = await crypto.phantomDecrypt(
                  sharedSecretB64: shared,
                  nonceB64: nonceB64,
                  ciphertextB64: dataB64,
                );
                final decryptedStr = utf8.decode(_b64Decode(decryptedB64));
                final json = jsonDecode(decryptedStr) as Map<String, dynamic>;

                _walletPublicKey = json['public_key'] as String?;
                _sessionToken = json['session'] as String?;

                if (_walletPublicKey == null || _sessionToken == null) {
                  _lastError = 'Wallet callback decrypted but missing public_key/session';
                  AppLogService().error(_logTag, 'Decrypted payload missing public_key or session');
                  _completeConnect(false);
                  return;
                }

                _dappPublicKeyB64 = pair.key;
                _dappSecretKeyB64 = pair.value;
                _sharedSecretB64 = shared;
                _connectKeyCandidates.clear();
                await _persistSession();
                _lastError = null;
                AppLogService().info(_logTag, 'Connected: wallet=$_walletPublicKey');
                _completeConnect(true);
                return;
              } catch (e) {
                lastErr = e;
                continue;
              }
            }
          }
        }
      }

      _lastError = 'Failed to decrypt wallet callback payload: $lastErr';
      AppLogService().error(_logTag, 'Decrypt failed (all decode paths): $lastErr');
      _connectKeyCandidates.clear();
      _completeConnect(false);
    }();
  }

  void handleSignCallback(Uri uri) {
    AppLogService().info(_logTag, 'Sign callback received');
    final params = _allParams(uri);

    final dataB58 = params['data'];
    final nonceB58 = params['nonce'];
    final signatureB58 = params['signature'];
    final plainTx = params['transaction'];
    final signError = _extractWalletError(params);

    if (plainTx != null && plainTx.isNotEmpty) {
      _lastError = null;
      AppLogService().info(_logTag, 'Sign callback has plaintext transaction');
      _completeSign(plainTx);
      return;
    }

    if (signatureB58 != null && signatureB58.isNotEmpty) {
      _lastError = null;
      AppLogService().info(_logTag, 'Sign callback has signature');
      _completeSign(signatureB58);
      return;
    }

    if (signError != null && (dataB58 == null || nonceB58 == null)) {
      _lastError = 'Wallet sign rejected: $signError';
      AppLogService().error(
        _logTag,
        'Sign error: $signError; uri=$uri; keys=${params.keys.toList()}',
      );
      _completeSign(null);
      return;
    }

    if (dataB58 == null || nonceB58 == null) {
      _lastError = 'Wallet sign callback missing data/nonce';
      AppLogService().error(_logTag, 'Sign callback missing data/nonce');
      _completeSign(null);
      return;
    }

    final dataB64 = _walletValueToB64PreferB58(dataB58);
    final nonceB64 = _walletValueToB64WithExpectedLen(nonceB58, expectedLen: 24);

    () async {
      try {
        final decryptedB64 = await crypto.phantomDecrypt(
          sharedSecretB64: _sharedSecretB64!,
          nonceB64: nonceB64,
          ciphertextB64: dataB64,
        );
        final decryptedStr = utf8.decode(_b64Decode(decryptedB64));
        final json = jsonDecode(decryptedStr) as Map<String, dynamic>;
        final tx = json['transaction'] as String?;
        if (tx != null && tx.isNotEmpty) {
          _lastError = null;
          AppLogService().info(_logTag, 'Sign callback returned signed transaction');
          _completeSign(tx);
          return;
        }
        final sig = json['signature'] as String? ?? signatureB58;
        AppLogService().info(_logTag, 'Transaction signed: $sig');
        _lastError = null;
        _completeSign(sig);
      } catch (e) {
        if (signError != null && signError.isNotEmpty) {
          _lastError = 'Wallet sign rejected: $signError';
          AppLogService().error(_logTag, 'Sign decrypt failed with wallet error: $signError; raw=$e');
          _completeSign(null);
        } else {
          _lastError = 'Wallet sign callback parse/decrypt failed';
          AppLogService().error(_logTag, 'Sign decrypt failed: $e');
          _completeSign(null);
        }
      }
    }();
  }

  void handleSignOnlyCallback(Uri uri) {
    AppLogService().info(_logTag, 'SignOnly callback received');
    final params = _allParams(uri);

    final dataB58 = params['data'];
    final nonceB58 = params['nonce'];
    final plainTx = params['transaction'];
    final signOnlyError = _extractWalletError(params);

    // Some wallets may include both error fields and a valid transaction.
    if (plainTx != null && plainTx.isNotEmpty) {
      _lastError = null;
      AppLogService().info(_logTag, 'SignOnly callback has plaintext transaction');
      _completeSignOnly(plainTx);
      return;
    }

    if (signOnlyError != null && (dataB58 == null || nonceB58 == null)) {
      _lastError = 'Wallet signTransaction rejected: $signOnlyError';
      AppLogService().error(
        _logTag,
        'SignOnly error: $signOnlyError; uri=$uri; keys=${params.keys.toList()}',
      );
      _completeSignOnly(null);
      return;
    }

    if (dataB58 == null || nonceB58 == null) {
      _lastError = 'Wallet signTransaction callback missing data/nonce';
      AppLogService().error(_logTag, 'SignOnly callback missing data/nonce');
      _completeSignOnly(null);
      return;
    }

    final dataB64 = _walletValueToB64PreferB58(dataB58);
    final nonceB64 = _walletValueToB64WithExpectedLen(nonceB58, expectedLen: 24);

    () async {
      try {
        final decryptedB64 = await crypto.phantomDecrypt(
          sharedSecretB64: _sharedSecretB64!,
          nonceB64: nonceB64,
          ciphertextB64: dataB64,
        );
        final decryptedStr = utf8.decode(_b64Decode(decryptedB64));
        final json = jsonDecode(decryptedStr) as Map<String, dynamic>;
        final signedTx = json['transaction'] as String?;
        if (signedTx != null) {
          _lastError = null;
          AppLogService().info(_logTag, 'Transaction signed (signOnly)');
          _completeSignOnly(signedTx);
        } else {
          _lastError = 'Wallet signTransaction response missing signed transaction';
          AppLogService().error(_logTag, 'SignOnly response missing transaction');
          _completeSignOnly(null);
        }
      } catch (e) {
        _lastError = 'Wallet signTransaction callback decrypt failed';
        AppLogService().error(_logTag, 'SignOnly decrypt failed: $e');
        _completeSignOnly(null);
      }
    }();
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
      return launchUrl(url, mode: LaunchMode.externalNonBrowserApplication);
    } catch (e) {
      AppLogService().error(_logTag, 'Launch URL failed: $e');
      return false;
    }
  }

  String _buildRedirectLink(String redirectPath) {
    final normalized = redirectPath
        .replaceFirst(RegExp(r'^/+'), '')
        .replaceAll(RegExp(r'/+$'), '');
    if (normalized.isEmpty) {
      return '$_kRedirectScheme://';
    }
    final parts = normalized.split('/');
    final host = parts.first;
    final rest = parts.length > 1 ? parts.sublist(1) : const <String>[];
    return Uri(scheme: _kRedirectScheme, host: host, pathSegments: rest).toString();
  }

  static String _b64Encode(List<int> bytes) {
    return base64Url.encode(bytes).replaceAll('=', '');
  }

  static Uint8List _b64Decode(String b64) {
    var s = b64;
    while (s.length % 4 != 0) {
      s += '=';
    }
    return base64Url.decode(s);
  }

  static Uint8List _randomBytes(int n) {
    final rng = Random.secure();
    return Uint8List.fromList(List.generate(n, (_) => rng.nextInt(256)));
  }

  static String _b58Encode(List<int> input) {
    return bs58.base58.encode(Uint8List.fromList(input));
  }

  static Uint8List _b58Decode(String input) {
    return Uint8List.fromList(bs58.base58.decode(input));
  }

  static String _b64ToB58(String b64) => _b58Encode(_b64Decode(b64));

  static String _b58ToB64(String b58) => _b64Encode(_b58Decode(b58));

  static String _walletValueToB64PreferB58(String value) {
    try {
      return _b58ToB64(value);
    } catch (_) {
      // Not base58. Some wallets may already return base64/base64url.
      return _normalizeAnyBase64ToB64(value);
    }
  }

  static String _walletValueToB64WithExpectedLen(
    String value, {
    required int expectedLen,
  }) {
    try {
      final b58 = _b58Decode(value);
      if (b58.length == expectedLen) {
        return _b64Encode(b58);
      }
    } catch (_) {}

    final fromB64 = _decodeAnyBase64(value);
    if (fromB64.length == expectedLen) {
      return _b64Encode(fromB64);
    }
    throw FormatException('unexpected decoded length, expected=$expectedLen');
  }

  static List<String> _walletValueToB64CandidatesWithExpectedLen(
    String value, {
    required int expectedLen,
  }) {
    final out = <String>[];
    void addIfValid(Uint8List bytes) {
      if (bytes.length == expectedLen) {
        final b64 = _b64Encode(bytes);
        if (!out.contains(b64)) out.add(b64);
      }
    }

    try {
      addIfValid(_b58Decode(value));
    } catch (_) {}
    try {
      addIfValid(_decodeAnyBase64(value));
    } catch (_) {}
    if (out.isEmpty) {
      throw FormatException('unexpected decoded length, expected=$expectedLen');
    }
    return out;
  }

  static List<String> _ciphertextB64Candidates(String value) {
    final out = <String>[];
    void add(Uint8List bytes) {
      if (bytes.isEmpty) return;
      final direct = _b64Encode(bytes);
      if (!out.contains(direct)) out.add(direct);
      // Some wallets may return ciphertext||tag instead of tag||ciphertext.
      if (bytes.length > 16) {
        final rotated = Uint8List.fromList([
          ...bytes.sublist(bytes.length - 16),
          ...bytes.sublist(0, bytes.length - 16),
        ]);
        final rotatedB64 = _b64Encode(rotated);
        if (!out.contains(rotatedB64)) out.add(rotatedB64);
      }
    }

    try {
      add(_b58Decode(value));
    } catch (_) {}
    try {
      add(_decodeAnyBase64(value));
    } catch (_) {}
    if (out.isEmpty) {
      throw FormatException('cannot decode ciphertext candidate');
    }
    return out;
  }

  static String _normalizeAnyBase64ToB64(String s) {
    return _b64Encode(_decodeAnyBase64(s));
  }

  static Uint8List _decodeAnyBase64(String s) {
    try {
      final normalized = s.replaceAll('-', '+').replaceAll('_', '/');
      return base64.decode(_pad4(normalized));
    } catch (_) {
      return base64Url.decode(_pad4(s));
    }
  }

  static String _pad4(String s) {
    var out = s;
    while (out.length % 4 != 0) {
      out += '=';
    }
    return out;
  }

  static Map<String, String> _allParams(Uri uri) {
    final merged = <String, String>{};
    for (final e in uri.queryParameters.entries) {
      merged[e.key] = _repairPossiblyPollutedValue(e.key, e.value);
    }
    final frag = uri.fragment;
    if (frag.isNotEmpty) {
      final qs = frag.startsWith('?') ? frag.substring(1) : frag;
      final fromFrag = Uri.splitQueryString(qs).map(
        (k, v) => MapEntry(k, _repairPossiblyPollutedValue(k, v)),
      );
      merged.addAll(fromFrag);
    }
    return merged;
  }

  static String _repairPossiblyPollutedValue(String key, String value) {
    const encodedKeys = <String>{
      'data',
      'nonce',
      'payload',
      'dapp_encryption_public_key',
      'phantom_encryption_public_key',
      'solflare_encryption_public_key',
      'wallet_encryption_public_key',
    };
    if (!encodedKeys.contains(key)) {
      return value;
    }
    // Some URI parsers decode '+' as space for query params.
    return value.replaceAll(' ', '+');
  }
}
