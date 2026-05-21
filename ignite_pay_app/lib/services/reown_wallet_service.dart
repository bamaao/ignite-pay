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
import 'dart:typed_data';

import 'package:flutter/widgets.dart';
import 'package:reown_appkit/reown_appkit.dart';

import 'package:ignite_pay_app/services/app_log_service.dart';
import 'package:ignite_pay_app/services/wallet_service.dart';

// ---------------------------------------------------------------------------
// Solana chain IDs (CAIP-2)
// ---------------------------------------------------------------------------
const _kSolanaDevnet = 'solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1';
const _kSolanaMainnet = 'solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp';

// WalletConnect project ID (from https://cloud.walletconnect.com)
const _kProjectId = '44c60e7b5a1b074f9f1b8e31f9f1b471';

// ---------------------------------------------------------------------------
// ReownWalletService -- WalletConnect v2 via reown_appkit
// ---------------------------------------------------------------------------

/// WalletService implementation using reown_appkit (WalletConnect v2).
///
/// Supports Phantom, Solflare (via deep links handled internally by reown),
/// Backpack, Trust Wallet, Ledger, and any WC2-compatible wallet.
class ReownWalletService extends WalletService {
  static final ReownWalletService _instance =
      ReownWalletService._internal();
  factory ReownWalletService() => _instance;
  ReownWalletService._internal();

  ReownAppKitModal? _appKit;
  bool _initialized = false;
  String _chainId = _kSolanaDevnet;

  // Completer for connect flow
  Completer<bool>? _connectCompleter;

  // Whether to use mainnet
  void setMainnet(bool mainnet) {
    _chainId = mainnet ? _kSolanaMainnet : _kSolanaDevnet;
  }

  /// Initialize the ReownAppKitModal. Must be called with a valid [context]
  /// before any other operations. Called once; subsequent calls are no-ops.
  Future<void> init(BuildContext context) async {
    if (_initialized) return;

    try {
      _appKit = ReownAppKitModal(
        context: context,
        projectId: _kProjectId,
        metadata: const PairingMetadata(
          name: 'Ignite Pay',
          description: 'Session key payment authorization',
          url: 'https://ignitepay.app',
          icons: ['https://ignitepay.app/icon.png'],
          redirect: Redirect(
            native: 'ignitepay://',
            universal: 'https://ignitepay.app',
          ),
        ),
      );

      await _appKit!.init();

      // Remove non-Solana networks to simplify wallet selection
      ReownAppKitModalNetworks.removeSupportedNetworks('eip155');
      ReownAppKitModalNetworks.removeSupportedNetworks('bip122');
      ReownAppKitModalNetworks.removeSupportedNetworks('polkadot');
      ReownAppKitModalNetworks.removeSupportedNetworks('tron');
      ReownAppKitModalNetworks.removeSupportedNetworks('mvx');
      ReownAppKitModalNetworks.removeSupportedNetworks('near');
      ReownAppKitModalNetworks.removeSupportedNetworks('cosmos');
      ReownAppKitModalNetworks.removeSupportedNetworks('ton');
      ReownAppKitModalNetworks.removeSupportedNetworks('sui');
      ReownAppKitModalNetworks.removeSupportedNetworks('stacks');

      // Select devnet by default
      await _selectChain(_chainId);

      // Subscribe to connection events
      _appKit!.onModalConnect.subscribe(_onConnect);
      _appKit!.onModalDisconnect.subscribe(_onDisconnect);

      _initialized = true;
      AppLogService().info('Reown', 'Initialized with chain=$_chainId');
    } catch (e) {
      AppLogService().error('Reown', 'Init failed: $e');
    }
  }

  Future<void> _selectChain(String chainId) async {
    final namespace = NamespaceUtils.getNamespaceFromChain(chainId);
    final id = ReownAppKitModalNetworks.getIdFromChain(chainId);
    final info = ReownAppKitModalNetworks.getNetworkInfo(namespace, id);
    if (info != null) {
      await _appKit?.selectChain(info);
    }
  }

  void _onConnect(ModalConnect? args) {
    AppLogService().info('Reown', 'Connected');
    if (_connectCompleter != null && !_connectCompleter!.isCompleted) {
      _connectCompleter!.complete(true);
    }
    notifyListeners();
  }

  void _onDisconnect(ModalDisconnect? args) {
    AppLogService().info('Reown', 'Disconnected');
    notifyListeners();
  }

  @override
  String? get walletPublicKey {
    if (_appKit?.session == null) return null;
    return _appKit!.session!.getAddress('solana');
  }

  @override
  bool get isConnected => _appKit?.isConnected ?? false;

  @override
  Future<void> loadSession() async {
    // reown_appkit restores sessions automatically in init()
    if (!_initialized) return;
  }

  @override
  Future<bool> connect() async {
    if (_appKit == null) {
      AppLogService().error('Reown', 'Not initialized — call init() first');
      return false;
    }

    if (isConnected) return true;

    _connectCompleter = Completer<bool>();

    // Open the wallet selection modal
    _appKit!.openModalView();

    try {
      final result = await _connectCompleter!.future
          .timeout(const Duration(minutes: 3));
      return result;
    } on TimeoutException {
      AppLogService().error('Reown', 'Connect timed out');
      return false;
    } catch (e) {
      AppLogService().error('Reown', 'Connect failed: $e');
      return false;
    }
  }

  @override
  Future<void> disconnect() async {
    if (_appKit == null) return;
    try {
      await _appKit!.disconnect();
    } catch (e) {
      AppLogService().error('Reown', 'Disconnect failed: $e');
    }
    notifyListeners();
  }

  @override
  Future<String?> signTransaction(String transactionB58) async {
    if (!isConnected || _appKit == null) {
      AppLogService().error('Reown', 'Not connected');
      return null;
    }

    try {
      // Convert base58 → bytes → base64 for WC2
      final txBytes = _b58Decode(transactionB58);
      final txBase64 = base64Encode(txBytes);

      final result = await _appKit!.request(
        topic: _appKit!.session?.topic,
        chainId: _chainId,
        request: SessionRequestParams(
          method: 'solana_signTransaction',
          params: {
            'transaction': txBase64,
          },
        ),
      );

      // Parse response — solana_signTransaction returns { "transaction": "<signedTxBase64>" }
      // For WC2 wallets it may also include { "signature": "<base58>" }
      if (result is Map) {
        if (result.containsKey('errorCode')) {
          AppLogService().error(
              'Reown', 'signTransaction error: ${result['errorMessage']}');
          return null;
        }
        // Some wallets return signed tx in base64 or base58
        final signedTx = result['transaction'];
        if (signedTx is String) {
          // Try to decode: if it's base64, convert to base58
          try {
            final bytes = base64Decode(signedTx);
            return _b58Encode(bytes);
          } catch (_) {
            // Already base58
            return signedTx;
          }
        }
      }

      AppLogService().error('Reown', 'Unexpected signTransaction response: $result');
      return null;
    } catch (e) {
      AppLogService().error('Reown', 'signTransaction failed: $e');
      return null;
    }
  }

  @override
  Future<String?> signAndSendTransaction(String transactionB58) async {
    if (!isConnected || _appKit == null) {
      AppLogService().error('Reown', 'Not connected');
      return null;
    }

    try {
      // Convert base58 → bytes → base64 for WC2
      final txBytes = _b58Decode(transactionB58);
      final txBase64 = base64Encode(txBytes);

      final result = await _appKit!.request(
        topic: _appKit!.session?.topic,
        chainId: _chainId,
        request: SessionRequestParams(
          method: 'solana_signAndSendTransaction',
          params: {
            'transaction': txBase64,
          },
        ),
      );

      // Parse response — solana_signAndSendTransaction returns { "signature": "<base58>" }
      if (result is Map) {
        if (result.containsKey('errorCode')) {
          AppLogService().error(
              'Reown', 'signAndSendTransaction error: ${result['errorMessage']}');
          return null;
        }
        final sig = result['signature'];
        if (sig is String) {
          AppLogService().info('Reown', 'Transaction sent: $sig');
          return sig;
        }
      }

      // Some wallets may return just the signature string
      if (result is String) {
        return result;
      }

      AppLogService().error('Reown', 'Unexpected signAndSendTransaction response: $result');
      return null;
    } catch (e) {
      AppLogService().error('Reown', 'signAndSendTransaction failed: $e');
      return null;
    }
  }

  /// Dispatch a deep link URL to reown_appkit for handling Phantom/Solflare/WC
  /// redirect callbacks. Returns true if the URL was handled.
  Future<bool> dispatchDeepLink(String url) async {
    if (_appKit == null) return false;
    try {
      return await _appKit!.dispatchEnvelope(url);
    } catch (e) {
      AppLogService().error('Reown', 'dispatchEnvelope failed: $e');
      return false;
    }
  }

  @override
  void dispose() {
    _appKit?.onModalConnect.unsubscribe(_onConnect);
    _appKit?.onModalDisconnect.unsubscribe(_onDisconnect);
    super.dispose();
  }

  // ── Base58 helpers (Bitcoin alphabet, used by Solana) ─────────────────

  static const _b58Alphabet =
      '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

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
}
