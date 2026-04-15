import 'package:dio/dio.dart';

/// A DIDComm message envelope returned from the mediator.
class DidcommMessage {
  final String msgId;
  final String jweEnvelope;
  final int createdAt;

  DidcommMessage({
    required this.msgId,
    required this.jweEnvelope,
    required this.createdAt,
  });

  factory DidcommMessage.fromJson(Map<String, dynamic> json) {
    return DidcommMessage(
      msgId: json['msg_id'] as String,
      jweEnvelope: json['jwe_envelope'] as String,
      createdAt: json['created_at'] as int,
    );
  }
}

/// HTTP client for the DIDComm mediator REST API.
class MediatorApi {
  final Dio _dio;

  MediatorApi({String? baseUrl})
      : _dio = Dio(BaseOptions(
          baseUrl: baseUrl ?? 'http://10.0.2.2:3000',
          connectTimeout: const Duration(seconds: 10),
          receiveTimeout: const Duration(seconds: 30),
        ));

  /// Update the base URL.
  void setBaseUrl(String url) {
    _dio.options.baseUrl = url;
  }

  /// Authenticate with the mediator and get a JWT token.
  Future<String> authenticate(String did, String signature) async {
    final response = await _dio.post('/v1/auth/token', data: {
      'did': did,
      'signature': signature,
    });
    return response.data['token'] as String;
  }

  /// Pull a list of messages from the mediator.
  Future<List<DidcommMessage>> pullMessages(
    String token, {
    String? afterId,
    int limit = 100,
  }) async {
    final queryParams = <String, dynamic>{'limit': limit};
    if (afterId != null) queryParams['after'] = afterId;

    final response = await _dio.get(
      '/v1/sync/list',
      queryParameters: queryParams,
      options: Options(headers: {'Authorization': 'Bearer $token'}),
    );

    final messages = (response.data['messages'] as List)
        .map((m) => DidcommMessage.fromJson(m as Map<String, dynamic>))
        .toList();
    return messages;
  }

  /// Get a single message by ID.
  Future<DidcommMessage> getMessage(String token, String msgId) async {
    final response = await _dio.get(
      '/v1/sync/messages/$msgId',
      options: Options(headers: {'Authorization': 'Bearer $token'}),
    );
    return DidcommMessage.fromJson(response.data);
  }

  /// Submit an encrypted command to an agent (downlink).
  Future<void> submitCommand(
    String token,
    String agentId,
    String jweEnvelope,
  ) async {
    await _dio.post(
      '/v1/agents/$agentId/command',
      data: {'jwe_envelope': jweEnvelope},
      options: Options(headers: {'Authorization': 'Bearer $token'}),
    );
  }

  /// Register an FCM device token with the mediator.
  Future<void> registerDeviceToken(String token, String fcmToken) async {
    await _dio.post(
      '/v1/devices/register-token',
      data: {'fcm_token': fcmToken, 'push_channel': 'fcm'},
      options: Options(headers: {'Authorization': 'Bearer $token'}),
    );
  }

  /// Register a WebSocket push channel preference (for Chinese users without FCM).
  Future<void> registerWebSocketChannel(String token) async {
    await _dio.post(
      '/v1/devices/register-token',
      data: {'push_channel': 'websocket'},
      options: Options(headers: {'Authorization': 'Bearer $token'}),
    );
  }

  /// Bind an agent DID to the authenticated user for message routing.
  Future<void> bindAgent(String token, String agentDid) async {
    await _dio.post(
      '/v1/agents/bind',
      data: {'agent_did': agentDid},
      options: Options(headers: {'Authorization': 'Bearer $token'}),
    );
  }
}
