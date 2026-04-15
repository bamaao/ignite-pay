import 'dart:async';
import 'dart:convert';
import 'package:flutter/foundation.dart';
import 'package:flutter/scheduler.dart';
import 'package:ignite_pay_app/services/fcm_service.dart';
import 'package:ignite_pay_app/services/mediator_api.dart';
import 'package:ignite_pay_app/src/rust/api/simple.dart' as rust;
import 'package:shared_preferences/shared_preferences.dart';
import 'package:web_socket_channel/web_socket_channel.dart';

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
  WebSocketChannel? _wsChannel;
  StreamSubscription? _wsSubscription;

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
  Future<void> initialize({String storagePath = './phone_data'}) async {
    if (_isInitialized) return;

    try {
      // Load saved mediator URLs
      final prefs = await SharedPreferences.getInstance();
      _mediatorWsUrl = prefs.getString('mediator_ws_url') ?? '';
      _mediatorHttpUrl = prefs.getString('mediator_http_url') ?? '';

      final info = await rust.initializeIdentity(storagePath: storagePath);
      _did = info.did;
      _didDocJson = info.didDocJson;

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
      await rust.connectMediator(storagePath: './phone_data', wsUrl: wsUrl);

      _isConnected = true;
      debugPrint('Connected to mediator: $wsUrl');
      notifyListeners();

      // Authenticate and pull any pending messages
      await _authenticateAndPull();

      if (_isChineseUser) {
        // Chinese users: register websocket push channel, maintain WS long connection
        await _initWebSocketChannel();
      } else {
        // Overseas users: use FCM for push notifications
        await _initFcm();
      }
    } catch (e) {
      debugPrint('Failed to connect to mediator: $e');
      _isConnected = false;
      notifyListeners();
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
    notifyListeners();
  }

  /// Send an authorization response back to the MCP server.
  Future<void> sendAuthResponse(AuthResponseData response) async {
    try {
      await rust.sendAuthResponse(
        storagePath: './phone_data',
        paymentId: response.paymentId,
        authorized: response.authorized,
        listAction: response.listAction,
        mcpDid: _pendingAuth?.merchantDid ?? '',
        sessionKeyInfo: null,
        listLabel: response.listLabel,
        listMaxAmount: response.listMaxAmount != null
            ? BigInt.from(response.listMaxAmount!)
            : null,
      );

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
      final sessionKey = await rust.createSessionKeyForPayment(
        storagePath: './phone_data',
        spendingLimit: BigInt.from(spendingLimit),
        durationSecs: durationSecs,
      );
      await rust.sendAuthResponse(
        storagePath: './phone_data',
        paymentId: paymentId,
        authorized: authorized,
        listAction: listAction,
        mcpDid: _pendingAuth?.merchantDid ?? '',
        sessionKeyInfo: sessionKey,
        listLabel: listLabel,
        listMaxAmount: listMaxAmount != null ? BigInt.from(listMaxAmount) : null,
      );

      debugPrint(
          'V2.0 Auth response with session key: $paymentId -> $authorized '
          '(limit: $spendingLimit lamports, duration: ${durationSecs}s, '
          'action: $listAction)');

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
      _authToken = await rust.authenticateWithMediator(
        mediatorUrl: _mediatorHttpUrl,
        did: _did,
      );

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
      debugPrint('Pull/decrypt failed: $e');
    }
  }

  /// Decrypt a JWE envelope and process the message.
  Future<void> _decryptAndProcess(String jweEnvelope) async {
    try {
      final decrypted = await rust.decryptMessage(
        storagePath: './phone_data',
        jwe: jweEnvelope,
      );

      final msg = DecryptedMsg(
        msgType: decrypted.msgType,
        paymentId: decrypted.paymentId,
        merchantDid: decrypted.merchantDid,
        amount: decrypted.amount?.toInt(),
        description: decrypted.description,
        rawBody: decrypted.rawBody,
        listCid: decrypted.listCid,
        listType: decrypted.listType,
        label: decrypted.label,
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

  /// Initialize WebSocket channel for Chinese users (direct push, no FCM).
  Future<void> _initWebSocketChannel() async {
    if (_authToken == null || _mediatorWsUrl.isEmpty) return;

    try {
      // Register websocket push channel preference with mediator
      await _api.registerWebSocketChannel(_authToken!);

      // Establish WS long connection for receiving messages directly
      _wsChannel = WebSocketChannel.connect(Uri.parse(_mediatorWsUrl));

      // Send identification message with DID so mediator can route to us
      _wsChannel!.sink.add('{"from":"$_did","type":"identify"}');

      _wsSubscription = _wsChannel!.stream.listen(
        (data) {
          if (data is String) {
            _onWsMessage(data);
          }
        },
        onError: (error) {
          debugPrint('WS channel error: $error');
          _reconnectWebSocket();
        },
        onDone: () {
          debugPrint('WS channel closed, attempting reconnect');
          _reconnectWebSocket();
        },
      );

      debugPrint('WebSocket channel initialized for user $_did');
    } catch (e) {
      debugPrint('Failed to initialize WebSocket channel: $e');
    }
  }

  /// Handle a message received directly via WebSocket.
  void _onWsMessage(String jweEnvelope) {
    debugPrint('WS message received (${jweEnvelope.length} bytes)');
    // Decrypt and process the JWE directly
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

  /// Bind an MCP/Skill agent DID to this user for message routing.
  Future<void> bindAgent(String agentDid) async {
    if (_authToken == null) return;
    try {
      await _api.bindAgent(_authToken!, agentDid);
      debugPrint('Bound agent $agentDid to user $_did');
    } catch (e) {
      debugPrint('Failed to bind agent: $e');
    }
  }

  /// Parse an OOB invitation URL from a QR code scan and send a connection request.
  /// Returns the MCP DID on success, or throws on error.
  Future<String> parseInvitationAndConnect(String invitationUrl) async {
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

      // Extract mediator WS URL from services
      final services = body['services'] as List<dynamic>? ?? [];
      String mediatorWsUrl = '';
      if (services.isNotEmpty) {
        mediatorWsUrl = (services.first as Map<String, dynamic>)['service_endpoint'] as String? ?? '';
      }

      debugPrint('Parsed OOB invitation: MCP DID=$mcpDid, mediator=$mediatorWsUrl');

      // Determine push channel based on locale
      final pushChannel = _isChineseUser ? 'websocket' : 'fcm';
      String? fcmToken;
      if (!_isChineseUser) {
        fcmToken = FcmService().fcmToken;
      }

      // Save mediator URL if it changed and connect to mediator
      if (mediatorWsUrl.isNotEmpty && mediatorWsUrl != _mediatorWsUrl) {
        await connectToMediator(mediatorWsUrl);
      } else if (!_isConnected && mediatorWsUrl.isNotEmpty) {
        await connectToMediator(mediatorWsUrl);
      }

      // Send connection request via the WS channel or HTTP API
      final connectionBody = <String, dynamic>{
        'push_channel': pushChannel,
      };
      if (fcmToken != null) {
        connectionBody['fcm_token'] = fcmToken;
      }

      // Build the connection-request message as JSON
      final connectionMsg = jsonEncode({
        'type': 'https://didcomm.org/ignite-pay/1.0/connection-request',
        'from': _did,
        'to': [mcpDid],
        'body': connectionBody,
      });

      // Send via WS if available, otherwise via mediator HTTP
      if (_wsChannel != null) {
        _wsChannel!.sink.add(connectionMsg);
        debugPrint('Connection request sent to MCP $mcpDid via WS');
      } else if (_authToken != null) {
        await _api.submitCommand(_authToken!, mcpDid, connectionMsg);
        debugPrint('Connection request sent to MCP $mcpDid via HTTP');
      } else {
        throw Exception('Not connected to mediator. Connect first.');
      }

      return mcpDid;
    } catch (e) {
      debugPrint('Failed to parse invitation and connect: $e');
      rethrow;
    }
  }

  /// Clear the pending auth request.
  void clearPendingAuth() {
    _pendingAuth = null;
    notifyListeners();
  }

  @override
  void dispose() {
    _wsSubscription?.cancel();
    _wsChannel?.sink.close();
    _authRequestController.close();
    super.dispose();
  }
}
