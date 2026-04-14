import 'dart:io';
import 'package:firebase_core/firebase_core.dart';
import 'package:firebase_messaging/firebase_messaging.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';

/// Callback for when a signal is received (foreground or background).
typedef OnSignalReceived = void Function(String msgId);

/// Service managing Firebase Cloud Messaging for push notifications.
class FcmService {
  static final FcmService _instance = FcmService._internal();
  factory FcmService() => _instance;
  FcmService._internal();

  final FirebaseMessaging _messaging = FirebaseMessaging.instance;
  final FlutterLocalNotificationsPlugin _localNotifications =
      FlutterLocalNotificationsPlugin();

  String? _fcmToken;
  OnSignalReceived? _onSignalReceived;

  /// Current FCM device token.
  String? get fcmToken => _fcmToken;

  /// Initialize Firebase and request permissions.
  Future<void> initialize({OnSignalReceived? onSignalReceived}) async {
    _onSignalReceived = onSignalReceived;

    await Firebase.initializeApp();

    // Request notification permissions
    if (Platform.isIOS) {
      await _messaging.requestPermission(
        alert: true,
        badge: true,
        sound: true,
      );
    } else {
      await _messaging.requestPermission();
    }

    // Get FCM token
    _fcmToken = await _messaging.getToken();
    debugPrint('FCM token: $_fcmToken');

    // Listen for token refresh
    _messaging.onTokenRefresh.listen((token) {
      _fcmToken = token;
      debugPrint('FCM token refreshed: $token');
    });

    // Initialize local notifications for foreground display
    const androidSettings =
        AndroidInitializationSettings('@mipmap/ic_launcher');
    const iosSettings = DarwinInitializationSettings();
    await _localNotifications.initialize(
      const InitializationSettings(
          android: androidSettings, iOS: iosSettings),
    );

    // Create Android notification channel
    const channel = AndroidNotificationChannel(
      'ignite_pay_signals',
      'Ignite Pay Signals',
      description: 'Payment authorization notifications',
      importance: Importance.high,
    );
    await _localNotifications
        .resolvePlatformSpecificImplementation<
            AndroidFlutterLocalNotificationsPlugin>()
        ?.createNotificationChannel(channel);

    // Foreground message handler
    FirebaseMessaging.onMessage.listen(_handleForegroundMessage);

    // Background message handler (must be top-level function)
    FirebaseMessaging.onBackgroundMessage(_firebaseMessagingBackgroundHandler);
  }

  /// Handle foreground FCM messages.
  void _handleForegroundMessage(RemoteMessage message) {
    debugPrint('Foreground FCM message: ${message.messageId}');

    final data = message.data;
    final type = data['type'] as String?;
    final msgId = data['msg_id'] as String?;

    if (type == 'SIGNAL' && msgId != null) {
      // Show local notification
      _showLocalNotification(msgId);

      // Trigger pull + decrypt
      _onSignalReceived?.call(msgId);
    }
  }

  /// Show a local notification for a received signal.
  void _showLocalNotification(String msgId) {
    const androidDetails = AndroidNotificationDetails(
      'ignite_pay_signals',
      'Ignite Pay Signals',
      channelDescription: 'Payment authorization notifications',
      importance: Importance.high,
      priority: Priority.high,
    );
    const details = NotificationDetails(android: androidDetails);

    _localNotifications.show(
      msgId.hashCode,
      'Payment Authorization',
      'New payment request received',
      details,
    );
  }
}

/// Top-level background message handler for Firebase.
@pragma('vm:entry-point')
Future<void> _firebaseMessagingBackgroundHandler(RemoteMessage message) async {
  await Firebase.initializeApp();
  debugPrint('Background FCM message: ${message.messageId}');
  // In background, the OS will display the notification.
  // On app resume, the DidcommService should pull pending messages.
}
