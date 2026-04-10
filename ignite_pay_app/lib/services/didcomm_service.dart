import 'dart:async';
import 'package:flutter/foundation.dart';

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
  final String listAction; // "none", "whitelist", "blacklist"

  AuthResponseData({
    required this.paymentId,
    required this.authorized,
    required this.listAction,
  });
}

/// Service managing DID identity and DIDComm WebSocket connection.
class DidcommService {
  static final DidcommService _instance = DidcommService._internal();
  factory DidcommService() => _instance;
  DidcommService._internal();

  String _did = '';
  String _didDocJson = '';
  String _mediatorWsUrl = '';
  bool _isConnected = false;

  final StreamController<AuthRequest> _authRequestController =
      StreamController<AuthRequest>.broadcast();

  /// Current DID identity string.
  String get did => _did;

  /// DID document as JSON string.
  String get didDocJson => _didDocJson;

  /// Whether connected to mediator.
  bool get isConnected => _isConnected;

  /// Stream of incoming auth requests.
  Stream<AuthRequest> get authRequests => _authRequestController.stream;

  /// Initialize DID identity (generates or loads from storage).
  Future<void> initialize({String storagePath = './phone_data'}) async {
    try {
      // In a real app, this would call the Rust bridge:
      // final info = await getOrCreateDid(storagePath);
      // _did = info.did;
      // _didDocJson = info.didDocJson;

      // Placeholder for now
      _did = 'did:ignite:zPhonePlaceholder${DateTime.now().millisecondsSinceEpoch % 10000}';
      _didDocJson = '{}';
      debugPrint('DID initialized: $_did');
    } catch (e) {
      debugPrint('Failed to initialize DID: $e');
    }
  }

  /// Connect to the DIDComm mediator WebSocket.
  Future<void> connectToMediator(String wsUrl) async {
    _mediatorWsUrl = wsUrl;
    // In a real app, this would call the Rust bridge WsClient.connect()
    _isConnected = true;
    debugPrint('Connected to mediator: $wsUrl');

    // Simulate receiving auth requests for demo
    _startMockListener();
  }

  /// Send an authorization response back to the MCP server.
  Future<void> sendAuthResponse(AuthResponseData response) async {
    // In a real app, this would call WsClient.send_auth_response()
    debugPrint(
        'Auth response: ${response.paymentId} -> ${response.authorized} (${response.listAction})');
  }

  /// Simulate incoming auth requests for demo purposes.
  void _startMockListener() {
    // No-op in production. The Rust WsClient callback would push to _authRequestController.
  }

  /// Simulate an incoming auth request (for testing).
  void simulateAuthRequest(AuthRequest request) {
    _authRequestController.add(request);
  }

  void dispose() {
    _authRequestController.close();
  }
}
