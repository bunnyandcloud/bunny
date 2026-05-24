import 'dart:io';

import 'package:firebase_core/firebase_core.dart';
import 'package:firebase_messaging/firebase_messaging.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:uuid/uuid.dart';

import '../firebase_options.dart';
import 'api.dart';
import 'server_store.dart';

@pragma('vm:entry-point')
Future<void> firebaseMessagingBackgroundHandler(RemoteMessage message) async {
  await Firebase.initializeApp();
}

class PushService {
  PushService._();
  static final instance = PushService._();

  final _notifications = FlutterLocalNotificationsPlugin();
  bool _initialized = false;
  String? _deviceId;

  Future<void> init() async {
    if (_initialized) return;
    const android = AndroidInitializationSettings('@mipmap/ic_launcher');
    const ios = DarwinInitializationSettings();
    await _notifications.initialize(
      const InitializationSettings(android: android, iOS: ios),
      onDidReceiveNotificationResponse: (_) {},
    );
    if (!kIsWeb && (Platform.isAndroid || Platform.isIOS)) {
      await _initFirebase();
    }
    _initialized = true;
  }

  Future<void> _initFirebase() async {
    try {
      final options = DefaultFirebaseOptions.currentPlatform;
      if (options == null) {
        debugPrint('bunny: Firebase not configured — push registration skipped');
        return;
      }
      await Firebase.initializeApp(options: options);
      FirebaseMessaging.onBackgroundMessage(firebaseMessagingBackgroundHandler);
      final messaging = FirebaseMessaging.instance;
      await messaging.requestPermission(alert: true, badge: true, sound: true);
      FirebaseMessaging.onMessage.listen(_showForegroundNotification);
      FirebaseMessaging.onMessageOpenedApp.listen((m) {
        debugPrint('bunny push opened: ${m.data}');
      });
    } catch (e) {
      debugPrint('bunny: Firebase init failed: $e');
    }
  }

  Future<bool> registerWithAgent(BunnyApi api, ServerStore store) async {
    await init();
    _deviceId ??= await _loadOrCreateDeviceId(store);
    try {
      final options = DefaultFirebaseOptions.currentPlatform;
      if (options == null) return false;
      final messaging = FirebaseMessaging.instance;
      final token = await messaging.getToken();
      if (token == null) return false;
      final platform = Platform.isIOS ? 'ios' : 'android';
      final configured = await api.registerPush(
        deviceId: _deviceId!,
        platform: platform,
        token: token,
      );
      messaging.onTokenRefresh.listen((t) async {
        await api.registerPush(
          deviceId: _deviceId!,
          platform: platform,
          token: t,
        );
      });
      return configured;
    } catch (e) {
      debugPrint('bunny: push register failed: $e');
      return false;
    }
  }

  Future<void> _showForegroundNotification(RemoteMessage message) async {
    final n = message.notification;
    if (n == null) return;
    const details = NotificationDetails(
      android: AndroidNotificationDetails(
        'bunny_alerts',
        'bunny alerts',
        importance: Importance.high,
        priority: Priority.high,
      ),
      iOS: DarwinNotificationDetails(),
    );
    await _notifications.show(
      message.hashCode,
      n.title,
      n.body,
      details,
    );
  }

  Future<String> _loadOrCreateDeviceId(ServerStore store) async {
    const key = 'bunny_device_id';
    final existing = await store.readPlain(key);
    if (existing != null) return existing;
    final id = const Uuid().v4();
    await store.writePlain(key, id);
    return id;
  }
}
