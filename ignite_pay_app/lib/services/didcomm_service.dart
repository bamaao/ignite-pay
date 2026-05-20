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
import 'dart:io';
import 'package:flutter/foundation.dart';
import 'package:flutter/scheduler.dart';
import 'package:ignite_pay_app/services/app_log_service.dart';
import 'package:ignite_pay_app/services/fcm_service.dart';
import 'package:ignite_pay_app/services/mediator_api.dart';
import 'package:ignite_pay_app/services/session_key_service.dart';
import 'package:ignite_pay_app/src/rust/api/simple.dart' as rust;
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// A payment authorization request from the MCP server.
class AuthRequest {
  final String paymentId;
  final String merchantDid;
  final int amount;
  final String? tokenMint;
  final String description;
  // F2: MCP-provided session key info for embedded payment flow
  final String? newSessionKeyPubkey;
  final String? newSessionKeySecretKey;
  final int? newSessionKeySpendingLimit;
  final int? newSessionKeyDurationSecs;
  final List<String>? newSessionKeyScopes;
  final String? newSessionKeyTokenMint;
  final int? newSessionKeySuggestedSolFunding;
  final int? newSessionKeySuggestedTokenFunding;
  final List<String>? availablePaymentMethods;
  final int? suggestedPerTxLimit;
  final int? suggestedDailyTxCountLimit;

  AuthRequest({
    required this.paymentId,
    required this.merchantDid,
    required this.amount,
    this.tokenMint,
    required this.description,
    this.newSessionKeyPubkey,
    this.newSessionKeySecretKey,
    this.newSessionKeySpendingLimit,
    this.newSessionKeyDurationSecs,
    this.newSessionKeyScopes,
    this.newSessionKeyTokenMint,
    this.newSessionKeySuggestedSolFunding,
    this.newSessionKeySuggestedTokenFunding,
    this.availablePaymentMethods,
    this.suggestedPerTxLimit,
    this.suggestedDailyTxCountLimit,
  });
}

/// A payment authorization response to send back.
class AuthResponseData {
  final String paymentId;
  final bool authorized;
  final String listAction; // "none", "add_whitelist", "add_blacklist", "remove_whitelist", "remove_blacklist", etc.
  final String? listLabel; // V1.1: user-assigned label
  final int? listMaxAmount; // V1.1: max amount for whitelist entry

  AuthResponseData({
    required this.paymentId,
    required this.authorized,
    required this.listAction,
    this.listLabel,
    this.listMaxAmount,
  });
}

/// A QR payment response from the MCP server.
class QrPaymentResult {
  final String orderId;
  final bool success;
  final String paymentProof;
  final String paymentMethod;
  final String? error;

  QrPaymentResult({
    required this.orderId,
    required this.success,
    required this.paymentProof,
    required this.paymentMethod,
    this.error,
  });
}

/// An MB deposit response from the MCP server.
class MbDepositResult {
  final bool success;
  final int depositAmount;
  final int? totalDeposited;
  final String? txSignature;
  final String token;
  final String? error;

  MbDepositResult({
    required this.success,
    required this.depositAmount,
    this.totalDeposited,
    this.txSignature,
    required this.token,
    this.error,
  });
}

/// A session fund request from the MCP server (F3/F7).
class SessionFundRequest {
  final String sessionKeyPubkey;
  final int requiredAmount;
  final int currentBalance;
  final int spendingLimitRemaining;
  final String? tokenMint;
  final String? reason;

  SessionFundRequest({
    required this.sessionKeyPubkey,
    required this.requiredAmount,
    required this.currentBalance,
    required this.spendingLimitRemaining,
    this.tokenMint,
    this.reason,
  });
}

/// A balance notification from the MCP server (F13).
class BalanceNotification {
  final String sessionKeyPubkey;
  final int balance;
  final int threshold;
  final int spendingLimitRemaining;

  BalanceNotification({
    required this.sessionKeyPubkey,
    required this.balance,
    required this.threshold,
    required this.spendingLimitRemaining,
  });
}

/// A session renew request from the MCP server (F14).
class SessionRenewRequest {
  final String oldSessionKeyPubkey;
  final int expiresAt;
  final String? newSessionKeyPubkey;
  final String? newSessionKeySecretKey;
  final int? newSessionKeySpendingLimit;
  final int? newSessionKeyDurationSecs;
  final List<String>? newSessionKeyScopes;
  final String? newSessionKeyTokenMint;
  final int? newSessionKeySuggestedSolFunding;
  final int? newSessionKeySuggestedTokenFunding;

  SessionRenewRequest({
    required this.oldSessionKeyPubkey,
    required this.expiresAt,
    this.newSessionKeyPubkey,
    this.newSessionKeySecretKey,
    this.newSessionKeySpendingLimit,
    this.newSessionKeyDurationSecs,
    this.newSessionKeyScopes,
    this.newSessionKeyTokenMint,
    this.newSessionKeySuggestedSolFunding,
    this.newSessionKeySuggestedTokenFunding,
  });
}

/// A paired MCP server with its identity info received from connection-response.
class PairedMcp {
  final String did;
  final String didDocJson;
  final String mediatorHttpUrl;
  final DateTime pairedAt;

  PairedMcp({
    required this.did,
    required this.didDocJson,
    required this.mediatorHttpUrl,
    required this.pairedAt,
  });

  Map<String, dynamic> toJson() => {
    'did': did,
    'didDocJson': didDocJson,
    'mediatorHttpUrl': mediatorHttpUrl,
    'pairedAt': pairedAt.toIso8601String(),
  };

  factory PairedMcp.fromJson(Map<String, dynamic> json) => PairedMcp(
    did: json['did'] as String,
    didDocJson: json['didDocJson'] as String,
    mediatorHttpUrl: json['mediatorHttpUrl'] as String,
    pairedAt: DateTime.parse(json['pairedAt'] as String),
  );
}

/// A decrypted DIDComm message.
class DecryptedMsg {
  final String msgType;
  final String? paymentId;
  final String? merchantDid;
  final int? amount;
  final String? tokenMint;
  final String? description;
  final String rawBody;
  final String? listCid; // V1.1: IPFS CID from list-sync-notification
  final String? listType; // V1.1: "whitelist" or "blacklist"
  final String? label; // V1.1: user-assigned label
  // F2: MCP-provided session key fields
  final String? newSessionKeyPubkey;
  final String? newSessionKeySecretKey;
  final int? newSessionKeySpendingLimit;
  final int? newSessionKeyDurationSecs;
  final List<String>? newSessionKeyScopes;
  final String? newSessionKeyTokenMint;
  final int? newSessionKeySuggestedSolFunding;
  final int? newSessionKeySuggestedTokenFunding;
  final List<String>? availablePaymentMethods;
  final int? suggestedPerTxLimit;
  final int? suggestedDailyTxCountLimit;
  // F3/F7: Session fund request fields
  final int? sessionFundRequiredAmount;
  final int? sessionFundCurrentBalance;
  final int? sessionFundSpendingLimitRemaining;
  final String? sessionFundTokenMint;
  final String? sessionFundReason;
  // F13: Balance notification fields
  final int? balanceNotificationBalance;
  final int? balanceNotificationThreshold;
  final int? balanceNotificationSpendingLimitRemaining;
  // F14: Session renew request fields
  final String? oldSessionKeyPubkey;
  final int? sessionRenewExpiresAt;
  // F16: Relayer payment method fields
  final String? relayerPubkey;
  final String? relayerUrl;

  DecryptedMsg({
    required this.msgType,
    this.paymentId,
    this.merchantDid,
    this.amount,
    this.tokenMint,
    this.description,
    required this.rawBody,
    this.listCid,
    this.listType,
    this.label,
    this.newSessionKeyPubkey,
    this.newSessionKeySecretKey,
    this.newSessionKeySpendingLimit,
    this.newSessionKeyDurationSecs,
    this.newSessionKeyScopes,
    this.newSessionKeyTokenMint,
    this.newSessionKeySuggestedSolFunding,
    this.newSessionKeySuggestedTokenFunding,
    this.availablePaymentMethods,
    this.suggestedPerTxLimit,
    this.suggestedDailyTxCountLimit,
    this.sessionFundRequiredAmount,
    this.sessionFundCurrentBalance,
    this.sessionFundSpendingLimitRemaining,
    this.sessionFundTokenMint,
    this.sessionFundReason,
    this.balanceNotificationBalance,
    this.balanceNotificationThreshold,
    this.balanceNotificationSpendingLimitRemaining,
    this.oldSessionKeyPubkey,
    this.sessionRenewExpiresAt,
    this.relayerPubkey,
    this.relayerUrl,
  });
}

/// Service managing DID identity, DIDComm connections, and message flow.
/// Uses ChangeNotifier for Provider state management.
class DidcommService extends ChangeNotifier {
  static DidcommService _instance = DidcommService._internal();
  factory DidcommService() => _instance;
  DidcommService._internal();

  /// Reset the singleton so tests get a fresh instance.
  @visibleForTesting
  static void resetInstance() {
    _instance = DidcommService._internal();
  }

  // State
  String _storagePath = '';
  String _did = '';
  String _didDocJson = '';
  String _mediatorWsUrl = '';
  String _mediatorHttpUrl = '';
  bool _isConnected = false;
  bool _isInitialized = false;
  String? _authToken;
  String? _lastPulledId;
  Timer? _messagePollTimer;

  final List<DecryptedMsg> _messages = [];
  final List<PairedMcp> _pairedMcps = [];
  AuthRequest? _pendingAuth;

  final MediatorApi _api = MediatorApi();

  // Streams
  final StreamController<AuthRequest> _authRequestController =
      StreamController<AuthRequest>.broadcast();
  final StreamController<QrPaymentResult> _qrPaymentResultController =
      StreamController<QrPaymentResult>.broadcast();
  final StreamController<MbDepositResult> _mbDepositResultController =
      StreamController<MbDepositResult>.broadcast();
  final StreamController<SessionFundRequest> _sessionFundRequestController =
      StreamController<SessionFundRequest>.broadcast();
  final StreamController<BalanceNotification> _balanceNotificationController =
      StreamController<BalanceNotification>.broadcast();
  final StreamController<SessionRenewRequest> _sessionRenewRequestController =
      StreamController<SessionRenewRequest>.broadcast();

  // Getters
  String get did => _did;
  String get didDocJson => _didDocJson;
  String get storagePath => _storagePath;
  String get mediatorWsUrl => _mediatorWsUrl;
  bool get isConnected => _isConnected;
  bool get isInitialized => _isInitialized;
  List<DecryptedMsg> get messages => List.unmodifiable(_messages);
  List<PairedMcp> get pairedMcps => List.unmodifiable(_pairedMcps);
  AuthRequest? get pendingAuth => _pendingAuth;
  int get pendingMessageCount => _messages.length;

  /// Stream of incoming auth requests.
  Stream<AuthRequest> get authRequests => _authRequestController.stream;
  Stream<QrPaymentResult> get qrPaymentResults => _qrPaymentResultController.stream;
  Stream<MbDepositResult> get mbDepositResults => _mbDepositResultController.stream;
  Stream<SessionFundRequest> get sessionFundRequests => _sessionFundRequestController.stream;
  Stream<BalanceNotification> get balanceNotifications => _balanceNotificationController.stream;
  Stream<SessionRenewRequest> get sessionRenewRequests => _sessionRenewRequestController.stream;

  /// Whether the current user is detected as a Chinese user based on locale.
  /// Chinese users use WebSocket direct push instead of FCM.
  bool get _isChineseUser {
    final locale = SchedulerBinding.instance.platformDispatcher.locale;
    final languageCode = locale.languageCode;
    final countryCode = locale.countryCode;

    // Check for zh_CN locale
    if (languageCode == 'zh' && countryCode == 'CN') return true;
    // Check for common Chinese time zones via locale script
    if (locale.scriptCode == 'Hans') return true;
    return false;
  }

  /// Initialize DID identity (generates or loads from storage).
  Future<void> initialize() async {
    if (_isInitialized) return;

    try {
      // Resolve app-internal storage directory
      final dir = await getApplicationSupportDirectory();
      _storagePath = dir.path;

      // Load saved mediator URLs
      final prefs = await SharedPreferences.getInstance();
      _mediatorWsUrl = prefs.getString('mediator_ws_url') ?? '';
      _mediatorHttpUrl = prefs.getString('mediator_http_url') ?? '';

      final info = await rust.initializeIdentity(storagePath: _storagePath);
      _did = info.did;
      _didDocJson = info.didDocJson;

      _isInitialized = true;
      AppLogService().info('DID', 'Initialized: $_did');
      await _loadPairedMcps();
      notifyListeners();
    } catch (e) {
      AppLogService().error('DID', 'Failed to initialize: $e');
    }
  }

  /// Connect to the DIDComm mediator.
  Future<void> connectToMediator(String wsUrl, {String? httpUrl}) async {
    _mediatorWsUrl = wsUrl;
    _mediatorHttpUrl = httpUrl ?? wsUrl.replaceFirst('ws', 'http').replaceAll(RegExp(r'/ws$'), '');

    // Save for later
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('mediator_ws_url', _mediatorWsUrl);
    await prefs.setString('mediator_http_url', _mediatorHttpUrl);

    _api.setBaseUrl(_mediatorHttpUrl);

    try {
      AppLogService().info('Mediator', 'Connecting Rust WsClient to $wsUrl (storagePath=$_storagePath)');
      await rust.connectMediator(storagePath: _storagePath, wsUrl: wsUrl);

      _isConnected = true;
      AppLogService().info('Mediator', 'WsClient connected to $wsUrl');
      notifyListeners();

      // Authenticate and pull any pending messages
      AppLogService().info('Mediator', 'Starting HTTP auth to $_mediatorHttpUrl (did=$_did)');
      await _authenticateAndPull();

      // Start polling messages received by the Rust WS connection
      _startMessagePolling();

      // Overseas users: also use FCM for push notifications
      if (!_isChineseUser) {
        await _initFcm();
      }
    } catch (e) {
      AppLogService().error('Mediator', 'Failed to connect: $e');
      _isConnected = false;
      notifyListeners();
    }
  }

  /// Disconnect from the mediator.
  Future<void> disconnect() async {
    _messagePollTimer?.cancel();
    _messagePollTimer = null;
    try {
      await rust.disconnectMediator();
    } catch (_) {}

    _isConnected = false;
    notifyListeners();
  }

  /// Send an authorization response back to the MCP server.
  Future<void> sendAuthResponse(AuthResponseData response) async {
    try {
      await rust.sendAuthResponse(
        storagePath: _storagePath,
        paymentId: response.paymentId,
        authorized: response.authorized,
        listAction: response.listAction,
        mcpDid: _pairedMcps.isNotEmpty ? _pairedMcps.first.did : (_pendingAuth?.merchantDid ?? ''),
        sessionKeyInfo: null,
        listLabel: response.listLabel,
        listMaxAmount: response.listMaxAmount != null
            ? BigInt.from(response.listMaxAmount!)
            : null,
        tokenMint: null,
        paymentMethod: null,
      );

      debugPrint(
          'Auth response: ${response.paymentId} -> ${response.authorized} (${response.listAction})');
      AppLogService().info('Auth', 'Response: ${response.paymentId} -> ${response.authorized} (${response.listAction})');

      _pendingAuth = null;
      notifyListeners();
    } catch (e) {
      AppLogService().error('Auth', 'Failed to send response: $e');
    }
  }

  /// V2.0: Send an authorization response with a session key.
  /// Reuses existing active session key if available, otherwise creates a new one.
  Future<void> sendAuthResponseWithSessionKey({
    required String paymentId,
    required bool authorized,
    required String listAction,
    required int spendingLimit,
    required int durationSecs,
    String? listLabel,
    int? listMaxAmount,
    int? dailyTxCountLimit,
    int? perTxLimit,
    String? tokenMint,
    String? paymentMethod,
  }) async {
    try {
      // Reuse existing session key if one is already active (avoids double creation)
      final existingKey = SessionKeyService().activeSessionKey;
      final sessionKey = existingKey != null
          ? null
          : await rust.createSessionKeyForPayment(
              storagePath: _storagePath,
              spendingLimit: BigInt.from(spendingLimit),
              durationSecs: durationSecs,
              tokenMint: tokenMint,
            );
      await rust.sendAuthResponse(
        storagePath: _storagePath,
        paymentId: paymentId,
        authorized: authorized,
        listAction: listAction,
        mcpDid: _pairedMcps.isNotEmpty ? _pairedMcps.first.did : (_pendingAuth?.merchantDid ?? ''),
        sessionKeyInfo: sessionKey,
        listLabel: listLabel,
        listMaxAmount: listMaxAmount != null ? BigInt.from(listMaxAmount) : null,
        dailyTxCountLimit: dailyTxCountLimit,
        perTxLimit: perTxLimit != null ? BigInt.from(perTxLimit) : null,
        tokenMint: tokenMint,
        paymentMethod: paymentMethod,
      );

      debugPrint(
          'V2.0 Auth response with session key: $paymentId -> $authorized '
          '(limit: $spendingLimit lamports, duration: ${durationSecs}s, '
          'action: $listAction)');
      AppLogService().info('Auth', 'V2.0 response: $paymentId -> $authorized (limit: $spendingLimit, action: $listAction)');

      _pendingAuth = null;
      notifyListeners();
    } catch (e) {
      AppLogService().error('Auth', 'Failed to send V2.0 response: $e');
      rethrow;
    }
  }

  /// Authenticate with mediator and pull pending messages.
  Future<void> _authenticateAndPull() async {
    if (_mediatorHttpUrl.isEmpty) return;

    try {
      _authToken = await rust.authenticateWithMediator(
        mediatorUrl: _mediatorHttpUrl,
        storagePath: _storagePath,
        did: _did,
      );

      if (_authToken != null) {
        await _pullAndDecryptMessages();
      }
    } catch (e) {
      AppLogService().error('Mediator', 'Auth/pull failed: $e');
    }
  }

  /// Pull messages from mediator and decrypt them.
  Future<void> _pullAndDecryptMessages() async {
    if (_authToken == null) return;

    try {
      final messages = await rust.pullMessages(
        mediatorUrl: _mediatorHttpUrl,
        token: _authToken!,
        afterId: _lastPulledId,
        limit: 50,
      );

      for (final msg in messages) {
        await _decryptAndProcess(msg.jweEnvelope);
        _lastPulledId = msg.msgId;
      }
    } catch (e) {
      AppLogService().error('Mediator', 'Pull/decrypt failed: $e');
    }
  }

  /// Send a connection-confirm message to the MCP as part of the 3-step handshake.
  /// Generates a random nonce, signs it with our Ed25519 key, and sends via HTTP POST.
  Future<void> _sendConnectionConfirm(String mcpDid, String mcpMediatorHttpUrl) async {
    try {
      // Generate random nonce
      final nonce = '${DateTime.now().millisecondsSinceEpoch}-${DateTime.now().microsecond}';
      AppLogService().info('DIDComm', 'Sending connection-confirm: nonce=$nonce, mcpDid=$mcpDid');

      // Sign the nonce with our Ed25519 key
      final signature = await rust.signNonce(
        storagePath: _storagePath,
        nonce: nonce,
      );

      // Build plaintext connection-confirm message
      final innerMsg = jsonEncode({
        'type': 'https://didcomm.org/ignite-pay/1.0/connection-confirm',
        'id': 'conn-confirm-${DateTime.now().millisecondsSinceEpoch}',
        'from': _did,
        'to': [mcpDid],
        'body': {
          'phone_nonce': nonce,
          'phone_signature': signature,
        },
      });

      // Wrap in forward message and send to MCP's mediator via HTTP POST
      final forwardMsg = jsonEncode({
        'type': 'https://didcomm.org/routing/2.0/forward',
        'id': 'fwd-confirm-${DateTime.now().millisecondsSinceEpoch}',
        'body': {'next': mcpDid},
        'attachments': [
          {
            'data': {'json': jsonDecode(innerMsg)},
          },
        ],
      });

      final httpClient = HttpClient();
      final req = await httpClient.postUrl(Uri.parse(mcpMediatorHttpUrl));
      req.headers.set('Content-Type', 'application/json');
      req.write(forwardMsg);
      final resp = await req.close();
      final statusCode = resp.statusCode;
      AppLogService().info('DIDComm', 'Connection-confirm sent, MCP mediator responded: $statusCode');
      if (statusCode != 200 && statusCode != 202) {
        final body = await resp.transform(utf8.decoder).join();
        AppLogService().error('DIDComm', 'MCP mediator rejected connection-confirm: $statusCode $body');
      }
    } catch (e) {
      AppLogService().error('DIDComm', 'Failed to send connection-confirm: $e');
    }
  }

  /// Decrypt a JWE envelope and process the message.
  /// Also handles plaintext JSON messages (e.g. connection-response forwarded
  /// by the mediator without encryption).
  Future<void> _decryptAndProcess(String rawMessage) async {
    // Log raw message details for debugging
    final preview = rawMessage.length > 200 ? '${rawMessage.substring(0, 200)}...' : rawMessage;
    final isJweLike = rawMessage.contains('"ciphertext"') && rawMessage.contains('"recipients"');
    AppLogService().info('DIDComm', 'recv msg: isJWE=$isJweLike, len=${rawMessage.length}, preview=$preview');

    try {
      // Pass the paired MCP's DID doc so the Rust agent can verify authcrypt JWE.
      final pairedMcp = _pairedMcps.isNotEmpty ? _pairedMcps.first : null;
      AppLogService().info('DIDComm', 'decryptMessage: pairedMcps=${_pairedMcps.length}, mcpDid=${pairedMcp?.did ?? "null"}, hasDoc=${pairedMcp?.didDocJson != null}');
      final decrypted = await rust.decryptMessage(
        storagePath: _storagePath,
        jwe: rawMessage,
        mcpDid: pairedMcp?.did,
        mcpDidDocJson: pairedMcp?.didDocJson,
      );

      // Extract fields from rawBody as fallback (Rust bridge may not pass them)
      String? effectiveTokenMint = decrypted.tokenMint;
      String? effectiveSkPubkey = decrypted.newSessionKeyPubkey;
      String? effectiveSkSecretKey = decrypted.newSessionKeySecretKey;
      int? effectiveSkSpendingLimit = decrypted.newSessionKeySpendingLimit?.toInt();
      int? effectiveSkDurationSecs = decrypted.newSessionKeyDurationSecs;
      List<String>? effectiveSkScopes = decrypted.newSessionKeyScopes;
      String? effectiveSkTokenMint = decrypted.newSessionKeyTokenMint;
      int? effectiveSkSolFunding = decrypted.newSessionKeySuggestedSolFunding?.toInt();
      int? effectiveSkTokenFunding = decrypted.newSessionKeySuggestedTokenFunding?.toInt();
      List<String>? effectivePaymentMethods = decrypted.availablePaymentMethods;
      int? effectivePerTxLimit = decrypted.suggestedPerTxLimit?.toInt();
      int? effectiveDailyTxCountLimit = decrypted.suggestedDailyTxCountLimit?.toInt();

      if (decrypted.rawBody.isNotEmpty) {
        try {
          final bodyMap = jsonDecode(decrypted.rawBody) as Map<String, dynamic>;
          // Try top-level flattened fields first (MCP v2 format)
          effectiveTokenMint ??= bodyMap['token_mint'] as String? ?? bodyMap['sk_token_mint'] as String?;
          effectiveSkPubkey ??= bodyMap['session_key_pubkey'] as String?;
          effectiveSkSecretKey ??= bodyMap['ephemeral_secret_key'] as String?;
          effectiveSkSpendingLimit ??= (bodyMap['spending_limit'] as num?)?.toInt();
          effectiveSkDurationSecs ??= bodyMap['duration_secs'] as int?;
          effectiveSkScopes ??= (bodyMap['scopes'] as List<dynamic>?)?.cast<String>();
          effectiveSkTokenMint ??= bodyMap['sk_token_mint'] as String?;
          effectiveSkSolFunding ??= (bodyMap['suggested_sol_funding'] as num?)?.toInt();
          effectiveSkTokenFunding ??= (bodyMap['suggested_token_funding'] as num?)?.toInt();
          effectivePerTxLimit ??= (bodyMap['suggested_per_tx_limit'] as num?)?.toInt();
          effectiveDailyTxCountLimit ??= (bodyMap['suggested_daily_tx_count_limit'] as num?)?.toInt();
          effectivePaymentMethods ??= (bodyMap['available_payment_methods'] as List<dynamic>?)?.cast<String>();
          // Also try nested new_session_key (MCP v1 format)
          final sk = bodyMap['new_session_key'] as Map<String, dynamic>?;
          if (sk != null) {
            effectiveTokenMint ??= sk['token_mint'] as String?;
            effectiveSkPubkey ??= sk['session_key_pubkey'] as String?;
            effectiveSkSecretKey ??= sk['ephemeral_secret_key'] as String?;
            effectiveSkSpendingLimit ??= (sk['spending_limit'] as num?)?.toInt();
            effectiveSkDurationSecs ??= sk['duration_secs'] as int?;
            effectiveSkScopes ??= (sk['scopes'] as List<dynamic>?)?.cast<String>();
            effectiveSkTokenMint ??= sk['token_mint'] as String?;
            effectiveSkSolFunding ??= (sk['suggested_sol_funding'] as num?)?.toInt();
            effectiveSkTokenFunding ??= (sk['suggested_token_funding'] as num?)?.toInt();
            effectivePerTxLimit ??= (sk['suggested_per_tx_limit'] as num?)?.toInt();
            effectiveDailyTxCountLimit ??= (sk['suggested_daily_tx_count_limit'] as num?)?.toInt();
          }
        } catch (_) {}
      }

      AppLogService().info('DIDComm', 'Fallback result: effectiveSkPubkey=$effectiveSkPubkey, effectiveSkSecretKey=${effectiveSkSecretKey != null ? "present" : "null"}, effectiveTokenMint=$effectiveTokenMint');

      final msg = DecryptedMsg(
        msgType: decrypted.msgType,
        paymentId: decrypted.paymentId,
        merchantDid: decrypted.merchantDid,
        amount: decrypted.amount?.toInt(),
        tokenMint: effectiveTokenMint,
        description: decrypted.description,
        rawBody: decrypted.rawBody,
        listCid: decrypted.listCid,
        listType: decrypted.listType,
        label: decrypted.label,
        newSessionKeyPubkey: effectiveSkPubkey,
        newSessionKeySecretKey: effectiveSkSecretKey,
        newSessionKeySpendingLimit: effectiveSkSpendingLimit,
        newSessionKeyDurationSecs: effectiveSkDurationSecs,
        newSessionKeyScopes: effectiveSkScopes,
        newSessionKeyTokenMint: effectiveSkTokenMint,
        newSessionKeySuggestedSolFunding: effectiveSkSolFunding,
        newSessionKeySuggestedTokenFunding: effectiveSkTokenFunding,
        availablePaymentMethods: effectivePaymentMethods,
        suggestedPerTxLimit: effectivePerTxLimit,
        suggestedDailyTxCountLimit: effectiveDailyTxCountLimit,
        sessionFundRequiredAmount: decrypted.sessionFundRequiredAmount?.toInt(),
        sessionFundCurrentBalance: decrypted.sessionFundCurrentBalance?.toInt(),
        sessionFundSpendingLimitRemaining: decrypted.sessionFundSpendingLimitRemaining?.toInt(),
        sessionFundTokenMint: decrypted.sessionFundTokenMint,
        sessionFundReason: decrypted.sessionFundReason,
        balanceNotificationBalance: decrypted.balanceNotificationBalance?.toInt(),
        balanceNotificationThreshold: decrypted.balanceNotificationThreshold?.toInt(),
        balanceNotificationSpendingLimitRemaining: decrypted.balanceNotificationSpendingLimitRemaining?.toInt(),
        oldSessionKeyPubkey: decrypted.oldSessionKeyPubkey,
        sessionRenewExpiresAt: decrypted.sessionRenewExpiresAt,
        relayerPubkey: decrypted.relayerPubkey,
        relayerUrl: decrypted.relayerUrl,
      );

      _messages.add(msg);

      AppLogService().info('DIDComm', 'Decrypted: ${msg.msgType} (paymentId: ${msg.paymentId ?? "N/A"})');
      AppLogService().info('DIDComm', 'DecryptedMsg: skPubkey=${msg.newSessionKeyPubkey}, skSecretKey=${msg.newSessionKeySecretKey != null ? "present" : "null"}, tokenMint=${msg.tokenMint}, rawBodyLen=${msg.rawBody.length}');
      if (msg.msgType.contains('payment-auth-request')) {
        // Log rawBody in chunks to avoid truncation
        final rb = msg.rawBody;
        for (int i = 0; i < rb.length; i += 500) {
          AppLogService().info('DIDComm', 'rawBody[$i]: ${rb.substring(i, i + 500 > rb.length ? rb.length : i + 500)}');
        }
      }

      // Check if it's a connection-response (pairing reply from MCP)
      if (msg.msgType.contains('connection-response')) {
        final body = jsonDecode(msg.rawBody) as Map<String, dynamic>;
        final accepted = body['accepted'] as bool? ?? false;
        if (accepted) {
          final mcpDidFromDoc = body['did_document'] != null
              ? (body['did_document'] as Map<String, dynamic>)['id'] as String? ?? ''
              : '';
          final mcpMediatorHttpUrl = body['mediator_http_url'] as String? ?? '';
          final didDocJson = body['did_document'] != null
              ? jsonEncode(body['did_document'])
              : '';
          final mcpNonce = body['mcp_nonce'] as String? ?? '';
          final mcpSignature = body['mcp_signature'] as String? ?? '';

          AppLogService().info('DIDComm', 'Connection-response: mcpDid=$mcpDidFromDoc, mediator=$mcpMediatorHttpUrl');

          if (mcpDidFromDoc.isNotEmpty && didDocJson.isNotEmpty) {
            // Verify MCP's signature
            if (mcpNonce.isNotEmpty && mcpSignature.isNotEmpty) {
              try {
                final valid = await rust.verifyDidSignature(
                  did: mcpDidFromDoc,
                  message: mcpNonce,
                  signatureB64: mcpSignature,
                );
                if (!valid) {
                  AppLogService().error('DIDComm', 'MCP signature verification FAILED');
                  return;
                }
                AppLogService().info('DIDComm', 'MCP signature verified');
              } catch (e) {
                AppLogService().error('DIDComm', 'MCP signature verification error: $e');
                return;
              }
            }

            // Save MCP info
            final existing = _pairedMcps.indexWhere((m) => m.did == mcpDidFromDoc);
            final paired = PairedMcp(
              did: mcpDidFromDoc,
              didDocJson: didDocJson,
              mediatorHttpUrl: mcpMediatorHttpUrl,
              pairedAt: DateTime.now(),
            );
            if (existing >= 0) {
              _pairedMcps[existing] = paired;
            } else {
              _pairedMcps.add(paired);
            }
            await _savePairedMcps();
            AppLogService().info('DIDComm', 'MCP paired and saved: $mcpDidFromDoc');

            // Register MCP as a peer in the Rust DIDComm agent
            try {
              await rust.registerMcpPeer(
                storagePath: _storagePath,
                mcpDid: mcpDidFromDoc,
                mcpDidDocJson: didDocJson,
              );
              AppLogService().info('DIDComm', 'MCP peer registered in DIDComm agent: $mcpDidFromDoc');
            } catch (e) {
              AppLogService().error('DIDComm', 'Failed to register MCP peer in agent: $e');
            }

            // Send connection-confirm with our signed nonce
            _sendConnectionConfirm(mcpDidFromDoc, mcpMediatorHttpUrl);
          }
        } else {
          AppLogService().warn('DIDComm', 'Connection-response rejected by MCP');
        }
        notifyListeners();
        return;
      }

      // Check if it's a payment-auth-request
      if (msg.msgType.contains('payment-auth-request')) {
        final body = jsonDecode(msg.rawBody) as Map<String, dynamic>;
        final newSk = body['new_session_key'] as Map<String, dynamic>?;
        // Resolve tokenMint from all possible sources
        final resolvedTokenMint = msg.tokenMint
            ?? msg.newSessionKeyTokenMint
            ?? body['token_mint'] as String?
            ?? newSk?['token_mint'] as String?;
        AppLogService().info('DIDComm', 'Auth-request debug: msg.tokenMint=${msg.tokenMint}, msg.newSessionKeyPubkey=${msg.newSessionKeyPubkey}, bodySkPubkey=${newSk?['session_key_pubkey']}, bodyTopLevelSkPubkey=${body['session_key_pubkey']}, resolvedTokenMint=$resolvedTokenMint');
        final resolvedSkPubkey = msg.newSessionKeyPubkey
            ?? body['session_key_pubkey'] as String?
            ?? newSk?['session_key_pubkey'] as String?;
        final resolvedSkSecretKey = msg.newSessionKeySecretKey
            ?? body['ephemeral_secret_key'] as String?;
        final authReq = AuthRequest(
          paymentId: msg.paymentId ?? '',
          merchantDid: msg.merchantDid ?? '',
          amount: msg.amount ?? 0,
          tokenMint: resolvedTokenMint,
          description: msg.description ?? '',
          newSessionKeyPubkey: resolvedSkPubkey,
          newSessionKeySecretKey: resolvedSkSecretKey,
          newSessionKeySpendingLimit: msg.newSessionKeySpendingLimit ?? (body['spending_limit'] as num?)?.toInt(),
          newSessionKeyDurationSecs: msg.newSessionKeyDurationSecs ?? body['duration_secs'] as int?,
          newSessionKeyScopes: msg.newSessionKeyScopes ?? (body['scopes'] as List<dynamic>?)?.cast<String>(),
          newSessionKeyTokenMint: msg.newSessionKeyTokenMint,
          newSessionKeySuggestedSolFunding: msg.newSessionKeySuggestedSolFunding,
          newSessionKeySuggestedTokenFunding: msg.newSessionKeySuggestedTokenFunding,
          availablePaymentMethods: msg.availablePaymentMethods,
          suggestedPerTxLimit: msg.suggestedPerTxLimit ?? newSk?['suggested_per_tx_limit'] as int?,
          suggestedDailyTxCountLimit: msg.suggestedDailyTxCountLimit ?? newSk?['suggested_daily_tx_count_limit'] as int?,
        );
        AppLogService().info('DIDComm', 'AuthRequest created: tokenMint=${authReq.tokenMint}, amount=${authReq.amount}, newSessionKeyPubkey=${authReq.newSessionKeyPubkey}');
        _pendingAuth = authReq;
        _authRequestController.add(authReq);
      }

      // Check if it's a qr-payment-response (MCP processed our QR scan payment)
      if (msg.msgType.contains('qr-payment-response')) {
        final body = jsonDecode(msg.rawBody) as Map<String, dynamic>;
        final qrResult = QrPaymentResult(
          orderId: body['order_id'] as String? ?? '',
          success: body['success'] as bool? ?? false,
          paymentProof: body['payment_proof'] as String? ?? '',
          paymentMethod: body['payment_method'] as String? ?? '',
          error: body['error'] as String?,
        );
        AppLogService().info('DIDComm', 'QR payment response: order=${qrResult.orderId} success=${qrResult.success}');
        _qrPaymentResultController.add(qrResult);
      }

      // Check if it's an mb-deposit-response (MCP processed our vault deposit)
      if (msg.msgType.contains('mb-deposit-response')) {
        final body = jsonDecode(msg.rawBody) as Map<String, dynamic>;
        final mbResult = MbDepositResult(
          success: body['success'] as bool? ?? false,
          depositAmount: (body['deposit_amount'] as int?) ?? 0,
          totalDeposited: body['total_deposited'] as int?,
          txSignature: body['tx_signature'] as String?,
          token: body['token'] as String? ?? 'SOL',
          error: body['error'] as String?,
        );
        AppLogService().info('DIDComm', 'MB deposit response: success=${mbResult.success} amount=${mbResult.depositAmount}');
        _mbDepositResultController.add(mbResult);
      }

      // F3/F7: Check if it's a session-fund-request
      if (msg.msgType.contains('session-fund-request')) {
        final fundReq = SessionFundRequest(
          sessionKeyPubkey: msg.rawBody.contains('session_key_pubkey')
              ? (jsonDecode(msg.rawBody) as Map<String, dynamic>)['session_key_pubkey'] as String? ?? ''
              : '',
          requiredAmount: msg.sessionFundRequiredAmount ?? 0,
          currentBalance: msg.sessionFundCurrentBalance ?? 0,
          spendingLimitRemaining: msg.sessionFundSpendingLimitRemaining ?? 0,
          tokenMint: msg.sessionFundTokenMint,
          reason: msg.sessionFundReason,
        );
        AppLogService().info('DIDComm', 'Session fund request: pubkey=${fundReq.sessionKeyPubkey} required=${fundReq.requiredAmount}');
        _sessionFundRequestController.add(fundReq);
      }

      // F13: Check if it's a balance-notification
      if (msg.msgType.contains('balance-notification')) {
        final notif = BalanceNotification(
          sessionKeyPubkey: msg.rawBody.contains('session_key_pubkey')
              ? (jsonDecode(msg.rawBody) as Map<String, dynamic>)['session_key_pubkey'] as String? ?? ''
              : '',
          balance: msg.balanceNotificationBalance ?? 0,
          threshold: msg.balanceNotificationThreshold ?? 0,
          spendingLimitRemaining: msg.balanceNotificationSpendingLimitRemaining ?? 0,
        );
        AppLogService().info('DIDComm', 'Balance notification: pubkey=${notif.sessionKeyPubkey} balance=${notif.balance}');
        _balanceNotificationController.add(notif);
      }

      // F14: Check if it's a session-renew-request
      if (msg.msgType.contains('session-renew-request')) {
        final body = jsonDecode(msg.rawBody) as Map<String, dynamic>;
        final newSk = body['new_session_key'] as Map<String, dynamic>?;
        final renewReq = SessionRenewRequest(
          oldSessionKeyPubkey: msg.oldSessionKeyPubkey ?? body['old_session_key_pubkey'] as String? ?? '',
          expiresAt: msg.sessionRenewExpiresAt ?? body['expires_at'] as int? ?? 0,
          newSessionKeyPubkey: msg.newSessionKeyPubkey ?? newSk?['session_key_pubkey'] as String?,
          newSessionKeySecretKey: msg.newSessionKeySecretKey ?? newSk?['ephemeral_secret_key'] as String?,
          newSessionKeySpendingLimit: msg.newSessionKeySpendingLimit ?? newSk?['spending_limit'] as int?,
          newSessionKeyDurationSecs: msg.newSessionKeyDurationSecs ?? newSk?['duration_secs'] as int?,
          newSessionKeyScopes: msg.newSessionKeyScopes ?? (newSk?['scopes'] as List<dynamic>?)?.cast<String>(),
          newSessionKeyTokenMint: msg.newSessionKeyTokenMint ?? newSk?['token_mint'] as String?,
          newSessionKeySuggestedSolFunding: msg.newSessionKeySuggestedSolFunding ?? newSk?['suggested_sol_funding'] as int?,
          newSessionKeySuggestedTokenFunding: msg.newSessionKeySuggestedTokenFunding ?? newSk?['suggested_token_funding'] as int?,
        );
        AppLogService().info('DIDComm', 'Session renew request: old=${renewReq.oldSessionKeyPubkey}');
        _sessionRenewRequestController.add(renewReq);
      }

      notifyListeners();
    } catch (e) {
      // JWE decryption failed — try re-registering MCP peer and retry once.
      // This handles the case where the Rust agent was recreated without the peer.
      final pairedMcp2 = _pairedMcps.isNotEmpty ? _pairedMcps.first : null;
      if (pairedMcp2 != null && isJweLike) {
        AppLogService().info('DIDComm', 'Retrying JWE decrypt after re-registering MCP peer');
        try {
          await rust.registerMcpPeer(
            storagePath: _storagePath,
            mcpDid: pairedMcp2.did,
            mcpDidDocJson: pairedMcp2.didDocJson,
          );
          final retry = await rust.decryptMessage(
            storagePath: _storagePath,
            jwe: rawMessage,
            mcpDid: pairedMcp2.did,
            mcpDidDocJson: pairedMcp2.didDocJson,
          );
          // Retry succeeded — process the decrypted message
          // Extract fields from rawBody as fallback
          String? retryTokenMint = retry.tokenMint;
          String? retrySkPubkey = retry.newSessionKeyPubkey;
          String? retrySkSecretKey = retry.newSessionKeySecretKey;
          int? retrySkSpendingLimit = retry.newSessionKeySpendingLimit?.toInt();
          int? retrySkDurationSecs = retry.newSessionKeyDurationSecs;
          List<String>? retrySkScopes = retry.newSessionKeyScopes;
          String? retrySkTokenMint = retry.newSessionKeyTokenMint;
          int? retrySkSolFunding = retry.newSessionKeySuggestedSolFunding?.toInt();
          int? retrySkTokenFunding = retry.newSessionKeySuggestedTokenFunding?.toInt();
          List<String>? retryPaymentMethods = retry.availablePaymentMethods;
          int? retryPerTxLimit = retry.suggestedPerTxLimit?.toInt();
          int? retryDailyTxCountLimit = retry.suggestedDailyTxCountLimit?.toInt();

          if (retry.rawBody.isNotEmpty) {
            try {
              final bodyMap = jsonDecode(retry.rawBody) as Map<String, dynamic>;
              // Try top-level flattened fields first
              retryTokenMint ??= bodyMap['token_mint'] as String? ?? bodyMap['sk_token_mint'] as String?;
              retrySkPubkey ??= bodyMap['session_key_pubkey'] as String?;
              retrySkSecretKey ??= bodyMap['ephemeral_secret_key'] as String?;
              retrySkSpendingLimit ??= (bodyMap['spending_limit'] as num?)?.toInt();
              retrySkDurationSecs ??= bodyMap['duration_secs'] as int?;
              retrySkScopes ??= (bodyMap['scopes'] as List<dynamic>?)?.cast<String>();
              retrySkTokenMint ??= bodyMap['sk_token_mint'] as String?;
              retrySkSolFunding ??= (bodyMap['suggested_sol_funding'] as num?)?.toInt();
              retrySkTokenFunding ??= (bodyMap['suggested_token_funding'] as num?)?.toInt();
              retryPerTxLimit ??= (bodyMap['suggested_per_tx_limit'] as num?)?.toInt();
              retryDailyTxCountLimit ??= (bodyMap['suggested_daily_tx_count_limit'] as num?)?.toInt();
              retryPaymentMethods ??= (bodyMap['available_payment_methods'] as List<dynamic>?)?.cast<String>();
              // Also try nested
              final sk = bodyMap['new_session_key'] as Map<String, dynamic>?;
              if (sk != null) {
                retryTokenMint ??= sk['token_mint'] as String?;
                retrySkPubkey ??= sk['session_key_pubkey'] as String?;
                retrySkSecretKey ??= sk['ephemeral_secret_key'] as String?;
                retrySkSpendingLimit ??= (sk['spending_limit'] as num?)?.toInt();
                retrySkDurationSecs ??= sk['duration_secs'] as int?;
                retrySkScopes ??= (sk['scopes'] as List<dynamic>?)?.cast<String>();
                retrySkTokenMint ??= sk['token_mint'] as String?;
                retrySkSolFunding ??= (sk['suggested_sol_funding'] as num?)?.toInt();
                retrySkTokenFunding ??= (sk['suggested_token_funding'] as num?)?.toInt();
                retryPerTxLimit ??= (sk['suggested_per_tx_limit'] as num?)?.toInt();
                retryDailyTxCountLimit ??= (sk['suggested_daily_tx_count_limit'] as num?)?.toInt();
              }
            } catch (_) {}
          }
          final msg = DecryptedMsg(
            msgType: retry.msgType,
            paymentId: retry.paymentId,
            merchantDid: retry.merchantDid,
            amount: retry.amount?.toInt(),
            tokenMint: retryTokenMint,
            description: retry.description,
            rawBody: retry.rawBody,
            listCid: retry.listCid,
            listType: retry.listType,
            label: retry.label,
            newSessionKeyPubkey: retrySkPubkey,
            newSessionKeySecretKey: retrySkSecretKey,
            newSessionKeySpendingLimit: retrySkSpendingLimit,
            newSessionKeyDurationSecs: retrySkDurationSecs,
            newSessionKeyScopes: retrySkScopes,
            newSessionKeyTokenMint: retrySkTokenMint,
            newSessionKeySuggestedSolFunding: retrySkSolFunding,
            newSessionKeySuggestedTokenFunding: retrySkTokenFunding,
            availablePaymentMethods: retryPaymentMethods,
            suggestedPerTxLimit: retryPerTxLimit,
            suggestedDailyTxCountLimit: retryDailyTxCountLimit,
            sessionFundRequiredAmount: retry.sessionFundRequiredAmount?.toInt(),
            sessionFundCurrentBalance: retry.sessionFundCurrentBalance?.toInt(),
            sessionFundSpendingLimitRemaining: retry.sessionFundSpendingLimitRemaining?.toInt(),
            sessionFundTokenMint: retry.sessionFundTokenMint,
            sessionFundReason: retry.sessionFundReason,
            balanceNotificationBalance: retry.balanceNotificationBalance?.toInt(),
            balanceNotificationThreshold: retry.balanceNotificationThreshold?.toInt(),
            balanceNotificationSpendingLimitRemaining: retry.balanceNotificationSpendingLimitRemaining?.toInt(),
            oldSessionKeyPubkey: retry.oldSessionKeyPubkey,
            sessionRenewExpiresAt: retry.sessionRenewExpiresAt,
            relayerPubkey: retry.relayerPubkey,
            relayerUrl: retry.relayerUrl,
          );
          _messages.add(msg);
          AppLogService().info('DIDComm', 'Retry decrypted: ${msg.msgType} (paymentId: ${msg.paymentId ?? "N/A"})');
          // Fall through to process payment-auth-request below
          if (msg.msgType.contains('payment-auth-request')) {
            final body = jsonDecode(msg.rawBody) as Map<String, dynamic>;
            final newSk = body['new_session_key'] as Map<String, dynamic>?;
            // Resolve tokenMint from all possible sources for retry path
            final retryResolvedTokenMint = msg.tokenMint
                ?? msg.newSessionKeyTokenMint
                ?? body['token_mint'] as String?
                ?? newSk?['token_mint'] as String?;
            AppLogService().info('DIDComm', 'Retry auth-request debug: msg.tokenMint=${msg.tokenMint}, resolved=$retryResolvedTokenMint');
            final authReq = AuthRequest(
              paymentId: msg.paymentId ?? '',
              merchantDid: msg.merchantDid ?? '',
              amount: msg.amount ?? 0,
              tokenMint: retryResolvedTokenMint,
              description: msg.description ?? '',
              newSessionKeyPubkey: msg.newSessionKeyPubkey,
              newSessionKeySecretKey: msg.newSessionKeySecretKey,
              newSessionKeySpendingLimit: msg.newSessionKeySpendingLimit,
              newSessionKeyDurationSecs: msg.newSessionKeyDurationSecs,
              newSessionKeyScopes: msg.newSessionKeyScopes,
              newSessionKeyTokenMint: msg.newSessionKeyTokenMint,
              newSessionKeySuggestedSolFunding: msg.newSessionKeySuggestedSolFunding,
              newSessionKeySuggestedTokenFunding: msg.newSessionKeySuggestedTokenFunding,
              availablePaymentMethods: msg.availablePaymentMethods,
              suggestedPerTxLimit: msg.suggestedPerTxLimit ?? newSk?['suggested_per_tx_limit'] as int?,
              suggestedDailyTxCountLimit: msg.suggestedDailyTxCountLimit ?? newSk?['suggested_daily_tx_count_limit'] as int?,
            );
            _pendingAuth = authReq;
            _authRequestController.add(authReq);
          }
          notifyListeners();
          return;
        } catch (retryErr) {
          AppLogService().error('DIDComm', 'Retry JWE decrypt also failed: $retryErr');
        }
      }

      // JWE decryption failed — try plaintext JSON fallback for messages
      // that the mediator forwards without encryption (e.g. connection-response).
      AppLogService().info('DIDComm', 'JWE decrypt failed: $e, trying plaintext fallback');
      try {
        final v = jsonDecode(rawMessage) as Map<String, dynamic>;
        final msgType = v['type'] as String? ?? '';
        final from = v['from'] as String? ?? '';
        AppLogService().info('DIDComm', 'Plaintext msg: type=$msgType, from=$from');

        if (msgType.contains('connection-response')) {
          await _handlePlaintextConnectionResponse(v);
          return;
        }

        AppLogService().info('DIDComm', 'Plaintext message type not handled: $msgType');
      } catch (e2) {
        AppLogService().error('DIDComm', 'Decrypt/parse failed (not JSON): $e');
      }
    }
  }

  /// Handle a plaintext connection-response (not JWE-encrypted).
  /// Verifies MCP's signature, saves MCP info, then sends connection-confirm.
  Future<void> _handlePlaintextConnectionResponse(Map<String, dynamic> msg) async {
    final body = msg['body'] as Map<String, dynamic>? ?? {};
    final accepted = body['accepted'] as bool? ?? false;
    if (!accepted) {
      AppLogService().warn('DIDComm', 'Plaintext connection-response rejected by MCP');
      notifyListeners();
      return;
    }

    final mcpDid = (msg['from'] as String?) ??
        (body['did_document'] != null
            ? (body['did_document'] as Map<String, dynamic>)['id'] as String? ?? ''
            : '');
    final mcpMediatorHttpUrl = body['mediator_http_url'] as String? ?? '';
    final didDocJson = body['did_document'] != null
        ? jsonEncode(body['did_document'])
        : '';
    final mcpNonce = body['mcp_nonce'] as String? ?? '';
    final mcpSignature = body['mcp_signature'] as String? ?? '';

    AppLogService().info('DIDComm', 'Connection-response: mcpDid=$mcpDid, mediator=$mcpMediatorHttpUrl, hasSig=${mcpSignature.isNotEmpty}');

    if (mcpDid.isEmpty || didDocJson.isEmpty) {
      AppLogService().error('DIDComm', 'Connection-response missing MCP DID or DID doc');
      return;
    }

    // Verify MCP's signature over their nonce
    if (mcpNonce.isNotEmpty && mcpSignature.isNotEmpty) {
      try {
        final valid = await rust.verifyDidSignature(
          did: mcpDid,
          message: mcpNonce,
          signatureB64: mcpSignature,
        );
        if (!valid) {
          AppLogService().error('DIDComm', 'MCP signature verification FAILED');
          return;
        }
        AppLogService().info('DIDComm', 'MCP signature verified');
      } catch (e) {
        AppLogService().error('DIDComm', 'MCP signature verification error: $e');
        return;
      }
    }

    // Signature verified — save MCP info
    final existing = _pairedMcps.indexWhere((m) => m.did == mcpDid);
    final paired = PairedMcp(
      did: mcpDid,
      didDocJson: didDocJson,
      mediatorHttpUrl: mcpMediatorHttpUrl,
      pairedAt: DateTime.now(),
    );
    if (existing >= 0) {
      _pairedMcps[existing] = paired;
    } else {
      _pairedMcps.add(paired);
    }

    // Register MCP as a peer in the Rust DIDComm agent so we can decrypt
    // authcrypt JWE messages from MCP.
    try {
      await rust.registerMcpPeer(
        storagePath: _storagePath,
        mcpDid: mcpDid,
        mcpDidDocJson: didDocJson,
      );
      AppLogService().info('DIDComm', 'MCP peer registered in DIDComm agent: $mcpDid');
    } catch (e) {
      AppLogService().error('DIDComm', 'Failed to register MCP peer in agent: $e');
    }
    await _savePairedMcps();
    AppLogService().info('DIDComm', 'MCP paired and saved: $mcpDid');

    // Send connection-confirm with our signed nonce
    _sendConnectionConfirm(mcpDid, mcpMediatorHttpUrl);
    notifyListeners();
  }

  /// Start periodic polling of messages received by the Rust WS connection.
  void _startMessagePolling() {
    _messagePollTimer?.cancel();
    _messagePollTimer = Timer.periodic(const Duration(milliseconds: 300), (_) {
      _pollRustMessages();
    });
  }

  /// Poll messages queued by the Rust WsClient and process them.
  Future<void> _pollRustMessages() async {
    try {
      final messages = await rust.drainMediatorMessages();
      if (messages.isNotEmpty) {
        AppLogService().info('Mediator', 'Polled ${messages.length} message(s) from Rust WS queue');
      }
      for (final msg in messages) {
        await _decryptAndProcess(msg);
      }
    } catch (e) {
      AppLogService().error('Mediator', 'Message poll failed: $e');
    }
  }

  /// Initialize FCM for push notifications (overseas users only).
  Future<void> _initFcm() async {
    try {
      await FcmService().initialize(
        onSignalReceived: _onFcmSignal,
      );

      // Register FCM token with mediator
      final fcmToken = FcmService().fcmToken;
      if (fcmToken != null && _authToken != null) {
        await rust.registerDeviceToken(
          mediatorUrl: _mediatorHttpUrl,
          authToken: _authToken!,
          fcmToken: fcmToken,
        );
      }
    } catch (e) {
      AppLogService().warn('FCM', 'Init failed (non-fatal): $e');
    }
  }

  /// Called when an FCM signal is received.
  void _onFcmSignal(String msgId) {
    AppLogService().info('FCM', 'Signal received for msg: $msgId');
    _pullAndDecryptMessages();
  }

  /// Handle an incoming auth request (called from Rust callback or pull).
  void handleAuthRequest(AuthRequest request) {
    _pendingAuth = request;
    _authRequestController.add(request);
    notifyListeners();
  }

  /// Simulate an incoming auth request (for testing).
  void simulateAuthRequest(AuthRequest request) {
    _pendingAuth = request;
    _authRequestController.add(request);
    notifyListeners();
  }

  /// Bind an MCP/Skill agent DID to this user for message routing.
  Future<void> bindAgent(String agentDid) async {
    if (_authToken == null) return;
    try {
      await _api.bindAgent(_authToken!, agentDid);
      AppLogService().info('Agent', 'Bound $agentDid to user $_did');
    } catch (e) {
      AppLogService().error('Agent', 'Failed to bind: $e');
    }
  }

  /// Parse an OOB invitation URL from a QR code scan and send a connection request.
  /// Returns the MCP DID on success, or throws on error.
  Future<String> parseInvitationAndConnect(String invitationUrl) async {
    AppLogService().info('QR', 'Scanned invitation URL (${invitationUrl.length} chars)');
    try {
      // Parse the invitation URL (didcomm://?_oob=<base64url-json>)
      final uri = Uri.parse(invitationUrl);
      final oobB64 = uri.queryParameters['_oob'];
      if (oobB64 == null || oobB64.isEmpty) {
        throw Exception('Missing _oob parameter in invitation URL');
      }

      // Decode base64url (add padding if needed)
      String padded = oobB64;
      while (padded.length % 4 != 0) {
        padded += '=';
      }
      final jsonBytes = base64Url.decode(padded);
      final invitation = jsonDecode(utf8.decode(jsonBytes)) as Map<String, dynamic>;

      // Extract MCP DID (from "from" field)
      final mcpDid = invitation['from'] as String? ?? '';
      if (mcpDid.isEmpty) throw Exception('Missing from in invitation');

      // Extract body
      final body = invitation['body'] as Map<String, dynamic>? ?? {};

      // Extract mediator endpoint URL from services
      // The invitation now contains the HTTP URL directly (not WSS).
      // services can contain Map objects or plain URL strings (DIDComm OOB spec)
      final services = body['services'] as List<dynamic>? ?? [];
      String mcpMediatorUrl = '';
      if (services.isNotEmpty) {
        final svc = services.first;
        if (svc is Map<String, dynamic>) {
          mcpMediatorUrl = svc['service_endpoint'] as String? ?? '';
        } else if (svc is String) {
          mcpMediatorUrl = svc;
        }
      }

      // Ensure URL ends with / for the POST endpoint
      if (mcpMediatorUrl.isNotEmpty && !mcpMediatorUrl.endsWith('/')) {
        mcpMediatorUrl = '$mcpMediatorUrl/';
      }

      debugPrint('Parsed OOB invitation: MCP DID=$mcpDid, mediator=$mcpMediatorUrl');
      AppLogService().info('QR', 'Parsed OOB: MCP DID=$mcpDid, mediator=$mcpMediatorUrl');

      // Determine push channel based on locale
      final pushChannel = _isChineseUser ? 'websocket' : 'fcm';
      String? fcmToken;
      if (!_isChineseUser) {
        fcmToken = FcmService().fcmToken;
      }

      // Ensure we are connected to our own mediator before sending.
      // We do NOT switch to the MCP's mediator — both parties may use
      // different mediators, so we send through our own connection.
      if (!_isConnected && _mediatorWsUrl.isNotEmpty) {
        debugPrint('Reconnecting to our mediator before sending connection-request...');
        AppLogService().info('QR', 'Reconnecting to own mediator before sending...');
        await connectToMediator(_mediatorWsUrl);
      }

      // Build the connection-request inner message
      final connectionBody = <String, dynamic>{
        'push_channel': pushChannel,
        'mediator_http_url': _mediatorHttpUrl.isNotEmpty
            ? _mediatorHttpUrl
            : _mediatorWsUrl.replaceFirst('ws', 'http').replaceAll(RegExp(r'/ws$'), ''),
        'did_document': jsonDecode(_didDocJson), // phone's DID doc so MCP can encrypt to us
      };
      if (fcmToken != null) {
        connectionBody['fcm_token'] = fcmToken;
      }

      final innerMsg = {
        'type': 'https://didcomm.org/ignite-pay/1.0/connection-request',
        'id': 'conn-req-${DateTime.now().millisecondsSinceEpoch}',
        'from': _did,
        'to': [mcpDid],
        'body': connectionBody,
      };

      // Wrap in a forward message so the mediator can route to the MCP's DID.
      // The mediator uses the `forward` protocol: body.next = target DID,
      // attachments[0].data.json = the actual message payload.
      final forwardMsg = jsonEncode({
        'type': 'https://didcomm.org/routing/2.0/forward',
        'id': 'fwd-${DateTime.now().millisecondsSinceEpoch}',
        'body': {'next': mcpDid},
        'attachments': [
          {
            'data': {'json': innerMsg},
          },
        ],
      });

      AppLogService().info('QR', 'Sending forward msg to MCP mediator: body=${jsonEncode(connectionBody)}');
      AppLogService().info('QR', 'Forward msg: next=$mcpDid, innerMsg=${jsonEncode(innerMsg)}');

      // Send the forward-wrapped connection-request to MCP's mediator via HTTP POST.
      // The mediator's POST / endpoint is unauthenticated and will queue the message
      // until it can be delivered to the MCP (which has a persistent WS connection).
      debugPrint('Sending forward-wrapped connection request to MCP mediator: $mcpMediatorUrl');
      AppLogService().info('QR', 'Sending connection-request to MCP mediator: $mcpMediatorUrl');
      final httpClient = HttpClient();
      final req = await httpClient.postUrl(Uri.parse(mcpMediatorUrl));
      req.headers.set('Content-Type', 'application/json');
      req.write(forwardMsg);
      final resp = await req.close();
      final statusCode = resp.statusCode;
      debugPrint('MCP mediator responded: $statusCode');
      AppLogService().info('QR', 'MCP mediator responded: $statusCode');
      if (statusCode != 200 && statusCode != 202) {
        final body = await resp.transform(utf8.decoder).join();
        AppLogService().error('QR', 'MCP mediator rejected: $statusCode $body');
        throw Exception('MCP mediator rejected message: $statusCode $body');
      }

      AppLogService().info('QR', 'Connection request sent to MCP: $mcpDid, waiting for handshake...');
      return mcpDid;
    } catch (e) {
      AppLogService().error('QR', 'Failed to pair: $e');
      rethrow;
    }
  }

  /// F3/F7: Respond to a session fund request.
  Future<void> respondToFundRequest({
    required String mcpDid,
    required String sessionKeyPubkey,
    required bool funded,
    required int newBalance,
    String? txSignature,
  }) async {
    try {
      await rust.sendSessionFundResponse(
        storagePath: _storagePath,
        mcpDid: mcpDid,
        sessionKeyPubkey: sessionKeyPubkey,
        funded: funded,
        newBalance: BigInt.from(newBalance),
        txSignature: txSignature,
      );
      AppLogService().info('DIDComm', 'Fund response sent: $sessionKeyPubkey -> funded=$funded');
    } catch (e) {
      AppLogService().error('DIDComm', 'Failed to send fund response: $e');
    }
  }

  /// F14: Respond to a session renew request.
  Future<void> respondToRenewRequest({
    required String mcpDid,
    required String oldSessionKeyPubkey,
    required String newSessionKeyPubkey,
    required bool renewed,
    String? txSignature,
  }) async {
    try {
      await rust.sendSessionRenewResponse(
        storagePath: _storagePath,
        mcpDid: mcpDid,
        oldSessionKeyPubkey: oldSessionKeyPubkey,
        newSessionKeyPubkey: newSessionKeyPubkey,
        renewed: renewed,
        txSignature: txSignature,
      );
      AppLogService().info('DIDComm', 'Renew response sent: $oldSessionKeyPubkey -> renewed=$renewed');
    } catch (e) {
      AppLogService().error('DIDComm', 'Failed to send renew response: $e');
    }
  }

  /// Send a session key rotate trigger to MCP.
  /// Called when the user taps "Renew Key" on the phone.
  Future<void> sendSessionKeyRotateTrigger({
    required String mcpDid,
    required String oldSessionKeyPubkey,
  }) async {
    try {
      await rust.sendSessionKeyRotateTrigger(
        storagePath: _storagePath,
        mcpDid: mcpDid,
        oldSessionKeyPubkey: oldSessionKeyPubkey,
      );
      AppLogService().info('DIDComm', 'Session key rotate trigger sent: $oldSessionKeyPubkey');
    } catch (e) {
      AppLogService().error('DIDComm', 'Failed to send rotate trigger: $e');
      rethrow;
    }
  }

  /// Clear the pending auth request.
  void clearPendingAuth() {
    _pendingAuth = null;
    notifyListeners();
  }

  /// Clear all cached messages and reset pull cursor.
  void clearMessages() {
    _messages.clear();
    _lastPulledId = null;
    notifyListeners();
  }

  /// Load paired MCPs from SharedPreferences.
  Future<void> _loadPairedMcps() async {
    final prefs = await SharedPreferences.getInstance();
    final json = prefs.getString('paired_mcps');
    if (json != null) {
      final list = jsonDecode(json) as List<dynamic>;
      _pairedMcps.clear();
      _pairedMcps.addAll(
        list.map((e) => PairedMcp.fromJson(e as Map<String, dynamic>)),
      );
      notifyListeners();
    }
  }

  /// Save paired MCPs to SharedPreferences.
  Future<void> _savePairedMcps() async {
    final prefs = await SharedPreferences.getInstance();
    final json = jsonEncode(_pairedMcps.map((e) => e.toJson()).toList());
    await prefs.setString('paired_mcps', json);
  }

  /// Remove a paired MCP by DID.
  Future<void> removePairedMcp(String did) async {
    _pairedMcps.removeWhere((m) => m.did == did);
    await _savePairedMcps();
    notifyListeners();
  }

  @override
  void dispose() {
    _messagePollTimer?.cancel();
    _authRequestController.close();
    _qrPaymentResultController.close();
    _mbDepositResultController.close();
    _sessionFundRequestController.close();
    _balanceNotificationController.close();
    _sessionRenewRequestController.close();
    super.dispose();
  }
}
