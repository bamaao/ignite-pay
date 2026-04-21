import 'dart:async';
import 'package:flutter/foundation.dart';
import 'package:flutter/scheduler.dart';
import 'package:ignite_pay_merchant/services/fcm_service.dart';
import 'package:ignite_pay_merchant/services/mediator_api.dart';
import 'package:ignite_pay_merchant/src/rust/api/merchant_didcomm.dart' as rust;
import 'package:ignite_pay_merchant/src/rust/api/merchant.dart' as merchant_rust;
import 'package:path_provider/path_provider.dart';
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

  final MediatorApi _api = MediatorApi();

  // Streams
  final StreamController<PaymentConfirmation> _confirmationController =
      StreamController<PaymentConfirmation>.broadcast();

  // Getters
  String get commDid => _commDid;
  bool get isConnected => _isConnected;
  bool get isInitialized => _isInitialized;
  String get pushChannel => _pushChannel;

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
      debugPrint('Merchant DIDComm initialized: $_commDid');
      notifyListeners();
    } catch (e) {
      debugPrint('Failed to initialize merchant DIDComm: $e');
    }
  }

  /// Connect to the DIDComm mediator.
  Future<void> connectToMediator(String wsUrl) async {
    _mediatorWsUrl = wsUrl;
    _mediatorHttpUrl = wsUrl.replaceFirst('ws', 'http');

    _api.setBaseUrl(_mediatorHttpUrl);

    try {
      await rust.connectMediator(
          storagePath: _storagePath, wsUrl: wsUrl);

      _isConnected = true;
      debugPrint('Merchant connected to mediator: $wsUrl');
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
    _pushChannel = '';
    notifyListeners();
  }

  /// Authenticate with mediator and pull pending messages.
  Future<void> _authenticateAndPull() async {
    if (_mediatorHttpUrl.isEmpty || _commDid.isEmpty) return;

    try {
      _authToken = await rust.authenticateWithMediator(
        mediatorUrl: _mediatorHttpUrl,
        did: _commDid,
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
        storagePath: _storagePath,
        jwe: jweEnvelope,
      );

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
            debugPrint('Order confirmation failed: $e');
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
      debugPrint('Decrypt failed: $e');
    }
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
          debugPrint('WS channel error: $error');
          _reconnectWebSocket();
        },
        onDone: () {
          debugPrint('WS channel closed, attempting reconnect');
          _reconnectWebSocket();
        },
      );

      debugPrint('WebSocket channel initialized for merchant $_commDid');
    } catch (e) {
      debugPrint('Failed to initialize WebSocket channel: $e');
    }
  }

  /// Handle a message received directly via WebSocket.
  void _onWsMessage(String jweEnvelope) {
    debugPrint('WS message received (${jweEnvelope.length} bytes)');
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
      debugPrint('FCM init failed (non-fatal): $e');
    }
  }

  /// Called when an FCM signal is received.
  void _onFcmSignal(String msgId) {
    debugPrint('FCM signal received for msg: $msgId');
    _pullAndDecryptMessages();
  }

  @override
  void dispose() {
    _wsSubscription?.cancel();
    _wsChannel?.sink.close();
    _confirmationController.close();
    super.dispose();
  }
}
