import 'package:flutter/foundation.dart';
import 'package:ignite_pay_app/src/rust/api/simple.dart' as bridge;

/// Payment QR data parsed from a merchant QR code.
class PaymentQrData {
  final String merchantDid;
  final int amount;
  final String description;
  final String orderId;
  final String hubEndpoint;
  final int timestamp;
  final String merchantMbPubkey;
  final String merchantMediatorUrl;

  PaymentQrData({
    required this.merchantDid,
    required this.amount,
    required this.description,
    required this.orderId,
    required this.hubEndpoint,
    required this.timestamp,
    this.merchantMbPubkey = '',
    this.merchantMediatorUrl = '',
  });
}

/// Channel info from local storage.
class LocalChannelInfo {
  final String channelId;
  final String hubEndpoint;
  final String userPubkey;
  final String providerPubkey;
  final String status;
  final int sequence;
  final int balance;
  final int totalDeposited;
  final int treeDepth;

  LocalChannelInfo({
    required this.channelId,
    required this.hubEndpoint,
    required this.userPubkey,
    required this.providerPubkey,
    required this.status,
    required this.sequence,
    required this.balance,
    required this.totalDeposited,
    required this.treeDepth,
  });
}

/// Payment result from a channel payment.
class ChannelPaymentResult {
  final String channelId;
  final int sequence;
  final int leafIndex;
  final String newRoot;

  ChannelPaymentResult({
    required this.channelId,
    required this.sequence,
    required this.leafIndex,
    required this.newRoot,
  });
}

/// Service for state channel operations.
/// Uses the Rust bridge functions for all operations.
class ChannelService extends ChangeNotifier {
  List<LocalChannelInfo> _channels = [];
  PaymentQrData? _pendingPayment;
  bool _isLoading = false;
  String? _error;

  List<LocalChannelInfo> get channels => _channels;
  PaymentQrData? get pendingPayment => _pendingPayment;
  bool get isLoading => _isLoading;
  String? get error => _error;

  /// Parse a QR code string into payment data using the Rust bridge.
  Future<PaymentQrData> parsePaymentQr(String qrData) async {
    final result = await bridge.parsePaymentQr(qrData: qrData);
    final data = PaymentQrData(
      merchantDid: result.merchantDid,
      amount: result.amount.toInt(),
      description: result.description,
      orderId: result.orderId,
      hubEndpoint: result.hubEndpoint,
      timestamp: result.timestamp,
      merchantMbPubkey: result.merchantMbPubkey,
      merchantMediatorUrl: result.merchantMediatorUrl,
    );
    setPendingPayment(data);
    return data;
  }

  /// Set a pending payment from a QR scan (for the confirmation screen).
  void setPendingPayment(PaymentQrData data) {
    _pendingPayment = data;
    notifyListeners();
  }

  /// Clear the pending payment.
  void clearPendingPayment() {
    _pendingPayment = null;
    notifyListeners();
  }

  /// Refresh the channel list from local storage via Rust bridge.
  Future<void> refreshChannels(String storagePath) async {
    _isLoading = true;
    _error = null;
    notifyListeners();

    try {
      final bridgeChannels = await bridge.listChannels(storagePath: storagePath);
      _channels = bridgeChannels.map((c) => LocalChannelInfo(
        channelId: c.channelId,
        hubEndpoint: c.hubEndpoint,
        userPubkey: c.userPubkey,
        providerPubkey: c.providerPubkey,
        status: c.status,
        sequence: c.sequence.toInt(),
        balance: c.balance.toInt(),
        totalDeposited: c.totalDeposited.toInt(),
        treeDepth: c.treeDepth,
      )).toList();
    } catch (e) {
      _error = e.toString();
      debugPrint('ChannelService.refreshChannels error: $e');
    }

    _isLoading = false;
    notifyListeners();
  }

  /// Execute a channel payment via the Rust bridge.
  Future<ChannelPaymentResult> channelPay({
    required String storagePath,
    required String channelId,
    required String hubEndpoint,
    required int amount,
    required String recipientPubkey,
  }) async {
    final result = await bridge.channelPay(
      storagePath: storagePath,
      channelId: channelId,
      hubEndpoint: hubEndpoint,
      amount: BigInt.from(amount),
      recipientPubkey: recipientPubkey,
    );
    return ChannelPaymentResult(
      channelId: result.channelId,
      sequence: result.sequence.toInt(),
      leafIndex: result.leafIndex,
      newRoot: result.newRoot,
    );
  }

  /// Open a new state channel via the Rust bridge.
  Future<String> openChannel({
    required String storagePath,
    required String hubEndpoint,
    required int deposit,
    required int treeDepth,
  }) async {
    final result = await bridge.openChannel(
      storagePath: storagePath,
      hubEndpoint: hubEndpoint,
      deposit: BigInt.from(deposit),
      treeDepth: treeDepth,
    );
    return result.channelId;
  }

  /// Find the first open channel, if any.
  LocalChannelInfo? get firstOpenChannel {
    try {
      return _channels.firstWhere(
        (c) => c.status == 'Open' || c.status == 'open',
      );
    } catch (_) {
      return null;
    }
  }

  /// Get total balance across all open channels.
  int get totalBalance {
    return _channels
        .where((c) => c.status == 'Open' || c.status == 'open')
        .fold(0, (sum, c) => sum + c.balance);
  }

  /// Format amount for display (assumes 9 decimal places like SOL lamports).
  static String formatAmount(int amount) {
    return (amount / 1000000000).toStringAsFixed(2);
  }
}
