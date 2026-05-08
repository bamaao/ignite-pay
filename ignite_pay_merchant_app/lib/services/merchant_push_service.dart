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
import 'package:ignite_pay_merchant/services/app_log_service.dart';
import 'package:ignite_pay_merchant/services/fcm_service.dart';
import 'package:ignite_pay_merchant/services/mediator_api.dart';
import 'package:ignite_pay_merchant/src/rust/api/merchant_didcomm.dart' as rust;
import 'package:ignite_pay_merchant/src/rust/api/merchant.dart' as merchant_rust;
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:web_socket_channel/web_socket_channel.dart';

/// A payment confirmation received via DIDComm push.
class PaymentConfirmation {
  final String orderId;
  final String? channelId;
  final int? leafIndex;
  final BigInt? sequence;
  final BigInt? amount;

  PaymentConfirmation({
    required this.orderId,
    this.channelId,
    this.leafIndex,
    this.sequence,
    this.amount,
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

/// Merchant push notification orchestration service.
/// Manages dual-channel push: WebSocket for Chinese users, FCM for overseas users.
class MerchantPushService extends ChangeNotifier {
  static final MerchantPushService _instance = MerchantPushService._internal();
  factory MerchantPushService() => _instance;
  MerchantPushService._internal();

  // State
  String _storagePath = '';
  String _commDid = '';
  String _mediatorWsUrl = '';
  String _mediatorHttpUrl = '';
  bool _isConnected = false;
  bool _isInitialized = false;
  String? _authToken;
  String? _lastPulledId;
  WebSocketChannel? _wsChannel;
  StreamSubscription? _wsSubscription;
  String _pushChannel = '';

  // Pending pairing state: MCP info received from connection-response,
  // waiting for connection-confirm-response to complete.
  String? _pendingMcpDid;
  String? _pendingMcpDidDocJson;
  String? _pendingMcpMediatorHttpUrl;

  final List<PairedMcp> _pairedMcps = [];

  final MediatorApi _api = MediatorApi();

  // Streams
  final StreamController<PaymentConfirmation> _confirmationController =
      StreamController<PaymentConfirmation>.broadcast();

  // Getters
  String get commDid => _commDid;
  bool get isConnected => _isConnected;
  bool get isInitialized => _isInitialized;
  String get pushChannel => _pushChannel;
  List<PairedMcp> get pairedMcps => List.unmodifiable(_pairedMcps);

  /// Stream of payment confirmations.
  Stream<PaymentConfirmation> get confirmations =>
      _confirmationController.stream;

  /// Whether the current user is detected as a Chinese user based on locale.
  bool get _isChineseUser {
    final locale = SchedulerBinding.instance.platformDispatcher.locale;
    final languageCode = locale.languageCode;
    final countryCode = locale.countryCode;
    if (languageCode == 'zh' && countryCode == 'CN') return true;
    if (locale.scriptCode == 'Hans') return true;
    return false;
  }

  /// Initialize DIDComm identity (generates or loads from storage).
  Future<void> initialize() async {
    if (_isInitialized) return;

    try {
      final dir = await getApplicationSupportDirectory();
      _storagePath = dir.path;

      final info =
          await rust.initializeMerchantComm(storagePath: _storagePath);
      _commDid = info.did;

      _isInitialized = true;
      AppLogService().info('DID', 'Merchant DIDComm initialized: $_commDid');
      _loadPairedMcps();
      notifyListeners();
    } catch (e) {
      AppLogService().error('DID', 'Failed to initialize: $e');
    }
  }

  /// Connect to the DIDComm mediator.
  /// Throws on failure so callers can show error dialogs.
  Future<void> connectToMediator(String wsUrl) async {
    _mediatorWsUrl = wsUrl;
    _mediatorHttpUrl = wsUrl.replaceFirst('ws', 'http').replaceAll(RegExp(r'/ws$'), '');

    _api.setBaseUrl(_mediatorHttpUrl);

    try {
      await rust.connectMediator(
          storagePath: _storagePath, wsUrl: wsUrl);

      _isConnected = true;
      AppLogService().info('Mediator', 'Connected to $wsUrl');
      notifyListeners();

      // Authenticate and pull any pending messages
      await _authenticateAndPull();

      if (_isChineseUser) {
        _pushChannel = 'websocket';
        await _initWebSocketChannel();
      } else {
        _pushChannel = 'fcm';
        await _initFcm();
      }
      notifyListeners();
    } catch (e) {
      AppLogService().error('Mediator', 'Failed to connect: $e');
      _isConnected = false;
      notifyListeners();
      rethrow;
    }
  }

  /// Disconnect from the mediator.
  Future<void> disconnect() async {
    try {
      _wsSubscription?.cancel();
      _wsSubscription = null;
      await _wsChannel?.sink.close();
      _wsChannel = null;
      await rust.disconnectMediator();
    } catch (_) {}

    _isConnected = false;
    _pushChannel = '';
    notifyListeners();
  }

  /// Authenticate with mediator and pull pending messages.
  Future<void> _authenticateAndPull() async {
    if (_mediatorHttpUrl.isEmpty || _commDid.isEmpty) return;

    try {
      _authToken = await rust.authenticateWithMediator(
        mediatorUrl: _mediatorHttpUrl,
        storagePath: _storagePath,
        did: _commDid,
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

  /// Decrypt a JWE envelope and process the message.
  /// Also handles plaintext JSON messages (e.g. connection-response forwarded
  /// by the mediator without encryption — DIDComm messages are authenticated
  /// via Ed25519 signatures, not relying on JWE encryption).
  Future<void> _decryptAndProcess(String rawMessage) async {
    try {
      final decrypted = await rust.decryptMessage(
        storagePath: _storagePath,
        jwe: rawMessage,
      );

      AppLogService().info('DIDComm', 'Decrypted: ${decrypted.msgType} (orderId: ${decrypted.orderId ?? "N/A"})');

      // Check if it's a connection-response (pairing reply from MCP)
      if (decrypted.msgType.contains('connection-response')) {
        await _handleConnectionResponseBody(decrypted.rawBody);
        return;
      }

      // Check if it's a connection-confirm-response (final step of 3-way handshake)
      if (decrypted.msgType.contains('connection-confirm-response')) {
        await _handleConnectionConfirmResponse(decrypted.rawBody);
        return;
      }

      // Handle payment confirmation messages
      if (decrypted.msgType.contains('channel-payment-confirm') ||
          decrypted.msgType.contains('payment-auth-response')) {
        // Confirm local order if we have order_id and channel data
        if (decrypted.orderId != null && decrypted.channelId != null) {
          try {
            await merchant_rust.confirmOrder(
              storagePath: _storagePath,
              orderId: decrypted.orderId!,
              channelId: decrypted.channelId ?? '',
              leafIndex: decrypted.leafIndex ?? 0,
              sequence: decrypted.sequence ?? BigInt.zero,
            );
          } catch (e) {
            AppLogService().error('Order', 'Confirmation failed: $e');
          }
        }

        // Emit confirmation event
        _confirmationController.add(PaymentConfirmation(
          orderId: decrypted.orderId ?? '',
          channelId: decrypted.channelId,
          leafIndex: decrypted.leafIndex,
          sequence: decrypted.sequence,
          amount: decrypted.amount,
        ));
      }

      notifyListeners();
    } catch (e) {
      // JWE decryption failed — try plaintext JSON fallback.
      // DIDComm messages are authenticated via Ed25519 signatures, not JWE.
      AppLogService().info('DIDComm', 'JWE decrypt failed, trying plaintext fallback');
      try {
        final v = jsonDecode(rawMessage) as Map<String, dynamic>;
        final msgType = v['type'] as String? ?? '';

        if (msgType.contains('connection-response')) {
          await _handlePlaintextConnectionResponse(v);
          return;
        }
        if (msgType.contains('connection-confirm-response')) {
          await _handleConnectionConfirmResponse(jsonEncode(v['body']));
          return;
        }

        AppLogService().info('DIDComm', 'Plaintext message type not handled: $msgType');
      } catch (e2) {
        AppLogService().error('DIDComm', 'Decrypt/parse failed: $e');
      }
    }
  }

  /// Handle a plaintext connection-response (not JWE-encrypted).
  Future<void> _handlePlaintextConnectionResponse(Map<String, dynamic> msg) async {
    final body = msg['body'] as Map<String, dynamic>? ?? {};
    await _handleConnectionResponseBody(jsonEncode(body));
  }

  /// Handle a connection-response body (both JWE-decrypted and plaintext paths).
  Future<void> _handleConnectionResponseBody(String rawBody) async {
    final body = jsonDecode(rawBody) as Map<String, dynamic>;
    final accepted = body['accepted'] as bool? ?? false;
    if (!accepted) {
      AppLogService().warn('DIDComm', 'Connection-response rejected by MCP');
      notifyListeners();
      return;
    }

    final mcpDidFromDoc = body['did_document'] != null
        ? (body['did_document'] as Map<String, dynamic>)['id'] as String? ?? ''
        : '';
    final mcpMediatorHttpUrl = body['mediator_http_url'] as String? ?? '';
    final didDocJson = body['did_document'] != null
        ? jsonEncode(body['did_document'])
        : '';

    AppLogService().info('DIDComm', 'Connection-response: accepted=$accepted, mcpDid=$mcpDidFromDoc');

    if (mcpDidFromDoc.isNotEmpty && didDocJson.isNotEmpty) {
      _pendingMcpDid = mcpDidFromDoc;
      _pendingMcpDidDocJson = didDocJson;
      _pendingMcpMediatorHttpUrl = mcpMediatorHttpUrl;

      _sendConnectionConfirm(mcpDidFromDoc, mcpMediatorHttpUrl);
    }
    notifyListeners();
  }

  /// Handle a connection-confirm-response (both JWE-decrypted and plaintext paths).
  Future<void> _handleConnectionConfirmResponse(String rawBody) async {
    final body = jsonDecode(rawBody) as Map<String, dynamic>;
    final accepted = body['accepted'] as bool? ?? false;
    final mcpNonce = body['mcp_nonce'] as String? ?? '';
    final mcpSignature = body['mcp_signature'] as String? ?? '';

    AppLogService().info('DIDComm', 'Connection-confirm-response: accepted=$accepted');

    if (accepted && _pendingMcpDid != null && mcpNonce.isNotEmpty && mcpSignature.isNotEmpty) {
      try {
        final valid = await rust.verifyDidSignature(
          did: _pendingMcpDid!,
          message: mcpNonce,
          signatureB64: mcpSignature,
        );

        if (valid) {
          AppLogService().info('DIDComm', 'MCP signature verified, pairing complete');

          final existing = _pairedMcps.indexWhere((m) => m.did == _pendingMcpDid);
          final paired = PairedMcp(
            did: _pendingMcpDid!,
            didDocJson: _pendingMcpDidDocJson ?? '',
            mediatorHttpUrl: _pendingMcpMediatorHttpUrl ?? '',
            pairedAt: DateTime.now(),
          );
          if (existing >= 0) {
            _pairedMcps[existing] = paired;
          } else {
            _pairedMcps.add(paired);
          }
          _savePairedMcps();
        } else {
          AppLogService().error('DIDComm', 'MCP signature verification FAILED');
        }
      } catch (e) {
        AppLogService().error('DIDComm', 'MCP signature verification error: $e');
      }
    } else if (!accepted) {
      AppLogService().warn('DIDComm', 'Connection-confirm-response: MCP rejected pairing');
    }

    _pendingMcpDid = null;
    _pendingMcpDidDocJson = null;
    _pendingMcpMediatorHttpUrl = null;
    notifyListeners();
  }

  /// Initialize WebSocket channel for Chinese users (direct push, no FCM).
  Future<void> _initWebSocketChannel() async {
    if (_authToken == null || _mediatorWsUrl.isEmpty) return;

    try {
      await _api.registerWebSocketChannel(_authToken!);

      _wsChannel = WebSocketChannel.connect(Uri.parse(_mediatorWsUrl));
      _wsChannel!.sink.add('{"from":"$_commDid","type":"identify"}');

      _wsSubscription = _wsChannel!.stream.listen(
        (data) {
          if (data is String) {
            _onWsMessage(data);
          }
        },
        onError: (error) {
          AppLogService().error('WS', 'Channel error: $error');
          _reconnectWebSocket();
        },
        onDone: () {
          AppLogService().warn('WS', 'Channel closed, reconnecting');
          _reconnectWebSocket();
        },
      );

      AppLogService().info('WS', 'Channel initialized for merchant $_commDid');
    } catch (e) {
      AppLogService().error('WS', 'Failed to initialize: $e');
    }
  }

  /// Handle a message received directly via WebSocket.
  void _onWsMessage(String jweEnvelope) {
    AppLogService().info('WS', 'Message received (${jweEnvelope.length} bytes)');
    _decryptAndProcess(jweEnvelope);
  }

  /// Attempt to reconnect the WebSocket after a delay, pulling missed messages.
  Future<void> _reconnectWebSocket() async {
    _wsSubscription?.cancel();
    _wsSubscription = null;
    _wsChannel = null;

    // Pull any messages that arrived while disconnected
    await _pullAndDecryptMessages();

    // Wait before reconnecting
    await Future.delayed(const Duration(seconds: 3));

    if (_isConnected && _isChineseUser) {
      await _initWebSocketChannel();
    }
  }

  /// Initialize FCM for push notifications.
  Future<void> _initFcm() async {
    try {
      await FcmService().initialize(
        onSignalReceived: _onFcmSignal,
      );

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
        'from': _commDid,
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

  /// Load paired MCPs from SharedPreferences.
  void _loadPairedMcps() {
    SharedPreferences.getInstance().then((prefs) {
      final json = prefs.getString('paired_mcps');
      if (json != null) {
        final list = jsonDecode(json) as List<dynamic>;
        _pairedMcps.clear();
        _pairedMcps.addAll(
          list.map((e) => PairedMcp.fromJson(e as Map<String, dynamic>)),
        );
        notifyListeners();
      }
    });
  }

  /// Save paired MCPs to SharedPreferences.
  Future<void> _savePairedMcps() async {
    final prefs = await SharedPreferences.getInstance();
    final json = jsonEncode(_pairedMcps.map((e) => e.toJson()).toList());
    await prefs.setString('paired_mcps', json);
  }

  /// Parse an OOB invitation URL from a QR code scan and send a connection request.
  /// Returns the MCP DID on success, or throws on error.
  Future<String> parseInvitationAndConnect(String invitationUrl) async {
    AppLogService().info('QR', 'Scanned invitation URL (${invitationUrl.length} chars)');
    try {
      // Parse invitation via Rust (single source of truth)
      final invitation = await rust.parseOobInvitation(invitationUrl: invitationUrl);
      final mcpDid = invitation.mcpDid;
      final mediatorWsUrl = invitation.mediatorWsUrl;

      debugPrint('Parsed OOB invitation: MCP DID=$mcpDid, mediator=$mediatorWsUrl');
      AppLogService().info('QR', 'Parsed OOB: MCP DID=$mcpDid, mediator=$mediatorWsUrl');

      // Determine push channel based on locale
      final pushChannel = _isChineseUser ? 'websocket' : 'fcm';
      String? fcmToken;
      if (!_isChineseUser) {
        fcmToken = FcmService().fcmToken;
      }

      // Connect to mediator if URL is present and not already connected
      if (mediatorWsUrl.isNotEmpty && !_isConnected) {
        await connectToMediator(mediatorWsUrl);
      }

      // Send connection request via Rust
      final mediatorHttpUrl = _mediatorHttpUrl.isNotEmpty
          ? _mediatorHttpUrl
          : _mediatorWsUrl.replaceFirst('ws', 'http').replaceAll(RegExp(r'/ws$'), '');
      AppLogService().info('QR', 'Sending connection-request: mcpDid=$mcpDid, mediatorHttpUrl=$mediatorHttpUrl, pushChannel=$pushChannel');
      await rust.sendConnectionRequest(
        storagePath: _storagePath,
        mcpDid: mcpDid,
        mcpDidDocJson: invitation.didDocJson,
        mediatorWsUrl: mediatorWsUrl.isNotEmpty ? mediatorWsUrl : _mediatorWsUrl,
        pushChannel: pushChannel,
        fcmToken: fcmToken,
        appMediatorWsUrl: mediatorHttpUrl,
      );

      debugPrint('Connection request sent to MCP $mcpDid');
      AppLogService().info('QR', 'Paired with MCP: $mcpDid');
      return mcpDid;
    } catch (e) {
      AppLogService().error('QR', 'Failed to pair: $e');
      rethrow;
    }
  }

  @override
  void dispose() {
    _wsSubscription?.cancel();
    _wsChannel?.sink.close();
    _confirmationController.close();
    super.dispose();
  }
}
