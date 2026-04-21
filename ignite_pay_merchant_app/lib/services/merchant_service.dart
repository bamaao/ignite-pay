import 'dart:async';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:ignite_pay_merchant/src/rust/frb_generated.dart';
import 'package:ignite_pay_merchant/src/rust/api/merchant.dart' as rust;
import 'package:ignite_pay_merchant/services/merchant_push_service.dart';

class PaymentOrder {
  final String orderId;
  final String merchantDid;
  final BigInt amount;
  final String description;
  final String hubEndpoint;
  final String status;
  final int createdAt;
  final int? confirmedAt;
  final String? channelId;
  final int? leafIndex;
  final BigInt? sequence;

  PaymentOrder({
    required this.orderId,
    required this.merchantDid,
    required this.amount,
    required this.description,
    required this.hubEndpoint,
    required this.status,
    required this.createdAt,
    this.confirmedAt,
    this.channelId,
    this.leafIndex,
    this.sequence,
  });
}

class MerchantService extends ChangeNotifier {
  String _did = '';
  String _didDocJson = '';
  String _hubEndpoint = '';
  String _mediatorWsUrl = '';
  List<PaymentOrder> _orders = [];
  String _storagePath = '';
  void Function(PaymentOrder)? _onPaymentConfirmed;

  String get did => _did;
  String get didDocJson => _didDocJson;
  String get hubEndpoint => _hubEndpoint;
  String get mediatorWsUrl => _mediatorWsUrl;
  String get storagePath => _storagePath;
  List<PaymentOrder> get orders => _orders;

  Future<void> initialize() async {
    await RustLib.init();
    final dir = await getApplicationSupportDirectory();
    _storagePath = dir.path;

    final prefs = await SharedPreferences.getInstance();
    _hubEndpoint = prefs.getString('hub_endpoint') ?? '';
    _mediatorWsUrl = prefs.getString('mediator_ws_url') ?? '';

    if (_hubEndpoint.isNotEmpty) {
      try {
        final info = await rust.initializeMerchant(storagePath: _storagePath);
        _did = info.did;
        _didDocJson = info.didDocJson;
      } catch (_) {
        // Identity not yet created
      }
      await refreshOrders();

      // Initialize push service instead of polling
      if (_mediatorWsUrl.isNotEmpty) {
        final pushService = MerchantPushService();
        await pushService.initialize();
        await pushService.connectToMediator(_mediatorWsUrl);

        // Listen for payment confirmations -> refresh orders + trigger callback
        pushService.confirmations.listen((confirmation) async {
          await refreshOrders();
          if (_onPaymentConfirmed != null) {
            final order = _orders.where(
              (o) => o.orderId == confirmation.orderId,
            ).firstOrNull;
            if (order != null) {
              _onPaymentConfirmed!(order);
            }
          }
        });
      }
    }
  }

  Future<void> generateIdentity() async {
    await rust.generateMerchantKeypair(storagePath: _storagePath);
    final info = await rust.initializeMerchant(storagePath: _storagePath);
    _did = info.did;
    _didDocJson = info.didDocJson;
    notifyListeners();
  }

  Future<String> generatePaymentQr(BigInt amount, String description) async {
    final qrText = await rust.generatePaymentQr(
      merchantDid: _did,
      amount: amount,
      description: description,
      hubEndpoint: _hubEndpoint,
    );
    await refreshOrders();
    return qrText;
  }

  Future<void> refreshOrders() async {
    try {
      final bridges = await rust.listOrders(storagePath: _storagePath, limit: 50);
      _orders = bridges.map((b) => PaymentOrder(
        orderId: b.orderId,
        merchantDid: b.merchantDid,
        amount: b.amount,
        description: b.description,
        hubEndpoint: b.hubEndpoint,
        status: b.status,
        createdAt: b.createdAt,
        confirmedAt: b.confirmedAt,
        channelId: b.channelId,
        leafIndex: b.leafIndex,
        sequence: b.sequence,
      )).toList();
      notifyListeners();
    } catch (_) {}
  }

  Future<void> saveConfig(String hubEndpoint, String mediatorWsUrl) async {
    _hubEndpoint = hubEndpoint;
    _mediatorWsUrl = mediatorWsUrl;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('hub_endpoint', hubEndpoint);
    await prefs.setString('mediator_ws_url', mediatorWsUrl);
    notifyListeners();
  }

  void setOnPaymentConfirmed(void Function(PaymentOrder) callback) {
    _onPaymentConfirmed = callback;
  }
}
