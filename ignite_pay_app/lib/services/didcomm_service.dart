import 'dart:async';
import 'package:flutter/foundation.dart';
import 'package:ignite_pay_app/services/fcm_service.dart';
import 'package:ignite_pay_app/services/mediator_api.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// A payment authorization request from the MCP server.
class AuthRequest {
  final String paymentId;
  final String merchantDid;
  final int amount;
  final String description;

  AuthRequest({
    required this.paymentId,
    required this.merchantDid,
    required this.amount,
    required this.description,
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

/// A decrypted DIDComm message.
class DecryptedMsg {
  final String msgType;
  final String? paymentId;
  final String? merchantDid;
  final int? amount;
  final String? description;
  final String rawBody;
  final String? listCid; // V1.1: IPFS CID from list-sync-notification
  final String? listType; // V1.1: "whitelist" or "blacklist"
  final String? label; // V1.1: user-assigned label

  DecryptedMsg({
    required this.msgType,
    this.paymentId,
    this.merchantDid,
    this.amount,
    this.description,
    required this.rawBody,
    this.listCid,
    this.listType,
    this.label,
  });
}

/// Service managing DID identity, DIDComm connections, and message flow.
/// Uses ChangeNotifier for Provider state management.
class DidcommService extends ChangeNotifier {
  static final DidcommService _instance = DidcommService._internal();
  factory DidcommService() => _instance;
  DidcommService._internal();

  // State
  String _did = '';
  String _didDocJson = '';
  String _mediatorWsUrl = '';
  String _mediatorHttpUrl = '';
  bool _isConnected = false;
  bool _isInitialized = false;
  String? _authToken;
  String? _lastPulledId;

  final List<DecryptedMsg> _messages = [];
  AuthRequest? _pendingAuth;

  final MediatorApi _api = MediatorApi();

  // Streams
  final StreamController<AuthRequest> _authRequestController =
      StreamController<AuthRequest>.broadcast();

  // Getters
  String get did => _did;
  String get didDocJson => _didDocJson;
  bool get isConnected => _isConnected;
  bool get isInitialized => _isInitialized;
  List<DecryptedMsg> get messages => List.unmodifiable(_messages);
  AuthRequest? get pendingAuth => _pendingAuth;
  int get pendingMessageCount => _messages.length;

  /// Stream of incoming auth requests.
  Stream<AuthRequest> get authRequests => _authRequestController.stream;

  /// Initialize DID identity (generates or loads from storage).
  Future<void> initialize({String storagePath = './phone_data'}) async {
    if (_isInitialized) return;

    try {
      // Load saved mediator URLs
      final prefs = await SharedPreferences.getInstance();
      _mediatorWsUrl = prefs.getString('mediator_ws_url') ?? '';
      _mediatorHttpUrl = prefs.getString('mediator_http_url') ?? '';

      // In production, this calls the Rust bridge:
      // final info = await initializeIdentity(storagePath: storagePath);
      // _did = info.did;
      // _didDocJson = info.didDocJson;

      // For now, use a placeholder until the bridge is regenerated
      _did = 'did:ignite:zPhone${DateTime.now().millisecondsSinceEpoch % 10000}';
      _didDocJson = '{}';

      _isInitialized = true;
      debugPrint('DID initialized: $_did');
      notifyListeners();
    } catch (e) {
      debugPrint('Failed to initialize DID: $e');
    }
  }

  /// Connect to the DIDComm mediator.
  Future<void> connectToMediator(String wsUrl, {String? httpUrl}) async {
    _mediatorWsUrl = wsUrl;
    _mediatorHttpUrl = httpUrl ?? wsUrl.replaceFirst('ws', 'http');

    // Save for later
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('mediator_ws_url', _mediatorWsUrl);
    await prefs.setString('mediator_http_url', _mediatorHttpUrl);

    _api.setBaseUrl(_mediatorHttpUrl);

    try {
      // In production, call Rust bridge:
      // await connectMediator(storagePath: './phone_data', wsUrl: wsUrl);

      _isConnected = true;
      debugPrint('Connected to mediator: $wsUrl');
      notifyListeners();

      // Authenticate and pull any pending messages
      await _authenticateAndPull();

      // Initialize FCM
      await _initFcm();
    } catch (e) {
      debugPrint('Failed to connect to mediator: $e');
      _isConnected = false;
      notifyListeners();
    }
  }

  /// Disconnect from the mediator.
  Future<void> disconnect() async {
    try {
      // In production, call Rust bridge:
      // await disconnectMediator();
    } catch (_) {}

    _isConnected = false;
    notifyListeners();
  }

  /// Send an authorization response back to the MCP server.
  Future<void> sendAuthResponse(AuthResponseData response) async {
    try {
      // In production, call Rust bridge:
      // await sendAuthResponse(
      //   storagePath: './phone_data',
      //   paymentId: response.paymentId,
      //   authorized: response.authorized,
      //   listAction: response.listAction,
      //   mcpDid: _pendingAuth?.merchantDid ?? '',
      // );

      debugPrint(
          'Auth response: ${response.paymentId} -> ${response.authorized} (${response.listAction})');

      _pendingAuth = null;
      notifyListeners();
    } catch (e) {
      debugPrint('Failed to send auth response: $e');
    }
  }

  /// V2.0: Send an authorization response with a session key.
  /// Creates a local session key first, then sends the auth response with session key data.
  Future<void> sendAuthResponseWithSessionKey({
    required String paymentId,
    required bool authorized,
    required String listAction,
    required int spendingLimit,
    required int durationSecs,
    String? listLabel,
    int? listMaxAmount,
  }) async {
    try {
      // V2.0: Create session key via Rust bridge
      // In production:
      // final sessionKey = await createSessionKeyForPayment(
      //   storagePath: './phone_data',
      //   spendingLimit: spendingLimit,
      //   durationSecs: durationSecs,
      // );
      // Then pass session key data to sendAuthResponse

      debugPrint(
          'V2.0 Auth response with session key: $paymentId -> $authorized '
          '(limit: $spendingLimit lamports, duration: ${durationSecs}s, '
          'action: $listAction)');

      // Send auth response with session key data
      await sendAuthResponse(AuthResponseData(
        paymentId: paymentId,
        authorized: authorized,
        listAction: listAction,
        listLabel: listLabel,
        listMaxAmount: listMaxAmount,
      ));

      _pendingAuth = null;
      notifyListeners();
    } catch (e) {
      debugPrint('Failed to send auth response with session key: $e');
      rethrow;
    }
  }

  /// Authenticate with mediator and pull pending messages.
  Future<void> _authenticateAndPull() async {
    if (_mediatorHttpUrl.isEmpty) return;

    try {
      // In production, call Rust bridge:
      // _authToken = await authenticateWithMediator(
      //   mediatorUrl: _mediatorHttpUrl,
      //   did: _did,
      // );

      // For now, use the HTTP API directly
      _authToken = await _api.authenticate(_did, 'placeholder');

      if (_authToken != null) {
        await _pullAndDecryptMessages();
      }
    } catch (e) {
      debugPrint('Auth/pull failed: $e');
    }
  }

  /// Pull messages from mediator and decrypt them.
  Future<void> _pullAndDecryptMessages() async {
    if (_authToken == null) return;

    try {
      final messages = await _api.pullMessages(
        _authToken!,
        afterId: _lastPulledId,
        limit: 50,
      );

      for (final msg in messages) {
        await _decryptAndProcess(msg.jweEnvelope);
        _lastPulledId = msg.msgId;
      }
    } catch (e) {
      debugPrint('Pull/decrypt failed: $e');
    }
  }

  /// Decrypt a JWE envelope and process the message.
  Future<void> _decryptAndProcess(String jweEnvelope) async {
    try {
      // In production, call Rust bridge:
      // final decrypted = await decryptMessage(
      //   storagePath: './phone_data',
      //   jwe: jweEnvelope,
      // );
      //
      // final msg = DecryptedMsg(
      //   msgType: decrypted.msgType,
      //   paymentId: decrypted.paymentId,
      //   merchantDid: decrypted.merchantDid,
      //   amount: decrypted.amount,
      //   description: decrypted.description,
      //   rawBody: decrypted.rawBody,
      // );

      // Placeholder until bridge is regenerated
      final msg = DecryptedMsg(
        msgType: 'placeholder',
        rawBody: jweEnvelope,
      );

      _messages.add(msg);

      // Check if it's a payment-auth-request
      if (msg.msgType.contains('payment-auth-request')) {
        final authReq = AuthRequest(
          paymentId: msg.paymentId ?? '',
          merchantDid: msg.merchantDid ?? '',
          amount: msg.amount ?? 0,
          description: msg.description ?? '',
        );
        _pendingAuth = authReq;
        _authRequestController.add(authReq);
      }

      notifyListeners();
    } catch (e) {
      debugPrint('Decrypt failed: $e');
    }
  }

  /// Initialize FCM for push notifications.
  Future<void> _initFcm() async {
    try {
      await FcmService().initialize(
        onSignalReceived: _onFcmSignal,
      );

      // Register FCM token with mediator
      final fcmToken = FcmService().fcmToken;
      if (fcmToken != null && _authToken != null) {
        await _api.registerDeviceToken(_authToken!, fcmToken);
      }
    } catch (e) {
      debugPrint('FCM init failed (non-fatal): $e');
    }
  }

  /// Called when an FCM signal is received.
  void _onFcmSignal(String msgId) {
    debugPrint('FCM signal received for msg: $msgId');
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

  /// Clear the pending auth request.
  void clearPendingAuth() {
    _pendingAuth = null;
    notifyListeners();
  }

  @override
  void dispose() {
    _authRequestController.close();
    super.dispose();
  }
}
