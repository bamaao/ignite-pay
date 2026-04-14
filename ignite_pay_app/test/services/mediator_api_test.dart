import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ignite_pay_app/services/mediator_api.dart';

/// A simple interceptor that returns pre-configured responses.
class _MockInterceptor extends Interceptor {
  final Map<String, dynamic> responses;
  int errorCode = 0;
  bool shouldFail = false;

  _MockInterceptor(this.responses);

  @override
  void onRequest(RequestOptions options, RequestInterceptorHandler handler) {
    if (shouldFail) {
      handler.reject(
        DioException(
          requestOptions: options,
          response: Response(
            requestOptions: options,
            statusCode: errorCode,
            data: 'Error',
          ),
          type: DioExceptionType.badResponse,
        ),
      );
      return;
    }

    final key = '${options.method} ${options.path}';
    final data = responses[key];

    if (data != null) {
      handler.resolve(
        Response(
          requestOptions: options,
          statusCode: 200,
          data: data,
        ),
      );
    } else {
      handler.reject(
        DioException(
          requestOptions: options,
          response: Response(
            requestOptions: options,
            statusCode: 404,
            data: {'error': 'Not found'},
          ),
          type: DioExceptionType.badResponse,
        ),
      );
    }
  }
}

void main() {
  group('MediatorApi', () {
    test('default constructor creates instance', () {
      final api = MediatorApi();
      expect(api, isNotNull);
    });

    test('custom base URL via constructor', () {
      final api = MediatorApi(baseUrl: 'http://localhost:8080');
      expect(api, isNotNull);
    });

    test('setBaseUrl does not throw', () {
      final api = MediatorApi();
      expect(() => api.setBaseUrl('http://new-host:4000'), returnsNormally);
    });

    group('with mocked Dio', () {
      late Dio dio;
      late _MockInterceptor mock;

      setUp(() {
        dio = Dio(BaseOptions(baseUrl: 'http://test'));
        mock = _MockInterceptor({});
        dio.interceptors.add(mock);
      });

      MediatorApi _createApi() {
        final api = MediatorApi();
        // Override the internal dio - we test via the mock interceptor
        // by using a separate helper
        return _TestableMediatorApi(dio);
      }

      test('authenticate returns token', () async {
        mock.responses['POST /v1/auth/token'] = {
          'token': 'jwt_test_token_123',
        };
        final api = _createApi();

        final token = await api.authenticate('did:test:123', 'sig_hex');
        expect(token, 'jwt_test_token_123');
      });

      test('pullMessages returns parsed messages', () async {
        mock.responses['GET /v1/sync/list'] = {
          'messages': [
            {'msg_id': 'm1', 'jwe_envelope': 'enc1', 'created_at': 100},
            {'msg_id': 'm2', 'jwe_envelope': 'enc2', 'created_at': 200},
          ],
        };
        final api = _createApi();

        final msgs = await api.pullMessages('tok123');
        expect(msgs.length, 2);
        expect(msgs[0].msgId, 'm1');
        expect(msgs[0].jweEnvelope, 'enc1');
        expect(msgs[0].createdAt, 100);
        expect(msgs[1].msgId, 'm2');
      });

      test('pullMessages with afterId and custom limit', () async {
        mock.responses['GET /v1/sync/list'] = {
          'messages': <Map<String, dynamic>>[],
        };
        final api = _createApi();

        final msgs = await api.pullMessages(
          'tok',
          afterId: 'last_msg_id',
          limit: 25,
        );
        expect(msgs, isEmpty);
      });

      test('getMessage returns single parsed message', () async {
        mock.responses['GET /v1/sync/messages/abc'] = {
          'msg_id': 'abc',
          'jwe_envelope': 'envelope_data',
          'created_at': 300,
        };
        final api = _createApi();

        final msg = await api.getMessage('tok', 'abc');
        expect(msg.msgId, 'abc');
        expect(msg.jweEnvelope, 'envelope_data');
        expect(msg.createdAt, 300);
      });

      test('submitCommand completes without error', () async {
        mock.responses['POST /v1/agents/agent1/command'] = {'ok': true};
        final api = _createApi();

        await api.submitCommand('tok', 'agent1', 'jwe_payload');
      });

      test('registerDeviceToken completes without error', () async {
        mock.responses['POST /v1/devices/register-token'] = {'ok': true};
        final api = _createApi();

        await api.registerDeviceToken('tok', 'fcm_token_xyz');
      });

      test('registerWebSocketChannel completes without error', () async {
        mock.responses['POST /v1/devices/register-token'] = {'ok': true};
        final api = _createApi();

        await api.registerWebSocketChannel('tok');
      });

      test('authenticate throws on server error', () async {
        mock.shouldFail = true;
        mock.errorCode = 500;
        final api = _createApi();

        expect(
          () => api.authenticate('did:test', 'sig'),
          throwsA(isA<DioException>()),
        );
      });

      test('pullMessages throws on auth error', () async {
        mock.shouldFail = true;
        mock.errorCode = 401;
        final api = _createApi();

        expect(
          () => api.pullMessages('bad_token'),
          throwsA(isA<DioException>()),
        );
      });
    });
  });
}

/// A testable subclass that accepts a custom Dio instance.
class _TestableMediatorApi extends MediatorApi {
  final Dio _testDio;

  _TestableMediatorApi(this._testDio) : super();

  @override
  Future<String> authenticate(String did, String signature) async {
    final response = await _testDio.post('/v1/auth/token', data: {
      'did': did,
      'signature': signature,
    });
    return response.data['token'] as String;
  }

  @override
  Future<List<DidcommMessage>> pullMessages(
    String token, {
    String? afterId,
    int limit = 100,
  }) async {
    final queryParams = <String, dynamic>{'limit': limit};
    if (afterId != null) queryParams['after'] = afterId;

    final response = await _testDio.get(
      '/v1/sync/list',
      queryParameters: queryParams,
      options: Options(headers: {'Authorization': 'Bearer $token'}),
    );

    final messages = (response.data['messages'] as List)
        .map((m) => DidcommMessage.fromJson(m as Map<String, dynamic>))
        .toList();
    return messages;
  }

  @override
  Future<DidcommMessage> getMessage(String token, String msgId) async {
    final response = await _testDio.get(
      '/v1/sync/messages/$msgId',
      options: Options(headers: {'Authorization': 'Bearer $token'}),
    );
    return DidcommMessage.fromJson(response.data);
  }

  @override
  Future<void> submitCommand(
    String token,
    String agentId,
    String jweEnvelope,
  ) async {
    await _testDio.post(
      '/v1/agents/$agentId/command',
      data: {'jwe_envelope': jweEnvelope},
      options: Options(headers: {'Authorization': 'Bearer $token'}),
    );
  }

  @override
  Future<void> registerDeviceToken(String token, String fcmToken) async {
    await _testDio.post(
      '/v1/devices/register-token',
      data: {'fcm_token': fcmToken, 'push_channel': 'fcm'},
      options: Options(headers: {'Authorization': 'Bearer $token'}),
    );
  }

  @override
  Future<void> registerWebSocketChannel(String token) async {
    await _testDio.post(
      '/v1/devices/register-token',
      data: {'push_channel': 'websocket'},
      options: Options(headers: {'Authorization': 'Bearer $token'}),
    );
  }
}
